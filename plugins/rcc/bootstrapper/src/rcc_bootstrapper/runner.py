"""Process-group execution, run journal, crash reconciliation, and bounded named queries."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .models import (
    ArtifactEntry,
    CancelResult,
    NamedQueryResult,
    ProjectKind,
    QueryName,
    RunRecord,
    WorkItemBatchSummary,
)
from .policy import (
    PolicyViolationError,
    default_datadir,
    redact_secrets,
    resolve_approved_target,
    sha256_file,
    validate_safe_identifier,
    validate_secret_env_map,
    is_trusted_project,
    verify_binary_before_execution,
)
from .scaffold import validate_project

_ACTIVE_PROCS: dict[str, tuple[subprocess.Popen[str], list[str]]] = {}
_RUN_STATE_LOCKS: dict[str, threading.Lock] = {}
_RUN_STATE_LOCKS_GUARD = threading.Lock()


def _run_state_lock(run_id: str) -> threading.Lock:
    with _RUN_STATE_LOCKS_GUARD:
        return _RUN_STATE_LOCKS.setdefault(run_id, threading.Lock())


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _runs_dir(datadir: Path | None = None) -> Path:
    base = datadir or default_datadir()
    d = base / "bootstrapper_runs"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _run_file(run_id: str, datadir: Path | None = None) -> Path:
    clean_id = validate_safe_identifier(run_id, field_name="run_id")
    return _runs_dir(datadir) / f"{clean_id}.json"


def save_run_record(record: RunRecord, datadir: Path | None = None) -> None:
    """Atomically persist a RunRecord JSON file."""
    path = _run_file(record.run_id, datadir)
    tmp_path = path.with_suffix(f".tmp-{uuid.uuid4().hex[:6]}")
    tmp_path.write_text(record.model_dump_json(indent=2), encoding="utf-8")
    os.replace(tmp_path, path)


def _is_pid_alive(pid: int | None) -> bool:
    if not pid or pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False


def load_run_record(run_id: str, datadir: Path | None = None) -> RunRecord:
    """Load a RunRecord and reconcile its state if a prior process crashed or restarted."""
    path = _run_file(run_id, datadir)
    if not path.is_file():
        raise PolicyViolationError("run_not_found", f"Run '{run_id}' does not exist.")
    record = RunRecord.model_validate_json(path.read_text(encoding="utf-8"))

    if record.state == "running" and record.run_id not in _ACTIVE_PROCS:
        # Process is not owned by the current live Python worker.
        # Do NOT falsely claim it succeeded or cleanly stopped; mark unknown_retained.
        alive = _is_pid_alive(record.pid)
        note = (
            f"Run was recorded as 'running' (pid={record.pid}) by a previous worker session "
            f"(pid_currently_alive={alive}); final outcome is unknown and retained as 'unknown_retained'."
        )
        record = record.model_copy(
            update={
                "state": "unknown_retained",
                "completed_at": record.completed_at or _now_iso(),
                "error_code": record.error_code or "worker_restarted_state_unknown",
                "error_message": record.error_message or note,
                "reconciled_note": note,
            }
        )
        save_run_record(record, datadir)

    return record


def list_run_records(
    *,
    project_dir: str | Path | None = None,
    limit: int = 20,
    datadir: Path | None = None,
) -> list[RunRecord]:
    """Return recent RunRecords (newest first), reconciling stale running records."""
    rdir = _runs_dir(datadir)
    records: list[RunRecord] = []
    canonical_filter = str(Path(project_dir).resolve()) if project_dir else None

    files = sorted(rdir.glob("*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
    for fpath in files:
        run_id = fpath.stem
        try:
            rec = load_run_record(run_id, datadir)
        except Exception:
            continue
        if canonical_filter and rec.project_dir != canonical_filter:
            continue
        records.append(rec)
        if len(records) >= max(1, min(limit, 100)):
            break
    return records


def _collect_artifacts(project_dir: Path) -> list[ArtifactEntry]:
    """Scan output/ directory inside project_dir for generated artifacts."""
    out_dir = project_dir / "output"
    if not out_dir.is_dir():
        return []
    artifacts: list[ArtifactEntry] = []
    for file_path in sorted(out_dir.rglob("*")):
        if file_path.is_file():
            rel = file_path.relative_to(project_dir).as_posix()
            artifacts.append(
                ArtifactEntry(
                    relative_path=rel,
                    size_bytes=file_path.stat().st_size,
                    sha256=sha256_file(file_path),
                )
            )
    return artifacts


def _terminate_process_group(proc: subprocess.Popen[str]) -> bool:
    """Terminate the entire process group owned by proc and return True if termination is confirmed."""
    if proc.poll() is not None:
        return True

    try:
        if sys.platform == "win32":
            proc.terminate()
        else:
            pgid = os.getpgid(proc.pid)
            os.killpg(pgid, signal.SIGTERM)
    except ProcessLookupError:
        return True
    except Exception:
        try:
            proc.terminate()
        except Exception:
            pass

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return True
        time.sleep(0.05)

    try:
        if sys.platform == "win32":
            proc.kill()
        else:
            pgid = os.getpgid(proc.pid)
            os.killpg(pgid, signal.SIGKILL)
    except ProcessLookupError:
        return True
    except Exception:
        try:
            proc.kill()
        except Exception:
            pass

    try:
        proc.wait(timeout=2.0)
        return True
    except Exception:
        return proc.poll() is not None


def _build_local_runner_script() -> str:
    """Python runner script executed in a child process group for local_python / work-items execution."""
    return r"""
