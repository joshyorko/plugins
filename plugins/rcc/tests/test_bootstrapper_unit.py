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
                    timeout_seconds=20,
                    trust_project=True,
                    approved_roots=[self.approved_root],
                    datadir=self.datadir,
                ),
            ),
        )
        worker.start()
        deadline = time.monotonic() + 5
        active_run_id = None
        while time.monotonic() < deadline:
            for path in self.datadir.glob("bootstrapper_runs/*.json"):
                saved = RunRecord.model_validate_json(path.read_text(encoding="utf-8"))
                if saved.state == "running" and saved.run_id in runner._ACTIVE_PROCS:
                    active_run_id = saved.run_id
                    break
            if active_run_id:
                break
            time.sleep(0.05)

        self.assertIsNotNone(active_run_id, "run never became active")
        cancelled = cancel_project_run(active_run_id, datadir=self.datadir)
        worker.join(timeout=5)
        self.assertFalse(worker.is_alive(), "cancelled run worker did not return")
        self.assertEqual(cancelled.state, "cancelled")
        self.assertTrue(cancelled.process_group_signaled)
        self.assertTrue(cancelled.termination_confirmed)
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


if __name__ == "__main__":
    unittest.main()
