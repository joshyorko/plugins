#!/usr/bin/env python3
"""Verify the published Luna Factory bundle and build its OCI derivative."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any


RELEASE_COMMIT = "ebe2753ed032347456ef9b1c193646469bf17c96"
RELEASE_TREE = "c8a411ff77c5ebca9ad63ca6dd9b9b192f30342b"
RELEASE_TAG = "v0.2.1"
RELEASE_REPOSITORY = "https://github.com/joshyorko/plugins"
RELEASE_VERSION = "0.2.1"
RELEASE_ARCHIVE_NAME = "luna-factory-0.2.1-x86_64-unknown-linux-gnu.tar.gz"
RELEASE_ARCHIVE_SHA256 = "771580a3be2db4e016cabad4e1fa16136a601c58d66494c901a35d37752f8dd2"
RELEASE_PROVENANCE_SHA256 = "eed1883f3b6fdbd6d779ba3a34cbb8bb001314051cd2b0b5d537e6b8f706f00e"
RELEASE_CHECKSUMS_SHA256 = "2f436fffa19c38ff126eff0221f680a65ee1ccbfd3d912cdd0be1cc9ed537e58"
RELEASE_BINARY_SHA256 = "8ce20a7f4bc17bfaa10afb4ca450e44318b67c6f9479aa3a7190da8a2b15e03b"
RELEASE_UI_SHA256 = "8ea6a2029aa29cac4577753df3f8a01466c92647e3dc1b34f33b6c48e5ce3004"
RELEASE_SKILL_SHA256 = "c03d30a21a67cf9a63e4262a4fbac172e53682e73c5bb12e6998950b736b1c74"
RELEASE_BUNDLE_DIR = "luna-factory-0.2.1-x86_64-unknown-linux-gnu"
MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[3]
ALLOWED_RELEASE_DELTA = {
    "docs/superpowers/plans/2026-10-10-luna-factory-oci.md",
    "plugins/luna-factory/docs/container-architecture.md",
    "plugins/luna-factory/docs/container-deployment.md",
    "plugins/luna-factory/docs/local-service.md",
    "plugins/luna-factory/docs/package.md",
    "plugins/luna-factory/server/src/config.rs",
    "plugins/luna-factory/server/src/http.rs",
    "plugins/luna-factory/server/tests/http_security.rs",
    "plugins/luna-factory/tests/test_container_build.py",
}
ALLOWED_RELEASE_PREFIXES = ("plugins/luna-factory/container/",)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_provenance(data: dict[str, Any]) -> None:
    source = data.get("source", {})
    components = data.get("components", {})
    if data.get("plugin", {}).get("version") != RELEASE_VERSION:
        raise ValueError("release version mismatch")
    if source.get("repository") != RELEASE_REPOSITORY or source.get("tag") != RELEASE_TAG:
        raise ValueError("release source identity mismatch")
    if source.get("commit") != RELEASE_COMMIT:
        raise ValueError("source commit mismatch")
    if source.get("tree") != RELEASE_TREE:
        raise ValueError("source tree mismatch")
    if data.get("artifact", {}).get("name") != RELEASE_ARCHIVE_NAME:
        raise ValueError("release archive name mismatch")
    if data.get("artifact", {}).get("sha256") != RELEASE_ARCHIVE_SHA256:
        raise ValueError("release archive provenance hash mismatch")
    for name, expected in (
        ("binary", RELEASE_BINARY_SHA256),
        ("ui", RELEASE_UI_SHA256),
        ("skill", RELEASE_SKILL_SHA256),
    ):
        if components.get(name, {}).get("sha256") != expected:
            raise ValueError(f"release {name} provenance hash mismatch")


def _checksum_entries(checksum_file: Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line in checksum_file.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            digest, name = line.split(maxsplit=1)
        except ValueError as error:
            raise ValueError("invalid checksum entry") from error
        name = name.lstrip(" *")
        path = PurePosixPath(name)
        if len(digest) != 64 or path.is_absolute() or ".." in path.parts or not name:
            raise ValueError("unsafe checksum entry")
        entries[name] = digest.lower()
    return entries


def verify_internal_checksums(package: Path) -> None:
    sums = package / "SHA256SUMS"
    if not sums.is_file() or sums.is_symlink():
        raise ValueError("package checksum manifest missing")
    entries = _checksum_entries(sums)
    for name, expected in entries.items():
        path = package.joinpath(*PurePosixPath(name).parts)
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"checksum target missing or linked: {name}")
        if sha256_file(path) != expected:
            raise ValueError(f"checksum mismatch: {name}")
    actual = {
        path.relative_to(package).as_posix()
        for path in package.rglob("*")
        if path.is_file() and path.name != "SHA256SUMS"
    }
    if actual != set(entries):
        raise ValueError("package has unlisted or missing files")


def verify_package(package: Path, provenance: dict[str, Any]) -> None:
    verify_internal_checksums(package)
    for relative, expected in (
        ("bin/luna-factoryd", RELEASE_BINARY_SHA256),
        ("ui/dist/index.html", RELEASE_UI_SHA256),
        ("skills/luna-factory/SKILL.md", RELEASE_SKILL_SHA256),
    ):
        path = package / relative
        if path.is_symlink() or not path.is_file() or sha256_file(path) != expected:
            raise ValueError(f"published {relative} hash mismatch")
    version = subprocess.run(
        [str(package / "bin/luna-factoryd"), "--version"],
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    ).stdout.strip()
    if version != f"luna-factoryd {RELEASE_VERSION}":
        raise ValueError("published binary version mismatch")
    if any(provenance.get("verification", {}).get(key) is not True for key in (
        "archives_identical",
        "extracted_checksums",
        "extracted_version",
        "restaging_identical",
    )):
        raise ValueError("release provenance verification record is incomplete")


def safe_extract_archive(archive: Path, destination: Path) -> None:
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    total = 0
    root = destination.resolve()
    with tarfile.open(archive, "r:gz") as source:
        for member in source.getmembers():
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not name.parts:
                raise ValueError(f"unsafe archive member: {member.name}")
            target = destination.joinpath(*name.parts)
            if target.resolve(strict=False).parent != target.parent.resolve(strict=False):
                raise ValueError(f"unsafe archive member: {member.name}")
            if member.isdir():
                target.mkdir(mode=0o755, parents=True, exist_ok=True)
                continue
            if not member.isfile():
                raise ValueError(f"unsupported archive member: {member.name}")
            total += member.size
            if member.size < 0 or total > MAX_ARCHIVE_BYTES:
                raise ValueError("release archive exceeds extraction limit")
            if target.exists() or target.is_symlink():
                raise ValueError(f"duplicate archive member: {member.name}")
            target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
            payload = source.extractfile(member)
            if payload is None:
                raise ValueError(f"archive member has no payload: {member.name}")
            with payload, target.open("xb") as output:
                shutil.copyfileobj(payload, output)
            target.chmod(stat.S_IMODE(member.mode) & 0o755)


def verify_release_files(release_dir: Path) -> tuple[Path, dict[str, Any]]:
    archive = release_dir / RELEASE_ARCHIVE_NAME
    provenance_path = release_dir / f"{RELEASE_ARCHIVE_NAME.removesuffix('.tar.gz')}-provenance.json"
    checksums = release_dir / "SHA256SUMS"
    if any(path.is_symlink() or not path.is_file() for path in (archive, provenance_path, checksums)):
        raise ValueError("release inputs must be regular files")
    if sha256_file(checksums) != RELEASE_CHECKSUMS_SHA256:
        raise ValueError("published SHA256SUMS asset hash mismatch")
    entries = _checksum_entries(checksums)
    for name, expected in (
        (RELEASE_ARCHIVE_NAME, RELEASE_ARCHIVE_SHA256),
        (provenance_path.name, RELEASE_PROVENANCE_SHA256),
    ):
        if entries.get(name) != expected:
            raise ValueError(f"published checksum manifest mismatch: {name}")
        if sha256_file(release_dir / name) != expected:
            raise ValueError(f"published asset hash mismatch: {name}")
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    validate_provenance(provenance)
    return archive, provenance


def build_inputs(source_root: Path) -> tuple[Path, ...]:
    server = source_root / "plugins/luna-factory/server"
    paths = [
        Path("plugins/luna-factory/server/Cargo.toml"),
        Path("plugins/luna-factory/server/Cargo.lock"),
        *(
            path.relative_to(source_root)
            for path in sorted((server / "src").rglob("*"))
            if path.is_file()
        ),
        Path("plugins/luna-factory/assets/logo.png"),
        Path("plugins/luna-factory/container/Containerfile"),
        Path("plugins/luna-factory/container/healthcheck.py"),
    ]
    missing = [path.as_posix() for path in paths if not (source_root / path).is_file()]
    if missing:
        raise ValueError(f"missing OCI build inputs: {', '.join(missing)}")
    return tuple(paths)


def hash_build_inputs(source_root: Path) -> str:
    digest = hashlib.sha256()
    for relative in build_inputs(source_root):
        content = (source_root / relative).read_bytes()
        digest.update(relative.as_posix().encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(content).digest())
    return digest.hexdigest()


def source_patch_sha256(source_root: Path) -> str:
    return hash_build_inputs(source_root)


def validate_release_delta(paths: list[str]) -> None:
    unexpected = [
        path
        for path in paths
        if path not in ALLOWED_RELEASE_DELTA
        and not any(path.startswith(prefix) for prefix in ALLOWED_RELEASE_PREFIXES)
    ]
    if unexpected:
        raise ValueError(f"unreviewed release source delta: {', '.join(unexpected)}")


def image_fingerprint(source_root: Path) -> str:
    digest = hashlib.sha256()
    digest.update(RELEASE_ARCHIVE_SHA256.encode())
    digest.update(hash_build_inputs(source_root).encode())
    return digest.hexdigest()


def image_tag(source_root: Path) -> str:
    return f"localhost/luna-factory:{RELEASE_VERSION}-oci-{image_fingerprint(source_root)[:12]}"


def build_image(release_dir: Path, source_root: Path, podman: str = "podman") -> dict[str, str]:
    head = subprocess.run(
        ["git", "-C", str(source_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    ancestor = subprocess.run(
        ["git", "-C", str(source_root), "merge-base", "--is-ancestor", RELEASE_COMMIT, head],
        check=False,
    )
    if ancestor.returncode != 0:
        raise ValueError(f"build HEAD must descend from pinned release {RELEASE_COMMIT}")
    dirty = subprocess.run(
        ["git", "-C", str(source_root), "status", "--porcelain", "--untracked-files=all"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty:
        raise ValueError("build source checkout must be clean")
    changed_paths = subprocess.run(
        ["git", "-C", str(source_root), "diff", "--name-only", f"{RELEASE_COMMIT}..{head}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    validate_release_delta(changed_paths)
    archive, provenance = verify_release_files(release_dir)
    patch_hash = source_patch_sha256(source_root)
    fingerprint = image_fingerprint(source_root)
    image = image_tag(source_root)
    with tempfile.TemporaryDirectory(prefix="luna-factory-oci-") as temporary:
        context = Path(temporary)
        release_root = context / "release"
        safe_extract_archive(archive, release_root)
        package = release_root / RELEASE_BUNDLE_DIR
        verify_package(package, provenance)
        source_logo = source_root / "plugins/luna-factory/assets/logo.png"
        if sha256_file(source_logo) != sha256_file(package / "assets/logo.png"):
            raise ValueError("pinned source logo differs from the released MCP icon")
        shutil.copytree(source_root / "plugins/luna-factory/server", context / "server", ignore=shutil.ignore_patterns("target", ".git"))
        (context / "assets").mkdir()
        shutil.copy2(source_root / "plugins/luna-factory/assets/logo.png", context / "assets/logo.png")
        shutil.copy2(source_root / "plugins/luna-factory/container/Containerfile", context / "Containerfile")
        shutil.copy2(source_root / "plugins/luna-factory/container/healthcheck.py", context / "healthcheck.py")
        subprocess.run(
            [
                podman,
                "build",
                "--pull=never",
                "--tag",
                image,
                "--build-arg",
                f"SOURCE_COMMIT={RELEASE_COMMIT}",
                "--build-arg",
                f"SOURCE_PATCH_SHA256={patch_hash}",
                "--build-arg",
                f"IMAGE_FINGERPRINT={fingerprint}",
                "--build-arg",
                f"RELEASE_ARCHIVE_SHA256={RELEASE_ARCHIVE_SHA256}",
                "--build-arg",
                f"RELEASE_UI_SHA256={RELEASE_UI_SHA256}",
                ".",
            ],
            cwd=context,
            check=True,
            stdout=sys.stderr,
        )
    image_id = subprocess.run(
        [podman, "image", "inspect", "--format", "{{.Id}}", image],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    image_digest = subprocess.run(
        [podman, "image", "inspect", "--format", "{{.Digest}}", image],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if not image_digest.startswith("sha256:") or len(image_digest) != len("sha256:") + 64:
        raise ValueError("Podman did not return an OCI manifest digest")
    runtime_check = subprocess.run(
        [
            podman,
            "run",
            "--rm",
            "--entrypoint",
            "python3",
            image,
            "-c",
            "import hashlib,json,pathlib,subprocess; root=pathlib.Path('/opt/luna-factory'); binary=pathlib.Path('/usr/local/bin/luna-factoryd'); version=subprocess.run([str(binary),'--version'],check=True,capture_output=True,text=True).stdout.strip(); print(json.dumps({'version':version,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'ui_sha256':hashlib.sha256((root/'ui/dist/index.html').read_bytes()).hexdigest(),'skill_sha256':hashlib.sha256((root/'skills/luna-factory/SKILL.md').read_bytes()).hexdigest()}))",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    runtime = json.loads(runtime_check.stdout)
    if runtime.get("version") != f"luna-factoryd {RELEASE_VERSION}":
        raise ValueError("OCI binary version mismatch")
    if runtime.get("ui_sha256") != RELEASE_UI_SHA256:
        raise ValueError("OCI UI hash differs from the published release")
    if runtime.get("skill_sha256") != RELEASE_SKILL_SHA256:
        raise ValueError("OCI skill hash differs from the published release")
    if runtime.get("binary_sha256") == RELEASE_BINARY_SHA256:
        raise ValueError("OCI binary unexpectedly matches the unpatched release binary")
    return {
        "image": image,
        "image_ref": f"localhost/luna-factory@{image_digest}",
        "image_digest": image_digest,
        "image_id": image_id,
        "release_archive_sha256": RELEASE_ARCHIVE_SHA256,
        "release_binary_sha256": RELEASE_BINARY_SHA256,
        "release_ui_sha256": RELEASE_UI_SHA256,
        "source_commit": RELEASE_COMMIT,
        "build_head": head,
        "source_patch_sha256": patch_hash,
        "image_fingerprint": fingerprint,
        "oci_binary_sha256": runtime["binary_sha256"],
        "oci_binary_version": runtime["version"],
        "oci_ui_sha256": runtime["ui_sha256"],
        "oci_skill_sha256": runtime["skill_sha256"],
        "oci_binary_is_derivative": "true",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-dir", type=Path, help="Directory with release archive, provenance, and SHA256SUMS")
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--podman", default="podman")
    parser.add_argument("--print-image-tag", action="store_true", help="Print the local tag for these source and image inputs")
    arguments = parser.parse_args()
    source_root = arguments.source_root.resolve()
    if arguments.print_image_tag:
        print(image_tag(source_root))
        return 0
    if arguments.release_dir is None:
        parser.error("--release-dir is required unless --print-image-tag is used")
    try:
        result = build_image(arguments.release_dir.resolve(), source_root, arguments.podman)
    except (OSError, ValueError, subprocess.SubprocessError, tarfile.TarError, json.JSONDecodeError) as error:
        print(f"OCI build rejected: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
