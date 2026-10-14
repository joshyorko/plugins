#!/usr/bin/env python3
"""GitHub-only CONNECT proxy for the opt-in Luna Factory OCI egress override.

Only ``CONNECT api.github.com:443`` and ``CONNECT github.com:443`` are tunnelled. Every
other host or port, every other method and every absolute-form request is refused before
any upstream connection is attempted. The upstream address must resolve to a public IP.

The proxy never inspects or logs tunnel bytes or request headers. Each request logs one
line: ``allowed host:port`` or ``denied host:port`` (``denied invalid-request`` when the
request has no well-formed authority).

Standard library only, so it runs unchanged on the pinned Python base image.
"""

from __future__ import annotations

import argparse
import asyncio
import ipaddress
import logging
import re
import socket
import sys
from typing import Awaitable, Callable


ALLOWED_TARGETS = frozenset({("api.github.com", 443), ("github.com", 443)})
MAX_HEAD_BYTES = 8 * 1024
HEAD_TIMEOUT = 10.0
CONNECT_TIMEOUT = 10.0
IDLE_TIMEOUT = 300.0
MAX_CLIENTS = 64
CHUNK = 64 * 1024
_HOST = re.compile(r"^[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?$")

LOG = logging.getLogger("luna-egress-proxy")

Connector = Callable[[str, int], Awaitable[tuple[asyncio.StreamReader, asyncio.StreamWriter]]]


class Refused(Exception):
    def __init__(self, status: str, label: str) -> None:
        super().__init__(status)
        self.status = status
        self.label = label


def parse_request_line(line: bytes) -> tuple[str, int]:
    """Return the allowed (host, port), or raise Refused. Never echoes the raw line."""
    try:
        text = line.decode("ascii").rstrip("\r\n")
    except UnicodeDecodeError:
        raise Refused("400 Bad Request", "invalid-request") from None
    parts = text.split(" ")
    if len(parts) != 3 or not parts[2].startswith("HTTP/1."):
        raise Refused("400 Bad Request", "invalid-request")
    method, target, _version = parts
    if method != "CONNECT":
        raise Refused("405 Method Not Allowed", "invalid-request")
    # CONNECT takes authority-form only: no scheme, path, userinfo or brackets.
    if any(mark in target for mark in ("/", "@", "[", "]", "?", "#")):
        raise Refused("400 Bad Request", "invalid-request")
    host, sep, port_text = target.rpartition(":")
    host = host.lower()
    if not sep or not _HOST.match(host) or not port_text.isdigit() or len(port_text) > 5:
        raise Refused("400 Bad Request", "invalid-request")
    port = int(port_text)
    if (host, port) not in ALLOWED_TARGETS:
        raise Refused("403 Forbidden", f"{host}:{port}")
    return host, port


def select_public_address(infos: list[tuple]) -> str:
    """Pick the first globally routable address; refuse loopback, private and link-local."""
    for _family, _type, _proto, _name, sockaddr in infos:
        try:
            address = ipaddress.ip_address(sockaddr[0])
        except ValueError:
            continue
        if address.is_global:
            return str(address)
    raise OSError("no public address")


async def connect_public(host: str, port: int) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
    loop = asyncio.get_running_loop()
    infos = await loop.getaddrinfo(host, port, type=socket.SOCK_STREAM)
    return await asyncio.open_connection(select_public_address(infos), port)


async def _pipe(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while True:
            data = await asyncio.wait_for(reader.read(CHUNK), IDLE_TIMEOUT)
            if not data:
                break
            writer.write(data)
            await writer.drain()
    except (asyncio.TimeoutError, ConnectionError, OSError):
        pass
    finally:
        try:
            if writer.can_write_eof():
                writer.write_eof()
        except (ConnectionError, OSError, RuntimeError):
            pass


async def _close(writer: asyncio.StreamWriter) -> None:
    writer.close()
    try:
        await writer.wait_closed()
    except (ConnectionError, OSError):
        pass


class EgressProxy:
    """``connect`` is injectable for tests only; the CLI always uses ``connect_public``."""

    def __init__(self, connect: Connector = connect_public, max_clients: int = MAX_CLIENTS) -> None:
        self._connect = connect
        self._slots = asyncio.Semaphore(max_clients)

    async def handle(self, client_reader: asyncio.StreamReader, client_writer: asyncio.StreamWriter) -> None:
        if self._slots.locked():
            await _close(client_writer)
            return
        async with self._slots:
            try:
                await self._serve(client_reader, client_writer)
            finally:
                await _close(client_writer)

    async def _serve(self, client_reader: asyncio.StreamReader, client_writer: asyncio.StreamWriter) -> None:
        try:
            head = await asyncio.wait_for(client_reader.readuntil(b"\r\n\r\n"), HEAD_TIMEOUT)
        except asyncio.IncompleteReadError as error:
            if error.partial:
                LOG.info("denied invalid-request")
            return
        except (asyncio.LimitOverrunError, asyncio.TimeoutError, ConnectionError, OSError):
            LOG.info("denied invalid-request")
            return
        try:
            host, port = parse_request_line(head.split(b"\r\n", 1)[0])
        except Refused as refusal:
            LOG.info("denied %s", refusal.label)
            await self._reply(client_writer, refusal.status)
            return
        try:
            upstream_reader, upstream_writer = await asyncio.wait_for(self._connect(host, port), CONNECT_TIMEOUT)
        except (asyncio.TimeoutError, OSError):
            LOG.info("denied %s:%d unreachable", host, port)
            await self._reply(client_writer, "502 Bad Gateway")
            return
        LOG.info("allowed %s:%d", host, port)
        try:
            await self._reply(client_writer, "200 Connection Established")
            await asyncio.gather(
                _pipe(client_reader, upstream_writer),
                _pipe(upstream_reader, client_writer),
            )
        finally:
            await _close(upstream_writer)

    @staticmethod
    async def _reply(writer: asyncio.StreamWriter, status: str) -> None:
        try:
            writer.write(f"HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".encode("ascii"))
            await writer.drain()
        except (ConnectionError, OSError):
            pass


async def start_server(host: str, port: int, connect: Connector = connect_public) -> asyncio.Server:
    proxy = EgressProxy(connect)
    return await asyncio.start_server(proxy.handle, host, port, limit=MAX_HEAD_BYTES)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--listen-host", default="0.0.0.0")
    parser.add_argument("--listen-port", type=int, default=3128)
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, stream=sys.stdout, format="%(message)s")

    async def run() -> None:
        server = await start_server(args.listen_host, args.listen_port)
        LOG.info("listening on %s:%d", args.listen_host, args.listen_port)
        async with server:
            await server.serve_forever()

    try:
        asyncio.run(run())
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
