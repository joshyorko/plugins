"""Release archives from disposable clean Git sources and a compiled ELF fixture."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import textwrap
import unittest

import test_package_runtime as package_fixture


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self):
        fixture = package_fixture.PackageRuntimeTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.fixture = fixture
        self.repo = fixture.base / "source"
        self.script = fixture.plugin / "scripts/build_release_archive.py"
        source = package_fixture.PLUGIN / "scripts/build_release_archive.py"
        if source.exists():
            shutil.copyfile(source, self.script)
        # A real local ELF exercises readelf/getconf and extracted --version;
        # no Codex process, operator state, or network is involved.
        subprocess.run(["cc", "-x", "c", "-o", str(fixture.binary), "-"],
                       input='#include <stdio.h>\nint main(void){puts("luna-factoryd 0.2.0");}\n',
                       text=True, check=True, capture_output=True)
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "-c", "core.hooksPath=/dev/null", "commit", "-qm", "release fixture")
        self.sha = self.git("rev-parse", "HEAD")
        self.tree = self.git("rev-parse", "HEAD^{tree}")
        self.git("tag", "v0.2.0")
        self.output = fixture.base / "release"

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def release(self, output=None, sha=None, tag="v0.2.0"):
        return subprocess.run([sys.executable, str(self.script), "--binary", str(self.fixture.binary),
                               "--ui", str(self.fixture.ui), "--output", str(output or self.output),
                               "--source-sha", sha or self.sha, "--tag", tag],
                              capture_output=True, text=True, timeout=30)

    def test_archive_is_deterministic_source_bound_and_extractable(self):
        first = self.release()
        self.assertEqual(first.returncode, 0, first.stderr)
        other = self.fixture.base / "other-release"
        # Input mtimes must not leak into archive or provenance.
        os.utime(self.fixture.binary, None)
        os.utime(self.fixture.ui, None)
        second = self.release(other)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(sorted(p.name for p in self.output.iterdir()),
                         sorted(p.name for p in other.iterdir()))
        for path in self.output.iterdir():
            self.assertEqual(path.read_bytes(), (other / path.name).read_bytes(), path.name)
        archive, = self.output.glob("*.tar.gz")
        provenance, = self.output.glob("*-provenance.json")
        record = json.loads(provenance.read_text())
        self.assertEqual(record["source"]["commit"], self.sha)
        self.assertEqual(record["source"]["tree"], self.tree)
        self.assertEqual(record["source"]["tag"], "v0.2.0")
        self.assertEqual(record["build"]["target"], "x86_64-unknown-linux-gnu")
        self.assertEqual(record["artifact"]["sha256"], hashlib.sha256(archive.read_bytes()).hexdigest())
        self.assertTrue(record["elf"]["glibc_symbol_versions"])
        self.assertTrue(record["verification"]["restaging_identical"])
        self.assertTrue(record["verification"]["extracted_checksums"])
        self.assertNotIn(str(self.fixture.base), provenance.read_text())
        extracted = self.fixture.base / "extracted"
        with tarfile.open(archive) as tar:
            members = tar.getmembers()
            self.assertTrue(all(m.uid == m.gid == 0 and not m.uname and not m.gname for m in members))
            self.assertTrue(all(m.mtime == record["source"]["timestamp"] for m in members))
            self.assertEqual([m.name for m in members], sorted(m.name for m in members))
            tar.extractall(extracted, filter="data")
        staged, = extracted.iterdir()
        subprocess.run(["sha256sum", "--check", "SHA256SUMS"], cwd=staged, check=True, capture_output=True)
        self.assertEqual(subprocess.check_output([staged / "bin/luna-factoryd", "--version"], text=True).strip(), "luna-factoryd 0.2.0")
        self.assertEqual((staged / "bin/luna-factoryd").stat().st_mode & 0o777, 0o755)

    def test_wrong_source_tag_and_dirty_checkout_are_rejected_without_artifacts(self):
        for kwargs, reason in [({"sha":"0" * 40}, "source SHA"), ({"tag":"v9.9.9"}, "tag")]:
            with self.subTest(reason=reason):
                result = self.release(**kwargs)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(reason, result.stderr)
                self.assertFalse(self.output.exists())
        (self.repo / "untracked.txt").write_text("uncommitted input")
        result = self.release()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("clean", result.stderr)
        self.assertFalse(self.output.exists())

    def test_non_elf_and_existing_output_are_rejected(self):
        self.fixture.binary.write_text("#!/bin/sh\necho luna-factoryd 0.2.0\n")
        result = self.release()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ELF", result.stderr)
        self.assertFalse(self.output.exists())
        self.output.mkdir()
        sentinel = self.output / "do-not-replace"
        sentinel.write_text("preserve")
        result = self.release()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), "preserve")

    def test_release_tag_must_resolve_to_the_exact_source_commit(self):
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-qm", "different source")
        changed = self.git("rev-parse", "HEAD")
        result = self.release(sha=changed)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tag", result.stderr)
        self.assertFalse(self.output.exists())

    def test_hosted_version_gate_selects_only_current_plugin_tag(self):
        workflow = (package_fixture.PLUGIN.parents[1] / ".github/workflows/release-artifacts.yml").read_text()
        step = workflow.split("      - name: Select versioned Luna runtime bundle\n", 1)[1].split("\n      - name:", 1)[0]
        script = textwrap.dedent(step.split("        run: |\n", 1)[1])
        for index, (tag, expected) in enumerate([("v0.2.0", "true"), ("v9.9.9", "false"), ("v0.2.0-rc1", "false")]):
            with self.subTest(tag=tag):
                output = self.fixture.base / f"gate-{index}"
                subprocess.run(["bash", "-e", "-c", script], cwd=self.repo, check=True,
                               env=dict(os.environ, RELEASE_TAG=tag, GITHUB_OUTPUT=str(output)),
                               capture_output=True, text=True)
                self.assertEqual(output.read_text(), f"enabled={expected}\n")


if __name__ == "__main__":
    unittest.main()
