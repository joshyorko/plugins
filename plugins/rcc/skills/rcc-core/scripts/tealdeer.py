#!/usr/bin/env python3
"""Opt-in management of RCC's Tealdeer page; never edits Tealdeer configuration."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
from contextlib import contextmanager, suppress
from dataclasses import dataclass
from functools import cmp_to_key
from pathlib import Path


class Conflict(RuntimeError):
    """An existing or edited user asset cannot safely be managed."""


@dataclass(frozen=True)
class Paths:
    page: Path
    cache: Path
    state: Path

    @property
    def anchor(self) -> Path:
        return self.state.parent / "rcc.page.md"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


@contextmanager
def locked(state: Path):
    state.parent.mkdir(parents=True, exist_ok=True)
    with (state.parent / ".lock").open("a+b") as handle:
        if os.name == "posix":
            import fcntl

            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        else:
            import msvcrt

            handle.write(b"0")
            handle.flush()
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        yield


def load(state: Path) -> dict:
    record = json.loads(state.read_text())
    if (not isinstance(record, dict) or record.get("schema_version") != 1
            or record.get("mode") not in {"link", "copy"}
            or record.get("source_kind") not in {"checkout", "snapshot"}
            or not isinstance(record.get("owned"), bool)):
        raise Conflict(f"Unrecognized RCC Tealdeer receipt: {state}")
    for key in ("source", "page", "cache"):
        if not isinstance(record.get(key), str) or not Path(record[key]).is_absolute():
            raise Conflict(f"Invalid {key} in RCC Tealdeer receipt: {state}")
    return record


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".rcc-tldr-", delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(data)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def save(state: Path, record: dict) -> None:
    atomic_write(state, (json.dumps(record, indent=2) + "\n").encode())


def canonical_page(path: Path) -> bool:
    if tuple(path.parts[-4:]) != ("rcc-core", "references", "tldr", "rcc.page.md"):
        return False
    try:
        return path.is_file() and json.loads((path.parents[4] / "plugin.json").read_text())["name"] == "rcc"
    except (OSError, ValueError, KeyError, IndexError):
        return False


def matches(page: Path, record: dict, additional_digest: str | None = None) -> bool:
    if record["mode"] == "link":
        return page.is_symlink() and os.readlink(page) == record.get("link_target")
    return (not page.is_symlink() and page.is_file()
            and digest(page.read_bytes()) in {record.get("sha256"), additional_digest})


def sync_record(source: Path, paths: Paths, record: dict) -> dict:
    if not record["owned"]:
        if not matches(paths.page, record):
            raise Conflict(f"Preserving changed Tealdeer page: {paths.page}")
        return record
    # Adopted/checkout sources remain under their checkout owner's update policy.
    if record["source_kind"] == "checkout":
        source = Path(record["source"])
    content = source.read_bytes()
    checksum = digest(content)
    if not matches(paths.page, record, checksum):
        raise Conflict(f"Preserving changed Tealdeer page: {paths.page}")
    if record["mode"] == "copy":
        if paths.page.read_bytes() != content:
            atomic_write(paths.page, content)
    elif record.get("anchored"):
        if (paths.anchor.is_symlink() or not paths.anchor.is_file()
                or digest(paths.anchor.read_bytes()) not in {record["sha256"], checksum}):
            raise Conflict(f"Preserving changed managed page: {paths.anchor}")
        if paths.anchor.read_bytes() != content:
            atomic_write(paths.anchor, content)
    record.update(source=str(source), sha256=checksum)
    paths.cache.mkdir(parents=True, exist_ok=True)
    save(paths.state, record)
    return record


def install(source: Path, paths: Paths, mode: str = "auto", source_kind: str = "checkout") -> dict:
    source = source.resolve(strict=True)
    if mode not in {"auto", "link", "copy"} or source_kind not in {"checkout", "snapshot"}:
        raise ValueError("Invalid RCC Tealdeer installation mode")
    with locked(paths.state):
        if paths.state.exists():
            record = load(paths.state)
            if Path(record["page"]) != paths.page:
                raise Conflict("Tealdeer paths changed; uninstall the previous integration first")
            return sync_record(source, paths, record)
        content = source.read_bytes()
        record = {"schema_version": 1, "source": str(source), "source_kind": source_kind,
                  "page": str(paths.page), "cache": str(paths.cache), "owned": True,
                  "mode": "copy", "sha256": digest(content), "anchored": False}
        anchor_created = False
        if paths.page.is_symlink():
            existing = paths.page.resolve()
            if existing != source and not canonical_page(existing):
                raise Conflict(f"Preserving unrelated Tealdeer symlink: {paths.page}")
            record.update(source=str(existing), source_kind="checkout", mode="link",
                          owned=False, link_target=os.readlink(paths.page))
        elif paths.page.exists():
            raise Conflict(f"Preserving user-owned Tealdeer page: {paths.page}")
        else:
            paths.cache.mkdir(parents=True, exist_ok=True)
            paths.page.parent.mkdir(parents=True, exist_ok=True)
            if mode != "copy":
                target = source
                if source_kind == "snapshot":
                    if paths.anchor.exists() or paths.anchor.is_symlink():
                        raise Conflict(f"Preserving existing managed-page location: {paths.anchor}")
                    with paths.anchor.open("xb") as handle:
                        handle.write(content)
                    anchor_created = True
                    target = paths.anchor
                try:
                    paths.page.symlink_to(target)
                except OSError:
                    if mode == "link" or paths.page.exists() or paths.page.is_symlink():
                        if anchor_created:
                            paths.anchor.unlink()
                        raise
                else:
                    record.update(mode="link", link_target=str(target), anchored=target != source)
            if record["mode"] == "copy":
                with paths.page.open("xb") as handle:
                    handle.write(content)
                if anchor_created:
                    paths.anchor.unlink(missing_ok=True)
        try:
            paths.cache.mkdir(parents=True, exist_ok=True)
            save(paths.state, record)
        except BaseException:
            with suppress(OSError):
                if record["owned"] and matches(paths.page, record):
                    paths.page.unlink()
                if (anchor_created and not paths.anchor.is_symlink() and paths.anchor.is_file()
                        and digest(paths.anchor.read_bytes()) == record["sha256"]):
                    paths.anchor.unlink()
            raise
        return record


def sync(source: Path, state: Path, source_kind: str = "checkout") -> dict | None:
    if not state.exists():
        return None
    with locked(state):
        if not state.exists():
            return None
        record = load(state)
        paths = Paths(Path(record["page"]), Path(record["cache"]), state)
        return sync_record(source.resolve(), paths, record)


def compare_versions(left: str, right: str) -> int:
    """Match Codex's SemVer ordering, with lexical fallback for other cache names."""
    def parsed(value: str):
        match = re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?", value)
        if not match:
            return None
        prerelease = match[4]
        identifiers = []
        for item in prerelease.split(".") if prerelease else []:
            if not item or (item.isdigit() and len(item) > 1 and item[0] == "0"):
                return None
            identifiers.append((0, int(item)) if item.isdigit() else (1, item))
        return tuple(map(int, match.group(1, 2, 3))), prerelease is None, tuple(identifiers)
    a, b = parsed(left), parsed(right)
    if a is None or b is None:
        a, b = left, right
    return (a > b) - (a < b)


