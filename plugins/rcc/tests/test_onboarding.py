"""Opt-in, real RCC onboarding and offline SDK compatibility checks.

Run with RCC_ONBOARDING_RUNTIME=1. RCC_ONBOARDING_HOME may select a disposable
prewarmed RCC home; otherwise the suite creates and removes its own private home.
"""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


@unittest.skipUnless(os.getenv("RCC_ONBOARDING_RUNTIME") == "1" and shutil.which("rcc"),
                     "set RCC_ONBOARDING_RUNTIME=1 with rcc installed")
class OnboardingRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="rcc-onboarding-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.root = Path(cls.temporary.name)
        cls.environment = os.environ.copy()
        cls.environment.update(
            ROBOCORP_HOME=os.getenv("RCC_ONBOARDING_HOME", str(cls.root / "rcc-home")),
            RCC_HOLOTREE_MODE="private",
        )
        # Prevent inherited hosted context/credentials from influencing fixtures.
        for key in list(cls.environment):
            if key.startswith(("RC_", "RPA_")):
                cls.environment.pop(key)

    def rcc(self, *args, cwd=None):
        result = subprocess.run(["rcc", *args], cwd=cwd or self.root,
                                env=self.environment, text=True, capture_output=True,
                                timeout=240)
        evidence = os.getenv("RCC_ONBOARDING_EVIDENCE")
        if evidence:
            directory = Path(evidence)
            directory.mkdir(parents=True, exist_ok=True)
            number = len(list(directory.glob("command-*.json"))) + 1
            (directory / f"command-{number:03d}.json").write_text(json.dumps({
                "argv": ["rcc", *args], "cwd": str(cwd or self.root),
                "stdout": result.stdout, "stderr": result.stderr,
                "exit_code": result.returncode,
            }, indent=2) + "\n")
        return result

    def retain(self, path):
        evidence = os.getenv("RCC_ONBOARDING_EVIDENCE")
        if evidence:
            directory = Path(evidence)
            directory.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, directory / path.name)

    def initialize(self, destination):
        catalog = self.rcc("robot", "initialize", "--list")
        self.assertEqual(catalog.returncode, 0, catalog.stdout + catalog.stderr)
        self.assertIn("- 01-python", catalog.stdout + catalog.stderr)
        return self.rcc("robot", "initialize", "-t", "01-python", "-d", str(destination))

    def test_initializer_preserves_nonempty_hidden_and_git_destinations(self):
        checked = []
        for relative in ("existing.txt", ".hidden", ".git/HEAD"):
            with self.subTest(existing=relative):
                destination = self.root / relative.replace("/", "-")
                sentinel = destination / relative
                sentinel.parent.mkdir(parents=True)
                sentinel.write_text("existing history or data\n")
                result = self.initialize(destination)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(sentinel.read_text(), "existing history or data\n")
                self.assertEqual([p for p in destination.rglob("*") if p.is_file()],
                                 [sentinel])
                checked.append({"existing": relative, "preserved": True})
        evidence = os.getenv("RCC_ONBOARDING_EVIDENCE")
        if evidence:
            (Path(evidence) / "preservation.json").write_text(json.dumps(checked, indent=2) + "\n")

    def test_first_pass_and_local_workitems_and_filesecrets(self):
        import strictyaml

        destination = self.root / "starter"
        result = self.initialize(destination)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        robot_path = destination / "robot.yaml"
        config = strictyaml.load(robot_path.read_text()).data
        task_name = next(iter(config["tasks"]))
        result = self.rcc("task", "run", "--robot", "robot.yaml", "--task", task_name,
                          cwd=destination)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("status: PASS", result.stdout + result.stderr)
        output = destination / config["artifactsDir"]
        self.assertTrue((output / "log.html").is_file())
        self.assertIn("Hello", (output / "greeting.txt").read_text())
        self.retain(output / "greeting.txt")
        self.retain(output / "log.html")

        fixture = Path(__file__).parent / "fixtures" / "local_tasks.py"
        shutil.copy2(fixture, destination / "local_tasks.py")
        # Preserve the generated environment and pins; add only fixture tasks.
        config["tasks"] = {
            name: {"shell": f"python -m robocorp.tasks run local_tasks.py -t {name}"}
            for name in ("producer", "consumer")
        }
        # JSON is valid YAML and lets the fixture avoid adding a YAML dependency.
        robot_path.write_text(json.dumps(config))
        devdata = destination / "devdata"
        devdata.mkdir(exist_ok=True)
        (devdata / "input.json").write_text(json.dumps([
            {"payload": {"rows": [{"id": "fixture-1"}]}, "files": {}}
        ]))
        (devdata / "test-secrets.json").write_text('{"fixture":{"token":"test-only"}}')
        local_env = {
            "RC_WORKITEM_ADAPTER": "FileAdapter",
            "RC_WORKITEM_INPUT_PATH": "devdata/input.json",
            "RC_WORKITEM_OUTPUT_PATH": "devdata/producer-output.json",
            "RC_VAULT_SECRET_MANAGER": "FileSecrets",
            "RC_VAULT_SECRETS_FILE": "devdata/test-secrets.json",
        }
        for name, expected in (("producer", {"id": "fixture-1", "status": "new"}),
                               ("consumer", {"id": "fixture-1", "processed": True})):
            env_path = devdata / f"env-{name}.json"
            env_path.write_text(json.dumps(local_env))
            result = self.rcc("task", "run", "--robot", "robot.yaml", "--task", name,
                              "-e", str(env_path), cwd=destination)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("status: PASS", result.stdout + result.stderr)
            items = json.loads((destination / local_env["RC_WORKITEM_OUTPUT_PATH"]).read_text())
            self.assertEqual([item["payload"] for item in items], [expected])
            self.retain(destination / local_env["RC_WORKITEM_OUTPUT_PATH"])
            local_env["RC_WORKITEM_INPUT_PATH"] = local_env["RC_WORKITEM_OUTPUT_PATH"]
            local_env["RC_WORKITEM_OUTPUT_PATH"] = "devdata/consumer-output.json"


if __name__ == "__main__":
    unittest.main()
