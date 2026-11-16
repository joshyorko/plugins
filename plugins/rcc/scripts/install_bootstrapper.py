#!/usr/bin/env python3
"""Install the pinned RCC bootstrapper package and native tools with stdlib Python only."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import stat
import sys
import tempfile
import time
import urllib.error
import urllib.request
from urllib.parse import urlparse
import zipfile
from pathlib import Path, PurePosixPath

PACKAGE_VERSION = "0.6.0"
PACKAGE_SHA256 = "d58a8711956f8a1ca29bf03d445f6090ad4393ac65319dc8fb4f3b191b19d058"
RUNTIME_VERSION = "1.1.1"
RCC_VERSION = "18.19.11"
PINS = {
    ("linux", "x86_64"): (
        "actions-runtime-1.1.1-linux64",
        "8c27fbed198aef16c98cdc7a45230fbc4708b06dff1c3523aeddfcdf7840efd0",
        "rcc-linux64",
        "4d68609044d3a39a1728d571d54c77a6174215adb40eb9acd69ce5ad34a6db1f",
        "https://github.com/joshyorko/actions/releases/download/actions-runtime-1.1.1/actions-runtime-1.1.1-linux64",
        "https://github.com/joshyorko/rcc/releases/download/v18.19.11/rcc-linux64",
    ),
    ("windows", "AMD64"): (
        "actions-runtime-1.1.1-windows64.exe",
        "0388d7f2e4080ba5c3a800bb556f8f01a65aa4834f3698eceb240888288fd8d2",
        "rcc-windows64.exe",
        "58f43a3b3c54032740f224cb4a8bbf68c47c865cf2652084fb139b8ecc01cfd0",
        (
            "https://github.com/joshyorko/actions/releases/download/actions-runtime-1.1.1/"
            "actions-runtime-1.1.1-windows64.exe"
        ),
        "https://github.com/joshyorko/rcc/releases/download/v18.19.11/rcc-windows64.exe",
    ),
}
PACKAGE_URL = (
    "https://github.com/joshyorko/plugins/releases/download/v0.6.0/"
    "rcc-actions-bootstrapper-0.6.0.zip"
)
PACKAGE_FILENAME = f"rcc-actions-bootstrapper-{PACKAGE_VERSION}.zip"
MAX_DOWNLOAD_BYTES = 130 * 1024 * 1024
MAX_PACKAGE_BYTES = 256 * 1024 * 1024
MAX_PACKAGE_FILES = 256


def platform_pin(system: str | None = None, machine: str | None = None) -> tuple[str, ...]:
    key = (system or sys.platform, machine or platform.machine())
    normalized = {"linux": "linux", "win32": "windows"}.get(key[0], key[0])
    normalized_machine = {"x86_64": "x86_64", "AMD64": "AMD64"}.get(key[1], key[1])
    try:
        return PINS[(normalized, normalized_machine)]
    except KeyError as exc:
        raise RuntimeError(
            f"unsupported platform {key[0]}/{key[1]}; supported: Linux x86_64 and Windows AMD64"
        ) from exc


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, destination: Path, expected_sha256: str) -> None:
    if urlparse(url).scheme != "https":
        raise RuntimeError("pinned release downloads must use HTTPS")
    for attempt in range(3):
        temp_path: Path | None = None
        try:
            destination.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".download-", delete=False) as out:
                temp_path = Path(out.name)
                request = urllib.request.Request(url, headers={"User-Agent": "rcc-bootstrapper-setup/0.6.0"})
                with urllib.request.urlopen(request, timeout=60) as response:
                    if urlparse(response.geturl()).scheme != "https":
                        raise RuntimeError("release server redirected to a non-HTTPS URL")
                    if response.status != 200:
                        raise RuntimeError(f"download returned HTTP {response.status}: {url}")
                    size = 0
                    while chunk := response.read(1024 * 1024):
                        size += len(chunk)
                        if size > MAX_DOWNLOAD_BYTES:
                            raise RuntimeError(f"download exceeded {MAX_DOWNLOAD_BYTES} bytes")
                        out.write(chunk)
                out.flush()
                os.fsync(out.fileno())
            # Close the temporary file before replacing the destination; Windows
            # refuses to replace a path while a handle is open without delete sharing.
            actual = digest_file(temp_path)
            if actual != expected_sha256:
                raise RuntimeError(f"SHA-256 mismatch for {url}: expected {expected_sha256}, got {actual}")
            os.replace(temp_path, destination)
            return
        except urllib.error.HTTPError as exc:
            if exc.code != 429 and not 500 <= exc.code < 600:
                raise RuntimeError(f"download returned HTTP {exc.code}: {url}") from exc
            if attempt == 2:
                raise RuntimeError(f"download retries exhausted for {url}: HTTP {exc.code}") from exc
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            if attempt == 2:
                raise RuntimeError(f"download retries exhausted for {url}: {exc}") from exc
        finally:
            if temp_path is not None:
                temp_path.unlink(missing_ok=True)
        time.sleep(0.5 * (attempt + 1))


def stage_local_artifact(source: Path, destination: Path, expected_sha256: str) -> None:
    source = source.expanduser().absolute()
    if not source.is_file() or _is_link_like(source):
        raise RuntimeError(f"local pinned artifact is missing or redirected: {source}")
    if source.stat().st_size > MAX_DOWNLOAD_BYTES:
        raise RuntimeError(f"local artifact exceeds {MAX_DOWNLOAD_BYTES} bytes: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    temp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".artifact-", delete=False) as out:
            temp_path = Path(out.name)
            with source.open("rb") as inp:
                shutil.copyfileobj(inp, out)
            out.flush()
            os.fsync(out.fileno())
        actual = digest_file(temp_path)
        if actual != expected_sha256:
            raise RuntimeError(f"SHA-256 mismatch for {source}: expected {expected_sha256}, got {actual}")
        os.replace(temp_path, destination)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def _is_link_like(path: Path) -> bool:
    if path.is_symlink():
        return True
    if os.name == "nt" and hasattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT") and path.exists():
        return bool(path.stat(follow_symlinks=False).st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)
    return False


def _ensure_private_directory(path: Path) -> Path:
    if _is_link_like(path):
        raise RuntimeError(f"refusing redirected user state directory: {path}")
    if path.exists() and not path.is_dir():
        raise RuntimeError(f"user state path is not a directory: {path}")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if _is_link_like(path) or not path.is_dir():
        raise RuntimeError(f"user state path changed during setup: {path}")
    return path


def _safe_members(archive: zipfile.ZipFile) -> list[zipfile.ZipInfo]:
    infos = archive.infolist()
    if not infos or len(infos) > MAX_PACKAGE_FILES:
        raise RuntimeError("package ZIP has an invalid entry count")
    member_types: dict[str, bool] = {}
    total = 0
    for info in infos:
        name = info.filename
        path = PurePosixPath(name)
        normalized = path.as_posix().rstrip("/")
        mode = (info.external_attr >> 16) & 0xFFFF
        if (
            not name
            or not path.parts
            or normalized in member_types
            or path.is_absolute()
            or ".." in path.parts
            or (path.parts and ":" in path.parts[0])
            or "\\" in name
            or (mode and (mode & 0o170000) == 0o120000)
        ):
            raise RuntimeError(f"unsafe or duplicate ZIP path: {name!r}")
        member_types[normalized] = info.is_dir()
        total += info.file_size
        if total > MAX_PACKAGE_BYTES or info.file_size > MAX_PACKAGE_BYTES:
            raise RuntimeError("package ZIP exceeds extraction size limits")
    for name, is_dir in member_types.items():
        parts = PurePosixPath(name).parts
        if any(member_types.get("/".join(parts[:index])) is False for index in range(1, len(parts))):
            raise RuntimeError(f"ZIP member is nested under a file: {name!r}")
        if not is_dir and any(other.startswith(name + "/") for other in member_types):
            raise RuntimeError(f"ZIP file conflicts with a child path: {name!r}")
    return infos


def extract_package(archive_path: Path, destination: Path) -> None:
    with zipfile.ZipFile(archive_path) as archive:
        infos = _safe_members(archive)
        names = {info.filename.rstrip("/") for info in infos}
        required = {
            "package.yaml",
            "__action_server_metadata__.json",
            "src/rcc_bootstrapper/bootstrapper_actions.py",
        }
        if missing := required - names:
            raise RuntimeError(f"package ZIP missing required files: {sorted(missing)}")
        destination.mkdir(parents=True, exist_ok=False)
        root = destination.resolve()
        for info in infos:
            if info.is_dir():
                (root / PurePosixPath(info.filename)).mkdir(parents=True, exist_ok=True)
                continue
            target = root / PurePosixPath(info.filename)
            target.parent.mkdir(parents=True, exist_ok=True)
            resolved_parent = target.parent.resolve()
            if not resolved_parent.is_relative_to(root):
                raise RuntimeError(f"ZIP member escapes installation directory: {info.filename}")
            with archive.open(info) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)


def verify_extracted_package(archive_path: Path, package_dir: Path) -> dict[str, str]:
    """Require an extracted package tree to match every byte in its pinned ZIP."""
    package_dir = package_dir.absolute()
    archive_path = archive_path.absolute()
    if _is_link_like(archive_path) or _is_link_like(archive_path.parent) or _is_link_like(archive_path.parent.parent):
        raise RuntimeError(f"pinned package or version directory is redirected: {archive_path}")
    if _is_link_like(package_dir) or not package_dir.is_dir():
        raise RuntimeError(f"extracted package directory is missing or redirected: {package_dir}")
    with zipfile.ZipFile(archive_path) as archive:
        infos = _safe_members(archive)
        expected_files: dict[str, tuple[int, str]] = {}
        expected_dirs = {""}
        for info in infos:
            relative = PurePosixPath(info.filename).as_posix().rstrip("/")
            parts = PurePosixPath(relative).parts
            for index in range(1, len(parts)):
                expected_dirs.add("/".join(parts[:index]))
            if info.is_dir():
                expected_dirs.add(relative)
                continue
            digest = hashlib.sha256()
            with archive.open(info) as member:
                for chunk in iter(lambda: member.read(1024 * 1024), b""):
                    digest.update(chunk)
            expected_files[relative] = (info.file_size, digest.hexdigest())

    actual_files: set[str] = set()
    actual_dirs = {""}
    for current, directories, files in os.walk(package_dir, followlinks=False):
        current_path = Path(current)
        for name in directories:
            item = current_path / name
            if _is_link_like(item):
                raise RuntimeError(f"extracted package contains a redirected directory: {item}")
            actual_dirs.add(item.relative_to(package_dir).as_posix())
        for name in files:
            item = current_path / name
            if _is_link_like(item) or not item.is_file():
                raise RuntimeError(f"extracted package contains a redirected or non-file entry: {item}")
            if item.stat().st_size > MAX_PACKAGE_BYTES:
                raise RuntimeError(f"extracted package file exceeds size limit: {item}")
            actual_files.add(item.relative_to(package_dir).as_posix())
    if actual_files != set(expected_files) or actual_dirs != expected_dirs:
        raise RuntimeError(
            "extracted package file tree differs from the verified ZIP "
            f"(missing={sorted(set(expected_files) - actual_files)}, "
            f"extra={sorted(actual_files - set(expected_files))})"
        )
    actual_hashes = {
        name: (
            (package_dir / PurePosixPath(name)).stat().st_size,
            digest_file(package_dir / PurePosixPath(name)),
        )
        for name in actual_files
    }
    if actual_hashes != expected_files:
        raise RuntimeError("extracted package contents differ from the verified ZIP")
    return actual_hashes


def verify_installed_package(archive_path: Path, package_dir: Path) -> dict[str, str]:
    actual = digest_file(archive_path)
    if actual != PACKAGE_SHA256:
        raise RuntimeError(f"installed package ZIP pin mismatch: expected {PACKAGE_SHA256}, got {actual}")
    return verify_extracted_package(archive_path, package_dir)


def _default_home() -> Path:
    configured = os.environ.get("RCC_BOOTSTRAPPER_HOME")
    if configured:
        return Path(configured).expanduser().absolute()
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local"))
    else:
        base = Path(os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share"))
    return (base / "rcc-bootstrapper").absolute()


class SetupLock:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.fd: int | None = None

    def __enter__(self) -> "SetupLock":
        deadline = time.monotonic() + 120
        while True:
            if _is_link_like(self.path):
                raise RuntimeError(f"refusing redirected setup lock: {self.path}")
            try:
                self.fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
                os.write(self.fd, str(os.getpid()).encode())
                return self
            except FileExistsError:
                if time.monotonic() >= deadline:
                    raise RuntimeError(
                        f"setup lock exists: {self.path}; inspect the owning process before removing it"
                    )
                time.sleep(0.25)

    def __exit__(self, *_: object) -> None:
        if self.fd is not None:
            os.close(self.fd)
        self.path.unlink(missing_ok=True)


def install(
    home: Path,
    *,
    package_url: str = PACKAGE_URL,
    package_archive: Path | None = None,
    offline_artifact_dir: Path | None = None,
) -> dict[str, str]:
    if package_archive and offline_artifact_dir:
        raise ValueError("choose either --package-archive or --offline-artifact-dir")
    runtime_name, runtime_sha, rcc_name, rcc_sha, runtime_url, rcc_url = platform_pin()
    home = home.expanduser().absolute()
    if _is_link_like(home):
        raise RuntimeError(f"refusing redirected bootstrapper home: {home}")
    # Unsupported hosts fail before any filesystem changes or network requests.
    _ensure_private_directory(home)
    with SetupLock(home / ".setup.lock"):
        version_root = _ensure_private_directory(home / "versions")
        versions = version_root / PACKAGE_VERSION
        if _is_link_like(versions):
            raise RuntimeError(f"refusing redirected immutable package version: {versions}")
        if versions.exists():
            receipt_path = versions / "install-receipt.json"
            if not receipt_path.is_file():
                raise RuntimeError(f"incomplete immutable package version exists: {versions}")
            receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            if (
                receipt.get("package_sha256") != PACKAGE_SHA256
                or receipt.get("runtime_sha256") != runtime_sha
                or receipt.get("rcc_sha256") != rcc_sha
                or digest_file(versions / "package.zip") != PACKAGE_SHA256
                or digest_file(versions / "bin" / runtime_name) != runtime_sha
                or digest_file(versions / "bin" / rcc_name) != rcc_sha
            ):
                raise RuntimeError(f"installed package version failed pin verification: {versions}")
            verify_installed_package(versions / "package.zip", versions / "package")
        else:
            work = Path(tempfile.mkdtemp(prefix=".setup-", dir=home))
            try:
                if offline_artifact_dir:
                    artifact_dir = offline_artifact_dir.expanduser().resolve()
                    stage_local_artifact(artifact_dir / PACKAGE_FILENAME, work / "package.zip", PACKAGE_SHA256)
                    stage_local_artifact(artifact_dir / runtime_name, work / runtime_name, runtime_sha)
                    stage_local_artifact(artifact_dir / rcc_name, work / rcc_name, rcc_sha)
                else:
                    if package_archive:
                        stage_local_artifact(package_archive, work / "package.zip", PACKAGE_SHA256)
                    else:
                        download(package_url, work / "package.zip", PACKAGE_SHA256)
                    download(runtime_url, work / runtime_name, runtime_sha)
                    download(rcc_url, work / rcc_name, rcc_sha)
                stage = work / "version"
                _ensure_private_directory(stage)
                extract_package(work / "package.zip", stage / "package")
                verify_installed_package(work / "package.zip", stage / "package")
                shutil.copyfile(work / "package.zip", stage / "package.zip")
                (stage / "bin").mkdir()
                shutil.copyfile(work / runtime_name, stage / "bin" / runtime_name)
                shutil.copyfile(work / rcc_name, stage / "bin" / rcc_name)
                if sys.platform != "win32":
                    for binary in (stage / "bin" / runtime_name, stage / "bin" / rcc_name):
                        binary.chmod(0o755)
                receipt = {
                    "package_version": PACKAGE_VERSION,
                    "package_sha256": PACKAGE_SHA256,
                    "runtime_version": RUNTIME_VERSION,
                    "runtime_sha256": runtime_sha,
                    "rcc_version": RCC_VERSION,
                    "rcc_sha256": rcc_sha,
                    "platform": f"{platform.system()}/{platform.machine()}",
                }
                (stage / "install-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
                os.replace(stage, versions)
            finally:
                shutil.rmtree(work, ignore_errors=True)
    state = _ensure_private_directory(home / "state")
    robocorp_home = _ensure_private_directory(state / "robocorp")
    runtime_state = _ensure_private_directory(state / "runtime")
    cache_home = _ensure_private_directory(state / "action-server-home")
    for cache_name in ("actions", "robots", "uv-cache"):
        _ensure_private_directory(cache_home / cache_name)
    runtime_name, _, rcc_name, _, _, _ = platform_pin()
    return {
        "installation": str(versions),
        "package_archive": str(versions / "package.zip"),
        "RCC_BOOTSTRAPPER_HOME": str(home),
        "package_dir": str(versions / "package"),
        "RCC_BOOTSTRAPPER_PACKAGE_DIR": str(versions / "package"),
        "action_server": str(versions / "bin" / runtime_name),
        "rcc": str(versions / "bin" / rcc_name),
        "datadir": str(runtime_state),
        "RCC_BOOTSTRAPPER_DATADIR": str(runtime_state),
        "ROBOCORP_HOME": str(robocorp_home),
        "ACTIONS_HOME": str(cache_home / "actions"),
        "ROBOTS_HOME": str(cache_home / "robots"),
        "UV_CACHE_DIR": str(cache_home / "uv-cache"),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--home", type=Path, default=_default_home())
    package_source = parser.add_mutually_exclusive_group()
    package_source.add_argument(
        "--package-archive",
        type=Path,
        default=None,
        help="Use this locally generated pinned package ZIP; binaries still download via HTTPS.",
    )
    package_source.add_argument(
        "--offline-artifact-dir",
        type=Path,
        default=None,
        help=f"Use local pinned files named {PACKAGE_FILENAME}, plus native Runtime and RCC assets.",
    )
    args = parser.parse_args()
    try:
        print(
            json.dumps(
                install(
                    args.home,
                    package_archive=args.package_archive,
                    offline_artifact_dir=args.offline_artifact_dir,
                ),
                indent=2,
            )
        )
    except Exception as exc:
        print(f"setup failed: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