def active_source(record: dict) -> Path:
    base = record.get("plugin_base")
    if record["source_kind"] == "checkout" or not base:
        return Path(record["source"])
    root = Path(base)
    versions = [p.name for p in root.iterdir() if p.is_dir() and not p.is_symlink()
                and re.fullmatch(r"[A-Za-z0-9_.+-]+", p.name)]
    version = "local" if "local" in versions else max(versions, key=cmp_to_key(compare_versions))
    return root / version / "skills/rcc-core/references/tldr/rcc.page.md"


def read_hooks(path: Path) -> tuple[dict, bytes | None]:
    if path.is_symlink():
        raise Conflict(f"Preserving symlinked Codex hooks configuration: {path}; use manual sync")
    before = path.read_bytes() if path.exists() else None
    data = json.loads(before) if before is not None else {}
    if (not isinstance(data, dict) or not isinstance(data.get("hooks", {}), dict)
            or not isinstance(data.get("hooks", {}).get("SessionStart", []), list)):
        raise Conflict(f"Preserving unsupported Codex hooks configuration: {path}")
    return data, before


def write_hooks(path: Path, data: dict, before: bytes | None) -> None:
    current = path.read_bytes() if path.exists() else None
    if current != before or path.is_symlink():
        raise Conflict(f"Codex hooks changed during setup; retry without replacing {path}")
    atomic_write(path, (json.dumps(data, indent=2) + "\n").encode())


