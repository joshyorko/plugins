#!/usr/bin/env python3
"""Fixture of codex app-server proxy: copy raw bytes, never convert JSONL/WS."""
import os
import select
import socket
import sys

path = sys.argv[sys.argv.index("--sock") + 1]
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
    connection.connect(path)
    while True:
        ready, _, _ = select.select([0, connection], [], [])
        if 0 in ready:
            data = os.read(0, 65536)
            if not data:
                break
            connection.sendall(data)
        if connection in ready:
            data = connection.recv(65536)
            if not data:
                break
            os.write(1, data)
