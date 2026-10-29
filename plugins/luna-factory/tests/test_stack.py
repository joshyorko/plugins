import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


CONTAINER = Path(__file__).resolve().parents[1] / "container"


class StackTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.deploy = self.root / "private deployment"
        self.deploy.mkdir()
        (self.deploy / ".env").write_text("LUNA_FACTORY_IMAGE=fixture\n")
        self.receipt = self.root / "commands.jsonl"
        engine = self.root / "podman"
        engine.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "with open(os.environ['STACK_TEST_RECEIPT'], 'a') as f:\n"
            "    f.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "sys.exit(int(os.environ.get('STACK_TEST_EXIT', '0')))\n"
        )
        engine.chmod(0o700)
        self.env = {
            **os.environ,
            "PATH": str(self.root) + os.pathsep + os.environ.get("PATH", ""),
            "STACK_TEST_RECEIPT": str(self.receipt),
        }

    def run_stack(self, *args, project="luna-test"):
        return subprocess.run(
            [sys.executable, str(CONTAINER / "stack.py"), "--root", str(self.deploy),
             "--project-name", project, *args],
            env=self.env, capture_output=True, text=True,
        )

    def commands(self):
        return [json.loads(line) for line in self.receipt.read_text().splitlines()]

    def test_up_validates_then_starts_only_named_project(self):
        result = self.run_stack("up")
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = self.commands()
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[0][-2:], ["config", "--quiet"])
        self.assertEqual(commands[1][-5:], ["up", "--detach", "--wait", "--wait-timeout", "60"])
        for command in commands:
            self.assertEqual(command[:3], ["compose", "--project-name", "luna-test"])
            self.assertEqual(command[3:5], ["--env-file", str(self.deploy / ".env")])
            self.assertIn(str(CONTAINER / "compose.yaml"), command)

    def test_down_preserves_state_and_keeps_egress_project_consistent(self):
        marker = self.deploy / "state.sqlite"
        marker.write_text("retained")
        result = self.run_stack("--github-egress", "down")
        self.assertEqual(result.returncode, 0, result.stderr)
        command, = self.commands()
        self.assertEqual(command[-1], "down")
        self.assertIn(str(CONTAINER / "compose.github-egress.yaml"), command)
        self.assertNotIn("--volumes", command)
        self.assertEqual(marker.read_text(), "retained")

    def test_bad_project_or_missing_env_never_invokes_engine(self):
        self.assertNotEqual(self.run_stack("up", project="../other").returncode, 0)
        (self.deploy / ".env").unlink()
        self.assertNotEqual(self.run_stack("up").returncode, 0)
        self.assertFalse(self.receipt.exists())

    def test_failed_validation_prevents_start_and_preserves_exit_status(self):
        self.env["STACK_TEST_EXIT"] = "19"
        result = self.run_stack("up")
        self.assertEqual(result.returncode, 19, result.stderr)
        self.assertEqual(len(self.commands()), 1)

    def test_status_is_read_only(self):
        result = self.run_stack("status")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands()[0][-2:], ["ps", "--all"])

    def test_private_overrides_are_used_for_every_action(self):
        override = self.root / "native transport.yaml"
        override.write_text("services: {}\n")
        result = self.run_stack("--file", str(override), "up")
        self.assertEqual(result.returncode, 0, result.stderr)
        for command in self.commands():
            self.assertIn(str(override), command)

    def test_missing_override_never_invokes_engine(self):
        result = self.run_stack("--file", str(self.root / "missing.yaml"), "up")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.receipt.exists())

    def test_compose_restarts_unless_explicitly_stopped(self):
        self.assertIn('restart: "unless-stopped"', (CONTAINER / "compose.yaml").read_text())


if __name__ == "__main__":
    unittest.main()