import importlib.util
import json
import os
import sys
import traceback
from pathlib import Path

def main():
    cfg_path = Path(os.environ["RCC_BOOTSTRAPPER_RUN_CONFIG"])
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))
    project_kind = cfg["project_kind"]
    project_dir = Path(cfg["project_dir"]).resolve()
    target_name = cfg["target_name"]
    inputs = cfg.get("inputs") or {}
    work_items_payload = cfg.get("work_items") or []
    simulate_failure = bool(cfg.get("simulate_failure", False))

    os.chdir(project_dir)
    if str(project_dir) not in sys.path:
        sys.path.insert(0, str(project_dir))
    src_dir = project_dir / "src"
    if src_dir.is_dir() and str(src_dir) not in sys.path:
        sys.path.insert(0, str(src_dir))

    output_dir = project_dir / "output"
    output_dir.mkdir(parents=True, exist_ok=True)
    os.environ["ROBOT_ARTIFACTS"] = str(output_dir)

    if simulate_failure:
        fail_artifact = output_dir / "failure_diagnostic.json"
        fail_artifact.write_text(
            json.dumps(
                {
                    "status": "simulated_failure",
                    "target_name": target_name,
                    "remediation": "Re-run with simulate_failure=False after inspecting failure_diagnosis.",
                },
                indent=2,
            ),
            encoding="utf-8",
        )
        print(f"Simulated failure triggered for target '{target_name}'.", file=sys.stderr)
        sys.exit(2)

    work_items_summary = None
    if work_items_payload:
        from actions import workitems
        from actions.work_items import State, get_context, init, seed_input

        db_path = output_dir / "workitems.db"
        os.environ["RC_WORKITEM_DB_PATH"] = str(db_path)
        os.environ["ACTION_SERVER_DATADIR"] = str(output_dir)
        init()
        seeded_ids = [seed_input(payload=item) for item in work_items_payload]

        processed_outputs = []
        for item in workitems.inputs:
            with item:
                out_payload = {
                    "source_id": item.id,
                    "processed_target": target_name,
                    "input": item.payload,
                    "status": "processed",
                }
                out_item = workitems.outputs.create(payload=out_payload)
                processed_outputs.append(out_item.payload)

        ctx = get_context()
        done_items = ctx.adapter.list_items(state=State.DONE)
        failed_items = ctx.adapter.list_items(state=State.FAILED)
        work_items_summary = {
            "seeded_count": len(seeded_ids),
            "processed_count": len(processed_outputs),
            "done_count": len(done_items),
            "failed_count": len(failed_items),
            "outputs_count": len(processed_outputs),
            "db_path": str(db_path),
            "sample_outputs": processed_outputs[:5],
        }
        (output_dir / "work_items_report.json").write_text(
            json.dumps(work_items_summary, indent=2),
            encoding="utf-8",
        )

    invoked_result = None
    if project_kind == "action":
        # Find matching @action or @tool function across *.py modules
        py_files = [
            p for p in sorted(project_dir.rglob("*.py"))
            if "tests" not in p.parts and ".venv" not in p.parts and "venv" not in p.parts
        ]
        target_fn = None
        for py_file in py_files:
            spec = importlib.util.spec_from_file_location(py_file.stem, py_file)
            if spec and spec.loader:
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                if hasattr(mod, target_name):
                    target_fn = getattr(mod, target_name)
                    break
                if target_name == "default":
                    for attr_name in dir(mod):
                        attr = getattr(mod, attr_name)
                        if callable(attr) and not attr_name.startswith("_") and getattr(attr, "__module__", "") == mod.__name__:
                            target_fn = attr
                            target_name = attr_name
                            break
                    if target_fn:
                        break
        if target_fn is not None and not work_items_payload:
            res = target_fn(**inputs)
            if hasattr(res, "model_dump"):
                invoked_result = res.model_dump()
            else:
                invoked_result = res
    else:
        tasks_py = project_dir / "tasks.py"
        if tasks_py.is_file():
            try:
                import robocorp.tasks  # noqa: F401
            except ImportError:
                import types

                robocorp_mod = types.ModuleType("robocorp")
                tasks_mod = types.ModuleType("robocorp.tasks")
                tasks_mod.task = lambda fn=None, **_kw: (fn if fn is not None else (lambda f: f))  # type: ignore[attr-defined]
                tasks_mod.get_output_dir = lambda: str(output_dir)  # type: ignore[attr-defined]
                robocorp_mod.tasks = tasks_mod  # type: ignore[attr-defined]
                sys.modules.setdefault("robocorp", robocorp_mod)
                sys.modules.setdefault("robocorp.tasks", tasks_mod)

            spec = importlib.util.spec_from_file_location("tasks", tasks_py)
            if spec and spec.loader:
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                fn = getattr(mod, target_name, None)
                if fn is None:
                    for candidate_name in ("minimal_task", "uv_native_smoke"):
                        if hasattr(mod, candidate_name):
                            fn = getattr(mod, candidate_name)
                            break
                if callable(fn):
                    res = fn(**inputs) if inputs else fn()
                    invoked_result = str(res) if res is not None else "completed"

    run_summary_file = output_dir / "run_result.json"
    run_summary_file.write_text(
        json.dumps(
            {
                "project_kind": project_kind,
                "target_name": target_name,
                "result": invoked_result,
                "work_items": work_items_summary,
            },
            indent=2,
            default=str,
        ),
        encoding="utf-8",
    )
    print(json.dumps({"status": "ok", "target": target_name, "artifact": str(run_summary_file)}))

