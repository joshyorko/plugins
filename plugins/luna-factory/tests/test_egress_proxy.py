"""GitHub-only egress proxy and compose network contracts. Loopback only; no real network."""

import asyncio
import importlib.util
import re
import socket
import unittest
from pathlib import Path


CONTAINER_DIR = Path(__file__).parents[1] / "container"
SPEC = importlib.util.spec_from_file_location("luna_factory_egress_proxy", CONTAINER_DIR / "egress_proxy.py")
PROXY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROXY)

SECRET = "ghs_not-a-real-token-but-must-never-be-logged"


def top_level_block(text: str, key: str) -> dict[str, list[str]]:
    """Children of a top-level YAML mapping key, each with its indented body lines."""
    children: dict[str, list[str]] = {}
    inside = False
    current = None
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith(" "):
            inside = line.rstrip() == f"{key}:"
            current = None
            continue
        if not inside:
            continue
        match = re.match(r"^  ([A-Za-z0-9_.-]+):\s*$", line)
        if match:
            current = match.group(1)
            children[current] = []
        elif current is not None:
            children[current].append(line.strip())
    return children


class EgressProxyTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.upstream_payloads = []
        self.connects = []

        async def echo(reader, writer):
            data = await reader.read(1024)
            self.upstream_payloads.append(data)
            writer.write(b"echo:" + data)
            await writer.drain()
            writer.close()

        self.upstream = await asyncio.start_server(echo, "127.0.0.1", 0)
        upstream_port = self.upstream.sockets[0].getsockname()[1]

        async def local_stand_in(host, port):
            # Test-only override: the allowlisted name is routed to a loopback listener.
            self.connects.append((host, port))
            return await asyncio.open_connection("127.0.0.1", upstream_port)

        self.server = await PROXY.start_server("127.0.0.1", 0, connect=local_stand_in)
        self.port = self.server.sockets[0].getsockname()[1]

    async def asyncTearDown(self):
        self.server.close()
        await self.server.wait_closed()
        self.upstream.close()
        await self.upstream.wait_closed()

    async def request(self, head: bytes, payload: bytes = b""):
        reader, writer = await asyncio.open_connection("127.0.0.1", self.port)
        writer.write(head)
        await writer.drain()
        status = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), 5)
        body = b""
        if payload:
            writer.write(payload)
            await writer.drain()
            body = await asyncio.wait_for(reader.read(1024), 5)
        writer.close()
        return status.split(b"\r\n", 1)[0].decode(), body

    async def test_allows_connect_to_github_and_logs_only_the_authority(self):
        for host in ("api.github.com", "github.com", "API.GitHub.com"):
            with self.subTest(host=host), self.assertLogs("luna-egress-proxy", "INFO") as logs:
                head = f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\nProxy-Authorization: Bearer {SECRET}\r\n\r\n"
                status, body = await self.request(head.encode(), f"tls-hello {SECRET}".encode())
                self.assertEqual(status, "HTTP/1.1 200 Connection Established")
                self.assertEqual(body, f"echo:tls-hello {SECRET}".encode())
                self.assertEqual(logs.output, [f"INFO:luna-egress-proxy:allowed {host.lower()}:443"])
        self.assertEqual(self.connects, [("api.github.com", 443), ("github.com", 443), ("api.github.com", 443)])

    async def assert_denied(self, head: bytes, expected_status: str, expected_log: str):
        with self.assertLogs("luna-egress-proxy", "INFO") as logs:
            status, _ = await self.request(head)
        self.assertEqual(status, f"HTTP/1.1 {expected_status}")
        self.assertEqual(logs.output, [f"INFO:luna-egress-proxy:{expected_log}"])
        self.assertNotIn(SECRET, "\n".join(logs.output))
        self.assertEqual(self.connects, [], "a denied request must never open an upstream connection")
        self.assertEqual(self.upstream_payloads, [])

    async def test_denies_other_hosts(self):
        for host in ("example.com", "uploads.github.com", "github.com.evil.test", "127.0.0.1", "localhost"):
            with self.subTest(host=host):
                await self.assert_denied(f"CONNECT {host}:443 HTTP/1.1\r\n\r\n".encode(), "403 Forbidden", f"denied {host}:443")

    async def test_denies_other_ports(self):
        for port in (80, 22, 8443, 4430):
            with self.subTest(port=port):
                await self.assert_denied(f"CONNECT api.github.com:{port} HTTP/1.1\r\n\r\n".encode(), "403 Forbidden", f"denied api.github.com:{port}")
        await self.assert_denied(b"CONNECT api.github.com HTTP/1.1\r\n\r\n", "400 Bad Request", "denied invalid-request")

    async def test_denies_non_connect_methods(self):
        for method in ("GET", "POST", "PUT", "DELETE", "OPTIONS", "connect"):
            with self.subTest(method=method):
                head = f"{method} api.github.com:443 HTTP/1.1\r\nAuthorization: Bearer {SECRET}\r\n\r\n"
                await self.assert_denied(head.encode(), "405 Method Not Allowed", "denied invalid-request")

    async def test_denies_absolute_and_origin_form(self):
        for head in (
            f"GET https://api.github.com/repos HTTP/1.1\r\nAuthorization: Bearer {SECRET}\r\n\r\n",
            "GET http://github.com:443/ HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: api.github.com\r\n\r\n",
        ):
            with self.subTest(head=head.split("\r\n", 1)[0]):
                await self.assert_denied(head.encode(), "405 Method Not Allowed", "denied invalid-request")
        for target in ("https://api.github.com:443", "user@api.github.com:443", "[::1]:443", "api.github.com:443/x"):
            with self.subTest(target=target):
                await self.assert_denied(f"CONNECT {target} HTTP/1.1\r\n\r\n".encode(), "400 Bad Request", "denied invalid-request")

    async def test_denies_malformed_and_oversized_heads(self):
        await self.assert_denied(b"CONNECT api.github.com:443\r\n\r\n", "400 Bad Request", "denied invalid-request")
        reader, writer = await asyncio.open_connection("127.0.0.1", self.port)
        with self.assertLogs("luna-egress-proxy", "INFO") as logs:
            writer.write(b"CONNECT api.github.com:443 HTTP/1.1\r\nX: " + b"a" * (PROXY.MAX_HEAD_BYTES + 1))
            await writer.drain()
            self.assertEqual(await asyncio.wait_for(reader.read(), 5), b"")
        writer.close()
        self.assertEqual(logs.output, ["INFO:luna-egress-proxy:denied invalid-request"])
        self.assertEqual(self.connects, [])

    def test_upstream_must_resolve_to_a_public_address(self):
        def info(address):
            return (socket.AF_INET6 if ":" in address else socket.AF_INET, socket.SOCK_STREAM, 6, "", (address, 443))

        for private in ("127.0.0.1", "10.0.0.5", "172.16.0.1", "192.168.1.1", "169.254.169.254", "::1", "fd00::1"):
            with self.subTest(address=private), self.assertRaises(OSError):
                PROXY.select_public_address([info(private)])
        self.assertEqual(PROXY.select_public_address([info("10.0.0.5"), info("140.82.112.6")]), "140.82.112.6")

    def test_default_connector_is_the_public_resolver(self):
        self.assertIs(PROXY.EgressProxy()._connect, PROXY.connect_public)
        self.assertEqual(PROXY.ALLOWED_TARGETS, frozenset({("api.github.com", 443), ("github.com", 443)}))


