#!/usr/bin/env python3
"""Stage the existing Luna Factory plugin with explicitly supplied local builds.

No build, install, activation, credential lookup, or network operation is performed.
The only executable invoked is the supplied trusted binary, with --version.
"""
from __future__ import annotations

import argparse
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import struct
import tomllib
import xml.etree.ElementTree as ET


METADATA = (
    "plugin.json", "mcp.json", ".codex-plugin/plugin.json", ".mcp.json",
    ".claude-plugin/plugin.json", "plugin.yaml", "__init__.py", "README.md",
    "docs/package.md", "docs/local-service.md", "docs/repository-onboarding.md",
    "docs/control-adoption.md", "docs/control-plan.md", "docs/control-wire.md",
    "docs/dogfood-recovery.md",
    "docs/integration-readiness.md", "docs/stack-ci-evidence.json",
    "docs/rollback-verification.json", "docs/cas-verification.md",
    "docs/cas-verification-results.txt", "docs/cas-runtime-evidence.json",
    "docs/graph-backend-evidence.md", "docs/continuation-verification.md",
    "docs/continuation-verification-results.txt",
    "assets/logo.svg", "assets/logo.png",
)
SKILL_FILES = (
    "SKILL.md", "agents/openai.yaml", "references/evals.md",
    "references/routing-and-evidence.md", "references/runtime-compatibility.md",
    "scripts/audit_runtime.py", "tests/test_audit_runtime.py",
)
SHARED_FIELDS = ("name", "version", "description", "author", "homepage", "repository", "license", "keywords")
MAX_UI = 4 * 1024 * 1024
MAX_BINARY = 512 * 1024 * 1024


