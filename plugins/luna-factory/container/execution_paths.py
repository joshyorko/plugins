#!/usr/bin/env python3
"""Read-only, two-namespace path probe. Does not contact Codex, grant or execute jobs.

Keep requests/receipts private: they contain operator filesystem paths. A successful
receipt proves visibility only in the namespace where `verify` actually ran.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess

MAX_BYTES = 512 * 1024 * 1024


def absolute(value):
    path = Path(value)
    if not path.is_absolute() or ".." in path.parts or path == Path("/"):
        raise ValueError("absolute non-root path without traversal required")
    return path


def mapped(config, value):
    path = absolute(value)
    matches = [(absolute(local), absolute(native)) for local, native in config.get("native_paths", {}).items()
               if path.is_relative_to(absolute(local))]
    if len(matches) != 1:
        raise ValueError("exactly one explicit native path mapping required")
    local, native = matches[0]
    if local.resolve(strict=True) != local:
        raise ValueError("symlinked Factory mapping root")
    return str(native / path.relative_to(local))


def digest_file(path):
    path = absolute(path)
    before = path.stat()
    if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_BYTES:
        raise ValueError("bounded regular file required")
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as source:
        while block := source.read(65536):
            total += len(block)
            if total > MAX_BYTES:
                raise ValueError("file size bound exceeded")
            digest.update(block)
    after = path.stat()
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
        raise ValueError("file changed during probe")
    return {"sha256": digest.hexdigest(), "bytes": total, "executable": bool(before.st_mode & 0o111)}


def git(root, *args):
    result = subprocess.run(["git", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null",
                             "-C", str(root), *args], check=True, capture_output=True, timeout=10)
    if len(result.stdout) > 1024 * 1024:
        raise ValueError("git output bound exceeded")
    return result.stdout


def repository(path):
    path = absolute(path)
    if path.resolve(strict=True) != path or Path(os.fsdecode(git(path, "rev-parse", "--show-toplevel")).strip()) != path:
        raise ValueError("canonical repository root required")
    index = git(path, "ls-files", "--stage", "-z")
    untracked = git(path, "ls-files", "--others", "--exclude-standard", "-z")
    names = set(untracked.split(b"\0")) - {b""}
    for record in filter(None, index.split(b"\0")):
        metadata, name = record.split(b"\t", 1)
        if metadata.startswith(b"160000 "):
            raise ValueError("submodules require separate qualification")
        names.add(name)
    if len(names) > 25000:
        raise ValueError("repository file count bound exceeded")
    digest = hashlib.sha256(b"luna-shared-path-snapshot-v1\0" + index)
    total = 0
    for raw in sorted(names):
        name = os.fsdecode(raw)
        relative = Path(name)
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("invalid repository relative path")
        file = path / relative
        if not file.parent.resolve().is_relative_to(path):
            raise ValueError("repository parent escapes root")
        digest.update(len(raw).to_bytes(8, "little") + raw)
        if file.is_symlink():
            digest.update(b"symlink\0" + os.fsencode(os.readlink(file)))
        elif not file.exists():
            digest.update(b"missing\0")
        else:
            facts = digest_file(file)
            total += facts["bytes"]
            if total > MAX_BYTES:
                raise ValueError("repository byte bound exceeded")
            digest.update(json.dumps(facts, sort_keys=True).encode())
    return {"head": os.fsdecode(git(path, "rev-parse", "HEAD")).strip(),
            "snapshot_sha256": digest.hexdigest(), "files": len(names)}


def prepare(config, factory_binary, workspace):
    if config.get("native_transport") != "existing_daemon":
        raise ValueError("existing_daemon configuration required")
    paths = [("factory_binary", factory_binary, "file"), ("codex_binary", config["codex_binary"], "file"),
             ("canonical_skill", config["skill_path"], "file"), ("disposable_canary", workspace, "repository")]
    paths += [(f"repository:{alias}", item["root"], "repository") for alias, item in config["repositories"].items()]
    entries = []
    for role, local, kind in paths:
        native = mapped(config, local)
        facts = repository(local) if kind == "repository" else digest_file(local)
        entries.append({"role": role, "native_path": native, "kind": kind, "expected": facts})
        if kind == "repository":
            common = os.fsdecode(git(local, "rev-parse", "--path-format=absolute", "--git-common-dir")).strip()
            entries.append({"role": f"{role}:git_common_dir", "native_path": mapped(config, common),
                            "kind": "git_common_dir", "expected": {"head": facts["head"]}})
    return {"schema": "luna-factory.execution-path-request.v1", "entries": entries}


def verify(request):
    if request.get("schema") != "luna-factory.execution-path-request.v1" or not 1 <= len(request["entries"]) <= 256:
        raise ValueError("invalid path probe request")
    checks = []
    for entry in request["entries"]:
        path = absolute(entry["native_path"])
        if entry["kind"] == "file":
            actual = digest_file(path)
        elif entry["kind"] == "repository":
            actual = repository(path)
        elif entry["kind"] == "git_common_dir":
            actual = {"head": os.fsdecode(git(path, "rev-parse", "HEAD")).strip()}
        else:
            raise ValueError("unknown probe entry kind")
        checks.append({"role": entry["role"], "passed": actual == entry["expected"]})
    return {"schema": "luna-factory.execution-path-receipt.v1", "passed": all(c["passed"] for c in checks),
            "scope": "invoking_process_namespace_only", "host_identity": "operator_must_verify",
            "qualification_grant": False, "inference_calls": 0, "checks": checks}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="mode", required=True)
    source = commands.add_parser("prepare")
    source.add_argument("--config", required=True, type=Path)
    source.add_argument("--factory-binary", required=True, type=Path)
    source.add_argument("--workspace", required=True, type=Path)
    target = commands.add_parser("verify")
    target.add_argument("--request", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.mode == "prepare":
        result = prepare(json.loads(args.config.read_text()), args.factory_binary, args.workspace)
    else:
        if args.request.stat().st_size > 1024 * 1024:
            raise ValueError("request too large")
        result = verify(json.loads(args.request.read_text()))
    fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    if args.mode == "verify" and not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
