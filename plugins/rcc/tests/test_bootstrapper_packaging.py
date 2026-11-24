from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import unittest
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from plugins.rcc.scripts import build_bootstrapper_package as builder
from plugins.rcc.scripts import install_bootstrapper as installer

sys.path.insert(0, str(Path(__file__).parents[1] / "bootstrapper" / "src"))
from rcc_bootstrapper.policy import PolicyViolationError, resolve_binary  # noqa: E402


def package_zip(path: Path) -> bytes:
    source = Path(__file__).parents[1] / "bootstrapper" / "src" / "rcc_bootstrapper"
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("package.yaml", "spec-version: v2\n")
        archive.writestr("__action_server_metadata__.json", "{}")
        archive.writestr("src/rcc_bootstrapper/__init__.py", (source / "__init__.py").read_bytes())
        archive.writestr("src/rcc_bootstrapper/models.py", (source / "models.py").read_bytes())
        archive.writestr("src/rcc_bootstrapper/policy.py", (source / "policy.py").read_bytes())
        archive.writestr("src/rcc_bootstrapper/bootstrapper_actions.py", "# fixture\n")
    return path.read_bytes()


class PackagingTests(unittest.TestCase):
    def test_package_builder_rejects_incompatible_compressor_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "not-created" / "package.zip"
            with patch.object(builder.zlib, "ZLIB_RUNTIME_VERSION", "1.3.1.zlib-ng"):
                with self.assertRaisesRegex(RuntimeError, "standard zlib"):
                    builder.build("action-server", output)
            self.assertFalse(output.parent.exists())

    def test_package_builder_preserves_home_and_sets_runtime_cache_homes(self) -> None:
        captured: dict[str, str] = {}

        def fake_package_build(args, **kwargs):
            captured.update(kwargs["env"])
            output_dir = Path(args[args.index("--output-dir") + 1])
            with zipfile.ZipFile(output_dir / "rcc-actions-bootstrapper.zip", "w") as archive:
                archive.writestr("package.yaml", "spec-version: v2\n")
                archive.writestr("__action_server_metadata__.json", "{}")
                archive.writestr("src/rcc_bootstrapper/bootstrapper_actions.py", "# fixture\n")
            return SimpleNamespace(returncode=0, stderr="")

        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "package.zip"
            with (
                patch.dict(os.environ, {"HOME": "/caller/home", "USERPROFILE": r"C:\\Caller\\Profile"}),
                patch.object(builder.subprocess, "run", side_effect=fake_package_build),
            ):
                builder.build("action-server", output)
            self.assertEqual(captured["HOME"], "/caller/home")
            self.assertEqual(captured["USERPROFILE"], r"C:\\Caller\\Profile")
            for key in ("ACTIONS_HOME", "ROBOTS_HOME", "ROBOCORP_HOME", "UV_CACHE_DIR"):
                self.assertIn("runtime-state", captured[key])
            self.assertEqual(zipfile.ZipFile(output).namelist(), [
                "__action_server_metadata__.json",
                "package.yaml",
                "src/rcc_bootstrapper/bootstrapper_actions.py",
            ])

    def test_setup_import_uses_only_stdlib_and_windows_assets_are_exe(self) -> None:
        subprocess.run(
            [
                sys.executable,
                "-S",
                "-c",
                "import sys; from plugins.rcc.scripts import install_bootstrapper; "
                "assert 'pydantic' not in sys.modules",
            ],
            check=True,
        )
        pin = installer.platform_pin("win32", "AMD64")
        self.assertTrue(pin[0].endswith(".exe"))
        self.assertTrue(pin[2].endswith(".exe"))

    def test_runtime_launcher_imports_policy_from_extracted_package_without_pydantic(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive_path = root / "package.zip"
            package_zip(archive_path)
            package_dir = root / "versions" / "0.6.0" / "package"
            installer.extract_package(archive_path, package_dir)
            shutil.copyfile(archive_path, package_dir.parent / "package.zip")
            env = {
                **os.environ,
                "RCC_BOOTSTRAPPER_HOME": str(root),
                "RCC_BOOTSTRAPPER_PACKAGE_DIR": str(package_dir),
            }
            runtime_script = Path(__file__).parents[1] / "scripts" / "bootstrapper_runtime.py"
            result = subprocess.run(
                [sys.executable, "-S", str(runtime_script), "--help"],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("start", result.stdout)
            self.assertNotIn("pydantic", result.stderr.lower())
            tampered = package_dir / "src/rcc_bootstrapper/policy.py"
            original = tampered.read_bytes()
            tampered.write_bytes(original + b"# tampered\n")
            rejected = subprocess.run(
                [sys.executable, "-S", str(runtime_script), "--help"],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("contents differ", rejected.stderr)

    def test_platform_gate_rejects_unsupported_before_creating_home(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp) / "must-not-exist"
            with patch.object(installer.platform, "machine", return_value="arm64"):
                with self.assertRaisesRegex(RuntimeError, "unsupported platform"):
                    installer.install(home)
            self.assertFalse(home.exists())

    def test_install_rejects_symlink_versions_directory(self) -> None:
        if not hasattr(os, "symlink"):
            self.skipTest("symlinks are unavailable")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            home = root / "home"
            home.mkdir(mode=0o700)
            outside = root / "outside"
            outside.mkdir()
            try:
                (home / "versions").symlink_to(outside, target_is_directory=True)
            except OSError as exc:
                self.skipTest(f"symlink creation unavailable: {exc}")
            with self.assertRaisesRegex(RuntimeError, "redirected user state"):
                installer.install(home)
            self.assertEqual(list(outside.iterdir()), [])

    def test_installed_versioned_binary_precedes_global_path_binary(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            runtime_name = installer.platform_pin()[0]
            pinned = home / "versions" / "0.6.0" / "bin" / runtime_name
            pinned.parent.mkdir(parents=True)
            pinned.write_bytes(b"pinned")
            with (
                patch.dict(os.environ, {"RCC_BOOTSTRAPPER_HOME": str(home)}),
                patch.object(installer.shutil, "which", return_value="/usr/bin/action-server"),
            ):
                self.assertEqual(resolve_binary("action-server"), pinned.resolve())

    def test_runtime_binary_resolution_rejects_redirected_version_tree(self) -> None:
        if os.name == "nt":
            self.skipTest("directory symlinks may require elevation on Windows")
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            outside = home / "outside"
            outside.mkdir()
            (home / "versions").symlink_to(outside, target_is_directory=True)
            with patch.dict(os.environ, {"RCC_BOOTSTRAPPER_HOME": str(home)}):
                with self.assertRaisesRegex(PolicyViolationError, "redirected versioned"):
                    resolve_binary("action-server")

    def test_zip_extraction_rejects_traversal_and_symlink(self) -> None:
        for entry_name, mode in (
            ("../escape", 0),
            ("C:/outside", 0),
            ("root/link", 0o120777),
            ("./package.yaml", 0),
            ("package.yaml/child", 0),
        ):
            with self.subTest(entry_name=entry_name), tempfile.TemporaryDirectory() as temp:
                archive_path = Path(temp) / "bad.zip"
                with zipfile.ZipFile(archive_path, "w") as archive:
                    for name in (
                        "package.yaml",
                        "__action_server_metadata__.json",
                        "src/rcc_bootstrapper/bootstrapper_actions.py",
                    ):
                        archive.writestr(name, "x")
                    info = zipfile.ZipInfo(entry_name)
                    info.external_attr = mode << 16
                    archive.writestr(info, "bad")
                with self.assertRaisesRegex(RuntimeError, "unsafe|duplicate|ZIP"):
                    installer.extract_package(archive_path, Path(temp) / "out")

    def test_download_checksum_failure_does_not_publish_file(self) -> None:
        class Response(io.BytesIO):
            status = 200

            def __enter__(self):
                return self

            def geturl(self):
                return "https://example.invalid/binary"

            def __exit__(self, *_args):
                self.close()

        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "binary"
            with patch.object(installer.urllib.request, "urlopen", return_value=Response(b"wrong")):
                with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                    installer.download("https://example.invalid/binary", destination, "0" * 64)
            self.assertFalse(destination.exists())

    def test_download_retries_transient_http_errors(self) -> None:
        class Response(io.BytesIO):
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                self.close()

            def geturl(self):
                return "https://example.invalid/binary"

        content = b"pinned binary"
        temporary_error = urllib.error.HTTPError("https://example.invalid/binary", 503, "busy", {}, io.BytesIO())
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "binary"
            with (
                patch.object(
                    installer.urllib.request,
                    "urlopen",
                    side_effect=[temporary_error, Response(content)],
                ) as open_url,
                patch.object(installer.time, "sleep"),
            ):
                installer.download("https://example.invalid/binary", destination, hashlib.sha256(content).hexdigest())
            self.assertEqual(open_url.call_count, 2)
            self.assertEqual(destination.read_bytes(), content)

    def test_download_closes_temp_handle_before_windows_style_replace(self) -> None:
        class Response(io.BytesIO):
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                self.close()

            def geturl(self):
                return "https://example.invalid/binary"

        content = b"Windows sharing semantics"
        handles = []
        original_named_temp = installer.tempfile.NamedTemporaryFile
        original_replace = installer.os.replace

        def capture_temp(*args, **kwargs):
            handle = original_named_temp(*args, **kwargs)
            handles.append(handle)
            return handle

        def windows_style_replace(source: Path, destination: Path) -> None:
            if not handles[-1].closed:
                raise PermissionError("sharing violation: source still open")
            original_replace(source, destination)

        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "binary"
            with (
                patch.object(installer.tempfile, "NamedTemporaryFile", side_effect=capture_temp),
                patch.object(installer.os, "replace", side_effect=windows_style_replace),
                patch.object(installer.urllib.request, "urlopen", return_value=Response(content)),
            ):
                installer.download("https://example.invalid/binary", destination, hashlib.sha256(content).hexdigest())
            self.assertTrue(handles[0].closed)
            self.assertEqual(destination.read_bytes(), content)

    def test_failed_binary_download_never_publishes_partial_version(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            data = package_zip(root / "fixture.zip")
            digest = hashlib.sha256(data).hexdigest()
            pins = (
                "runtime",
                "a" * 64,
                "rcc",
                "b" * 64,
                "https://example.invalid/runtime",
                "https://example.invalid/rcc",
            )

            def fail_second_download(url: str, destination: Path, _sha: str) -> None:
                if url.endswith("runtime"):
                    raise RuntimeError("simulated unavailable runtime binary")
                destination.write_bytes(data)

            home = root / "private-home"
            with (
                patch.object(installer, "platform_pin", return_value=pins),
                patch.object(installer, "PACKAGE_SHA256", digest),
                patch.object(installer, "download", side_effect=fail_second_download),
            ):
                with self.assertRaisesRegex(RuntimeError, "unavailable runtime"):
                    installer.install(home, package_url="https://example.invalid/package")
            self.assertFalse((home / "versions" / "0.6.0").exists())
            self.assertFalse(any(path.name.startswith(".setup-") for path in home.iterdir()))

    def test_install_is_atomic_idempotent_and_keeps_state_separate(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_root = Path(temp)
            package_data = package_zip(temp_root / "fixture.zip")
            package_sha = hashlib.sha256(package_data).hexdigest()
            runtime_data, rcc_data = b"runtime", b"rcc"
            runtime_sha = hashlib.sha256(runtime_data).hexdigest()
            rcc_sha = hashlib.sha256(rcc_data).hexdigest()
            pins = (
                "runtime.exe",
                runtime_sha,
                "rcc.exe",
                rcc_sha,
                "https://example.invalid/runtime",
                "https://example.invalid/rcc",
            )
            calls: list[str] = []

            def fake_download(url: str, destination: Path, expected_sha256: str) -> None:
                calls.append(url)
                data = package_data if url == "https://package.invalid" else (
                    runtime_data if url.endswith("runtime") else rcc_data
                )
                self.assertEqual(hashlib.sha256(data).hexdigest(), expected_sha256)
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(data)

            home = temp_root / "user" / "rcc-bootstrapper"
            with (
                patch.object(installer, "platform_pin", return_value=pins),
                patch.object(installer, "PACKAGE_SHA256", package_sha),
                patch.object(installer, "PACKAGE_URL", "https://package.invalid"),
                patch.object(installer, "download", side_effect=fake_download),
                patch.object(installer.sys, "platform", "win32"),
            ):
                first = installer.install(home, package_url="https://package.invalid")
                package_dir = Path(first["package_dir"])
                tracked = package_dir / "src/rcc_bootstrapper/policy.py"
                original = tracked.read_bytes()
                tracked.write_bytes(original + b"# modified\n")
                with self.assertRaisesRegex(RuntimeError, "contents differ"):
                    installer.install(home, package_url="https://package.invalid")
                tracked.write_bytes(original)
                extra = package_dir / "unexpected.txt"
                extra.write_text("unexpected", encoding="utf-8")
                with self.assertRaisesRegex(RuntimeError, "file tree differs"):
                    installer.install(home, package_url="https://package.invalid")
                extra.unlink()
                second = installer.install(home, package_url="https://package.invalid")
                if os.name != "nt":
                    state = home / "state"
                    robocorp = state / "robocorp"
                    saved_robocorp = state / "robocorp.saved"
                    robocorp.rename(saved_robocorp)
                    robocorp.symlink_to(temp_root / "outside", target_is_directory=True)
                    with self.assertRaisesRegex(RuntimeError, "redirected user state"):
                        installer.install(home, package_url="https://package.invalid")
                    robocorp.unlink()
                    saved_robocorp.rename(robocorp)
            self.assertEqual(first, second)
            self.assertEqual(len(calls), 3)
            versions = Path(first["installation"])
            self.assertEqual(installer.digest_file(versions / "package.zip"), package_sha)
            self.assertTrue((Path(first["package_dir"]) / "src/rcc_bootstrapper/bootstrapper_actions.py").is_file())
            self.assertEqual(Path(first["ROBOCORP_HOME"]), home / "state" / "robocorp")
            self.assertFalse((home / "versions" / "0.6.0" / "state").exists())
            receipt = json.loads((versions / "install-receipt.json").read_text(encoding="utf-8"))
            self.assertEqual(receipt["package_sha256"], package_sha)
            private_dirs = [home, home / "versions", versions, home / "state", home / "state" / "robocorp"]
            for directory in private_dirs:
                self.assertTrue(directory.is_dir())
                if os.name != "nt":
                    self.assertEqual(directory.stat().st_mode & 0o777, 0o700)

    def test_candidate_digest_is_required_at_install_and_source_free_launch(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "candidate.zip"
            digest = hashlib.sha256(package_zip(archive)).hexdigest()
            home = root / "install"
            runtime_name, _, rcc_name, _, runtime_url, rcc_url = installer.platform_pin()
            binary = b"native fixture"
            binary_sha256 = hashlib.sha256(binary).hexdigest()
            pins = (runtime_name, binary_sha256, rcc_name, binary_sha256, runtime_url, rcc_url)

            def fake_download(url: str, destination: Path, expected_sha256: str) -> None:
                self.assertIn(url, (runtime_url, rcc_url))
                self.assertEqual(expected_sha256, binary_sha256)
                destination.write_bytes(binary)

            with (
                patch.object(installer, "platform_pin", return_value=pins),
                patch.object(installer, "download", side_effect=fake_download) as download,
            ):
                with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                    installer.install(home, package_archive=archive)
                with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                    installer.install(home, package_archive=archive, candidate_package_sha256="0" * 64)
                download.assert_not_called()
                installed = installer.install(home, package_archive=archive, candidate_package_sha256=digest)
                self.assertEqual(
                    installed,
                    installer.install(home, package_archive=archive, candidate_package_sha256=digest),
                )
                self.assertEqual(download.call_count, 2)
            self.assertFalse((home / "versions" / installer.PACKAGE_VERSION).exists())

            launcher = Path(__file__).resolve().parents[1] / "scripts" / "bootstrapper_runtime.py"
            env = {
                **os.environ,
                "RCC_BOOTSTRAPPER_HOME": str(home),
                "RCC_BOOTSTRAPPER_PACKAGE_DIR": installed["package_dir"],
                "RCC_BOOTSTRAPPER_CANDIDATE_SHA256": digest,
            }
            env.pop("PYTHONPATH", None)
            command = [sys.executable, "-S", str(launcher), "--help"]
            result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("start", result.stdout)

            # A ZIP comment changes its digest without changing any extracted file.
            with zipfile.ZipFile(installed["package_archive"], "a") as staged:
                staged.comment = b"modified after install"
            result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("installed package ZIP pin mismatch", result.stderr)


if __name__ == "__main__":
    unittest.main()
