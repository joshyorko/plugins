"""Execution-mode contracts must never discard inputs or change runtimes."""
from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "plugins/rcc/bootstrapper/src"))

from rcc_bootstrapper.policy import PolicyViolationError  # noqa: E402
from rcc_bootstrapper.runner import execute_project_run  # noqa: E402


class ExecutionModeTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="bootstrapper-mode-")
        self.root = Path(self.scratch.name)
        self.project = self.root / "robot"
        self.project.mkdir()
        self.datadir = self.root / "state"
        self.process = SimpleNamespace(pid=424242, returncode=0, communicate=lambda **_kw: ("", ""))
        self.environment = patch.dict(os.environ, {
            "RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS": str(self.root),
        })
        self.environment.start()
        self.validation = patch("rcc_bootstrapper.runner.validate_project", return_value=SimpleNamespace(valid=True, entrypoints=["Task"]))
        self.validation.start()

    def tearDown(self):
        self.validation.stop()
        self.environment.stop()
        self.scratch.cleanup()

    def run_project(self, **kwargs):
        return execute_project_run(
            "robot", self.project, target_name="Task", trust_project=True,
            approved_roots=[self.root], datadir=self.datadir, **kwargs,
        )

    def test_unsupported_rcc_requests_fail_before_dispatch(self):
        cases = [
            ({"inputs": {"text": "synthetic input"}}, "rcc_cli_inputs_unsupported"),
            ({"work_items": [{"text": "synthetic item"}]}, "rcc_cli_work_items_unsupported"),
            ({"simulate_failure": True}, "rcc_cli_simulation_unsupported"),
        ]
        for request, code in cases:
            with self.subTest(code=code), patch("rcc_bootstrapper.runner.verify_binary_before_execution", return_value=(Path("/trusted/rcc"), None)), \
                 patch("rcc_bootstrapper.runner.subprocess.Popen", return_value=self.process) as launched:
                with self.assertRaises(PolicyViolationError) as failure:
                    self.run_project(execution_mode="rcc_cli", **request)
                self.assertEqual(failure.exception.code, code)
                launched.assert_not_called()
                self.assertFalse(self.datadir.exists(), "Rejected requests must not create run state")

    def test_rcc_mode_refuses_action_projects(self):
        with patch("rcc_bootstrapper.runner.subprocess.Popen", return_value=self.process) as launched:
            with self.assertRaises(PolicyViolationError) as failure:
                execute_project_run("action", self.project, execution_mode="rcc_cli", trust_project=True,
                                    approved_roots=[self.root], datadir=self.datadir)
            self.assertEqual(failure.exception.code, "rcc_cli_project_kind_unsupported")
            launched.assert_not_called()

    def test_unknown_mode_never_falls_back_to_local_python(self):
        with patch("rcc_bootstrapper.runner.subprocess.Popen", return_value=self.process) as launched:
            with self.assertRaises(PolicyViolationError) as failure:
                self.run_project(execution_mode="rcc-clii")
            self.assertEqual(failure.exception.code, "invalid_execution_mode")
            launched.assert_not_called()

    def test_supported_rcc_robot_stays_on_rcc(self):
        process = SimpleNamespace(pid=424242, returncode=0, communicate=lambda **_kw: ("", ""))
        with patch("rcc_bootstrapper.runner.verify_binary_before_execution", return_value=(Path("/trusted/rcc"), None)), \
             patch("rcc_bootstrapper.runner.subprocess.Popen", return_value=process) as launched:
            result = self.run_project(execution_mode="rcc_cli", inputs={}, work_items=[])
        self.assertEqual(result.state, "succeeded")
        self.assertEqual(result.command_summary[0], "rcc")
        self.assertEqual(launched.call_args.args[0][0], "/trusted/rcc")
        self.assertNotIn("RCC_BOOTSTRAPPER_RUN_CONFIG", launched.call_args.kwargs["env"])

    def test_default_local_python_preserves_keyword_inputs(self):
        (self.project / "tasks.py").write_text(
            "import json, os\nfrom pathlib import Path\n"
            "def Task(text):\n"
            "    Path(os.environ['ROBOT_ARTIFACTS'], 'observed.json').write_text(json.dumps({'text': text}))\n"
        )
        result = self.run_project(inputs={"text": "synthetic retained input"})
        self.assertEqual(result.state, "succeeded")
        self.assertEqual(json.loads((self.project / "output/observed.json").read_text()),
                         {"text": "synthetic retained input"})


if __name__ == "__main__":
    unittest.main()
