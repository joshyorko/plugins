#!/usr/bin/env python3
"""Verify and archive a trusted Linux build from an exact clean release source.

No build, download, installation, service activation, or publication occurs here.
The supplied native binary is executed only with --version, including after extraction.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True
import package_runtime

TARGET = "x86_64-unknown-linux-gnu"


def run(args: list[str], cwd: Path | None = None) -> str:
    return subprocess.run(args, cwd=cwd, check=True, capture_output=True,
                          text=True, timeout=30).stdout.strip()


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_identity(repo: Path, expected: str, tag: str, version: str) -> dict:
    commit = run(["git", "rev-parse", "HEAD"], repo)
    if not re.fullmatch(r"[0-9a-f]{40}", expected) or commit != expected:
        raise ValueError("source SHA must equal the exact checked-out commit")
    if tag != f"v{version}":
        raise ValueError("release tag must match the plugin version")
    if run(["git", "rev-parse", f"refs/tags/{tag}^{{commit}}"], repo) != commit:
        raise ValueError("release tag must resolve to the exact source SHA")
    if run(["git", "status", "--porcelain", "--untracked-files=all"], repo):
        raise ValueError("release source must be clean, including untracked files")
    return {"repository": "https://github.com/joshyorko/plugins", "commit": commit,
            "tree": run(["git", "rev-parse", "HEAD^{tree}"], repo), "tag": tag,
            "timestamp": int(run(["git", "show", "-s", "--format=%ct", "HEAD"], repo))}


def elf_requirements(binary: Path) -> dict:
    with binary.open("rb") as stream:
        header = stream.read(20)
    if (platform.system() != "Linux" or platform.machine() != "x86_64"
            or len(header) != 20 or header[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(header[18:20], "little") != 62):
        raise ValueError("release binary must be native Linux x86_64 ELF64")
    versions = run(["readelf", "--version-info", str(binary)])
    dynamic = run(["readelf", "-d", str(binary)])
    program = run(["readelf", "-l", str(binary)])
    glibc = sorted(set(re.findall(r"GLIBC_([0-9]+(?:\.[0-9]+)+)", versions)),
                   key=lambda value: tuple(map(int, value.split("."))))
    interpreter = re.findall(r"Requesting program interpreter: ([^\]]+)\]", program)
    needed = sorted(re.findall(r"\(NEEDED\).*Shared library: \[([^\]]+)\]", dynamic))
    if not glibc or interpreter != ["/lib64/ld-linux-x86-64.so.2"] or "libc.so.6" not in needed:
        raise ValueError("release ELF must expose the supported GNU loader and GLIBC requirements")
    return {"class": "ELF64", "machine": "x86_64", "interpreter": interpreter[0],
            "needed_libraries": needed, "glibc_symbol_versions": glibc,
            "minimum_glibc_symbol_version": glibc[-1]}


def inventory(root: Path) -> dict:
    return {p.relative_to(root).as_posix(): {"sha256": sha256(p), "mode": p.stat().st_mode & 0o777}
            for p in sorted(root.rglob("*")) if p.is_file()}


def archive_package(staged: Path, archive: Path, name: str, timestamp: int) -> None:
    with archive.open("xb") as destination:
        with gzip.GzipFile(filename="", mode="wb", fileobj=destination, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as tar:
                for path in [staged, *sorted(staged.rglob("*"))]:
                    relative = path.relative_to(staged).as_posix()
                    entry = name if relative == "." else f"{name}/{relative}"
                    info = tar.gettarinfo(str(path), arcname=entry)
                    if not (info.isfile() or info.isdir()):
                        raise ValueError("archive accepts regular files and directories only")
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    info.mtime = timestamp
                    info.mode = 0o755 if info.isdir() or relative == "bin/luna-factoryd" else 0o644
                    info.pax_headers = {}
                    if info.isfile():
                        with path.open("rb") as stream:
                            tar.addfile(info, stream)
                    else:
                        tar.addfile(info)


def build(binary: Path, ui: Path, output: Path, expected: str, tag: str) -> None:
    plugin = Path(__file__).resolve().parents[1]
    repo = plugin.parents[1]
    binary, ui, output = map(package_runtime.checked_path, (binary, ui, output))
    if output.exists():
        raise ValueError("release output already exists; choose a new directory")
    if output.is_relative_to(repo) or repo.is_relative_to(output):
        raise ValueError("release output must be outside the source checkout")
    version = json.loads((plugin / "plugin.json").read_text())["version"]
    source = source_identity(repo, expected, tag, version)
    elf = elf_requirements(binary)
    os_release = platform.freedesktop_os_release()
    tooling = {name: run(args).splitlines()[0] for name, args in {
        "rustc": ["rustc", "--version"], "cargo": ["cargo", "--version"],
        "node": ["node", "--version"], "npm": ["npm", "--version"],
        "readelf": ["readelf", "--version"], "cc": ["cc", "--version"],
    }.items()}
    tooling["python"] = platform.python_version()
    builder = {"target": TARGET, "os_id": os_release["ID"], "os_version": os_release["VERSION_ID"],
               "glibc": run(["getconf", "GNU_LIBC_VERSION"]), "tooling": tooling}
    name = f"luna-factory-{version}-{TARGET}"
    # All potentially failing validation is confined to a temporary sibling;
    # publish the complete directory only after restaging/extraction checks.
    with tempfile.TemporaryDirectory(prefix="luna-release-", dir=output.parent) as scratch:
        work = Path(scratch)
        first, second = work / "first", work / "second"
        package_runtime.stage(binary, ui, first)
        package_runtime.stage(binary, ui, second)
        if inventory(first) != inventory(second):
            raise ValueError("restaging identical inputs changed package bytes or modes")
        products = work / "products"
        products.mkdir()
        archive = products / f"{name}.tar.gz"
        archive_package(first, archive, name, source["timestamp"])
        comparison = work / "comparison.tar.gz"
        archive_package(second, comparison, name, source["timestamp"])
        if sha256(archive) != sha256(comparison):
            raise ValueError("normalized release archives are not identical")
        extracted = work / "extracted"
        with tarfile.open(archive) as tar:
            tar.extractall(extracted, filter="data")
        package = extracted / name
        run(["sha256sum", "--check", "SHA256SUMS"], package)
        if inventory(package) != inventory(first):
            raise ValueError("extraction changed package files or modes")
        if run([str(package / "bin/luna-factoryd"), "--version"]) != f"luna-factoryd {version}":
            raise ValueError("extracted binary version mismatch")
        receipt = json.loads((package / "runtime-receipt.json").read_text())
        if source_identity(repo, expected, tag, version) != source:
            raise ValueError("source identity changed during packaging")
        provenance = {"schema_version": 1, "plugin": receipt["plugin"], "source": source,
                      "build": builder, "elf": elf,
                      "artifact": {"name": archive.name, "sha256": sha256(archive)},
                      "components": {component: {"sha256": sha256(package / path)} for component, path in {
                          "binary": "bin/luna-factoryd", "ui": "ui/dist/index.html",
                          "skill": "skills/luna-factory/SKILL.md", "receipt": "runtime-receipt.json",
                      }.items()},
                      "verification": {"restaging_identical": True, "archives_identical": True,
                                       "extracted_checksums": True, "extracted_version": True},
                      "limits": "Artifact verification; not reproducible compilation, live native execution, or ChatGPT acceptance."}
        (products / f"{name}-provenance.json").write_bytes(package_runtime.json_bytes(provenance))
        products.rename(output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "ui", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    try:
        build(args.binary, args.ui, args.output, args.source_sha, args.tag)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"Cannot archive Luna Factory: {error}", file=sys.stderr)
        return 1
    print("Verified Luna Factory archive and provenance")
    return 0


if __name__ == "__main__":
    sys.exit(main())
