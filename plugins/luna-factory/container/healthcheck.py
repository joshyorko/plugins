#!/usr/bin/env python3
"""Stateless MCP initialize probe; it performs no Factory tool calls."""

import json
import os
import sys
import urllib.request


def main() -> int:
    origin = os.environ.get("LUNA_PUBLISHED_ORIGIN", "")
    if not origin.startswith("http://127.0.0.1:"):
        return 1
    authority = origin.removeprefix("http://")
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "clientInfo": {"name": "luna-factory-container-health", "version": "1"},
            "capabilities": {},
        },
    }
    request = urllib.request.Request(
        "http://127.0.0.1:8787/mcp",
        data=json.dumps(payload).encode(),
        headers={
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json",
            "Host": authority,
            "Origin": origin,
            "MCP-Protocol-Version": "2025-11-25",
            "Mcp-Method": "initialize",
        },
    )
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(request, timeout=4) as response:
            raw = response.read(1024 * 1024).decode("utf-8")
        if raw.startswith(("event:", "data:")):
            raw = "\n".join(
                line[5:].strip() for line in raw.splitlines() if line.startswith("data:")
            )
        result = json.loads(raw).get("result", {})
        info = result.get("serverInfo", {})
        return 0 if info.get("name") == "luna-factory" and info.get("version") == "0.2.1" else 1
    except (OSError, UnicodeError, ValueError, json.JSONDecodeError):
        return 1


if __name__ == "__main__":
    sys.exit(main())
