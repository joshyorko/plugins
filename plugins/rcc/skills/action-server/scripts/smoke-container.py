#!/usr/bin/env python3
"""Exercise the bundled ACTIONS container in an isolated Docker Compose project."""

from __future__ import annotations

import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def main() -> None:
    skill_dir = Path(__file__).resolve().parents[1]
    scratch = Path(tempfile.mkdtemp(prefix="actions-container-smoke-"))
    project_dir = scratch / "project"
    shutil.copytree(skill_dir / "assets/container", project_dir)
    project_name = "actions-smoke-" + secrets.token_hex(6)
    environment = os.environ.copy()
    # Ignore ambient application overrides: this check exercises the bundled pins.
    for name in ("ACTIONS_RUNTIME_VERSION", "ACTIONS_CORE_VERSION", "COMPOSE_FILE"):
        environment.pop(name, None)
    environment["ACTION_SERVER_HOST_PORT"] = "0"
    secret_dir = project_dir / ".secrets"
    secret_dir.mkdir(mode=0o700)
    api_key = secrets.token_hex(32)
    key_file = secret_dir / "action-server-api-key"
    key_file.write_text(api_key + "\n")
    key_file.chmod(0o444)
    action_file = project_dir / "my_actions.py"
    original_actions = action_file.read_text()
    action_file.write_text(
        original_actions
        + '\n\n@action(is_consequential=False)\n'
        + 'def retired_echo() -> str:\n'
        + '    """Return a fixed value to test removal during image updates."""\n'
        + '    return "retired"\n'
    )
    log_path = scratch / "docker.log"
    compose_command = [
        "docker", "compose", "--project-directory", str(project_dir),
        "--project-name", project_name,
    ]
    print(f"Diagnostics: {log_path}", flush=True)

    with log_path.open("w") as log:
        def compose(*args: str, capture: bool = False) -> str:
            result = subprocess.run(
                compose_command + list(args), env=environment,
                stdout=subprocess.PIPE if capture else log,
                stderr=log, text=True, timeout=900, check=True,
            )
            return result.stdout.strip() if capture else ""

        def request(path: str, payload=None, *, authorized=True, headers=None):
            request_headers = {"Content-Type": "application/json"}
            if authorized:
                request_headers["Authorization"] = "Bearer " + api_key
            request_headers.update(headers or {})
            data = json.dumps(payload).encode() if payload is not None else None
            with urlopen(Request(base_url + path, data=data, headers=request_headers), timeout=60) as response:
                body = response.read().decode()
                if response.headers.get_content_type() == "text/event-stream":
                    messages = [line[6:] for line in body.splitlines() if line.startswith("data: ")]
                    body = next(message for message in messages if "\"jsonrpc\"" in message)
                return json.loads(body) if body else None, response.headers

        def start() -> str:
            compose("up", "-d", "--force-recreate", "--wait", "--wait-timeout", "180")
            address = compose("port", "action-server", "8080", capture=True)
            return "http://" + address

        try:
            print("Building the PyPI ACTIONS starter...", flush=True)
            compose("build")
            base_url = start()
            assert compose("exec", "-T", "action-server", "id", "-u", capture=True) == "10001"
            compose("exec", "-T", "action-server", "test", "!", "-e", "/opt/actions/.secrets")
            versions = json.loads(compose(
                "exec", "-T", "action-server", "python", "-c",
                "import importlib.metadata as m,json; print(json.dumps({p:m.version(p) for p in ('actions-runtime','actions-core','mcp')}))",
                capture=True,
            ))
            assert versions["actions-runtime"] == "1.0.2", versions
            assert versions["actions-core"] == "1.0.1", versions
            assert versions["mcp"].split(".")[0] == "2", versions
            schema, _ = request("/openapi.json")
            action_path = next(path for path in schema["paths"] if path.endswith("/container-echo/run"))
            retired_path = next(path for path in schema["paths"] if path.endswith("/retired-echo/run"))
            for auth_headers in ({}, {"Authorization": "Bearer invalid-smoke-key"}):
                try:
                    request(action_path, {"message": "hello"}, authorized=False, headers=auth_headers)
                except HTTPError as error:
                    assert error.code in (401, 403), error.code
                else:
                    raise AssertionError("An unauthenticated or invalid-key action call succeeded")
            result, _ = request(action_path, {"message": "hello"})
            assert result == "container:hello", result
            runs, _ = request("/api/runs")
            run_id = runs[0]["id"]
            retained_run, _ = request("/api/runs/" + run_id)
            assert retained_run["result"] is not None, retained_run
            print("HTTP action, authorization, non-root user, and package pins passed.", flush=True)

            mcp_headers = {"Accept": "application/json, text/event-stream"}
            for auth_headers in ({}, {"Authorization": "Bearer invalid-smoke-key"}):
                try:
                    request("/mcp", {
                        "jsonrpc": "2.0", "id": 0, "method": "tools/list", "params": {},
                    }, authorized=False, headers={**mcp_headers, **auth_headers})
                except HTTPError as error:
                    assert error.code in (401, 403), error.code
                else:
                    raise AssertionError("An unauthenticated or invalid-key MCP request succeeded")
            initialized, response_headers = request("/mcp", {
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                           "clientInfo": {"name": "actions-container-smoke", "version": "1"}},
            }, headers=mcp_headers)
            assert "result" in initialized, initialized
            mcp_headers["MCP-Protocol-Version"] = initialized["result"]["protocolVersion"]
            if response_headers.get("Mcp-Session-Id"):
                mcp_headers["Mcp-Session-Id"] = response_headers["Mcp-Session-Id"]
            request("/mcp", {"jsonrpc": "2.0", "method": "notifications/initialized"}, headers=mcp_headers)
            listed, _ = request("/mcp", {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}, headers=mcp_headers)
            assert any(tool["name"] == "container_echo" for tool in listed["result"]["tools"]), listed
            called, _ = request("/mcp", {
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "container_echo", "arguments": {"message": "mcp"}},
            }, headers=mcp_headers)
            assert not called["result"].get("isError"), called
            assert any("container:mcp" in block.get("text", "") for block in called["result"]["content"]), called
            print("MCP v2 initialization, tools/list, and tools/call passed.", flush=True)

            base_url = start()
            recovered_run, _ = request("/api/runs/" + run_id)
            assert recovered_run["result"] == retained_run["result"]
            result, _ = request(action_path, {"message": "restart"})
            assert result == "container:restart", result
            print("Container recreation retained the run and callable action.", flush=True)

            action_file.write_text(original_actions.replace('f"container:{message}"', 'f"updated:{message}"'))
            print("Rebuilding changed and removed actions against the existing volume...", flush=True)
            compose("build")
            base_url = start()
            schema, _ = request("/openapi.json")
            assert retired_path not in schema["paths"], "Removed action remained advertised"
            # The community runtime serves stateless Streamable HTTP MCP.
            fresh_mcp_headers = {
                "Accept": "application/json, text/event-stream",
                "MCP-Protocol-Version": mcp_headers["MCP-Protocol-Version"],
            }
            listed, _ = request("/mcp", {
                "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {},
            }, headers=fresh_mcp_headers)
            assert all(tool["name"] != "retired_echo" for tool in listed["result"]["tools"]), listed
            called, _ = request("/mcp", {
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": {"name": "container_echo", "arguments": {"message": "update-mcp"}},
            }, headers=fresh_mcp_headers)
            assert not called["result"].get("isError"), called
            assert any("updated:update-mcp" in block.get("text", "") for block in called["result"]["content"]), called
            result, _ = request(action_path, {"message": "update"})
            assert result == "updated:update", result
            recovered_run, _ = request("/api/runs/" + run_id)
            assert recovered_run["result"] == retained_run["result"]
            print("Image update refreshed the catalog and retained run history. PASS", flush=True)
        finally:
            try:
                compose("logs", "--no-color")
            finally:
                # Only this randomly named test project's state is disposable.
                compose("down", "--volumes", "--remove-orphans", "--rmi", "local")
                key_file.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
