import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "container/execution_paths.py"
SPEC = importlib.util.spec_from_file_location("execution_paths", SCRIPT)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


class ExecutionPathTests(unittest.TestCase):
    def test_two_namespace_receipt_compares_bytes_and_worktree_without_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            local, host = root / "factory", root / "host"
            local.mkdir()
            repository = local / "canary"
            repository.mkdir()
            subprocess.run(["git", "init", "-q", str(repository)], check=True)
            subprocess.run(["git", "-C", str(repository), "-c", "user.name=Fixture", "-c", "user.email=fixture@localhost",
                            "commit", "--allow-empty", "-qm", "fixture"], check=True)
            (repository / "proof.txt").write_text("untracked fixture\n")
            for name in ["factory-binary", "codex-binary", "SKILL.md"]:
                (local / name).write_text("fixture only, never executable\n")
            shutil.copytree(local, host)
            config = {"native_transport": "existing_daemon", "native_paths": {str(local): str(host)},
                      "codex_binary": str(local / "codex-binary"), "skill_path": str(local / "SKILL.md"),
                      "repositories": {"approved": {"root": str(repository)}}}
            request = PROBE.prepare(config, local / "factory-binary", repository)
            receipt = PROBE.verify(request)
            self.assertTrue(receipt["passed"])
            self.assertFalse(receipt["qualification_grant"])
            self.assertEqual(receipt["inference_calls"], 0)
            self.assertEqual(receipt["host_identity"], "operator_must_verify")
            self.assertNotIn("native_path", json.dumps(receipt))
            (host / "canary/proof.txt").write_text("changed host content")
            self.assertFalse(PROBE.verify(request)["passed"])
            (host / "codex-binary").write_text("changed daemon binary")
            receipt = PROBE.verify(request)
            self.assertIn({"role": "codex_binary", "passed": False}, receipt["checks"])

    def test_unmapped_or_ambiguous_paths_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertRaises(ValueError, PROBE.mapped, {"native_paths": {}}, root)
            self.assertRaises(ValueError, PROBE.mapped,
                              {"native_paths": {str(root): "/host/root", str(root / "nested"): "/host/nested"}}, root / "nested")
            for path in ["relative", "/", "/host/../escape"]:
                self.assertRaises(ValueError, PROBE.absolute, path)

    def test_receipt_output_is_private_and_never_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / "file"
            file.write_text("fixture")
            request = root / "request.json"
            request.write_text(json.dumps({"schema": "luna-factory.execution-path-request.v1", "entries": [
                {"role": "fixture", "native_path": str(file), "kind": "file", "expected": PROBE.digest_file(file)}]}))
            receipt = root / "receipt.json"
            command = ["python3", str(SCRIPT), "--output", str(receipt), "verify", "--request", str(request)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual(receipt.stat().st_mode & 0o777, 0o600)
            before = receipt.read_bytes()
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual(receipt.read_bytes(), before)