class ComposeEgressTests(unittest.TestCase):
    def test_default_compose_has_no_non_internal_network(self):
        compose = (CONTAINER_DIR / "compose.yaml").read_text()
        networks = top_level_block(compose, "networks")
        self.assertTrue(networks)
        for name, body in networks.items():
            self.assertIn("internal: true", body, f"default network {name} must be internal")
        self.assertNotIn("PROXY", compose.upper())
        self.assertNotIn("github-egress", compose)

    def test_default_overrides_add_no_egress(self):
        for path in sorted(CONTAINER_DIR.glob("compose*.yaml")):
            if path.name == "compose.github-egress.yaml":
                continue
            with self.subTest(path=path.name):
                for name, body in top_level_block(path.read_text(), "networks").items():
                    self.assertIn("internal: true", body, f"{path.name} network {name} must be internal")

    def test_github_egress_override_is_hardened_and_github_only(self):
        override = (CONTAINER_DIR / "compose.github-egress.yaml").read_text()
        containerfile = (CONTAINER_DIR / "Containerfile").read_text()
        python_base = re.search(r"^ARG PYTHON_BASE=(\S+)$", containerfile, re.M).group(1)
        services = top_level_block(override, "services")
        self.assertEqual(set(services), {"github-egress-proxy", "luna-factory"})
        proxy = services["github-egress-proxy"]
        self.assertIn(f"image: {python_base}", proxy)
        for line in ('user: "65532:65532"', "read_only: true", "- ALL", "- no-new-privileges:true", "- private", "- github-egress"):
            self.assertIn(line, proxy)
        self.assertIn("target: /opt/luna-egress/egress_proxy.py", proxy)
        self.assertNotIn("ports:", override)
        luna = services["luna-factory"]
        self.assertIn("HTTPS_PROXY: http://github-egress-proxy:3128", luna)
        self.assertIn("https_proxy: http://github-egress-proxy:3128", luna)
        self.assertIn("NO_PROXY: 127.0.0.1,localhost,::1", luna)
        self.assertNotIn("networks:", luna, "Luna itself stays on the internal network only")
        self.assertFalse(any(line.upper().startswith("HTTP_PROXY") for line in luna))
        networks = top_level_block(override, "networks")
        self.assertEqual(networks, {"github-egress": ["driver: bridge", "internal: false"]})


if __name__ == "__main__":
    unittest.main()