if __name__ == "__main__":
    main()
"""


def execute_project_run(
    project_kind: ProjectKind,
    project_dir: str | Path,
    *,
    target_name: str = "default",
    inputs: dict[str, Any] | None = None,
    work_items: list[dict[str, Any]] | None = None,
    secrets: dict[str, str] | None = None,
    execution_mode: str = "local_python",
    timeout_seconds: float = 60.0,
    simulate_failure: bool = False,
    trust_project: bool = False,
    approved_roots: list[str | Path] | None = None,
    datadir: Path | None = None,
    rcc_bin: str | Path | None = None,
) -> RunRecord:
    """Execute an approved Action or Robot workload with process-group ownership and secret redaction."""
    if not trust_project or not is_trusted_project(project_dir):
        raise PolicyViolationError(
            "untrusted_project_execution_refused",
            "Execution requires explicit per-run trust and a project inside operator-configured trusted roots.",
        )

    canonical_dir, _ = resolve_approved_target(
        project_dir,
        approved_roots=approved_roots,
        allow_root_itself=False,
    )
    clean_target = validate_safe_identifier(target_name, field_name="target_name")
    validated_secrets = validate_secret_env_map(secrets)
    secret_values = list(validated_secrets.values())

    # Validate project manifest and syntax before running
    validation = validate_project(
        project_kind,
        canonical_dir,
        approved_roots=approved_roots,
    )
    if not validation.valid:
        first_err = validation.issues[0]
        raise PolicyViolationError(
            f"validation_failed_{first_err.code}",
            f"Project failed validation before run: {first_err.message}",
        )

    if timeout_seconds <= 0 or timeout_seconds > 600:
        raise PolicyViolationError(
            "invalid_timeout",
            "timeout_seconds must be between 0.1 and 600 seconds.",
        )

    if execution_mode not in {"local_python", "rcc_cli"}:
        raise PolicyViolationError(
            "invalid_execution_mode",
            "execution_mode must be 'local_python' or 'rcc_cli'.",
        )
    if execution_mode == "rcc_cli":
        if project_kind != "robot":
            raise PolicyViolationError(
                "rcc_cli_project_kind_unsupported",
                "rcc_cli executes Robot tasks only; select local_python explicitly for Actions.",
            )
        if inputs:
            raise PolicyViolationError(
                "rcc_cli_inputs_unsupported",
                "rcc_cli does not forward keyword inputs; use the Robot's declared input interface or select local_python explicitly.",
            )
        if work_items:
            raise PolicyViolationError(
                "rcc_cli_work_items_unsupported",
                "rcc_cli does not seed work items; select local_python explicitly for bootstrapper-managed work items.",
            )
        if simulate_failure:
            raise PolicyViolationError(
                "rcc_cli_simulation_unsupported",
                "rcc_cli does not simulate failures; select local_python explicitly for the failure control.",
            )

    run_id = f"run-{uuid.uuid4().hex[:12]}"
    run_lock = _run_state_lock(run_id)
    started_at = _now_iso()
    start_monotonic = time.monotonic()

    rdir = _runs_dir(datadir)
    config_file = rdir / f".cfg-{run_id}.json"

    env = os.environ.copy()
    # Inject secrets via environment variables only (never on CLI argv)
    for sec_key, sec_val in validated_secrets.items():
        env[sec_key] = sec_val

    if execution_mode == "rcc_cli":
        verified_rcc, _ = verify_binary_before_execution("rcc", rcc_bin)
        robot_yaml = canonical_dir / "robot.yaml"
        task_arg = clean_target if clean_target != "default" else (validation.entrypoints[0] if validation.entrypoints else "RunTask")
        cmd = [str(verified_rcc), "run", "-r", str(robot_yaml), "--task", task_arg]
        cmd_summary = ["rcc", "run", "-r", str(robot_yaml), "--task", task_arg]
    else:
        config_payload = {
            "project_kind": project_kind,
            "project_dir": str(canonical_dir),
            "target_name": clean_target,
            "inputs": inputs or {},
            "work_items": work_items or [],
            "simulate_failure": simulate_failure,
        }
        config_file.write_text(json.dumps(config_payload), encoding="utf-8")
        try:
            os.chmod(config_file, 0o600)
        except OSError:
            pass
        env["RCC_BOOTSTRAPPER_RUN_CONFIG"] = str(config_file)
        cmd = [sys.executable, "-c", _build_local_runner_script()]
        cmd_summary = ["python", "-m", "rcc_bootstrapper.runner", project_kind, clean_target]

    popen_kwargs: dict[str, Any] = {
        "cwd": str(canonical_dir),
        "env": env,
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
        "text": True,
    }
    if sys.platform == "win32":
        popen_kwargs["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    else:
        popen_kwargs["start_new_session"] = True

    proc = subprocess.Popen(cmd, **popen_kwargs)
    pgid = proc.pid if sys.platform != "win32" else None

    initial_record = RunRecord(
        run_id=run_id,
        project_kind=project_kind,
        project_dir=str(canonical_dir),
        target_name=clean_target,
        state="running",
        pid=proc.pid,
        pgid=pgid,
        started_at=started_at,
        command_summary=cmd_summary,
    )
    save_run_record(initial_record, datadir)
    _ACTIVE_PROCS[run_id] = (proc, secret_values)

    timed_out = False
    try:
        stdout_raw, stderr_raw = proc.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        timed_out = True
        _terminate_process_group(proc)
        try:
            stdout_raw, stderr_raw = proc.communicate(timeout=2.0)
        except Exception:
            stdout_raw, stderr_raw = "", ""
    finally:
        if config_file.exists():
            config_file.unlink(missing_ok=True)

    duration_ms = int((time.monotonic() - start_monotonic) * 1000)
    completed_at = _now_iso()
    exit_code = proc.returncode

    stdout_redacted = redact_secrets((stdout_raw or "")[-4096:], secret_values)
    stderr_redacted = redact_secrets((stderr_raw or "")[-4096:], secret_values)

    # Check if cancelled concurrently while running
    persisted_now = _run_file(run_id, datadir)
    was_cancelled = False
    if persisted_now.is_file():
        try:
            disk_state = json.loads(persisted_now.read_text(encoding="utf-8")).get("state")
            if disk_state == "cancelled":
                was_cancelled = True
        except Exception:
            pass

    if was_cancelled:
        final_state = "cancelled"
        err_code = "run_cancelled"
        err_msg = "Run was cancelled by operator request."
    elif timed_out:
        final_state = "timed_out"
        err_code = "run_timed_out"
        err_msg = f"Run exceeded timeout of {timeout_seconds}s and its process group was terminated."
    elif exit_code == 0:
        final_state = "succeeded"
        err_code = None
        err_msg = None
    else:
        final_state = "failed"
        err_code = f"nonzero_exit_{exit_code}"
        first_err_line = (stderr_redacted.strip().splitlines() or ["Process exited with non-zero status."])[-1]
        err_msg = f"Command exited with code {exit_code}: {first_err_line}"

    artifacts = _collect_artifacts(canonical_dir)
    wi_summary: WorkItemBatchSummary | None = None
    wi_report_path = canonical_dir / "output" / "work_items_report.json"
    if wi_report_path.is_file():
        try:
            wi_summary = WorkItemBatchSummary.model_validate_json(wi_report_path.read_text(encoding="utf-8"))
        except Exception:
            wi_summary = None

    final_record = RunRecord(
        run_id=run_id,
        project_kind=project_kind,
        project_dir=str(canonical_dir),
        target_name=clean_target,
        state=final_state,
        exit_code=exit_code,
        pid=proc.pid,
        pgid=pgid,
        started_at=started_at,
        completed_at=completed_at,
        duration_ms=duration_ms,
        command_summary=cmd_summary,
        stdout_tail=stdout_redacted,
        stderr_tail=stderr_redacted,
        error_code=err_code,
        error_message=err_msg,
        secrets_redacted=True,
        artifacts=artifacts,
        work_items=wi_summary,
    )
    with run_lock:
        persisted_now = _run_file(run_id, datadir)
        if persisted_now.is_file():
            try:
                if json.loads(persisted_now.read_text(encoding="utf-8")).get("state") == "cancelled":
                    final_record = final_record.model_copy(
                        update={
                            "state": "cancelled",
                            "error_code": "run_cancelled",
                            "error_message": "Run was cancelled by operator request.",
                        }
                    )
            except Exception:
                pass
        save_run_record(final_record, datadir)
        _ACTIVE_PROCS.pop(run_id, None)
    return final_record


def cancel_project_run(run_id: str, *, datadir: Path | None = None) -> CancelResult:
    """Cancel an active owned run or report crash-reconciled state."""
    path = _run_file(run_id, datadir)
    if not path.is_file():
        raise PolicyViolationError("run_not_found", f"Run '{run_id}' does not exist.")
    with _run_state_lock(run_id):
        record = RunRecord.model_validate_json(path.read_text(encoding="utf-8"))
        prev_state = record.state

        active_entry = _ACTIVE_PROCS.get(record.run_id)
        if active_entry is not None:
            proc, _ = active_entry
            confirmed = _terminate_process_group(proc)
            new_state = "cancelled" if confirmed else "unknown_retained"
            updated = record.model_copy(
                update={
                    "state": new_state,
                    "completed_at": _now_iso(),
                    "error_code": "run_cancelled",
                    "error_message": "Run cancelled via bootstrapper_cancel.",
                }
            )
            save_run_record(updated, datadir)
            return CancelResult(
                run_id=record.run_id,
                previous_state=prev_state,
                state=new_state,
                process_group_signaled=True,
                termination_confirmed=confirmed,
                message="Process group terminated and confirmed." if confirmed else "Signal sent, termination unconfirmed.",
            )

        # Not in _ACTIVE_PROCS: reconcile if it was marked running
        reconciled = load_run_record(run_id, datadir)
        return CancelResult(
            run_id=reconciled.run_id,
            previous_state=prev_state,
            state=reconciled.state,
            process_group_signaled=False,
            termination_confirmed=reconciled.state in {"succeeded", "failed", "cancelled", "timed_out"},
            message=(
                f"Run is already in terminal/reconciled state '{reconciled.state}'."
                if reconciled.state != "unknown_retained"
                else "Run belonged to a prior worker session; marked 'unknown_retained' without signaling unrelated PIDs."
            ),
        )


def execute_named_query(
    query: QueryName,
    *,
    project_dir: str | Path | None = None,
    run_id: str | Path | None = None,
    approved_roots: list[str | Path] | None = None,
    datadir: Path | None = None,
) -> NamedQueryResult:
    """Execute one of the 4 bounded named queries (project_summary, run_summary, work_items_summary, failure_diagnosis)."""
    canonical_dir: Path | None = None
    if project_dir:
        canonical_dir, _ = resolve_approved_target(
            project_dir,
            approved_roots=approved_roots,
            allow_root_itself=False,
        )

    record: RunRecord | None = None
    if run_id:
        record = load_run_record(str(run_id), datadir)
        if canonical_dir is None:
            canonical_dir = Path(record.project_dir)

    if query == "project_summary":
        if canonical_dir is None or not canonical_dir.is_dir():
            raise PolicyViolationError("project_dir_required", "project_summary requires an existing approved project_dir.")
        kind: ProjectKind = "action" if (canonical_dir / "package.yaml").is_file() else "robot"
        val = validate_project(kind, canonical_dir, approved_roots=approved_roots)
        recent_runs = list_run_records(project_dir=canonical_dir, limit=5, datadir=datadir)
        artifacts = _collect_artifacts(canonical_dir)
        return NamedQueryResult(
            query=query,
            project_dir=str(canonical_dir),
            summary={
                "project_kind": kind,
                "valid": val.valid,
                "manifest_name": val.manifest_name,
                "entrypoints": val.entrypoints,
                "issues": [i.model_dump() for i in val.issues],
                "recent_runs": [
                    {
                        "run_id": r.run_id,
                        "state": r.state,
                        "target_name": r.target_name,
                        "exit_code": r.exit_code,
                    }
                    for r in recent_runs
                ],
            },
            artifacts=artifacts,
        )

    if query == "run_summary":
        if record is None:
            recent = list_run_records(project_dir=canonical_dir, limit=1, datadir=datadir)
            if not recent:
                raise PolicyViolationError("run_not_found", "No runs found for run_summary query.")
            record = recent[0]
        return NamedQueryResult(
            query=query,
            project_dir=record.project_dir,
            run_id=record.run_id,
            summary={
                "run_id": record.run_id,
                "project_kind": record.project_kind,
                "target_name": record.target_name,
                "state": record.state,
                "exit_code": record.exit_code,
                "duration_ms": record.duration_ms,
                "error_code": record.error_code,
                "error_message": record.error_message,
                "reconciled_note": record.reconciled_note,
            },
            redacted_logs={
                "stdout": record.stdout_tail,
                "stderr": record.stderr_tail,
            },
            artifacts=record.artifacts,
        )

    if query == "work_items_summary":
        if record is None and canonical_dir is not None:
            recent = list_run_records(project_dir=canonical_dir, limit=1, datadir=datadir)
            record = recent[0] if recent else None
        wi_data: dict[str, Any] = {}
        if record and record.work_items:
            wi_data = record.work_items.model_dump()
        elif canonical_dir and (canonical_dir / "output" / "work_items_report.json").is_file():
            wi_data = json.loads((canonical_dir / "output" / "work_items_report.json").read_text(encoding="utf-8"))
        else:
            wi_data = {
                "seeded_count": 0,
                "processed_count": 0,
                "done_count": 0,
                "failed_count": 0,
                "outputs_count": 0,
            }
        return NamedQueryResult(
            query=query,
            project_dir=str(canonical_dir) if canonical_dir else (record.project_dir if record else None),
            run_id=record.run_id if record else None,
            summary=wi_data,
            artifacts=record.artifacts if record else (_collect_artifacts(canonical_dir) if canonical_dir else []),
        )

    if query == "failure_diagnosis":
        if record is None:
            recent = list_run_records(project_dir=canonical_dir, limit=5, datadir=datadir)
            failed_runs = [r for r in recent if r.state in {"failed", "timed_out", "unknown_retained"}]
            if not failed_runs:
                raise PolicyViolationError("no_failed_run", "No failed, timed_out, or unknown_retained run found to diagnose.")
            record = failed_runs[0]

        remediation = "Inspect stderr log tail and validate project manifest with bootstrapper_validate."
        if record.state == "timed_out":
            remediation = "Increase timeout_seconds or inspect task for blocking I/O."
        elif record.state == "unknown_retained":
            remediation = "Worker restarted while run was active; verify output artifacts and re-run cleanly."
        elif "Simulated failure" in record.stderr_tail:
            remediation = "Re-run bootstrapper_run with simulate_failure=False to complete the retry drill."

        return NamedQueryResult(
            query=query,
            project_dir=record.project_dir,
            run_id=record.run_id,
            summary={
                "run_id": record.run_id,
                "state": record.state,
                "exit_code": record.exit_code,
                "error_code": record.error_code,
                "error_message": record.error_message,
                "recommended_remediation": remediation,
                "retry_ready": True,
            },
            redacted_logs={
                "stdout": record.stdout_tail,
                "stderr": record.stderr_tail,
            },
            artifacts=record.artifacts,
        )

    raise PolicyViolationError("unknown_query", f"Unsupported query '{query}'.")
