"""Unit and security regression tests for plugins/rcc/bootstrapper (covering all 36 legacy action defect classes)."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
BOOTSTRAPPER_SRC = ROOT / "plugins" / "rcc" / "bootstrapper" / "src"
SCRIPTS_DIR = ROOT / "plugins" / "rcc" / "scripts"

if str(BOOTSTRAPPER_SRC) not in sys.path:
    sys.path.insert(0, str(BOOTSTRAPPER_SRC))
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from rcc_bootstrapper.models import RunRecord  # noqa: E402
from rcc_bootstrapper.policy import (  # noqa: E402
    PolicyViolationError,
    redact_secrets,
    resolve_approved_target,
    verify_binary_before_execution,
)
from rcc_bootstrapper.runner import (  # noqa: E402
    cancel_project_run,
    execute_named_query,
    execute_project_run,
    load_run_record,
    save_run_record,
)
from rcc_bootstrapper.scaffold import (  # noqa: E402
    ACTION_TEMPLATES,
    ROBOT_TEMPLATES,
    plan_scaffold,
    scaffold_project,
    validate_project,
)


class BootstrapperUnitAndSecurityTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="rcc-bootstrapper-unit-")
        self.tmp_root = Path(self._tmp.name).resolve()
        self.approved_root = self.tmp_root / "workspace"
        self.approved_root.mkdir(parents=True, exist_ok=True)
        self.datadir = self.tmp_root / "datadir"
        self.datadir.mkdir(parents=True, exist_ok=True)
        self._old_roots = os.environ.get("RCC_BOOTSTRAPPER_APPROVED_ROOTS")
        self._old_trusted_roots = os.environ.get("RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS")
        self._old_datadir = os.environ.get("RCC_BOOTSTRAPPER_DATADIR")
        self._old_test_mode = os.environ.get("RCC_BOOTSTRAPPER_TEST_MODE")
        os.environ["RCC_BOOTSTRAPPER_APPROVED_ROOTS"] = str(self.approved_root)
        os.environ["RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS"] = str(self.approved_root)
        os.environ["RCC_BOOTSTRAPPER_DATADIR"] = str(self.datadir)
        os.environ["RCC_BOOTSTRAPPER_TEST_MODE"] = "1"

    def tearDown(self) -> None:
        if self._old_roots is None:
            os.environ.pop("RCC_BOOTSTRAPPER_APPROVED_ROOTS", None)
        else:
            os.environ["RCC_BOOTSTRAPPER_APPROVED_ROOTS"] = self._old_roots
        if self._old_trusted_roots is None:
            os.environ.pop("RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS", None)
        else:
            os.environ["RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS"] = self._old_trusted_roots
        if self._old_datadir is None:
            os.environ.pop("RCC_BOOTSTRAPPER_DATADIR", None)
        else:
            os.environ["RCC_BOOTSTRAPPER_DATADIR"] = self._old_datadir
        if self._old_test_mode is None:
            os.environ.pop("RCC_BOOTSTRAPPER_TEST_MODE", None)
        else:
            os.environ["RCC_BOOTSTRAPPER_TEST_MODE"] = self._old_test_mode
        self._tmp.cleanup()

    def test_approved_root_and_path_traversal_defense(self) -> None:
        # Valid subdirectory inside approved root succeeds
        target, root = resolve_approved_target(self.approved_root / "my-proj", approved_roots=[self.approved_root])
        self.assertEqual(root, self.approved_root)
        self.assertEqual(target, self.approved_root / "my-proj")

        # Parent traversal ('..') is rejected
        with self.assertRaises(PolicyViolationError) as ctx:
            resolve_approved_target(f"{self.approved_root}/../escape", approved_roots=[self.approved_root])
        self.assertEqual(ctx.exception.code, "path_traversal_rejected")

        # Outside approved root is rejected
        outside = self.tmp_root / "outside-proj"
        with self.assertRaises(PolicyViolationError) as ctx2:
            resolve_approved_target(outside, approved_roots=[self.approved_root])
        self.assertEqual(ctx2.exception.code, "path_outside_approved_root")

        # Approved root itself is rejected as a project target
        with self.assertRaises(PolicyViolationError) as ctx3:
            resolve_approved_target(self.approved_root, approved_roots=[self.approved_root], allow_root_itself=False)
        self.assertEqual(ctx3.exception.code, "target_is_approved_root")

        # Symlink escaping approved root is rejected
        outside.mkdir(parents=True, exist_ok=True)
        symlink_path = self.approved_root / "evil-link"
        symlink_path.symlink_to(outside)
        with self.assertRaises(PolicyViolationError) as ctx4:
            resolve_approved_target(symlink_path / "sub", approved_roots=[self.approved_root])
        self.assertIn(ctx4.exception.code, {"symlink_escape_rejected", "path_outside_approved_root"})

    def test_fail_closed_binary_sha256_verification(self) -> None:
        fake_bin = self.tmp_root / "fake-action-server"
        fake_bin.write_text("#!/bin/sh\necho fake\n", encoding="utf-8")
        fake_bin.chmod(0o755)
        with self.assertRaises(PolicyViolationError) as ctx:
            verify_binary_before_execution("action-server", fake_bin)
        self.assertEqual(ctx.exception.code, "untrusted_binary_digest")

    def test_plan_does_not_mutate_disk_and_scaffold_is_non_empty_and_atomic(self) -> None:
        # Regression for legacy bug #1 & #12: bootstrap_action_package and create_robot wiped files to 0 bytes
        for kind, tpl in [("action", "basic"), ("action", "workflow-producer-consumer"), ("robot", "01-python")]:
            dest = self.approved_root / f"proj-{kind}-{tpl}"
            plan = plan_scaffold(kind, tpl, dest, approved_roots=[self.approved_root])  # type: ignore[arg-type]
            self.assertTrue(plan.can_scaffold)
            self.assertFalse(dest.exists(), "plan_scaffold must not touch disk")

            res = scaffold_project(kind, tpl, dest, approved_roots=[self.approved_root])  # type: ignore[arg-type]
            self.assertTrue(res.atomic_staging_used)
            self.assertGreater(len(res.files), 0)
            for f in res.files:
                if not f.relative_path.endswith("__init__.py"):
                    self.assertGreater(f.size_bytes, 0, f"Scaffolded file {f.relative_path} must not be 0 bytes")
                self.assertFalse((dest / "actions.py").exists(), "Must never create actions.py shadowing actions-core")

            val = validate_project(kind, dest, approved_roots=[self.approved_root])  # type: ignore[arg-type]
            self.assertTrue(val.valid, f"Validation failed for {kind}/{tpl}: {val.issues}")

    def test_published_template_generation_fails_closed_without_verified_binary(self) -> None:
        old_test_mode = os.environ.pop("RCC_BOOTSTRAPPER_TEST_MODE", None)
        try:
            with self.assertRaises(PolicyViolationError) as ctx:
                scaffold_project(
                    "robot",
                    "01-python",
                    self.approved_root / "missing-generator",
                    approved_roots=[self.approved_root],
                    rcc_bin=self.tmp_root / "missing-rcc",
                )
            self.assertEqual(ctx.exception.code, "generator_binary_missing")
        finally:
            if old_test_mode is not None:
                os.environ["RCC_BOOTSTRAPPER_TEST_MODE"] = old_test_mode

    def test_non_empty_and_dotfile_destination_overwrite_refused(self) -> None:
        # Hidden dotfile / .git directory must prevent overwrite
        dot_git_dest = self.approved_root / "existing-git-repo"
        (dot_git_dest / ".git").mkdir(parents=True, exist_ok=True)
        with self.assertRaises(PolicyViolationError) as ctx:
            scaffold_project("action", "basic", dot_git_dest, approved_roots=[self.approved_root])
        self.assertEqual(ctx.exception.code, "destination_not_empty")
        self.assertTrue((dot_git_dest / ".git").is_dir())

    def test_actions_py_shadowing_detected_by_validator(self) -> None:
        # Regression for legacy bug #6: actions.py shadows actions-core
        dest = self.approved_root / "shadow-pkg"
        scaffold_project("action", "basic", dest, approved_roots=[self.approved_root])
        (dest / "actions.py").write_text("# bad shadow file\n", encoding="utf-8")
        val = validate_project("action", dest, approved_roots=[self.approved_root])
        self.assertFalse(val.valid)
        codes = {i.code for i in val.issues}
        self.assertIn("actions_module_shadowing", codes)

    def test_secret_redaction_and_no_cli_argv_leakage(self) -> None:
        # Regression for legacy bug #4: secrets passed on sys.argv and printed to stdout
        dest = self.approved_root / "secret-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        secret_val = "SUPER_SECRET_TOKEN_887123"
        # Have tasks.py print the environment variable to stdout and stderr to test redaction
        (dest / "tasks.py").write_text(
            "import os, sys\n"
            "from pathlib import Path\n"
            "def minimal_task():\n"
            "    tok = os.environ.get('MY_API_SECRET', '')\n"
            "    print(f'stdout leaked {tok}')\n"
            "    print(f'stderr leaked {tok}', file=sys.stderr)\n"
            "    out = Path(os.environ.get('ROBOT_ARTIFACTS', 'output'))\n"
            "    out.mkdir(parents=True, exist_ok=True)\n"
            "    (out / 'ok.txt').write_text('done\\n', encoding='utf-8')\n",
            encoding="utf-8",
        )
        rec = execute_project_run(
            "robot",
            dest,
            target_name="minimal_task",
            secrets={"MY_API_SECRET": secret_val},
            trust_project=True,
            approved_roots=[self.approved_root],
            datadir=self.datadir,
        )
        self.assertEqual(rec.state, "succeeded")
        self.assertNotIn(secret_val, " ".join(rec.command_summary))
        self.assertNotIn(secret_val, rec.stdout_tail)
        self.assertNotIn(secret_val, rec.stderr_tail)
        self.assertIn("[REDACTED]", rec.stdout_tail)
        self.assertIn("[REDACTED]", rec.stderr_tail)

        # Also test generic bearer/api_key pattern redaction
        raw_log = "Authorization: Bearer abcdef1234567890 and api_key=xyz987654"
        scrubbed = redact_secrets(raw_log)
        self.assertNotIn("abcdef1234567890", scrubbed)
        self.assertNotIn("xyz987654", scrubbed)

    def test_nonzero_exit_code_never_swallowed_and_crash_reconciliation(self) -> None:
        # Regression for legacy bug #11 & #17: _run swallowed CalledProcessError as success
        dest = self.approved_root / "fail-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        rec = execute_project_run(
            "robot",
            dest,
            target_name="minimal_task",
            simulate_failure=True,
            trust_project=True,
            approved_roots=[self.approved_root],
            datadir=self.datadir,
        )
        self.assertEqual(rec.state, "failed")
        self.assertEqual(rec.exit_code, 2)
        self.assertIsNotNone(rec.error_code)

        # Named query failure_diagnosis works on the failed run
        diag = execute_named_query(
            "failure_diagnosis",
            project_dir=dest,
            run_id=rec.run_id,
            approved_roots=[self.approved_root],
            datadir=self.datadir,
        )
        self.assertTrue(diag.summary["retry_ready"])

        # Crash reconciliation: if a run was persisted as 'running' by a prior worker and is not in _ACTIVE_PROCS,
        # load_run_record transitions it to 'unknown_retained' (never 'succeeded' or 'cancelled').
        orphan = RunRecord(
            run_id="run-orphan999999",
            project_kind="robot",
            project_dir=str(dest),
            target_name="minimal_task",
            state="running",
            pid=99999999,
            pgid=99999999,
            started_at="2026-04-20T00:00:00+00:00",
            command_summary=["python", "tasks.py"],
        )
        save_run_record(orphan, self.datadir)
        reconciled = load_run_record("run-orphan999999", self.datadir)
        self.assertEqual(reconciled.state, "unknown_retained")
        self.assertEqual(reconciled.error_code, "worker_restarted_state_unknown")

        cancel_res = cancel_project_run("run-orphan999999", datadir=self.datadir)
        self.assertEqual(cancel_res.state, "unknown_retained")
        self.assertFalse(cancel_res.process_group_signaled)

    def test_timeout_reaps_process_group(self) -> None:
        dest = self.approved_root / "sleep-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        (dest / "tasks.py").write_text(
            "import time\ndef minimal_task():\n    time.sleep(30)\n",
            encoding="utf-8",
        )
        t0 = time.monotonic()
        rec = execute_project_run(
            "robot",
            dest,
            target_name="minimal_task",
            timeout_seconds=0.5,
            trust_project=True,
            approved_roots=[self.approved_root],
            datadir=self.datadir,
        )
        elapsed = time.monotonic() - t0
        self.assertEqual(rec.state, "timed_out")
        self.assertLess(elapsed, 10.0)

    def test_cancel_terminates_active_owned_process_group(self) -> None:
        from rcc_bootstrapper import runner

        dest = self.approved_root / "cancel-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        (dest / "tasks.py").write_text("import time\ndef minimal_task():\n    time.sleep(30)\n", encoding="utf-8")
        result: dict[str, RunRecord] = {}
        worker = threading.Thread(
            target=lambda: result.setdefault(
                "record",
                execute_project_run(
                    "robot",
                    dest,
                    target_name="minimal_task",
                    timeout_seconds=10,
                    trust_project=True,
                    approved_roots=[self.approved_root],
                    datadir=self.datadir,
                ),
            ),
        )

        def active_run_for_project() -> str | None:
            for run_id in tuple(runner._ACTIVE_PROCS):
                saved = load_run_record(run_id, self.datadir)
                if saved.project_dir == str(dest) and saved.state == "running":
                    return run_id
            return None

        worker.start()
        cancelled = None
        try:
            deadline = time.monotonic() + 5
            active_run_id = None
            while time.monotonic() < deadline:
                active_run_id = active_run_for_project()
                if active_run_id:
                    break
                time.sleep(0.05)

            self.assertIsNotNone(active_run_id, "run never became active")
            cancelled = cancel_project_run(active_run_id, datadir=self.datadir)
            self.assertEqual(cancelled.state, "cancelled")
            self.assertTrue(cancelled.process_group_signaled)
            self.assertTrue(cancelled.termination_confirmed)
        finally:
            if worker.is_alive():
                active_run_id = active_run_for_project()
                if active_run_id:
                    cancel_project_run(active_run_id, datadir=self.datadir)
                worker.join(timeout=12)
            self.assertFalse(worker.is_alive(), "run worker remained active after cancellation/timeout cleanup")

        self.assertIsNotNone(cancelled)
        self.assertEqual(result["record"].state, "cancelled")

    def test_occupied_port_refusal_in_runtime_launcher(self) -> None:
        import socket
        from bootstrapper_runtime import start_owned_runtime

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            s.listen(1)
            occupied_port = int(s.getsockname()[1])

            with self.assertRaises(PolicyViolationError) as ctx:
                start_owned_runtime(
                    datadir=self.datadir,
                    host="127.0.0.1",
                    port=occupied_port,
                    approved_roots=[self.approved_root],
                    api_key="test-key",
                )
            self.assertEqual(ctx.exception.code, "port_occupied_by_unrelated_process")

    def test_runtime_start_requires_authentication_before_binary_launch(self) -> None:
        import bootstrapper_runtime

        old_key = os.environ.pop("RCC_BOOTSTRAPPER_API_KEY", None)
        try:
            for key in (None, "   "):
                with self.assertRaises(PolicyViolationError) as ctx:
                    bootstrapper_runtime.start_owned_runtime(
                        datadir=self.datadir,
                        approved_roots=[self.approved_root],
                        api_key=key,
                        runtime_binary="/missing/runtime",
                        rcc_binary="/missing/rcc",
                    )
                self.assertEqual(ctx.exception.code, "runtime_api_key_required")
        finally:
            if old_key is not None:
                os.environ["RCC_BOOTSTRAPPER_API_KEY"] = old_key

    def test_windows_runtime_liveness_and_identity_do_not_send_signals(self) -> None:
        import bootstrapper_runtime as runtime

        with (
            patch.object(runtime.sys, "platform", "win32"),
            patch.object(runtime, "_windows_process_state", return_value=(True, "windows-creation:123")),
            patch.object(runtime.os, "kill") as kill,
        ):
            self.assertTrue(runtime._is_pid_alive(45678))
            self.assertEqual(runtime._proc_start_token(45678), "windows-creation:123")
            kill.assert_not_called()

    def test_windows_runtime_stop_targets_owned_process_tree(self) -> None:
        import bootstrapper_runtime as runtime

        metadata = runtime._instance_metadata_path(self.datadir)
        metadata.parent.mkdir(parents=True, exist_ok=True)
        metadata.write_text("{}", encoding="utf-8")
        with (
            patch.object(runtime.sys, "platform", "win32"),
            patch.object(runtime, "inspect_owned_instance", return_value={"state": "running", "metadata": {"pid": 45678}}),
            patch.object(runtime, "_is_pid_alive", return_value=False),
            patch.object(runtime.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run,
            patch.object(runtime.os, "kill") as kill,
        ):
            stopped = runtime.stop_owned_runtime(datadir=self.datadir)
            self.assertTrue(stopped["terminated"])
            self.assertEqual(run.call_args.args[0], ["taskkill", "/PID", "45678", "/T", "/F"])
            kill.assert_not_called()
        self.assertFalse(metadata.exists())

    def test_windows_runtime_failed_stop_preserves_ownership(self) -> None:
        import bootstrapper_runtime as runtime

        metadata = runtime._instance_metadata_path(self.datadir)
        metadata.parent.mkdir(parents=True, exist_ok=True)
        metadata.write_text("{}", encoding="utf-8")
        with (
            patch.object(runtime.sys, "platform", "win32"),
            patch.object(runtime, "inspect_owned_instance", return_value={"state": "running", "metadata": {"pid": 45678}}),
            patch.object(runtime, "_is_pid_alive", return_value=True),
            patch.object(runtime.subprocess, "run", return_value=SimpleNamespace(returncode=1)),
        ):
            with self.assertRaisesRegex(PolicyViolationError, "owned Windows process tree"):
                runtime.stop_owned_runtime(datadir=self.datadir)
        self.assertTrue(metadata.exists())

    def test_runtime_start_preserves_home_and_isolates_supported_cache_variables(self) -> None:
        import bootstrapper_runtime

        runtime_path = self.tmp_root / "runtime"
        rcc_path = self.tmp_root / "rcc"
        process = SimpleNamespace(pid=123456, poll=lambda: None)
        with (
            patch.dict(
                os.environ,
                {"HOME": "/caller/home", "USERPROFILE": r"C:\\Caller\\Profile"},
            ),
            patch.object(
                bootstrapper_runtime,
                "verify_binary_before_execution",
                side_effect=[(runtime_path, "a" * 64), (rcc_path, "b" * 64)],
            ),
            patch.object(
                bootstrapper_runtime,
                "inspect_binary_health",
                return_value=SimpleNamespace(version="1.1.1"),
            ),
            patch.object(bootstrapper_runtime, "_is_port_open", side_effect=[False, True]),
            patch.object(bootstrapper_runtime.subprocess, "Popen", return_value=process) as popen,
        ):
            bootstrapper_runtime.start_owned_runtime(
                datadir=self.datadir,
                approved_roots=[self.approved_root],
                api_key="test-key",
                runtime_binary=runtime_path,
                rcc_binary=rcc_path,
                robocorp_home=self.tmp_root / "private-robocorp",
            )
        child_env = popen.call_args.kwargs["env"]
        self.assertEqual(child_env["HOME"], "/caller/home")
        self.assertEqual(child_env["USERPROFILE"], r"C:\\Caller\\Profile")
        cache_root = self.datadir.parent / "action-server-home"
        self.assertEqual(child_env["ACTIONS_HOME"], str((cache_root / "actions").resolve()))
        self.assertEqual(child_env["ROBOTS_HOME"], str((cache_root / "robots").resolve()))
        self.assertEqual(child_env["UV_CACHE_DIR"], str((cache_root / "uv-cache").resolve()))
        self.assertEqual(child_env["ROBOCORP_HOME"], str((self.tmp_root / "private-robocorp").resolve()))

    def test_stale_runtime_metadata_is_archived_without_signaling_pid(self) -> None:
        import bootstrapper_runtime

        metadata_path = self.datadir / bootstrapper_runtime.INSTANCE_METADATA_FILENAME
        metadata_path.write_text(
            json.dumps({
                "pid": 99999999,
                "proc_start_token": "linux-starttime:1",
                "host": "127.0.0.1",
                "port": 8765,
                "datadir": str(self.datadir),
            }),
            encoding="utf-8",
        )
        status = bootstrapper_runtime.inspect_owned_instance(self.datadir)
        self.assertEqual(status["state"], "stale")
        self.assertTrue(bootstrapper_runtime._reconcile_stale_metadata(self.datadir))
        self.assertFalse(metadata_path.exists())
        archives = list(self.datadir.glob("bootstrapper-instance.stale-*.json"))
        self.assertEqual(len(archives), 1)
        self.assertEqual(json.loads(archives[0].read_text(encoding="utf-8"))["pid"], 99999999)

    def test_run_requires_explicit_project_trust(self) -> None:
        dest = self.approved_root / "untrusted-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        with self.assertRaises(PolicyViolationError) as ctx:
            execute_project_run(
                "robot",
                dest,
                target_name="minimal_task",
                approved_roots=[self.approved_root],
                datadir=self.datadir,
            )
        self.assertEqual(ctx.exception.code, "untrusted_project_execution_refused")

    def test_caller_cannot_authorize_project_outside_server_trusted_roots(self) -> None:
        dest = self.approved_root / "untrusted-robot"
        scaffold_project("robot", "01-python", dest, approved_roots=[self.approved_root])
        old_trusted_roots = os.environ.pop("RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS", None)
        try:
            with self.assertRaises(PolicyViolationError) as ctx:
                execute_project_run(
                    "robot",
                    dest,
                    target_name="minimal_task",
                    trust_project=True,
                    approved_roots=[self.approved_root],
                    datadir=self.datadir,
                )
            self.assertEqual(ctx.exception.code, "untrusted_project_execution_refused")
        finally:
            if old_trusted_roots is not None:
                os.environ["RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS"] = old_trusted_roots

    def test_template_catalog_completeness(self) -> None:
        self.assertEqual(
            set(ACTION_TEMPLATES.keys()),
            {"basic", "mcp-v2-showcase", "workflow-producer-consumer", "canvas-query"},
        )
        self.assertIn("01-python", ROBOT_TEMPLATES)
        self.assertIn("03-python-work-items", ROBOT_TEMPLATES)
        self.assertIn("06-python-uv-native", ROBOT_TEMPLATES)

    def test_serve_owned_runtime_tracks_and_cleans_instance_metadata(self) -> None:
        import bootstrapper_runtime

        runtime_path = self.tmp_root / "runtime"
        rcc_path = self.tmp_root / "rcc"

        class FakeProc:
            def __init__(self) -> None:
                self.pid = 234567
                self._done = False

            def poll(self) -> int | None:
                return 0 if self._done else None

            def wait(self, timeout: float | None = None) -> int:
                # Instance metadata must exist while foreground child is active.
                meta = bootstrapper_runtime._load_instance_metadata(datadir)
                assert meta is not None and meta["pid"] == self.pid
                self._done = True
                return 0

        datadir = self.datadir
        with (
            patch.object(
                bootstrapper_runtime,
                "verify_binary_before_execution",
                side_effect=[(runtime_path, "a" * 64), (rcc_path, "b" * 64)],
            ),
            patch.object(
                bootstrapper_runtime,
                "inspect_binary_health",
                return_value=SimpleNamespace(version="1.1.1"),
            ),
            patch.object(bootstrapper_runtime, "_is_port_open", return_value=False),
            patch.object(bootstrapper_runtime.subprocess, "Popen", return_value=FakeProc()) as popen,
        ):
            rc = bootstrapper_runtime.serve_owned_runtime(
                datadir=datadir,
                approved_roots=[self.approved_root],
                api_key="serve-secret-key",
                runtime_binary=runtime_path,
                rcc_binary=rcc_path,
                robocorp_home=self.tmp_root / "private-robocorp",
            )
        self.assertEqual(rc, 0)
        self.assertFalse(bootstrapper_runtime._instance_metadata_path(datadir).exists())
        self.assertIn("--api-key", popen.call_args.args[0])

    def test_systemd_install_writes_private_env_and_unit_without_embedding_secret(self) -> None:
        import stat as stat_mod
        import bootstrapper_runtime

        unit_dir = self.tmp_root / "systemd-user"
        env_file = self.tmp_root / "service config" / "rcc-bootstrapper.env"
        spaced_root = self.tmp_root / "my workspace %100"
        spaced_root.mkdir(parents=True, exist_ok=True)
        trusted_root = spaced_root / "trusted"
        trusted_root.mkdir(parents=True, exist_ok=True)

        old_key = os.environ.pop("RCC_BOOTSTRAPPER_API_KEY", None)
        try:
            with patch.object(bootstrapper_runtime.sys, "platform", "linux"):
                # Missing approved root fails closed
                with self.assertRaises(PolicyViolationError) as ctx_root:
                    bootstrapper_runtime.install_systemd_service(
                        datadir=self.datadir,
                        unit_dir=unit_dir,
                        env_file=env_file,
                        approved_roots=[],
                        generate_api_key=True,
                    )
                self.assertEqual(ctx_root.exception.code, "approved_root_required")

                # Missing API key fails closed
                with self.assertRaises(PolicyViolationError) as ctx_key:
                    bootstrapper_runtime.install_systemd_service(
                        datadir=self.datadir,
                        unit_dir=unit_dir,
                        env_file=env_file,
                        approved_roots=[spaced_root],
                    )
                self.assertEqual(ctx_key.exception.code, "runtime_api_key_required")

                # Generate key + install unit
                installed = bootstrapper_runtime.install_systemd_service(
                    datadir=self.datadir,
                    unit_dir=unit_dir,
                    env_file=env_file,
                    approved_roots=[spaced_root],
                    trusted_roots=[trusted_root],
                    generate_api_key=True,
                )
                self.assertEqual(installed["status"], "installed")
                self.assertEqual(installed["api_key_source"], "generated")
                self.assertTrue(env_file.is_file())
                if os.name != "nt":
                    self.assertEqual(stat_mod.S_IMODE(env_file.stat().st_mode), 0o600)

                generated_key = bootstrapper_runtime._read_env_file_key(env_file, "RCC_BOOTSTRAPPER_API_KEY")
                self.assertIsNotNone(generated_key)
                self.assertGreaterEqual(len(generated_key or ""), 32)
                # Secret must never be echoed in JSON result or unit file
                self.assertNotIn(generated_key or "", json.dumps(installed))
                unit_path = Path(installed["unit_path"])
                unit_text = unit_path.read_text(encoding="utf-8")
                self.assertNotIn(generated_key or "", unit_text)
                # The space forces quoting on all hosts. JSON supplies the
                # expected quote and backslash escaping.
                quoted_env_file = json.dumps(str(env_file), ensure_ascii=False).replace("%", "%%")
                self.assertIn(f"EnvironmentFile={quoted_env_file}", unit_text.splitlines())
                self.assertIn("%%100", unit_text)
                self.assertIn('"serve"', unit_text)
                self.assertIn("--trusted-root", unit_text)

                # Re-running without --rotate-api-key preserves existing secret idempotently
                reinstalled = bootstrapper_runtime.install_systemd_service(
                    datadir=self.datadir,
                    unit_dir=unit_dir,
                    env_file=env_file,
                    approved_roots=[spaced_root],
                    generate_api_key=True,
                )
                self.assertEqual(reinstalled["api_key_source"], "existing_env_file")
                self.assertEqual(
                    bootstrapper_runtime._read_env_file_key(env_file, "RCC_BOOTSTRAPPER_API_KEY"),
                    generated_key,
                )

                # Uninstall removes unit file, keeps env file by default, never touches datadir
                sentinel = self.datadir / "server.db"
                sentinel.write_bytes(b"sqlite-fixture")
                uninstalled = bootstrapper_runtime.uninstall_systemd_service(
                    unit_dir=unit_dir,
                    env_file=env_file,
                    remove_env_file=False,
                    run_systemctl=False,
                )
                self.assertTrue(uninstalled["unit_removed"])
                self.assertFalse(uninstalled["env_file_removed"])
                self.assertFalse(unit_path.exists())
                self.assertTrue(env_file.exists())
                self.assertTrue(sentinel.exists())

                # Uninstall with remove_env_file=True removes the env file
                uninstalled2 = bootstrapper_runtime.uninstall_systemd_service(
                    unit_dir=unit_dir,
                    env_file=env_file,
                    remove_env_file=True,
                    run_systemctl=False,
                )
                self.assertTrue(uninstalled2["env_file_removed"])
                self.assertFalse(env_file.exists())
                self.assertTrue(sentinel.exists())
        finally:
            if old_key is not None:
                os.environ["RCC_BOOTSTRAPPER_API_KEY"] = old_key

    def test_systemd_install_rejects_non_linux_and_symlink_paths(self) -> None:
        import bootstrapper_runtime

        with patch.object(bootstrapper_runtime.sys, "platform", "win32"):
            with self.assertRaises(PolicyViolationError) as ctx_plat:
                bootstrapper_runtime.install_systemd_service(
                    datadir=self.datadir,
                    approved_roots=[self.approved_root],
                    api_key="test-key",
                )
            self.assertEqual(ctx_plat.exception.code, "systemd_linux_only")

        if hasattr(os, "symlink") and os.name != "nt":
            outside = self.tmp_root / "outside-systemd"
            outside.mkdir()
            symlink_unit_dir = self.tmp_root / "unit-dir-link"
            symlink_unit_dir.symlink_to(outside)
            with patch.object(bootstrapper_runtime.sys, "platform", "linux"):
                with self.assertRaises(PolicyViolationError) as ctx_link:
                    bootstrapper_runtime.install_systemd_service(
                        datadir=self.datadir,
                        unit_dir=symlink_unit_dir,
                        env_file=self.tmp_root / "ok.env",
                        approved_roots=[self.approved_root],
                        api_key="test-key",
                    )
                self.assertEqual(ctx_link.exception.code, "symlink_service_path")

        with self.assertRaises(PolicyViolationError) as ctx_ctrl:
            bootstrapper_runtime._systemd_quote_arg("bad\nExecStart=/bin/sh")
        self.assertEqual(ctx_ctrl.exception.code, "invalid_service_value")


if __name__ == "__main__":
    unittest.main()
