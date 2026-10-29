from __future__ import annotations

import errno
import importlib.util
import json
import os
import shutil
import shlex
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "plugins/rcc/skills/rcc-core/scripts/tealdeer.py"


class ManagedTealdeerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.assertTrue(HELPER.is_file(), "packaged Tealdeer synchronizer is missing")
        spec = importlib.util.spec_from_file_location("rcc_tealdeer", HELPER)
        self.helper = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = self.helper
        spec.loader.exec_module(self.helper)
        temporary = tempfile.TemporaryDirectory(prefix="rcc-tealdeer-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.make_source("checkout", "first revision")
        self.paths = self.helper.Paths(
            page=self.root / "custom pages/rcc.page.md",
            cache=self.root / "cache/tldr-pages",
            state=self.root / "state/rcc-tealdeer/state.json",
        )

    def make_source(self, name: str, revision: str) -> Path:
        plugin = self.root / name / "plugins/rcc"
        page = plugin / "skills/rcc-core/references/tldr/rcc.page.md"
        page.parent.mkdir(parents=True)
        page.write_text(f"# rcc\n\n> {revision}.\n\n- Show version:\n\n`rcc version`\n")
        (plugin / "plugin.json").write_text(json.dumps({"name": "rcc"}))
        return page

    def test_no_opt_in_means_no_files_or_directories(self) -> None:
        self.helper.sync(self.source, self.paths.state)
        self.assertFalse(self.paths.state.parent.exists())
        self.assertFalse(self.paths.page.parent.exists())

    def test_active_cache_uses_numeric_version_order_and_prefers_local(self) -> None:
        base = self.root / "installed/rcc"
        for name in ["0.3.9", "0.3.10"]:
            (base / name).mkdir(parents=True)
        record = {"source_kind": "snapshot", "plugin_base": str(base), "source": str(self.source)}
        self.assertEqual(self.helper.active_source(record),
                         base / "0.3.10/skills/rcc-core/references/tldr/rcc.page.md")
        (base / "local").mkdir()
        self.assertEqual(self.helper.active_source(record),
                         base / "local/skills/rcc-core/references/tldr/rcc.page.md")

    def test_checkout_link_follows_updates_and_uninstalls(self) -> None:
        self.helper.install(self.source, self.paths)
        self.assertTrue(self.paths.page.is_symlink())
        self.assertEqual(self.paths.page.resolve(), self.source)
        inode = self.paths.page.lstat().st_ino
        self.source.write_text("updated canonical page\n")
        self.helper.sync(self.source, self.paths.state)
        self.assertEqual(self.paths.page.read_text(), "updated canonical page\n")
        self.assertEqual(self.paths.page.lstat().st_ino, inode)
        self.helper.uninstall(self.paths.state)
        self.assertFalse(self.paths.page.is_symlink())
        self.assertTrue(self.source.exists())
        self.helper.uninstall(self.paths.state)

    def test_existing_canonical_symlink_is_adopted_without_replacement(self) -> None:
        self.paths.page.parent.mkdir(parents=True)
        relative = os.path.relpath(self.source, self.paths.page.parent)
        self.paths.page.symlink_to(relative)
        inode = self.paths.page.lstat().st_ino
        other_bundle = self.make_source("new plugin cache", "new bundled revision")
        self.helper.install(other_bundle, self.paths, source_kind="snapshot")
        self.helper.sync(other_bundle, self.paths.state, source_kind="snapshot")
        self.assertEqual(self.paths.page.readlink(), Path(relative))
        self.assertEqual(self.paths.page.lstat().st_ino, inode)
        self.helper.uninstall(self.paths.state)
        self.assertTrue(self.paths.page.is_symlink(), "uninstall must retain an adopted user link")

    def test_user_page_is_preserved_even_when_it_matches_the_bundled_page(self) -> None:
        self.paths.page.parent.mkdir(parents=True)
        shutil.copy2(self.source, self.paths.page)
        before = self.paths.page.read_bytes()
        with self.assertRaises(self.helper.Conflict):
            self.helper.install(self.source, self.paths)
        self.assertEqual(self.paths.page.read_bytes(), before)
        self.assertFalse(self.paths.state.exists())

    def test_foreign_and_dangling_links_are_preserved(self) -> None:
        self.paths.page.parent.mkdir(parents=True)
        self.paths.page.symlink_to(self.root / "missing-user-page")
        with self.assertRaises(self.helper.Conflict):
            self.helper.install(self.source, self.paths)
        self.assertEqual(self.paths.page.readlink(), self.root / "missing-user-page")
        self.assertFalse(self.paths.state.exists())

    def test_symlink_failure_falls_back_to_a_copy_that_upgrades(self) -> None:
        with mock.patch.object(self.helper.os, "symlink", side_effect=OSError(errno.EPERM, "blocked")):
            self.helper.install(self.source, self.paths)
        self.assertTrue(self.paths.page.is_file())
        self.assertFalse(self.paths.page.is_symlink())
        self.source.write_text("copy upgrade\n")
        self.helper.sync(self.source, self.paths.state)
        self.assertEqual(self.paths.page.read_text(), "copy upgrade\n")
        self.helper.uninstall(self.paths.state)
        self.assertFalse(self.paths.page.exists())

    def test_user_edits_to_a_managed_copy_are_not_overwritten_or_deleted(self) -> None:
        self.helper.install(self.source, self.paths, mode="copy")
        self.paths.page.write_text("my custom commands\n")
        with self.assertRaises(self.helper.Conflict):
            self.helper.sync(self.source, self.paths.state)
        self.helper.uninstall(self.paths.state)
        self.assertEqual(self.paths.page.read_text(), "my custom commands\n")

    def test_user_replacement_of_a_managed_link_is_preserved(self) -> None:
        self.helper.install(self.source, self.paths)
        self.paths.page.unlink()
        self.paths.page.write_text("user replacement\n")
        with self.assertRaises(self.helper.Conflict):
            self.helper.sync(self.source, self.paths.state)
        self.helper.uninstall(self.paths.state)
        self.assertEqual(self.paths.page.read_text(), "user replacement\n")

    def test_snapshot_upgrade_survives_removal_of_the_old_plugin_cache(self) -> None:
        self.helper.install(self.source, self.paths, source_kind="snapshot")
        inode = self.paths.page.lstat().st_ino
        self.assertNotEqual(self.paths.page.resolve(), self.source)
        old_content = self.paths.page.read_text()
        shutil.rmtree(self.root / "checkout")
        self.assertEqual(self.paths.page.read_text(), old_content)
        replacement = self.make_source("cache 0.3.1", "new marketplace page")
        self.helper.sync(replacement, self.paths.state, source_kind="snapshot")
        self.assertEqual(self.paths.page.read_bytes(), replacement.read_bytes())
        self.assertEqual(self.paths.page.lstat().st_ino, inode)

    def test_explicit_snapshot_copy_preserves_an_unowned_anchor_file(self) -> None:
        self.paths.anchor.parent.mkdir(parents=True)
        self.paths.anchor.write_text("preexisting file\n")
        self.helper.install(self.source, self.paths, mode="copy", source_kind="snapshot")
        self.assertTrue(self.paths.anchor.exists())
        self.assertEqual(self.paths.anchor.read_text(), "preexisting file\n")

    def test_forced_link_failure_leaves_no_page_or_receipt(self) -> None:
        with mock.patch.object(self.helper.os, "symlink", side_effect=OSError(errno.EPERM, "blocked")):
            with self.assertRaises(OSError):
                self.helper.install(self.source, self.paths, mode="link", source_kind="snapshot")
        self.assertFalse(self.paths.page.is_symlink())
        self.assertFalse(self.paths.state.exists())
        self.assertFalse(self.paths.anchor.exists())

    def test_copy_sync_recovers_an_interrupted_receipt_update(self) -> None:
        self.helper.install(self.source, self.paths, mode="copy")
        self.source.write_text("new canonical revision\n")
        self.paths.page.write_bytes(self.source.read_bytes())
        self.helper.sync(self.source, self.paths.state)
        self.assertEqual(self.paths.page.read_bytes(), self.source.read_bytes())

    def test_failed_cache_setup_does_not_leave_an_untracked_page(self) -> None:
        self.paths.cache.parent.mkdir(parents=True)
        self.paths.cache.write_text("existing cache path is a file\n")
        with self.assertRaises(OSError):
            self.helper.install(self.source, self.paths, mode="copy")
        self.assertFalse(self.paths.page.exists())
        self.assertFalse(self.paths.state.exists())

    def test_failed_receipt_write_rolls_back_only_the_new_page(self) -> None:
        with mock.patch.object(self.helper, "save", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.helper.install(self.source, self.paths, source_kind="snapshot")
        self.assertFalse(self.paths.page.is_symlink())
        self.assertFalse(self.paths.anchor.exists())
        self.assertFalse(self.paths.state.exists())


@unittest.skipUnless(shutil.which("tldr") and os.name == "posix", "real Tealdeer requires Linux/macOS")
class TealdeerInstallerTests(unittest.TestCase):
    def setUp(self) -> None:
        if not subprocess.check_output(["tldr", "--version"], text=True).startswith("tealdeer "):
            self.skipTest("Tealdeer client is required")
        temporary = tempfile.TemporaryDirectory(prefix="rcc-tealdeer-installer-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.environment = os.environ.copy()
        for key, directory in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"),
                               ("XDG_CACHE_HOME", "cache"), ("XDG_DATA_HOME", "data"),
                               ("XDG_STATE_HOME", "state"), ("CODEX_HOME", "codex")]:
            self.environment[key] = str(self.root / directory)
        config = self.root / "config/tealdeer/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text('[updates]\nauto_update = true\n# keep my existing configuration\n')
        self.config = config
        self.config_before = config.read_bytes()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        codex = self.bin / "codex"
        codex.write_text("#!/bin/sh\nexit 0\n")
        codex.chmod(0o755)
        self.environment["PATH"] = f"{self.bin}{os.pathsep}{os.environ['PATH']}"
        self.cwd = self.root / "unrelated directory"
        self.cwd.mkdir()

    def run_command(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(args, cwd=self.cwd, env=self.environment,
                              text=True, capture_output=True, timeout=45)

    def bundle(self, name: str, revision: str) -> Path:
        plugin = self.root / name
        script = plugin / "skills/rcc-core/scripts/tealdeer.py"
        script.parent.mkdir(parents=True)
        shutil.copy2(HELPER, script)
        page = plugin / "skills/rcc-core/references/tldr/rcc.page.md"
        page.parent.mkdir(parents=True)
        page.write_text(f"# rcc\n\n> {revision}.\n\n- Show version:\n\n`rcc version`\n")
        (plugin / "plugin.json").write_text(json.dumps({"name": "rcc"}))
        return plugin

    def test_default_install_has_no_tealdeer_side_effects(self) -> None:
        result = self.run_command("bash", str(ROOT / "scripts/install-codex-assets.sh"),
                                  "--repo-path", str(ROOT), "--codex-home", str(self.root / "codex"),
                                  "--skip-repo-sync")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / "data").exists())
        self.assertFalse((self.root / "state").exists())
        self.assertEqual(self.config.read_bytes(), self.config_before)

    def test_configured_paths_and_marketplace_upgrade_hook_work_offline(self) -> None:
        custom = self.root / "my custom pages"
        cache = self.root / "my custom cache"
        self.config.write_text('[updates]\nauto_update = true\n[directories]\n'
                               f'custom_pages_dir = {json.dumps(str(custom))}\n'
                               f'cache_dir = {json.dumps(str(cache))}\n')
        config_before = self.config.read_bytes()
        for mode in ("auto", "copy"):
            with self.subTest(mode=mode):
                old = self.bundle(f"codex/plugins/cache/{mode}/rcc/0.3.9", f"old {mode} revision")
                helper = old / "skills/rcc-core/scripts/tealdeer.py"
                result = self.run_command(sys.executable, str(helper), "install", "--mode", mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                hooks = json.loads((self.root / "codex/hooks.json").read_text())
                command = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
                shutil.rmtree(old)
                result = self.run_command("tldr", "--no-auto-update", "rcc")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"old {mode} revision", result.stdout)
                new = self.bundle(f"codex/plugins/cache/{mode}/rcc/0.3.10", f"new {mode} revision")
                result = self.run_command("sh", "-c", command)
                self.assertEqual(result.returncode, 0, result.stderr)
                result = self.run_command("tldr", "--no-auto-update", "rcc")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"new {mode} revision", result.stdout)
                self.assertEqual(self.config.read_bytes(), config_before)
                result = self.run_command(sys.executable, str(new / "skills/rcc-core/scripts/tealdeer.py"), "uninstall")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertFalse((custom / "rcc.page.md").exists())

    def test_user_hooks_are_preserved_and_setup_does_not_duplicate_its_hook(self) -> None:
        path = self.root / "codex/hooks.json"
        path.parent.mkdir(parents=True)
        original = {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "echo user-hook"}]}],
                              "Stop": [{"hooks": [{"type": "command", "command": "echo keep-me"}]}]}}
        path.write_text(json.dumps(original))
        for _ in range(2):
            result = self.run_command(sys.executable, str(HELPER), "install")
            self.assertEqual(result.returncode, 0, result.stderr)
        installed = json.loads(path.read_text())
        self.assertEqual(len(installed["hooks"]["SessionStart"]), 2)
        self.assertEqual(installed["hooks"]["Stop"], original["hooks"]["Stop"])
        result = self.run_command(sys.executable, str(HELPER), "uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(path.read_text()), original)

    def test_malformed_or_symlinked_hook_config_is_not_replaced(self) -> None:
        path = self.root / "codex/hooks.json"
        path.parent.mkdir(parents=True)
        path.write_text("user configuration needing repair\n")
        result = self.run_command(sys.executable, str(HELPER), "install")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.read_text(), "user configuration needing repair\n")
        path.unlink()
        target = self.root / "user hooks.json"
        target.write_text('{}\n')
        path.symlink_to(target)
        inode = path.lstat().st_ino
        result = self.run_command(sys.executable, str(HELPER), "install")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.lstat().st_ino, inode)
        self.assertEqual(target.read_text(), '{}\n')
        self.assertFalse((self.root / "data").exists())

    def test_edited_user_hook_is_preserved_and_becomes_a_noop_after_uninstall(self) -> None:
        result = self.run_command(sys.executable, str(HELPER), "install")
        self.assertEqual(result.returncode, 0, result.stderr)
        path = self.root / "codex/hooks.json"
        data = json.loads(path.read_text())
        group = data["hooks"]["SessionStart"][0]
        group["hooks"][0]["timeout"] = 12
        path.write_text(json.dumps(data))
        result = self.run_command(sys.executable, str(HELPER), "uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(path.read_text()), data)
        result = self.run_command("sh", "-c", group["hooks"][0]["command"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "data/tealdeer/pages/rcc.page.md").is_symlink())

    def test_preexisting_identical_hook_is_adopted_and_retained(self) -> None:
        state = self.root / "state/agent-plugins/rcc-tealdeer/state.json"
        runner = state.parent / "tealdeer.py"
        group = {"hooks": [{"type": "command", "command": shlex.join(
            ["python3", str(runner), "sync", "--managed-hook", "--state-file", str(state)])}]}
        path = self.root / "codex/hooks.json"
        path.parent.mkdir(parents=True)
        original = {"hooks": {"SessionStart": [group]}}
        path.write_text(json.dumps(original))
        result = self.run_command(sys.executable, str(HELPER), "install")
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.run_command(sys.executable, str(HELPER), "uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(path.read_text()), original)
        self.assertTrue(runner.exists())

    def test_preexisting_identical_runner_is_not_replaced_or_deleted(self) -> None:
        runner = self.root / "state/agent-plugins/rcc-tealdeer/tealdeer.py"
        runner.parent.mkdir(parents=True)
        shutil.copy2(HELPER, runner)
        inode = runner.stat().st_ino
        result = self.run_command(sys.executable, str(HELPER), "install")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(runner.stat().st_ino, inode)
        result = self.run_command(sys.executable, str(HELPER), "uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(runner.exists())

    def test_second_codex_home_is_rejected_before_changing_the_page_or_receipt(self) -> None:
        old = self.bundle("codex/plugins/cache/test/rcc/0.3.0", "original page")
        result = self.run_command(sys.executable, str(old / "skills/rcc-core/scripts/tealdeer.py"), "install")
        self.assertEqual(result.returncode, 0, result.stderr)
        page = self.root / "data/tealdeer/pages/rcc.page.md"
        state = self.root / "state/agent-plugins/rcc-tealdeer/state.json"
        before_page, before_state = page.read_bytes(), state.read_bytes()
        new = self.bundle("other-codex/plugins/cache/test/rcc/0.3.1", "must not replace page")
        result = self.run_command(sys.executable, str(new / "skills/rcc-core/scripts/tealdeer.py"),
                                  "install", "--codex-home", str(self.root / "other-codex"))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(page.read_bytes(), before_page)
        self.assertEqual(state.read_bytes(), before_state)

    def test_copy_install_stays_opted_in_across_archive_installer_reruns(self) -> None:
        checkout = self.root / "stable archive checkout"
        (checkout / "scripts").mkdir(parents=True)
        (checkout / "marketplaces").mkdir()
        (checkout / "marketplaces/catalog.json").write_text("{}")
        shutil.copy2(ROOT / "scripts/install-codex-assets.sh", checkout / "scripts/install-codex-assets.sh")
        shutil.copytree(ROOT / "plugins/rcc", checkout / "plugins/rcc")
        args = ["bash", str(checkout / "scripts/install-codex-assets.sh"),
                "--repo-path", str(checkout), "--codex-home", str(self.root / "codex"),
                "--skip-repo-sync", "--install-method", "archive"]
        result = self.run_command(*args, "--rcc-tldr-mode", "copy")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        page = self.root / "data/tealdeer/pages/rcc.page.md"
        self.assertFalse(page.is_symlink())
        source = checkout / "plugins/rcc/skills/rcc-core/references/tldr/rcc.page.md"
        source.write_text(source.read_text().replace("RCC command-line tool", "Upgraded RCC command-line tool"))
        result = self.run_command(*args)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(page.read_bytes(), source.read_bytes())
        result = self.run_command("tldr", "--no-auto-update", "rcc")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Upgraded RCC", result.stdout)

    def test_opt_in_through_installer_and_safe_uninstall_from_any_directory(self) -> None:
        args = ["bash", str(ROOT / "scripts/install-codex-assets.sh"),
                "--repo-path", str(ROOT), "--codex-home", str(self.root / "codex"),
                "--skip-repo-sync", "--with-rcc-tldr"]
        result = self.run_command(*args)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = self.run_command("tldr", "--no-auto-update", "rcc")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("rcc provider list", result.stdout)
        page = self.root / "data/tealdeer/pages/rcc.page.md"
        self.assertTrue(page.is_symlink())
        inode = page.lstat().st_ino
        result = self.run_command(*args[:-1])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(page.lstat().st_ino, inode)
        result = self.run_command("bash", str(ROOT / "scripts/uninstall-codex-assets.sh"),
                                  "--codex-home", str(self.root / "codex"))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(page.is_symlink())
        self.assertEqual(self.config.read_bytes(), self.config_before)


@unittest.skipUnless(os.name == "posix", "shell bootstrap requires a POSIX host")
class BootstrapTealdeerFlagTests(unittest.TestCase):
    def test_public_entrypoint_forwards_opt_in_to_git_and_archive_installers(self) -> None:
        with tempfile.TemporaryDirectory(prefix="rcc-bootstrap-flags-") as temporary:
            root = Path(temporary)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            git = bin_dir / "git"
            git.write_text("#!/bin/sh\nexit 0\n")
            git.chmod(0o755)
            delegate = root / "fixture/scripts/install-codex-assets.sh"
            delegate.parent.mkdir(parents=True)
            delegate.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$RCC_INSTALL_ARGS"\n')
            archive = root / "fixture.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                handle.add(root / "fixture", arcname="plugins-main")
            curl = bin_dir / "curl"
            curl.write_text('#!/usr/bin/env python3\nimport os, shutil, sys\n'
                            'shutil.copyfile(os.environ["RCC_TEST_ARCHIVE"], sys.argv[sys.argv.index("-o")+1])\n')
            curl.chmod(0o755)
            for method in ("git", "archive"):
                with self.subTest(method=method):
                    repo = root / method
                    if method == "git":
                        shutil.copytree(root / "fixture", repo)
                        (repo / ".git").mkdir()
                    log = root / f"{method}-args.txt"
                    environment = os.environ.copy()
                    environment.update(PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                                       RCC_INSTALL_ARGS=str(log), RCC_TEST_ARCHIVE=str(archive))
                    result = subprocess.run(["bash", str(ROOT / "install.sh"), "--repo-path", str(repo),
                                             "--codex-home", str(root / "codex"), "--ref", "main",
                                             "--install-method", method, "--with-rcc-tldr",
                                             "--rcc-tldr-mode", "copy"], cwd=root, env=environment,
                                            text=True, capture_output=True, timeout=30)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    args = log.read_text().splitlines()
                    self.assertIn("--with-rcc-tldr", args)
                    self.assertEqual(args[args.index("--rcc-tldr-mode") + 1], "copy")


if __name__ == "__main__":
    unittest.main()
