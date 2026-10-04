#!/usr/bin/env python3
"""Audit installed native schemas and read-only capabilities. Never starts a turn.

Default: preserve the operator's CODEX_HOME and configuration. --clean-config is
an explicitly weaker protocol test, not authentication/profile execution proof.
The report deliberately omits raw stderr, configuration, skill text and models
other than the exact requested Luna capability.
"""
from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

MAX_FRAME = 1024 * 1024
TIMEOUT = 20
MODEL = "gpt-6-luna"
EFFORTS = ("none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra")
METHODS = ("initialize", "model/list", "skills/list", "thread/start", "thread/resume",
           "thread/read", "thread/list", "thread/loaded/list", "thread/turns/list", "thread/items/list",
           "turn/start", "turn/steer", "turn/interrupt")
SCHEMAS = ("v1/InitializeParams", "v2/ModelListParams", "v2/ModelListResponse",
           "v2/SkillsListParams", "v2/ThreadStartParams", "v2/ThreadStartResponse",
           "v2/ThreadResumeParams", "v2/TurnStartParams", "v2/TurnSteerParams",
           "v2/TurnInterruptParams", "v2/ThreadReadParams", "v2/ThreadListParams",
           "v2/ThreadLoadedListResponse", "v2/ThreadTurnsListParams",
           "v2/TurnCompletedNotification", "v2/ItemCompletedNotification",
           "v2/ThreadStatusChangedNotification", "v2/ThreadItemsListResponse")


def catalog_projection(catalog: dict) -> dict:
    exact = [item for item in catalog.get("data", []) if item.get("model") == MODEL]
    supported = {effort.get("reasoningEffort") for item in exact
                 for effort in item.get("supportedReasoningEfforts", [])}
    return {"exact_luna_available": bool(exact),
            "supported_efforts": [effort for effort in EFFORTS if effort in supported],
            "routing_evidence": "catalog_only"}


def classify_stderr(stderr: bytes) -> str:
    if b"failed to initialize sqlite state runtime" in stderr:
        return "state_runtime_initialization_failed"
    if b"--profile only applies" in stderr:
        return "app_server_profile_flag_unsupported"
    return "native_process_exited_or_transport_failed"


def schema_evidence(directory: Path) -> dict:
    request = json.loads((directory / "ClientRequest.json").read_text())
    available = {method for item in request["oneOf"]
                 for method in item.get("properties", {}).get("method", {}).get("enum", [])}
    schemas = {}
    for name in SCHEMAS:
        raw = (directory / (name + ".json")).read_bytes()
        schema = json.loads(raw)
        schemas[name] = {"sha256": hashlib.sha256(raw).hexdigest(),
                         "required": schema.get("required", []),
                         "properties": sorted(schema.get("properties", {}))}
    turn = json.loads((directory / "v2/TurnStartParams.json").read_text())
    skills = [item for item in turn["definitions"]["UserInput"]["oneOf"]
              if item.get("title") == "SkillUserInput"]
    completed = json.loads((directory / "v2/TurnCompletedNotification.json").read_text())
    event_items = [item for item in completed["definitions"]["ThreadItem"]["oneOf"]
                   if item.get("title") in {"AgentMessageThreadItem", "CollabAgentToolCallThreadItem"}]
    history=json.loads((directory / "v2/ThreadItemsListResponse.json").read_text())
    return {"native_history_item_entry_schema":history["definitions"]["ThreadItemEntry"],
            "native_event_item_schemas": event_items,
            "message_phase_schema": completed["definitions"]["MessagePhase"],
            "methods": {method: method in available for method in METHODS},
            "schemas": schemas, "native_skill_input_schema": skills[0] if skills else None}


