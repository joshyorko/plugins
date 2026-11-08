"""Bounded regression checks for verification failures and process cleanup."""

import ctypes
import hashlib
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "plugins/rcc/skills/verify-rcc/scripts/verify-rcc"
ONBOARDING = ROOT / "plugins/rcc/tests/test_onboarding.py"


@unittest.skipUnless(os.name == "posix", "Verifier requires POSIX process groups and locks")
class RccVerifierTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="rcc-verifier-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.evidence = self.root / "evidence"
        self.evidence.mkdir()

    def test_changed_or_removed_source_prevents_command_execution(self):
        for removed in (False, True):
            with self.subTest(removed=removed):
                repo = self.root / f"repo-{removed}"
                repo.mkdir()
                run = self.root / f"run-{removed}"
                run.mkdir()
                source = repo / "tracked.py"
                if not removed:
                    source.write_text("changed source")
                state = {
                    "run_dir": str(run), "repo": str(repo),
                    "evidence_dir": str(self.evidence), "python": sys.executable,
                    "inputs": {"tracked.py": hashlib.sha256(b"original source").hexdigest()},
                    "features": {},
                }
                (run / ".verify-rcc.json").write_text(json.dumps(state))
                result = subprocess.run(
                    [sys.executable, str(HELPER), "drive", "health", "--run-dir",
                     str(run), "--repo", str(repo)], capture_output=True, text=True, timeout=5,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Source changed since launch: tracked.py", result.stderr)
                self.assertEqual(list(self.evidence.glob("command-*.json")), [])
                receipt = json.loads((self.evidence / "receipt.json").read_text())
                self.assertEqual(receipt["features"]["health"], "failed")

    def strict_onboarding(self, *extra, script=ONBOARDING):
        environment = os.environ.copy()
        environment.update(RCC_ONBOARDING_RUNTIME="1", PATH=str(self.root / "empty-bin"))
        return subprocess.run(
            [sys.executable, str(script), "--require-runtime", *extra],
            env=environment, text=True, capture_output=True, timeout=5,
        )

    def test_missing_rcc_cannot_pass_through_skipped_runtime_tests(self):
        result = self.strict_onboarding()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("skipped=2", result.stderr)

    def test_empty_runtime_suite_cannot_pass(self):
        empty_suite = self.root / "empty_onboarding.py"
        source = ONBOARDING.read_text().replace("    def test_", "    def removed_")
        empty_suite.write_text(source)
        baseline = subprocess.run([sys.executable, str(empty_suite)], capture_output=True,
                                  text=True, timeout=5)
        self.assertIn("Ran 0 tests", baseline.stderr)
        result = self.strict_onboarding(script=empty_suite)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ERROR", result.stderr)

    def test_timeout_reaps_the_child_and_retains_failure_evidence(self):
        helper = runpy.run_path(str(HELPER))
        pid_file = self.root / "timeout-child.pid"
        child = ("import os,time; from pathlib import Path; "
                 f"Path({str(pid_file)!r}).write_text(str(os.getpid())); time.sleep(8)")
        handler_before = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(subprocess.TimeoutExpired):
            helper["command"]([sys.executable, "-c", child], self.root, self.evidence,
                              os.environ.copy(), timeout=0.5)
        self.assertEqual(signal.getsignal(signal.SIGTERM), handler_before)
        record = json.loads((self.evidence / "command-001.json").read_text())
        self.assertTrue(record["interrupted"])
        self.assertEqual(record["error"], "TimeoutExpired")
        self.assertEqual(record["exit_code"], -signal.SIGKILL)
        if pid_file.exists():
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pid_file.read_text()), 0)

    @unittest.skipUnless(sys.platform == "linux", "Uses Linux child-subreaper cleanup")
    def test_sigterm_reaps_the_child_and_retains_failure_evidence(self):
        # Adopt a regressed helper's orphan so even a failing test reaps it.
        libc = ctypes.CDLL(None, use_errno=True)
        previous = ctypes.c_int()
        self.assertEqual(libc.prctl(37, ctypes.byref(previous), 0, 0, 0), 0)
        self.assertEqual(libc.prctl(36, 1, 0, 0, 0), 0)
        pid_file = self.root / "signal-child.pid"
        child = ("import os,time; from pathlib import Path; "
                 f"Path({str(pid_file)!r}).write_text(str(os.getpid())); time.sleep(8)")
        wrapper = ("import os,runpy,sys\n"
                   f"helper=runpy.run_path({str(HELPER)!r})\n"
                   f"helper['command']([sys.executable,'-c',{child!r}],"
                   f"helper['Path']({str(self.root)!r}),"
                   f"helper['Path']({str(self.evidence)!r}),os.environ.copy(),timeout=10)\n")
        process = None
        child_pid = None
        try:
            process = subprocess.Popen([sys.executable, "-c", wrapper],
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, start_new_session=True)
            deadline = time.monotonic() + 3
            while not pid_file.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(pid_file.exists(), "Bounded fixture did not start")
            child_pid = int(pid_file.read_text())
            process.send_signal(signal.SIGTERM)
            process.communicate(timeout=3)
            self.assertEqual(process.returncode, 128 + signal.SIGTERM)
            with self.assertRaises(ProcessLookupError):
                os.kill(child_pid, 0)
            record = json.loads((self.evidence / "command-001.json").read_text())
            self.assertTrue(record["interrupted"])
            self.assertEqual(record["error"], "SystemExit")
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=3)
            if child_pid is None and pid_file.exists():
                child_pid = int(pid_file.read_text())
            if child_pid is not None:
                try:
                    os.killpg(child_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    os.waitpid(child_pid, 0)
                except ChildProcessError:
                    pass
            libc.prctl(36, previous.value, 0, 0, 0)


if __name__ == "__main__":
    unittest.main()
