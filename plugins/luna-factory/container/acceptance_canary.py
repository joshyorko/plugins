#!/usr/bin/env python3
"""Verify the container MCP surface using only read and settings tools."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.request
from typing import Any


RELEASE_UI_SHA256 = "8ea6a2029aa29cac4577753df3f8a01466c92647e3dc1b34f33b6c48e5ce3004"
WORKBENCH_URI = "ui://luna-factory/workbench.html"
CLIENT = {"name": "luna-factory-oci-canary", "version": "1"}
CAPABILITIES = {
    "extensions": {
        "io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}
    }
}
ALLOWED_TOOL_CALLS = {
    "get_factory_capabilities",
    "list_factory_runs",
    "read_factory_settings",
    "update_factory_settings",
}
FORBIDDEN_TOOL_CALLS = {
    "start_factory",
    "resume_factory_run",
    "cancel_factory_run",
    "steer_factory_run",
    "reconcile_factory_run",
    "inspect_factory_cas",
}


class McpClient:
    def __init__(self, url: str) -> None:
        self.url = url
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        values = dict(params or {})
        if method != "initialize":
            values["_meta"] = {
                "io.modelcontextprotocol/protocolVersion": "2025-11-25",
                "io.modelcontextprotocol/clientInfo": CLIENT,
                "io.modelcontextprotocol/clientCapabilities": CAPABILITIES,
            }
        headers = {
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json",
            "MCP-Protocol-Version": "2025-11-25",
            "Mcp-Method": method,
        }
        if values.get("uri"):
            headers["Mcp-Name"] = values["uri"]
        request = urllib.request.Request(
            self.url,
            data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": values}).encode(),
            headers=headers,
        )
        with self.opener.open(request, timeout=10) as response:
            raw = response.read(1024 * 1024).decode("utf-8")
        if raw.startswith(("event:", "data:")):
            raw = "\n".join(
                line[5:].strip() for line in raw.splitlines() if line.startswith("data:")
            )
        envelope = json.loads(raw)
        if "error" in envelope:
            raise RuntimeError(f"{method} failed: {envelope['error'].get('code')}")
        return envelope["result"]

    def call(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        if name in FORBIDDEN_TOOL_CALLS or name not in ALLOWED_TOOL_CALLS:
            raise RuntimeError(f"canary tool-call policy rejected {name}")
        result = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        if result.get("isError") is True:
            raise RuntimeError(f"{name} returned an MCP tool error")
        structured = result.get("structuredContent")
        if isinstance(structured, dict):
            return structured
        raise RuntimeError(f"{name} returned no structured result")


def run(url: str, mode: str) -> dict[str, Any]:
    client = McpClient(url)
    initialized = client.request(
        "initialize",
        {"protocolVersion": "2025-11-25", "clientInfo": CLIENT, "capabilities": CAPABILITIES},
    )
    server = initialized.get("serverInfo", {})
    if server.get("name") != "luna-factory" or server.get("version") != "0.2.1":
        raise RuntimeError("unexpected MCP server identity/version")

    tools = client.request("tools/list").get("tools", [])
    names = {tool.get("name") for tool in tools}
    if (
        len(tools) != 21
        or not ALLOWED_TOOL_CALLS.issubset(names)
        or not FORBIDDEN_TOOL_CALLS.issubset(names)
    ):
        raise RuntimeError("MCP tool catalog mismatch")
    resources = client.request("resources/list").get("resources", [])
    if not any(
        resource.get("uri") == WORKBENCH_URI
        and resource.get("mimeType") == "text/html;profile=mcp-app"
        for resource in resources
    ):
        raise RuntimeError("bundled MCP Apps resource missing")
    resource = client.request("resources/read", {"uri": WORKBENCH_URI})
    contents = resource.get("contents", [])
    if len(contents) != 1 or contents[0].get("mimeType") != "text/html;profile=mcp-app":
        raise RuntimeError("bundled workbench resource shape mismatch")
    ui_hash = hashlib.sha256(contents[0].get("text", "").encode()).hexdigest()
    if ui_hash != RELEASE_UI_SHA256:
        raise RuntimeError("served workbench hash differs from the published 0.2.1 UI")

    capabilities = client.call("get_factory_capabilities")
    if capabilities.get("status_inference_calls") != 0:
        raise RuntimeError("status read reported inference")
    run_list = client.call("list_factory_runs")
    if run_list.get("runs") != []:
        raise RuntimeError("canary ledger must start and remain empty")
    settings = client.call("read_factory_settings").get("values", {})
    if mode == "prepare":
        if settings.get("profile") != "default":
            raise RuntimeError("canary settings were already initialized; refusing to overwrite")
        settings = client.call(
            "update_factory_settings", {"set": {"profile": "oci-canary"}}
        ).get("values", {})
        if settings.get("profile") != "oci-canary":
            raise RuntimeError("settings update did not persist the canary profile")
    elif mode == "verify":
        if settings.get("profile") != "oci-canary":
            raise RuntimeError("persisted canary profile missing after container recreation")
    else:
        raise ValueError("unsupported canary mode")

    return {
        "mode": mode,
        "endpoint": url,
        "server": server["name"],
        "version": server["version"],
        "protocol": initialized.get("protocolVersion"),
        "tool_count": len(tools),
        "workbench_uri": WORKBENCH_URI,
        "workbench_sha256": ui_hash,
        "inference_calls": capabilities["status_inference_calls"],
        "factory_runs": len(run_list["runs"]),
        "persisted_profile": settings["profile"],
        "execution_tool_calls": 0,
        "cas_calls": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:18788/mcp")
    parser.add_argument("--mode", choices=("prepare", "verify"), required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.url, args.mode), sort_keys=True))
    except (OSError, UnicodeError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        print(f"canary failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