async def probe(binary: str, clean: bool, cwd: Path, skill: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="luna-native-audit-") as temporary:
        env = dict(os.environ)
        if clean:
            env["CODEX_HOME"] = temporary
        process = await asyncio.create_subprocess_exec(
            binary, "app-server", "--stdio", stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
            env=env, cwd=cwd, limit=MAX_FRAME)
        stderr = bytearray()

        async def drain_stderr():
            while chunk := await process.stderr.read(4096):
                # Bounded internal classification only; never output raw stderr.
                stderr.extend(chunk[:max(0, 8192 - len(stderr))])

        drain = asyncio.create_task(drain_stderr())
        next_id = 0
        observed_methods = []

        async def request(method, params):
            nonlocal next_id
            # A hard allowlist makes future script edits unable to accidentally
            # turn this capability audit into an inference test.
            if method not in {"initialize", "model/list", "skills/list"}:
                raise ValueError("audit request is not read-only")
            next_id += 1
            observed_methods.append(method)
            process.stdin.write((json.dumps({"id": next_id, "method": method, "params": params}) + "\n").encode())
            await process.stdin.drain()
            while True:
                line = await process.stdout.readline()
                if not line:
                    raise ConnectionError("native stdout closed")
                if len(line) > MAX_FRAME:
                    raise ValueError("native response frame exceeds bound")
                message = json.loads(line)
                if message.get("id") == next_id:
                    if "error" in message:
                        code = message["error"].get("code")
                        raise RuntimeError(f"native_rpc_error_{code if isinstance(code, int) else 'unknown'}")
                    return message["result"]

        result = {"profile_evidence": "not_proven_clean_config" if clean else "inherited_config",
                  "inference_calls": 0, "effective_routing": "unverified"}
        phase = "initialize"
        try:
            await asyncio.wait_for(request("initialize", {"clientInfo": {"name": "luna_factory_audit", "version": "0.2.0"}}), TIMEOUT)
            process.stdin.write(b'{"method":"initialized"}\n')
            await process.stdin.drain()
            result["initialize"] = "passed"
            phase = "model/list"
            cursor = None
            cursors = set()
            models = []
            for _ in range(100):
                params = {"limit": 100, "includeHidden": True}
                if cursor is not None:
                    params["cursor"] = cursor
                page = await asyncio.wait_for(request("model/list", params), TIMEOUT)
                models.extend(page["data"])
                if len(models) > 10_000:
                    raise ValueError("catalog bound exceeded")
                cursor = page.get("nextCursor")
                if cursor is None:
                    break
                if not isinstance(cursor, str) or cursor in cursors:
                    raise ValueError("invalid catalog cursor")
                cursors.add(cursor)
            else:
                raise ValueError("catalog pagination bound exceeded")
            result["catalog"] = catalog_projection({"data": models})
            phase = "skills/list"
            skills = await asyncio.wait_for(request("skills/list", {"cwds": [str(cwd)], "forceReload": False}), TIMEOUT)
            entries = [entry for item in skills.get("data", []) for entry in item.get("skills", [])]
            result["canonical_skill_discovered"] = any(
                entry.get("name") == "luna-factory" and Path(entry.get("path", "")).resolve() == skill
                for entry in entries)
            result["status"] = "passed" if result["catalog"]["exact_luna_available"] else "blocked"
            if result["status"] == "blocked":
                result["blocker"] = "exact_gpt_6_luna_absent"
        except (OSError, ValueError, KeyError, RuntimeError, asyncio.TimeoutError) as error:
            result.update(status="blocked", failed_phase=phase,
                          blocker="request_timeout" if isinstance(error, asyncio.TimeoutError)
                          else "native_rpc_error" if isinstance(error, RuntimeError)
                          else "native_transport_or_schema_error")
        finally:
            if process.stdin:
                process.stdin.close()
            try:
                await asyncio.wait_for(process.wait(), 2)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()
            await drain
            result["process_exit_code"] = process.returncode
            result["read_only_requests"] = observed_methods
            if result.get("status") == "blocked" and process.returncode:
                result["blocker"] = classify_stderr(bytes(stderr))
        return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", default="codex", help="trusted installed Codex executable")
    parser.add_argument("--clean-config", action="store_true", help="isolated protocol test only; does not prove inherited runtime/auth")
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    parser.add_argument("--skill", type=Path, default=Path(__file__).resolve().parents[1] / "skills/luna-factory/SKILL.md")
    parser.add_argument("--output", type=Path, help="write sanitized capability evidence JSON")
    parser.add_argument("--schema-output", type=Path, help="write installed-schema contract digest JSON")
    args = parser.parse_args()
    # CLI/version/schema generation cannot perform a model turn.
    version = subprocess.run([args.codex, "--version"], capture_output=True, text=True, timeout=10, check=True)
    with tempfile.TemporaryDirectory(prefix="luna-native-schemas-") as temporary:
        subprocess.run([args.codex, "app-server", "generate-json-schema", "--out", temporary], capture_output=True, timeout=30, check=True)
        schemas = schema_evidence(Path(temporary))
    report = {"codex_version": version.stdout.strip(), "protocol_generated_from_installed_binary": True,
              "required_methods_present": all(schemas["methods"].values()),
              "canonical_skill_sha256": hashlib.sha256(args.skill.resolve().read_bytes()).hexdigest(),
              "native": asyncio.run(probe(args.codex, args.clean_config, args.cwd.resolve(), args.skill.resolve()))}
    if args.schema_output:
        args.schema_output.write_text(json.dumps(schemas, indent=2, sort_keys=True) + "\n")
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(encoded)
    print(encoded, end="")
    return 0 if report["native"]["status"] == "passed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
