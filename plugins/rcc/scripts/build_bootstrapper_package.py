#!/usr/bin/env python3
"""Build the canonical RCC bootstrapper Action package as a compact ZIP."""

from __future__ import annotations

import argparse
import os
import subprocess
import tempfile
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
PACKAGE_DIR = REPO_ROOT / "plugins/rcc/bootstrapper"


def build(action_server: str, output: Path) -> None:
    """Use Actions Runtime packaging, then deterministically deflate its ZIP."""
    output = output.expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rcc-bootstrapper-package-") as temp:
        native_zip = Path(temp) / "rcc-actions-bootstrapper.zip"
        datadir = Path(temp) / "datadir"
        native_dir = Path(temp) / "native"
        private_home = Path(temp) / "home"
        native_dir.mkdir()
        private_home.mkdir()
        env = os.environ.copy()
        env.update(
            {
                "HOME": str(private_home),
                "ACTIONS_HOME": str(private_home / ".actions"),
                "ROBOCORP_HOME": str(private_home / "robocorp"),
                "XDG_CACHE_HOME": str(private_home / ".cache"),
                "XDG_CONFIG_HOME": str(private_home / ".config"),
                "XDG_DATA_HOME": str(private_home / ".local" / "share"),
            }
        )
        if os.name == "nt":
            env["USERPROFILE"] = str(private_home)
        result = subprocess.run(
            [
                action_server,
                "package",
                "build",
                "--input-dir",
                str(PACKAGE_DIR),
                "--output-dir",
                str(native_dir),
                "--override",
                "--datadir",
                str(datadir),
                "--json",
            ],
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        if result.returncode:
            raise RuntimeError(f"action-server package build failed: {result.stderr.strip()}")
        native_zip = Path(temp) / "native" / "rcc-actions-bootstrapper.zip"
        if not native_zip.is_file():
            raise RuntimeError("action-server package build did not create its ZIP")

        output_tmp = output.with_name(f".{output.name}.tmp-{os.getpid()}")
        try:
            with zipfile.ZipFile(native_zip) as source, zipfile.ZipFile(
                output_tmp, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
            ) as target:
                names: set[str] = set()
                for item in source.infolist():
                    if item.filename in names:
                        raise RuntimeError(f"duplicate package member: {item.filename}")
                    names.add(item.filename)
                    if item.is_dir():
                        continue
                    data = source.read(item)
                    info = zipfile.ZipInfo(item.filename, date_time=(1980, 1, 1, 0, 0, 0))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.external_attr = item.external_attr
                    info.create_system = item.create_system
                    target.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
            with zipfile.ZipFile(output_tmp) as check:
                if check.testzip() is not None:
                    raise RuntimeError("deflated package ZIP failed CRC validation")
                required = {
                    "package.yaml",
                    "__action_server_metadata__.json",
                    "src/rcc_bootstrapper/bootstrapper_actions.py",
                }
                missing = required.difference(check.namelist())
                if missing:
                    raise RuntimeError(f"package is missing required files: {sorted(missing)}")
            os.replace(output_tmp, output)
        finally:
            output_tmp.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--action-server", default="action-server")
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / "dist/rcc-actions-bootstrapper.zip",
    )
    args = parser.parse_args()
    build(args.action_server, args.output)
    print(args.output.resolve())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