def validate_branding(files: dict[str, bytes], interface: dict) -> None:
    for field in ("logo", "composerIcon"):
        if interface.get(field) != "./assets/logo.png":
            raise ValueError(f"branding {field} must reference ./assets/logo.png")
    png = files["assets/logo.png"]
    if len(png) > 5 * 1024 * 1024 or len(png) < 33 or png[:16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR":
        raise ValueError("branding PNG is missing or invalid")
    width, height = struct.unpack(">II", png[16:24])
    if width != height or not 48 <= width <= 4096:
        raise ValueError("branding PNG must be square, 48 to 4096 pixels")
    svg = files["assets/logo.svg"]
    if len(svg) > 5 * 1024 * 1024 or b"<!DOCTYPE" in svg.upper():
        raise ValueError("branding SVG must be small and self-contained")
    try:
        root = ET.fromstring(svg)
        size = tuple(float(value) for value in root.attrib["viewBox"].split())
        if root.tag != "{http://www.w3.org/2000/svg}svg" or len(size) != 4 or size[2] != size[3] or not 48 <= size[2] <= 4096:
            raise ValueError("branding SVG must have a valid square viewBox")
        for element in root.iter():
            if element.tag.rsplit("}", 1)[-1] not in {"svg", "rect", "g", "path", "title", "desc"}:
                raise ValueError("branding SVG contains unsupported active or external content")
            if any(name.lower().startswith("on") or "href" in name.lower() or "url(" in value.lower() for name, value in element.attrib.items()):
                raise ValueError("branding SVG contains active or external content")
    except (ET.ParseError, KeyError, TypeError, ValueError) as error:
        raise ValueError("branding SVG must be valid, square, and self-contained") from error


def checked_path(path: Path) -> Path:
    """Reject symlinks in every existing component, including output parents."""
    if ".." in path.parts:
        raise ValueError(f"path traversal is not allowed: {path}")
    path = path.absolute()
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError(f"symlink is not allowed: {component}")
    return path


def read_file(path: Path, label: str, limit: int = MAX_UI) -> tuple[bytes, os.stat_result]:
    path = checked_path(path)
    try:
        if not stat.S_ISREG(path.stat().st_mode):
            raise ValueError(f"{label} must be a regular file")
        with path.open("rb") as handle:
            info = os.fstat(handle.fileno())
            if not stat.S_ISREG(info.st_mode):
                raise ValueError(f"{label} must be a regular file")
            if info.st_size > limit:
                raise ValueError(f"{label} exceeds {limit} bytes")
            data = handle.read(limit + 1)
            if len(data) > limit:
                raise ValueError(f"{label} exceeds {limit} bytes")
            return data, info
    except FileNotFoundError as error:
        raise ValueError(f"missing {label}: {path}") from error


def tree_files(root: Path) -> list[Path]:
    checked_path(root)
    if not root.is_dir():
        raise ValueError(f"missing source directory: {root}")
    files = []
    for directory, dirs, names in os.walk(root, followlinks=False):
        for name in dirs + names:
            checked_path(Path(directory) / name)
        # Python caches are generated local files, never package content.
        dirs[:] = sorted(name for name in dirs if name != "__pycache__")
        files.extend(Path(directory) / name for name in sorted(names))
    return files


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


class SingleHTML(HTMLParser):
    def __init__(self):
        super().__init__()
        self.html = False
        self.app = False
        self.script = False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        self.html |= tag == "html"
        self.app |= attrs.get("id") == "app"
        self.script |= tag == "script" and "src" not in attrs
        # The current workbench has no fetched assets or nested documents.
        if any(name in attrs for name in ("src", "srcset", "poster")) or tag in ("iframe", "object", "embed", "base"):
            raise ValueError("UI must be self-contained; external assets are not allowed")
        if tag == "link" and "href" in attrs:
            raise ValueError("UI must be self-contained; external links are not allowed")


def validate_metadata(files: dict[str, bytes], inputs: dict[str, dict[str, bytes]]) -> str:
    portable = json.loads(files["plugin.json"])
    if portable.get("name") != "luna-factory":
        raise ValueError("plugin identity must be luna-factory")
    version = portable.get("version", "")
    if not isinstance(version, str) or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
        raise ValueError("plugin version must be a semantic version")
    extension = portable.get("extensions", {}).get("com.openai", {})
    if set(extension) != {"interface"}:
        raise ValueError("portable OpenAI metadata must contain only the existing interface; no private app mapping")
    validate_branding(files, extension["interface"])
    for name in (".codex-plugin/plugin.json", ".claude-plugin/plugin.json"):
        compat = json.loads(files[name])
        for key in SHARED_FIELDS:
            if compat.get(key) != portable.get(key):
                raise ValueError(f"{name} {key} differs from portable identity/version metadata")
        if name.startswith(".codex"):
            if compat.get("interface") != extension["interface"]:
                raise ValueError("Codex interface differs from portable interface")
            if compat.get("skills") != "./skills/" or compat.get("mcpServers") != "./.mcp.json":
                raise ValueError("Codex skill/MCP pointers must stay package-relative")
    # The repository generator deliberately writes JSON-compatible YAML.
    hermes = json.loads(files["plugin.yaml"])
    for key in ("name", "version", "description"):
        if hermes.get(key) != portable.get(key):
            raise ValueError(f"Hermes {key} differs from portable identity/version metadata")
    if hermes.get("kind") != "standalone":
        raise ValueError("Hermes plugin kind must remain standalone")
    crate = tomllib.loads(inputs["runtime"]["server/Cargo.toml"].decode())["package"]
    ui = json.loads(inputs["ui"]["ui/package.json"])
    if crate.get("name") != "luna-factoryd" or ui.get("name") != "luna-factory-workbench":
        raise ValueError("runtime/UI identity does not match Luna Factory")
    if crate.get("version") != version or ui.get("version") != version:
        raise ValueError("runtime/UI version differs from plugin version")
    for name, transport in (("mcp.json", "streamable-http"), (".mcp.json", "http")):
        mapping = json.loads(files[name])
        expected = {"luna-factory": {"type": transport, "url": "http://127.0.0.1:8787/mcp"}}
        allowed = {"mcpServers", "$schema"} if name == "mcp.json" else {"mcpServers"}
        if not set(mapping) <= allowed or mapping.get("mcpServers") != expected:
            raise ValueError(f"{name} MCP mapping must keep the credential-free loopback contract")
    skill = files["skills/luna-factory/SKILL.md"].decode()
    if not skill.startswith("---\n") or "\nname: luna-factory\n" not in skill.split("\n---", 1)[0]:
        raise ValueError("canonical skill identity must be luna-factory")
    return version


def stage(binary: Path, ui: Path, output: Path) -> None:
    root = checked_path(Path(__file__).absolute().parents[1])
    binary, ui, output = (checked_path(path) for path in (binary, ui, output))
    # Protect source directories even if the requested destination does not exist.
    checkout = root.parent.parent if root.parent.name == "plugins" else root
    for source in (checkout, binary, ui):
        if output == source or output in source.parents or source in output.parents:
            raise ValueError("output/source overlap is not allowed")
    if output.exists():
        raise ValueError(f"output already exists; choose a new destination: {output}")
    if not output.parent.is_dir():
        raise ValueError("output parent must already exist")

    skill_root = root / "skills/luna-factory"
    if {path.name for path in (root / "skills").iterdir()} != {"luna-factory"}:
        raise ValueError("package must contain exactly one canonical skill")
    actual_skill = {path.relative_to(skill_root).as_posix() for path in tree_files(skill_root)}
    if actual_skill != set(SKILL_FILES):
        raise ValueError(f"unexpected skill inventory: {sorted(actual_skill ^ set(SKILL_FILES))}")
    files = {}
    for name in (*METADATA, *("skills/luna-factory/" + name for name in SKILL_FILES)):
        files[name] = read_file(root / name, "branding " + name if name.startswith("assets/") else name)[0]

    input_names = {
        "runtime": ["server/Cargo.toml", "server/Cargo.lock", "assets/logo.png"],
        "ui": ["ui/package.json", "ui/package-lock.json", "ui/index.html", "ui/tsconfig.json", "ui/vite.config.ts"],
    }
    for kind, directory in (("runtime", "server/src"), ("ui", "ui/src"), ("ui", "ui/tooling")):
        for path in tree_files(root / directory):
            suffixes = {".rs"} if kind == "runtime" else {".ts", ".css"}
            if path.suffix not in suffixes or any(part.startswith(".") for part in path.relative_to(root).parts):
                raise ValueError(f"unexpected build input: {path.relative_to(root)}")
            input_names[kind].append(path.relative_to(root).as_posix())
    inputs, newest = {}, {}
    for kind, names in input_names.items():
        inputs[kind], newest[kind] = {}, 0
        for name in sorted(names):
            data, info = read_file(root / name, name)
            inputs[kind][name] = data
            newest[kind] = max(newest[kind], info.st_mtime_ns)
    version = validate_metadata(files, inputs)
    binary_data, binary_info = read_file(binary, "binary", MAX_BINARY)
    html_data, html_info = read_file(ui, "UI")
    if not binary_data or not binary_info.st_mode & 0o111:
        raise ValueError("binary must be non-empty and executable")
    if binary_info.st_mtime_ns < newest["runtime"]:
        raise ValueError("stale binary: rebuild after the runtime sources and lockfile")
    if html_info.st_mtime_ns < newest["ui"]:
        raise ValueError("stale UI: rebuild after the UI sources and lockfile")
    html = SingleHTML()
    html.feed(html_data.decode("utf-8"))
    if not (html.html and html.app and html.script):
        raise ValueError("UI must be built, self-contained HTML with an app root and inline script")
    try:
        probe = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=5, check=True)
    except (OSError, subprocess.SubprocessError) as error:
        raise ValueError("binary version probe failed; provide a trusted local build for this machine") from error
    runtime_version = f"luna-factoryd {version}"
    if probe.stdout.strip() != runtime_version:
        raise ValueError(f"binary version must be {runtime_version}")
    if read_file(binary, "binary", MAX_BINARY)[0] != binary_data:
        raise ValueError("binary changed during validation; rebuild and retry")
    files["bin/luna-factoryd"] = binary_data
    files["ui/dist/index.html"] = html_data
    receipt = {
        "schema_version": 1,
        "plugin": {"name": "luna-factory", "version": version},
        "runtime_version": runtime_version,
        "build_inputs": {kind: {name: digest(data) for name, data in values.items()} for kind, values in inputs.items()},
        "files": {name: {"sha256": digest(data), "size": len(data), "mode": "0755" if name == "bin/luna-factoryd" else "0644"} for name, data in sorted(files.items())},
    }
    files["runtime-receipt.json"] = json_bytes(receipt)
    files["SHA256SUMS"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(files.items())).encode()
    # Exclusive creation protects an existing destination, including empty ones.
    # Write the receipt/checksum last; an interrupted stage is never a valid package.
    output.mkdir(mode=0o755)
    for name, data in files.items():
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("xb") as handle:
            handle.write(data)
        destination.chmod(0o755 if name == "bin/luna-factoryd" else 0o644)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="trusted, already-built local luna-factoryd")
    parser.add_argument("--ui", type=Path, required=True, help="already-built single-file UI HTML")
    parser.add_argument("--output", type=Path, required=True, help="new package directory; parent must exist")
    args = parser.parse_args()
    try:
        stage(args.binary, args.ui, args.output)
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f"Cannot stage Luna Factory: {error}", file=sys.stderr)
        return 1
    print(f"Staged Luna Factory at {args.output}; verify SHA256SUMS before installation")
    return 0


if __name__ == "__main__":
    sys.exit(main())
