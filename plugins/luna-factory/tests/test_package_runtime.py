"""Exercise the staging CLI against disposable source and build fixtures."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import sys
import tempfile
import unittest
import zlib


PLUGIN = Path(__file__).resolve().parents[1]
METADATA = (
    "plugin.json", "mcp.json", ".codex-plugin/plugin.json", ".mcp.json",
    ".claude-plugin/plugin.json", "plugin.yaml", "__init__.py", "README.md",
    "docs/package.md", "docs/local-service.md", "docs/repository-onboarding.md",
    "docs/control-adoption.md", "docs/control-plan.md", "docs/control-wire.md",
    "assets/logo.svg", "assets/logo.png",
)
SKILL_FILES = (
    "SKILL.md", "agents/openai.yaml", "references/evals.md",
    "references/routing-and-evidence.md", "references/runtime-compatibility.md",
    "scripts/audit_runtime.py", "tests/test_audit_runtime.py",
)


class PackageRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.plugin = self.base / "source" / "plugins" / "luna-factory"
        self.plugin.mkdir(parents=True)
        for name in METADATA:
            source = PLUGIN / name
            self.write(name, source.read_bytes() if source.exists() else "Local service runbook\n")
        for name in SKILL_FILES:
            self.write("skills/luna-factory/" + name, (PLUGIN / "skills/luna-factory" / name).read_text())
        for name in ("server/Cargo.toml", "server/Cargo.lock", "ui/package.json", "ui/package-lock.json",
                     "ui/index.html", "ui/tsconfig.json", "ui/vite.config.ts", "ui/tooling/singlefile.ts"):
            self.write(name, (PLUGIN / name).read_text())
        self.write("server/src/main.rs", "fn main() {}\n")
        self.write("ui/src/main.ts", "document.title = 'Luna Factory';\n")
        self.script = self.plugin / "scripts/package_runtime.py"
        self.script.parent.mkdir()
        if (PLUGIN / "scripts/package_runtime.py").exists():
            shutil.copyfile(PLUGIN / "scripts/package_runtime.py", self.script)
        for path in self.plugin.rglob("*"):
            if path.is_file():
                os.utime(path, (100, 100))
        self.binary = self.base / "build" / "luna-factoryd"
        self.binary.parent.mkdir()
        self.binary.write_text("#!/bin/sh\nprintf 'luna-factoryd 0.2.0\\n'\n")
        self.binary.chmod(0o755)
        self.ui = self.base / "build" / "index.html"
        self.ui.write_text('<!doctype html><html><head><title>Luna Factory</title><style>body{color:black}</style></head><body><div id="app"></div><script type="module">document.title="Luna Factory";</script></body></html>')
        self.output = self.base / "staged"

    def write(self, name, content):
        path = self.plugin / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content)
        return path

    def run_package(self, output=None):
        return subprocess.run(
            [sys.executable, str(self.script), "--binary", str(self.binary),
             "--ui", str(self.ui), "--output", str(output or self.output)],
            text=True, capture_output=True, timeout=15,
        )

    def assert_rejected(self, message, output=None):
        result = self.run_package(output)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertFalse((output or self.output).exists())
        return result

    def test_stages_one_complete_plugin_with_deterministic_receipt_and_checksums(self):
        result = self.run_package()
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = set(METADATA) | {"skills/luna-factory/" + p for p in SKILL_FILES}
        expected |= {"bin/luna-factoryd", "ui/dist/index.html", "runtime-receipt.json", "SHA256SUMS"}
        actual = {p.relative_to(self.output).as_posix() for p in self.output.rglob("*") if p.is_file()}
        self.assertEqual(actual, expected)
        receipt = json.loads((self.output / "runtime-receipt.json").read_text())
        self.assertEqual(receipt["plugin"], {"name": "luna-factory", "version": "0.2.0"})
        self.assertEqual(receipt["runtime_version"], "luna-factoryd 0.2.0")
        self.assertEqual(set(receipt["files"]), expected - {"runtime-receipt.json", "SHA256SUMS"})
        self.assertIn("server/src/main.rs", receipt["build_inputs"]["runtime"])
        self.assertIn("ui/src/main.ts", receipt["build_inputs"]["ui"])
        self.assertNotIn(str(self.base), (self.output / "runtime-receipt.json").read_text())
        sums = (self.output / "SHA256SUMS").read_text().splitlines()
        self.assertEqual({line.split("  ")[1] for line in sums}, expected - {"SHA256SUMS"})
        for line in sums:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        for name, record in receipt["files"].items():
            path = self.output / name
            self.assertEqual(record["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(record["size"], path.stat().st_size)
            expected_mode = 0o755 if name == "bin/luna-factoryd" else 0o644
            self.assertEqual(record["mode"], f"{expected_mode:04o}")
            self.assertEqual(path.stat().st_mode & 0o777, expected_mode)
        self.assertEqual((self.output / "bin/luna-factoryd").read_bytes(), self.binary.read_bytes())
        self.assertEqual((self.output / "ui/dist/index.html").read_bytes(), self.ui.read_bytes())
        second = self.base / "second"
        result = self.run_package(second)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((second / "SHA256SUMS").read_bytes(), (self.output / "SHA256SUMS").read_bytes())
        self.assertEqual((second / "runtime-receipt.json").read_bytes(), (self.output / "runtime-receipt.json").read_bytes())

    def test_missing_binary_is_rejected(self):
        self.binary.unlink()
        self.assert_rejected("binary")

    def branding(self, width=512, height=512, path="./assets/logo.png"):
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        png += chunk(b"IDAT", zlib.compress((b"\0" + b"\x7c\x3a\xed\xff" * width) * height)) + chunk(b"IEND", b"")
        asset = self.plugin / "assets/logo.png"
        asset.parent.mkdir(exist_ok=True)
        asset.write_bytes(png)
        os.utime(self.binary, None)
        for name in ("plugin.json", ".codex-plugin/plugin.json"):
            document = json.loads((self.plugin / name).read_text())
            interface = document["extensions"]["com.openai"]["interface"] if name == "plugin.json" else document["interface"]
            interface.update(logo=path, composerIcon=path)
            self.write(name, json.dumps(document))
        return png

    def test_branding_is_present_in_staging_and_checksum_inventory(self):
        png = self.branding()
        result = self.run_package()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.output / "assets/logo.png").is_file(), "manifest branding asset was not staged")
        self.assertEqual((self.output / "assets/logo.png").read_bytes(), png)
        self.assertIn("assets/logo.png", (self.output / "SHA256SUMS").read_text())

    def test_missing_non_square_small_and_escaping_branding_are_rejected(self):
        for index, (width, height, path) in enumerate([(512, 128, "./assets/logo.png"), (16, 16, "./assets/logo.png"), (512, 512, "../outside.png")]):
            with self.subTest(width=width, height=height, path=path):
                self.branding(width, height, path)
                self.assert_rejected("branding", self.base / f"invalid-branding-{index}")
        self.branding()
        (self.plugin / "assets/logo.png").unlink()
        self.assert_rejected("branding", self.base / "missing-branding")

    def test_missing_ui_is_rejected(self):
        self.ui.unlink()
        self.assert_rejected("UI")

    def test_non_executable_binary_is_rejected(self):
        self.binary.chmod(0o644)
        self.assert_rejected("executable")

    def test_stale_binary_is_rejected(self):
        os.utime(self.binary, (50, 50))
        self.assert_rejected("stale binary")

    def test_stale_ui_is_rejected(self):
        os.utime(self.ui, (50, 50))
        self.assert_rejected("stale UI")

    def test_wrong_binary_version_is_rejected(self):
        self.binary.write_text("#!/bin/sh\nprintf 'luna-factoryd 0.1.0\\n'\n")
        self.assert_rejected("binary version")

    def test_binary_version_failure_is_rejected(self):
        self.binary.write_text("#!/bin/sh\nexit 1\n")
        self.assert_rejected("binary version")

    def test_wrong_plugin_identity_is_rejected(self):
        data = json.loads((self.plugin / "plugin.json").read_text())
        data["name"] = "luna-factory-app"
        self.write("plugin.json", json.dumps(data))
        self.assert_rejected("identity")

    def test_version_mismatches_are_rejected(self):
        for name in (".codex-plugin/plugin.json", ".claude-plugin/plugin.json", "plugin.yaml", "ui/package.json"):
            with self.subTest(name=name):
                original = (self.plugin / name).read_text()
                data = json.loads(original)
                data["version"] = "9.9.9"
                self.write(name, json.dumps(data))
                self.assert_rejected("version")
                self.write(name, original)

    def test_runtime_crate_version_mismatch_is_rejected(self):
        path = self.plugin / "server/Cargo.toml"
        path.write_text(path.read_text().replace('version = "0.2.0"', 'version = "9.9.9"', 1))
        self.assert_rejected("version")

    def test_compatibility_interface_drift_is_rejected(self):
        path = self.plugin / ".codex-plugin/plugin.json"
        data = json.loads(path.read_text())
        data["interface"]["displayName"] = "Another app"
        path.write_text(json.dumps(data))
        self.assert_rejected("interface")

    def test_mcp_endpoint_drift_or_credentials_are_rejected(self):
        for value in ({"type": "http", "url": "http://127.0.0.1:8888/mcp"},
                      {"type": "http", "url": "http://127.0.0.1:8787/mcp", "headers": {"Authorization": "fake-test-value"}}):
            with self.subTest(value=value):
                self.write(".mcp.json", json.dumps({"mcpServers": {"luna-factory": value}}))
                self.assert_rejected("MCP")

    def test_external_ui_assets_are_rejected(self):
        for asset in ('<script src="/src/main.ts"></script>', '<link rel="stylesheet" href="assets/x.css">', '<img src="https://example.invalid/a.png">'):
            with self.subTest(asset=asset):
                self.ui.write_text('<!doctype html><html><body><div id="app"></div>' + asset + '</body></html>')
                self.assert_rejected("self-contained")

    def test_empty_and_oversize_ui_are_rejected(self):
        for content in ("", "x" * (4 * 1024 * 1024 + 1)):
            with self.subTest(size=len(content)):
                self.ui.write_text(content)
                self.assert_rejected("UI")

    def test_existing_destination_is_not_replaced(self):
        self.output.mkdir()
        marker = self.output / "keep.txt"
        marker.write_text("keep")
        result = self.run_package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exists", result.stderr)
        self.assertEqual(marker.read_text(), "keep")
        self.assertEqual(list(self.output.iterdir()), [marker])

    def test_source_overlap_is_rejected(self):
        self.assert_rejected("overlap", self.plugin / "staged")

    def test_output_inside_source_checkout_is_rejected(self):
        self.assert_rejected("overlap", self.base / "source" / "staged")

    def test_traversing_output_is_rejected(self):
        result = self.run_package(self.base / "build" / ".." / "staged")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("traversal", result.stderr)
        self.assertFalse(self.output.exists())

    def test_non_regular_ui_is_rejected(self):
        self.ui.unlink()
        self.ui.mkdir()
        self.assert_rejected("regular file")

    def test_unexpected_sensitive_build_input_is_rejected(self):
        self.write("ui/src/.env", "FAKE_TEST_TOKEN=not-a-secret")
        self.assert_rejected("unexpected build input")

    def test_output_parent_must_exist(self):
        self.assert_rejected("parent", self.base / "missing" / "staged")
        self.assertFalse((self.base / "missing").exists())

    def test_symlink_output_parent_is_rejected(self):
        target = self.base / "target"
        target.mkdir()
        link = self.base / "linked"
        link.symlink_to(target, target_is_directory=True)
        self.assert_rejected("symlink", link / "staged")
        self.assertEqual(list(target.iterdir()), [])

    def test_symlink_input_and_skill_escape_are_rejected(self):
        for source in (self.binary, self.ui, self.plugin / "skills/luna-factory/SKILL.md"):
            with self.subTest(source=source):
                saved = source.read_bytes()
                mode = source.stat().st_mode
                target = self.base / "elsewhere"
                target.write_bytes(saved)
                source.unlink()
                source.symlink_to(target)
                self.assert_rejected("symlink")
                source.unlink()
                source.write_bytes(saved)
                source.chmod(mode)

    def test_unexpected_sensitive_skill_file_is_rejected(self):
        self.write("skills/luna-factory/.env", "FAKE_TEST_TOKEN=not-a-secret")
        self.assert_rejected("unexpected skill")

    def test_alternate_skill_is_rejected(self):
        self.write("skills/other/SKILL.md", "---\nname: other\n---\n")
        self.assert_rejected("one canonical skill")

    def test_unselected_local_files_and_caches_are_not_copied(self):
        self.write(".env", "FAKE_TEST_TOKEN=not-a-secret")
        self.write("server/target/ignored", "not a runtime")
        self.write("ui/node_modules/ignored", "not a UI")
        self.write("skills/luna-factory/scripts/__pycache__/audit_runtime.cpython-312.pyc", "cache")
        result = self.run_package()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.output / ".env").exists())
        self.assertFalse((self.output / "server").exists())
        self.assertFalse((self.output / "ui/node_modules").exists())
        self.assertFalse((self.output / "skills/luna-factory/scripts/__pycache__").exists())


if __name__ == "__main__":
    unittest.main()
