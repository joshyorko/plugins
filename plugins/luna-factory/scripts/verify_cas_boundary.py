#!/usr/bin/env python3
"""Isolated Factory MCP -> real CAS Runtime inspection; no native daemon/worker.

Run using the pinned CAS checkout's test virtualenv (mcp/httpx installed).
Every service, ledger, target file and repository is disposable. No credentials,
existing ledger, production target or native transport are accepted as inputs.
"""

import argparse
import asyncio
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

CAS_HEAD = "bf0b3823e033b9b5abd86904e0a565d6b3586206"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def ready(proc, number, log):
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"Fixture service exited; inspect {log}")
        try:
            with socket.create_connection(("127.0.0.1", number), timeout=0.2):
                return
        except OSError:
            time.sleep(0.1)
    raise RuntimeError(f"Fixture startup timed out; inspect {log}")


async def exercise(number):
    import httpx
    from mcp import ClientSession
    from mcp.client.streamable_http import streamable_http_client

    async with httpx.AsyncClient(trust_env=False, timeout=20) as http:
        async with streamable_http_client(
            f"http://127.0.0.1:{number}/mcp", http_client=http
        ) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                catalog = await session.list_tools()
                tool = next(t for t in catalog.tools if t.name == "inspect_factory_cas")
                assert tool.annotations.read_only_hint
                assert tool.meta["ui"]["visibility"] == ["model", "app"]
                response = await session.call_tool(
                    "inspect_factory_cas", {"target_alias": "isolated"}
                )
                assert not response.is_error, response.model_dump(mode="json")
                result = response.structured_content
                assert result["target"]["status"] == "resolved"
                assert result["target"]["binding_verified"] is False
                assert result["qualification"]["execution_eligible"] is False
                assert result["receipt"] is None and result["thread"] is None
                runs = await session.call_tool("list_factory_runs", {})
                assert runs.structured_content["runs"] == []
                return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cas-source", required=True, type=Path)
    parser.add_argument("--action-server", required=True, type=Path)
    parser.add_argument("--factory", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    source = args.cas_source.resolve(strict=True)
    runtime = args.action_server.resolve(strict=True)
    factory = args.factory.resolve(strict=True)
    assert (
        subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source, text=True
        ).strip()
        == CAS_HEAD
    )
    assert not subprocess.check_output(["git", "diff", "HEAD", "--"], cwd=source)
    processes = []
    logs = []
    with tempfile.TemporaryDirectory(prefix="luna-cas-contract-") as temporary:
        work = Path(temporary)
        cas_port, factory_port = port(), port()
        while factory_port == cas_port:
            factory_port = port()
        repo = work / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "fixture",
            ],
            cwd=repo,
            check=True,
        )
        targets = work / "targets.json"
        targets.write_text(
            json.dumps(
                {
                    "targets": {
                        "local": {
                            "transport": "local",
                            "socket_path": str(work / "absent-native.sock"),
                        }
                    }
                }
            )
        )
        config = work / "factory.json"
        config.write_text(
            json.dumps(
                {
                    "listen": f"127.0.0.1:{factory_port}",
                    "database": str(work / "factory-state" / "runs.sqlite"),
                    "codex_binary": str(work / "absent-codex"),
                    "skill_path": str(root / "skills/luna-factory/SKILL.md"),
                    "repositories": {
                        "fixture": {"root": str(repo), "max_finish": "local_candidate"}
                    },
                    "profiles": {"default": {"effort": "low"}},
                    "limits": {"capacity": 1, "repair_attempts": 0, "wall_seconds": 30},
                    "cas_targets": {
                        "isolated": {
                            "endpoint": f"http://127.0.0.1:{cas_port}",
                            "package": "codex-action-server",
                            "target": "local",
                            "cwd": str(repo),
                        }
                    },
                }
            )
        )
        # Retain PATH only for runtime dependency tools; all identity/state paths
        # are explicit and private, and the target socket does not exist.
        env = {key: os.environ[key] for key in ("PATH", "HOME") if key in os.environ}
        env.update(
            {
                "CODEX_ACTION_TARGETS": str(targets),
                "CODEX_ACTION_RECEIPTS": str(work / "receipts"),
                "CODEX_HOME": str(work / "codex-home"),
                "CODEX_ACTION_DATA": str(work / "cas-state"),
                "CODEX_ACTION_PORT": str(cas_port),
                "CODEX_ACTION_PROFILE": "observe",
                "ACTION_SERVER_BIN": str(runtime),
            }
        )
        try:
            for command, number, name, child_env in [
                (["bash", str(source / "scripts/run.sh")], cas_port, "cas", env),
                (
                    [
                        str(factory),
                        "serve",
                        "--config",
                        str(config),
                        "--ui",
                        str(root / "ui/dist/index.html"),
                    ],
                    factory_port,
                    "factory",
                    env,
                ),
            ]:
                log_path = work / f"{name}.log"
                log = log_path.open("w")
                logs.append(log)
                proc = subprocess.Popen(
                    command,
                    cwd=source,
                    env=child_env,
                    stdout=log,
                    stderr=log,
                    start_new_session=True,
                )
                processes.append(proc)
                ready(proc, number, log_path)
            result = asyncio.run(exercise(factory_port))
            print(
                json.dumps(
                    {
                        "cas_head": CAS_HEAD,
                        "factory_sha256": hashlib.sha256(
                            factory.read_bytes()
                        ).hexdigest(),
                        "runtime_sha256": hashlib.sha256(
                            runtime.read_bytes()
                        ).hexdigest(),
                        "acceptance": "real_factory_mcp_to_real_cas_http_inspection",
                        "native_dispatches": 0,
                        "ledger": "new_empty_disposable",
                        "host": "python_mcp_client_not_chatgpt",
                        "result": result,
                    },
                    indent=2,
                )
            )
        finally:
            for proc in reversed(processes):
                if proc.poll() is None:
                    os.killpg(proc.pid, signal.SIGTERM)
                    try:
                        proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait(timeout=10)
            for log in logs:
                log.close()


if __name__ == "__main__":
    main()