def register_hook(source: Path, state: Path, codex_home: Path) -> None:
    """Only explicit setup adds a user hook; portable plugin hooks are not dispatched by Codex 0.164."""
    hook_path = codex_home / "hooks.json"
    with locked(state):
        record = load(state)
        if record.get("hook_path", str(hook_path)) != str(hook_path):
            raise Conflict("Tealdeer is registered with another Codex home; uninstall it before moving")
        data, before = read_hooks(hook_path)
        runner = state.parent / "tealdeer.py"
        content = Path(__file__).read_bytes()
        runner_existed = runner.exists()
        runner_owned = not runner_existed or record.get("runner_owned", False)
        if runner.is_symlink() or (runner.exists() and digest(runner.read_bytes()) not in {record.get("runner_sha256"), digest(content)}):
            raise Conflict(f"Preserving changed synchronization helper: {runner}")
        if runner_existed and not runner_owned and runner.read_bytes() != content:
            raise Conflict(f"Preserving adopted synchronization helper: {runner}")
        group = {"hooks": [{"type": "command", "command": shlex.join(
            ["python3", str(runner), "sync", "--managed-hook", "--state-file", str(state)])}]}
        hooks = data.setdefault("hooks", {})
        starts = hooks.setdefault("SessionStart", [])
        previous = record.get("hook_group")
        if previous and previous != group and previous in starts:
            starts.remove(previous)
        hook_owned = record.get("hook_owned", False)
        if group not in starts:
            starts.append(group)
            hook_owned = True
        metadata = {"hook_path": str(hook_path), "hook_group": group,
                    "runner_sha256": digest(content), "hook_owned": hook_owned,
                    "runner_owned": runner_owned}
        if "hook_created" not in record:
            original = json.loads(before) if before is not None else {}
            metadata.update(hook_created=before is None,
                            hook_had_hooks="hooks" in original,
                            hook_had_start="SessionStart" in original.get("hooks", {}))
        plugin = source.parents[4]
        if plugin.parent.parent.parent == codex_home / "plugins/cache" and plugin.parent.name == "rcc":
            metadata["plugin_base"] = str(plugin.parent)
        if not runner_existed or (runner_owned and runner.read_bytes() != content):
            atomic_write(runner, content)
        record.update(metadata)
        save(state, record)
        if before is None or json.loads(before) != data:
            write_hooks(hook_path, data, before)


