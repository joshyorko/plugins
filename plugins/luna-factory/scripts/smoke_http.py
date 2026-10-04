#!/usr/bin/env python3
"""Smoke a built daemon and bundled UI over loopback; never starts native inference."""
import argparse
import json
import pathlib
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--ui", type=pathlib.Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="luna-http-smoke-") as temporary:
        root = pathlib.Path(temporary)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        config = {"listen": f"127.0.0.1:{port}", "database": str(root / "state/runs.sqlite"),
                  "codex_binary": str(root / "native-deliberately-absent"), "skill_path": str(root / "SKILL.md"),
                  "repositories": {}, "profiles": {}, "limits": {"capacity": 1, "repair_attempts": 1, "wall_seconds": 30}}
        config_path = root / "config.json"
        config_path.write_text(json.dumps(config))
        with (root / "stderr.log").open("w") as stderr:
            process = subprocess.Popen([str(args.binary.resolve()), "serve", "--config", str(config_path), "--ui", str(args.ui.resolve())], stdout=subprocess.DEVNULL, stderr=stderr)
            try:
                def rpc(method, params):
                    params["_meta"] = {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "luna-http-smoke", "version": "1"}, "io.modelcontextprotocol/clientCapabilities": {"extensions": {"io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}}}}
                    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream", "MCP-Protocol-Version": "2026-07-28", "Mcp-Method": method}
                    if "name" in params or "uri" in params:
                        headers["Mcp-Name"] = params.get("name", params.get("uri"))
                    request = urllib.request.Request(f"http://127.0.0.1:{port}/mcp", data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(), headers=headers)
                    with urllib.request.urlopen(request, timeout=5) as response:
                        return json.load(response)["result"]
                deadline = time.monotonic() + 10
                while True:
                    try:
                        discovery = rpc("server/discover", {})
                        break
                    except urllib.error.URLError:
                        if process.poll() is not None or time.monotonic() >= deadline:
                            raise RuntimeError("isolated daemon did not become ready")
                        time.sleep(0.1)
                assert "2026-07-28" in discovery["supportedVersions"]
                tools = rpc("tools/list", {})["tools"]
                by_name = {tool["name"]: tool for tool in tools}
                assert by_name["open_factory"]["_meta"]["openai/ui"]["entrypoints"] == [{"type": "global"}]
                assert by_name["open_factory_panel"]["_meta"]["openai/ui"]["entrypoints"] == [{"type": "thread"}]
                resource = rpc("resources/read", {"uri": "ui://luna-factory/workbench.html"})
                html = resource["contents"][0]
                assert html["mimeType"] == "text/html;profile=mcp-app"
                assert html["text"] == args.ui.read_text()
                result = rpc("tools/call", {"name": "open_factory", "arguments": {}})
                assert result["structuredContent"]["runs"] == []
                assert result["structuredContent"]["capabilities"]["status_inference_calls"] == 0
                print(json.dumps({"result": "passed", "protocol": "2026-07-28", "tools": len(tools), "ui_bytes": len(html["text"].encode()), "native_inference": "not_available_or_requested", "chatgpt_tunnel_mobile": "unproved"}))
            finally:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()
