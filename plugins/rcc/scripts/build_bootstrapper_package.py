#!/usr/bin/env python3
"""Build the canonical RCC bootstrapper Action package as a compact ZIP."""

from __future__ import annotations

import argparse
import json
import os
import stat
import subprocess
import tempfile
import zipfile
import zlib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
PACKAGE_DIR = REPO_ROOT / "plugins/rcc/bootstrapper"


def build(action_server: str, output: Path) -> None:
    """Use Actions Runtime packaging, then deterministically deflate its ZIP."""
    if "zlib-ng" in zlib.ZLIB_RUNTIME_VERSION:
        raise RuntimeError(
            "Release packaging requires standard zlib, not zlib-ng; "
            "use the CI Python 3.11 toolchain (for example uv run --python 3.11). "
            "Different compressors produce different pinned ZIP bytes."
        )
    output = output.expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rcc-bootstrapper-package-") as temp:
        native_zip = Path(temp) / "rcc-actions-bootstrapper.zip"
        datadir = Path(temp) / "datadir"
        native_dir = Path(temp) / "native"
        private_cache_root = Path(temp) / "runtime-state"
        native_dir.mkdir()
        private_cache_root.mkdir()
        env = os.environ.copy()
        env.update(
            {
                "ACTIONS_HOME": str(private_cache_root / "actions"),
                "ROBOTS_HOME": str(private_cache_root / "robots"),
                "ROBOCORP_HOME": str(private_cache_root / "robocorp"),
                "UV_CACHE_DIR": str(private_cache_root / "uv-cache"),
            }
        )
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
                for item in sorted(source.infolist(), key=lambda entry: entry.filename):
                    if item.filename in names:
                        raise RuntimeError(f"duplicate package member: {item.filename}")
                    names.add(item.filename)
                    if item.is_dir():
                        continue
                    data = source.read(item)
                    if item.filename == "__action_server_metadata__.json":
                        data = json.dumps(
                            json.loads(data), ensure_ascii=False, sort_keys=True, separators=(",", ":")
                        ).encode("utf-8") + b"\n"
                    info = zipfile.ZipInfo(item.filename, date_time=(1980, 1, 1, 0, 0, 0))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.external_attr = (stat.S_IFREG | 0o644) << 16
                    info.create_system = 3
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