def remove_hook(record: dict, state: Path) -> None:
    if "hook_path" not in record or not record.get("hook_owned"):
        return
    path = Path(record["hook_path"])
    try:
        data, before = read_hooks(path)
        starts = data.get("hooks", {}).get("SessionStart", [])
        if record["hook_group"] in starts:
            starts.remove(record["hook_group"])
            if not starts and not record.get("hook_had_start"):
                data["hooks"].pop("SessionStart", None)
            if not data.get("hooks") and not record.get("hook_had_hooks"):
                data.pop("hooks", None)
            if not data and record.get("hook_created"):
                if path.read_bytes() != before:
                    raise Conflict(f"Codex hooks changed during uninstall: {path}")
                path.unlink()
            else:
                write_hooks(path, data, before)
        elif starts:
            return  # A user-modified hook may still refer to the durable helper.
        runner = state.parent / "tealdeer.py"
        if str(runner) in json.dumps(data, ensure_ascii=False):
            return
        if record.get("runner_owned") and not runner.is_symlink() and runner.is_file() and digest(runner.read_bytes()) == record["runner_sha256"]:
            runner.unlink()
    except (Conflict, OSError, ValueError):
        # Disabling the receipt still makes a retained/edited hook a no-op.
        return


def uninstall(state: Path) -> None:
    if not state.exists():
        return
    with locked(state):
        if not state.exists():
            return
        record = load(state)
        remove_hook(record, state)
        page = Path(record["page"])
        if record["owned"] and matches(page, record):
            page.unlink()
        anchor = state.parent / "rcc.page.md"
        if (record.get("anchored") and not anchor.is_symlink() and anchor.is_file()
                and digest(anchor.read_bytes()) == record["sha256"]):
            anchor.unlink()
        state.unlink()


def state_path() -> Path:
    root = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    if not root.is_absolute():
        root = Path.home() / ".local/state"
    return root / "agent-plugins/rcc-tealdeer/state.json"


def discover(state: Path) -> Paths:
    version = subprocess.run(["tldr", "--version"], capture_output=True, text=True, check=True)
    if not version.stdout.startswith("tealdeer "):
        raise Conflict("This optional integration requires the existing Tealdeer client")
    result = subprocess.run(["tldr", "--no-auto-update", "--show-paths"],
                            capture_output=True, text=True, check=True)
    reported = {}
    for line in result.stdout.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            reported[key.strip()] = re.sub(r"\s+\([^\n]*\)$", "", value.strip())
    page_dir, cache = Path(reported["Custom pages dir"]), Path(reported["Pages dir"])
    if not page_dir.is_absolute() or not cache.is_absolute():
        raise Conflict("Tealdeer must report absolute custom-page and cache paths")
    return Paths(page_dir / "rcc.page.md", cache, state)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["install", "sync", "uninstall", "status"])
    parser.add_argument("--mode", choices=["auto", "link", "copy"], default="auto")
    parser.add_argument("--source-kind", choices=["auto", "checkout", "snapshot"], default="auto")
    parser.add_argument("--codex-home", type=Path)
    parser.add_argument("--state-file", type=Path)
    parser.add_argument("--managed-hook", action="store_true")
    args = parser.parse_args()
    state = args.state_file or state_path()
    source = Path(__file__).resolve().parents[1] / "references/tldr/rcc.page.md"
    kind = args.source_kind
    if kind == "auto":
        kind = "checkout" if len(source.parents) > 6 and (source.parents[6] / "marketplaces/catalog.json").is_file() else "snapshot"
    try:
        if args.command == "install":
            codex_home = (args.codex_home or Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")).resolve()
            if state.exists() and load(state).get("hook_path", str(codex_home / "hooks.json")) != str(codex_home / "hooks.json"):
                raise Conflict("Tealdeer is registered with another Codex home; uninstall it before moving")
            read_hooks(codex_home / "hooks.json")
            record = install(source, discover(state), args.mode, kind)
            register_hook(source, state, codex_home)
            print(f"RCC Tealdeer enabled ({record['mode']}): {record['page']}")
        elif args.command == "sync":
            if args.managed_hook:
                if not state.exists():
                    return 0
                source = active_source(load(state))
            sync(source, state, kind)
        elif args.command == "uninstall":
            uninstall(state)
            print("RCC Tealdeer disabled; adopted or changed user pages were preserved")
        else:
            print(json.dumps(load(state) if state.exists() else {"enabled": False}, indent=2))
    except (Conflict, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"RCC Tealdeer: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
