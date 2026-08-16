#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = ROOT / "marketplaces" / "catalog.json"


INIT_TEMPLATE = """\
from __future__ import annotations

from pathlib import Path


def register(ctx):
    base = Path(__file__).parent
    skills_dir = base / "skills"
    if not skills_dir.is_dir():
        return

    for skill_md in sorted(skills_dir.glob("*/SKILL.md")):
        name = skill_md.parent.name
        description = _read_description(skill_md)
        ctx.register_skill(name=name, path=skill_md, description=description)


def _read_description(skill_md: Path) -> str:
    try:
        text = skill_md.read_text(encoding="utf-8")
    except OSError:
        return ""

    if not text.startswith("---"):
        return ""

    end = text.find("\\n---", 3)
    if end == -1:
        return ""

    for line in text[3:end].splitlines():
        if line.startswith("description:"):
            return line.split(":", 1)[1].strip().strip("\\\"'")
    return ""
"""


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def plugin_manifest(plugin_root: Path) -> dict:
    return load_json(plugin_root / "plugin.json")


def hermes_plugin_yaml(plugin: dict, manifest: dict) -> str:
    description = manifest.get("description") or plugin["description"]
    # Keep this JSON-compatible YAML so the repo can validate without PyYAML.
    data = {
        "name": plugin["name"],
        "version": manifest.get("version", "1.0.0"),
        "description": description,
        "kind": "standalone",
    }
    return json.dumps(data, indent=2, sort_keys=False) + "\n"


def write_if_changed(path: Path, content: str, check: bool) -> bool:
    current = path.read_text(encoding="utf-8") if path.exists() else None
    if current == content:
        return False
    if check:
        print(f"stale: {path.relative_to(ROOT)}")
        return True
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    print(f"wrote {path.relative_to(ROOT)}")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build Hermes plugin compatibility shims.")
    parser.add_argument("--check", action="store_true", help="Fail if generated Hermes plugin files are out of date.")
    args = parser.parse_args(argv)

    catalog = load_json(CATALOG_PATH)
    changed = False

    for plugin in catalog["plugins"]:
        plugin_root = ROOT / "plugins" / plugin["name"]
        manifest = plugin_manifest(plugin_root)
        changed |= write_if_changed(plugin_root / "plugin.yaml", hermes_plugin_yaml(plugin, manifest), args.check)
        changed |= write_if_changed(plugin_root / "__init__.py", INIT_TEMPLATE, args.check)

    if args.check and changed:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
