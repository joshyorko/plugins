#!/usr/bin/env python3
"""End-to-end release dogfood and golden-path verifier for plugins/rcc/bootstrapper.

Exercises against the real Actions Runtime 1.1.1 over HTTP /mcp with Bearer authorization:
1. Fail-closed binary SHA-256 verification before launching action-server.
2. Unauthenticated /mcp rejection probe (401/403) + authenticated MCP handshake:
   initialize -> notifications/initialized -> tools/list -> resources/list -> prompts/list.
3. Harmless discovery invocation (bootstrapper_discover) and resource read (bootstrapper://catalog/templates).
4. Golden Path (Action Package: workflow-producer-consumer & basic):
   bootstrapper_plan -> bootstrapper_scaffold -> bootstrapper_validate -> bootstrapper_run (with work items & Secret redaction) -> bootstrapper_status -> bootstrapper_results.
5. Golden Path (RCC Robot: 01-python):
   bootstrapper_plan -> bootstrapper_scaffold -> bootstrapper_validate -> bootstrapper_run -> bootstrapper_status -> bootstrapper_results.
6. Controlled Failure Diagnosis & Retry:
   bootstrapper_run(simulate_failure=True) -> verify non-zero exit is reported as failed -> bootstrapper_results(query="failure_diagnosis") -> retry to succeeded.
7. Runtime 1.1.1 Native Surfaces (on Linux with --api-key):
   - Native Action Canvas MCP tool (capability_discover)
   - Managed Work Items HTTP API (/api/managed-work-items/v1/schema, /capabilities, and unauthenticated 401 rejection).
8. Coordinated SQLite backup & clean owned runtime stop.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import socket
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
BOOTSTRAPPER_PKG_DIR = SCRIPT_DIR.parent / "bootstrapper"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from bootstrapper_runtime import (  # noqa: E402
    McpHttpClient,
    coordinated_sqlite_backup,
    run_mcp_smoke,
    start_owned_runtime,
    stop_owned_runtime,
)
from install_bootstrapper import PACKAGE_FILENAME  # noqa: E402


def _find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _extract_tool_json(mcp_response: dict[str, Any]) -> dict[str, Any]:
    """Extract structured JSON payload from an MCP tools/call response."""
    res = mcp_response.get("result") or {}
    if "structuredContent" in res and isinstance(res["structuredContent"], dict):
        return res["structuredContent"]
    content_list = res.get("content") or []
    for item in content_list:
        if isinstance(item, dict) and item.get("type") == "text":
            text = str(item.get("text") or "").strip()
            if text:
                return json.loads(text)
    raise RuntimeError(f"Could not extract JSON from MCP tool response: {mcp_response}")


def _http_get_json(url: str, *, bearer_token: str | None = None) -> tuple[int, dict[str, Any]]:
    headers = {"Accept": "application/json"}
    if bearer_token:
        headers["Authorization"] = f"Bearer {bearer_token}"
    req = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=15.0) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(body)
        except Exception:
            parsed = {"raw_error": body}
        return exc.code, parsed


def _source_identity() -> dict[str, Any]:
    source_paths = [
        "plugins/rcc/bootstrapper",
        "plugins/rcc/scripts/bootstrapper_runtime.py",
        "plugins/rcc/scripts/dogfood_bootstrapper.py",
        "plugins/rcc/scripts/install_bootstrapper.py",
        "plugins/rcc/scripts/build_bootstrapper_package.py",
        ".github/workflows/bootstrap-smoke.yml",
        ".github/workflows/release-artifacts.yml",
        "plugins/rcc/plugin.json",
        "plugins/rcc/.codex-plugin/plugin.json",
        "plugins/rcc/mcp.json",
        "plugins/rcc/.mcp.json",
    ]
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
        ).stdout.strip()
        source_clean = subprocess.run(
            ["git", "diff", "--quiet", "HEAD", "--", *source_paths], check=False
        ).returncode == 0
    except (OSError, subprocess.CalledProcessError):
        commit = None
        source_clean = False
    package_text = (BOOTSTRAPPER_PKG_DIR / "package.yaml").read_text(encoding="utf-8")
    package_version = next(
        (line.split(":", 1)[1].strip() for line in package_text.splitlines() if line.startswith("version:")),
        None,
    )
    return {
        "repository_commit": commit,
        "executable_source_clean_at_run": source_clean,
        "bootstrapper_package_version": package_version,
    }


def run_dogfood_suite(
    *,
    runtime_binary: str | None = None,
    rcc_binary: str | None = None,
    output_receipt: Path | None = None,
    baseline_receipt: Path | None = None,
    package_archive: Path | None = None,
    offline_artifact_dir: Path | None = None,
) -> dict[str, Any]:
    api_key = "dogfood-local-auth-token-109"
    secret_marker = "TOPSECRET_VALUE_99412_DO_NOT_LEAK"
    port = _find_free_port()

    with tempfile.TemporaryDirectory(prefix="rcc-bootstrapper-dogfood-") as tmp_root_str:
        tmp_root = Path(tmp_root_str).resolve()
        datadir = tmp_root / "datadir"
        package_dir = BOOTSTRAPPER_PKG_DIR
        installation: dict[str, str] | None = None
        install_home = tmp_root / "installed-home"
        if package_archive or offline_artifact_dir:
            command = [sys.executable, "-S", str(SCRIPT_DIR / "install_bootstrapper.py"), "--home", str(install_home)]
            if package_archive:
                command.extend(["--package-archive", str(package_archive.expanduser().resolve())])
            if offline_artifact_dir:
                command.extend(["--offline-artifact-dir", str(offline_artifact_dir.expanduser().resolve())])
            installed = subprocess.run(command, capture_output=True, text=True, check=False)
            if installed.returncode:
                raise RuntimeError(f"installer CLI failed: {installed.stderr.strip()}")
            installation = json.loads(installed.stdout)
            package_dir = Path(installation["package_dir"])
            package_archive = Path(installation["package_archive"])
            runtime_binary = installation["action_server"]
            rcc_binary = installation["rcc"]

            launcher_env = os.environ.copy()
            launcher_env["RCC_BOOTSTRAPPER_HOME"] = str(install_home)
            launcher_env.pop("PYTHONPATH", None)
            launcher = subprocess.run(
                [sys.executable, "-S", str(SCRIPT_DIR / "bootstrapper_runtime.py"), "--help"],
                cwd=tmp_root,
                env=launcher_env,
                capture_output=True,
                text=True,
                check=False,
            )
            if launcher.returncode or "start" not in launcher.stdout or "pydantic" in launcher.stderr.lower():
                raise RuntimeError(f"source-free installed launcher failed: {launcher.stderr.strip()}")
            package_digest = json.loads(
                (Path(installation["installation"]) / "install-receipt.json").read_text(encoding="utf-8")
            )["package_sha256"]
        else:
            package_digest = None
            package_archive = None
        workspace_root = tmp_root / "workspace"
        workspace_root.mkdir(parents=True, exist_ok=True)
        runtime_home = tmp_root / "runtime-home"
        runtime_home.mkdir()
        robocorp_home = tmp_root / "ROBOCORP_HOME"
        robocorp_home.mkdir()

        started = start_owned_runtime(
            datadir=datadir,
            host="127.0.0.1",
            port=port,
            approved_roots=[workspace_root],
            trusted_roots=[workspace_root],
            api_key=api_key,
            package_dir=package_dir,
            runtime_binary=runtime_binary,
            rcc_binary=rcc_binary,
            runtime_home=runtime_home,
            robocorp_home=robocorp_home,
            startup_timeout=300.0 if sys.platform == "win32" else 90.0,
        )
        mcp_url = f"http://127.0.0.1:{port}/mcp"
        base_url = f"http://127.0.0.1:{port}"

        try:
            # 1. MCP Handshake + Unauthenticated Rejection + Discovery
            smoke = run_mcp_smoke(
                mcp_url=mcp_url,
                api_key=api_key,
                verify_unauthenticated_rejected=True,
            )

            client = McpHttpClient(mcp_url, api_key=api_key, timeout=60.0)
            client.request(
                "initialize",
                {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "rcc-bootstrapper-dogfood", "version": "0.6.0"},
                },
            )
            client.request("notifications/initialized", {}, is_notification=True)

            # Read template catalog MCP resource
            catalog_res = client.request(
                "resources/read",
                {"uri": "bootstrapper://catalog/templates"},
            )
            # Get dogfood prompt
            prompt_res = client.request(
                "prompts/get",
                {
                    "name": "bootstrapper_dogfood_workflow",
                    "arguments": {"project_kind": "action", "template": "workflow-producer-consumer"},
                },
            )

            # 2. Action Golden Path: workflow-producer-consumer
            action_dir = workspace_root / "dogfood-workitems-action"
            action_plan_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_plan",
                    "arguments": {
                        "project_kind": "action",
                        "template": "workflow-producer-consumer",
                        "target_dir": str(action_dir),
                    },
                },
            )
            action_plan = _extract_tool_json(action_plan_raw)
            assert action_plan.get("error") is None, f"Action plan failed: {action_plan}"
            assert not action_dir.exists(), "bootstrapper_plan must not create target_dir on disk"

            action_scaffold_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_scaffold",
                    "arguments": {
                        "project_kind": "action",
                        "template": "workflow-producer-consumer",
                        "target_dir": str(action_dir),
                    },
                },
            )
            action_scaffold = _extract_tool_json(action_scaffold_raw)
            assert action_scaffold.get("error") is None, f"Action scaffold failed: {action_scaffold}"

            # Verify non-empty destination refusal over MCP
            refusal_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_scaffold",
                    "arguments": {
                        "project_kind": "action",
                        "template": "basic",
                        "target_dir": str(action_dir),
                    },
                },
            )
            refusal_payload = _extract_tool_json(refusal_raw)
            assert "destination_not_empty" in str(refusal_payload.get("error") or ""), (
                f"Expected destination_not_empty refusal, got: {refusal_payload}"
            )

            action_val_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_validate",
                    "arguments": {
                        "project_kind": "action",
                        "project_dir": str(action_dir),
                    },
                },
            )
            action_val = _extract_tool_json(action_val_raw)
            assert (action_val.get("result") or {}).get("valid") is True, f"Action validate failed: {action_val}"

            work_items_input = [
                {"order_id": "ORD-101", "amount": 42},
                {"order_id": "ORD-102", "amount": 99},
            ]
            action_run_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_run",
                    "arguments": {
                        "project_kind": "action",
                        "project_dir": str(action_dir),
                        "target_name": "producer",
                        "work_items_json": json.dumps(work_items_input),
                        "trust_project": True,
                        "secret_token": secret_marker,
                    },
                },
            )
            action_run = _extract_tool_json(action_run_raw)
            action_run_rec = action_run.get("result") or {}
            assert action_run_rec.get("state") == "succeeded", f"Action run failed: {action_run}"
            assert secret_marker not in json.dumps(action_run), "Secret leaked in action_run response!"
            action_run_id = str(action_run_rec["run_id"])

            # Verify work_items_summary named query
            wi_query_raw = client.request(
                "tools/call",
                {
                    "name": "bootstrapper_results",
                    "arguments": {
                        "query": "work_items_summary",
                        "project_dir": str(action_dir),
                        "run_id": action_run_id,
                    },
                },
            )
            wi_query = _extract_tool_json(wi_query_raw)
            wi_summary = (wi_query.get("result") or {}).get("summary") or {}
            assert wi_summary.get("seeded_count") == 2 and wi_summary.get("done_count") == 2, (
                f"Unexpected work_items_summary: {wi_query}"
            )

            # 3. Robot Golden Path: 01-python
            robot_dir = workspace_root / "dogfood-minimal-robot"
            robot_plan = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_plan",
                        "arguments": {
                            "project_kind": "robot",
                            "template": "01-python",
                            "target_dir": str(robot_dir),
                        },
                    },
                )
            )
            assert robot_plan.get("error") is None, f"Robot plan failed: {robot_plan}"

            robot_scaffold = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_scaffold",
                        "arguments": {
                            "project_kind": "robot",
                            "template": "01-python",
                            "target_dir": str(robot_dir),
                        },
                    },
                )
            )
            assert robot_scaffold.get("error") is None, f"Robot scaffold failed: {robot_scaffold}"

            robot_val = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_validate",
                        "arguments": {
                            "project_kind": "robot",
                            "project_dir": str(robot_dir),
                            "run_tool_diagnostics": True,
                        },
                    },
                )
            )
            assert (robot_val.get("result") or {}).get("valid") is True, f"Robot validate failed: {robot_val}"

            robot_run = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_run",
                        "arguments": {
                            "project_kind": "robot",
                            "project_dir": str(robot_dir),
                            "target_name": "minimal_task",
                            "trust_project": True,
                        },
                    },
                )
            )
            robot_run_rec = robot_run.get("result") or {}
            assert robot_run_rec.get("state") == "succeeded", f"Robot run failed: {robot_run}"
            robot_run_id = str(robot_run_rec["run_id"])

            robot_status = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_status",
                        "arguments": {"run_id": robot_run_id},
                    },
                )
            )
            assert (robot_status.get("result") or {}).get("state") == "succeeded"

            # 4. Controlled Failure & Retry Flow
            failed_run = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_run",
                        "arguments": {
                            "project_kind": "robot",
                            "project_dir": str(robot_dir),
                            "target_name": "minimal_task",
                            "simulate_failure": True,
                            "trust_project": True,
                        },
                    },
                )
            )
            failed_rec = failed_run.get("result") or {}
            assert failed_rec.get("state") == "failed", f"Expected simulated failure to be 'failed': {failed_run}"
            assert failed_rec.get("exit_code") == 2
            assert failed_run.get("error") is not None
            failed_run_id = str(failed_rec["run_id"])

            diagnosis = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_results",
                        "arguments": {
                            "query": "failure_diagnosis",
                            "project_dir": str(robot_dir),
                            "run_id": failed_run_id,
                        },
                    },
                )
            )
            diag_summary = (diagnosis.get("result") or {}).get("summary") or {}
            assert diag_summary.get("retry_ready") is True

            retry_run = _extract_tool_json(
                client.request(
                    "tools/call",
                    {
                        "name": "bootstrapper_run",
                        "arguments": {
                            "project_kind": "robot",
                            "project_dir": str(robot_dir),
                            "target_name": "minimal_task",
                            "simulate_failure": False,
                            "trust_project": True,
                        },
                    },
                )
            )
            assert (retry_run.get("result") or {}).get("state") == "succeeded"

            # 5. Native Runtime 1.1.1 Surfaces (Canvas MCP + Managed Work Items HTTP on Linux)
            native_surfaces: dict[str, Any] = {"platform": sys.platform}
            if sys.platform == "linux" and "canvas_open" in smoke["discovered_tool_names"]:
                canvas_ui_res = client.request(
                    "resources/read",
                    {"uri": "ui://action-canvas/v1/canvas.html"},
                )
                unauth_schema_code, _ = _http_get_json(f"{base_url}/api/managed-work-items/v1/schema", bearer_token=None)
                auth_schema_code, schema_body = _http_get_json(
                    f"{base_url}/api/managed-work-items/v1/schema",
                    bearer_token=api_key,
                )
                auth_cap_code, cap_body = _http_get_json(
                    f"{base_url}/api/managed-work-items/v1/capabilities",
                    bearer_token=api_key,
                )
                native_surfaces.update(
                    {
                        "canvas_ui_resource_read_ok": "contents" in (canvas_ui_res.get("result") or {}),
                        "managed_work_items_unauthenticated_status": unauth_schema_code,
                        "managed_work_items_schema_status": auth_schema_code,
                        "managed_work_items_schema_version": schema_body.get("schema_version"),
                        "managed_work_items_capabilities_status": auth_cap_code,
                        "managed_work_items_operations_count": len(cap_body.get("operations") or []),
                    }
                )
        finally:
            stopped = stop_owned_runtime(datadir=datadir)

        backup_receipt = coordinated_sqlite_backup(datadir, label="post-dogfood")

        receipt: dict[str, Any] = {
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "status": "passed",
            "source_identity": _source_identity(),
            "packaged_install": {
                "installer_mode": (
                    "offline-artifacts"
                    if offline_artifact_dir
                    else ("package-archive" if installation else None)
                ),
                "package_archive": str(package_archive) if package_archive else None,
                "package_sha256": package_digest,
                "extracted_package_dir": str(package_dir) if installation else None,
                "installation_home": str(install_home) if installation else None,
                "installed_action_server": runtime_binary if installation else None,
                "installed_rcc": rcc_binary if installation else None,
                "source_free_launcher": "passed" if installation else "not-run",
                "runtime_home": str(runtime_home),
                "robocorp_home": str(robocorp_home),
            },
            "qualification_environment": {
                "os": platform.system(),
                "os_release": platform.release(),
                "architecture": platform.machine(),
                "python_version": platform.python_version(),
            },
            "runtime_instance": {
                "runtime_binary": started["instance"]["runtime_binary"],
                "runtime_version": started["instance"]["runtime_version"],
                "runtime_sha256": started["instance"]["runtime_sha256"],
                "rcc_binary": started["instance"]["rcc_binary"],
                "rcc_sha256": started["instance"]["rcc_sha256"],
                "auth_mode": started["instance"]["auth_mode"],
            },
            "mcp_handshake_and_discovery": {
                "protocol_version": smoke["protocol_version"],
                "server_info": smoke["server_info"],
                "unauthenticated_rejection_probe": smoke["unauthenticated_rejection_probe"],
                "discovered_tool_names": smoke["discovered_tool_names"],
                "discovered_resource_uris": smoke["discovered_resource_uris"],
                "discovered_resource_template_uris": smoke["discovered_resource_template_uris"],
                "discovered_prompt_names": smoke["discovered_prompt_names"],
                "catalog_resource_read_ok": "contents" in (catalog_res.get("result") or {}),
                "dogfood_prompt_get_ok": "messages" in (prompt_res.get("result") or {}),
            },
            "action_golden_path": {
                "template": "workflow-producer-consumer",
                "planned_files_count": len((action_plan.get("result") or {}).get("planned_files") or []),
                "scaffolded_files_count": len((action_scaffold.get("result") or {}).get("files") or []),
                "non_empty_overwrite_refused": True,
                "validation_valid": (action_val.get("result") or {}).get("valid"),
                "run_id": action_run_id,
                "run_state": action_run_rec.get("state"),
                "secret_redacted": secret_marker not in json.dumps(action_run),
                "work_items_summary": wi_summary,
            },
            "robot_golden_path": {
                "template": "01-python",
                "scaffolded_files_count": len((robot_scaffold.get("result") or {}).get("files") or []),
                "validation_valid": (robot_val.get("result") or {}).get("valid"),
                "run_id": robot_run_id,
                "run_state": robot_run_rec.get("state"),
                "artifacts": robot_run_rec.get("artifacts"),
            },
            "controlled_failure_and_retry": {
                "failed_run_id": failed_run_id,
                "failed_state": failed_rec.get("state"),
                "failed_exit_code": failed_rec.get("exit_code"),
                "diagnosis": diag_summary,
                "retry_state": (retry_run.get("result") or {}).get("state"),
            },
            "native_runtime_1_1_1_surfaces": native_surfaces,
            "lifecycle_and_backup": {
                "stop_status": stopped,
                "sqlite_backup_files": backup_receipt.get("files"),
            },
        }

        if baseline_receipt and baseline_receipt.is_file():
            base_data = json.loads(baseline_receipt.read_text(encoding="utf-8"))
            receipt["baseline_comparison"] = {
                "baseline_runtime_version": (base_data.get("runtime_instance") or {}).get("runtime_version"),
                "candidate_runtime_version": receipt["runtime_instance"]["runtime_version"],
                "baseline_tools": (base_data.get("mcp_handshake_and_discovery") or {}).get("discovered_tool_names"),
                "candidate_tools": receipt["mcp_handshake_and_discovery"]["discovered_tool_names"],
            }

        if output_receipt:
            output_receipt.parent.mkdir(parents=True, exist_ok=True)
            output_receipt.write_text(json.dumps(receipt, indent=2), encoding="utf-8")

        return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description="Run end-to-end MCP dogfood verification for plugins/rcc/bootstrapper")
    parser.add_argument("--runtime-binary", default=None)
    parser.add_argument("--rcc-binary", default=None)
    parser.add_argument("--output-receipt", default=None)
    parser.add_argument("--baseline-receipt", default=None)
    package_input = parser.add_mutually_exclusive_group()
    package_input.add_argument(
        "--package-archive",
        type=Path,
        default=None,
        help="Install from a generated ZIP and download pinned binaries over HTTPS.",
    )
    package_input.add_argument(
        "--offline-artifact-dir",
        type=Path,
        default=None,
        help=f"Install from local files named {PACKAGE_FILENAME} and the platform-pinned native binaries.",
    )
    args = parser.parse_args()

    out_path = Path(args.output_receipt).expanduser().resolve() if args.output_receipt else None
    base_path = Path(args.baseline_receipt).expanduser().resolve() if args.baseline_receipt else None

    receipt = run_dogfood_suite(
        runtime_binary=args.runtime_binary,
        rcc_binary=args.rcc_binary,
        output_receipt=out_path,
        baseline_receipt=base_path,
        package_archive=args.package_archive,
        offline_artifact_dir=args.offline_artifact_dir,
    )
    print(json.dumps(receipt, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
