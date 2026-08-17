from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
LAUNCHER = REPO_ROOT / "plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp"
BUNDLED_MODULE = REPO_ROOT / "plugins/rcc/dagger"


def _run_launcher(tmp_path: Path, *, module_override: Path | None = None) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    args_file = tmp_path / "dagger-args"
    dagger = bin_dir / "dagger"
    dagger.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$DAGGER_ARGS_FILE"\n')
    dagger.chmod(0o755)

    env = os.environ.copy()
    env["PATH"] = f"{bin_dir}{os.pathsep}{env['PATH']}"
    env["DAGGER_ARGS_FILE"] = str(args_file)
    env.pop("RCC_DAGGER_REPO", None)
    env.pop("RCC_DAGGER_MODULE", None)
    if module_override is not None:
        env["RCC_DAGGER_REPO"] = str(module_override)

    result = subprocess.run(
        [str(LAUNCHER)],
        cwd=tmp_path,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    dagger_args = args_file.read_text().splitlines() if args_file.exists() else []
    return result, dagger_args


class RccDaggerMcpLauncherTests(unittest.TestCase):
    def test_uses_bundled_module_outside_a_dagger_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            result, dagger_args = _run_launcher(Path(temp_dir))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            dagger_args,
            [
                "--silent",
                "mcp",
                "--stdio",
                "--mod",
                str(BUNDLED_MODULE.resolve()),
            ],
        )

    def test_prefers_explicit_module_override(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            tmp_path = Path(temp_dir)
            override = tmp_path / "rcc"
            (override / ".dagger").mkdir(parents=True)
            (override / "dagger.json").write_text("{}\n")
            result, dagger_args = _run_launcher(tmp_path, module_override=override)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(dagger_args[-2:], ["--mod", str(override.resolve())])

    def test_rejects_invalid_explicit_module_override(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            tmp_path = Path(temp_dir)
            invalid = tmp_path / "not-a-module"
            invalid.mkdir()
            result, dagger_args = _run_launcher(tmp_path, module_override=invalid)

        self.assertEqual(result.returncode, 2)
        self.assertIn("must contain dagger.json and .dagger/", result.stderr)
        self.assertEqual(dagger_args, [])


if __name__ == "__main__":
    unittest.main()
