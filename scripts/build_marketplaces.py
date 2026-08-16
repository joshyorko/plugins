#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = ROOT / "marketplaces" / "catalog.json"


def load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def dump_json(data: dict) -> str:
    return json.dumps(data, indent=2, sort_keys=False) + "\n"


def write_if_changed(path: Path, content: str, check: bool) -> bool:
    current = path.read_text() if path.exists() else None
    if current == content:
        return False
    if check:
        print(f"stale: {path.relative_to(ROOT)}")
        return True
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    print(f"wrote {path.relative_to(ROOT)}")
    return True


def build_codex_marketplace(catalog: dict) -> dict:
    plugins = []
    for plugin in catalog["plugins"]:
        plugin_root = ROOT / "plugins" / plugin["name"]
        codex_manifest = load_json(plugin_root / ".codex-plugin" / "plugin.json")
        interface = codex_manifest.get("interface", {})
        plugins.append(
            {
                "name": plugin["name"],
                "description": plugin["description"],
                "tags": plugin.get("tags", []),
                "interface": interface,
                "source": {
                    "source": "local",
                    "path": f"./plugins/{plugin['name']}",
                },
                "policy": {
                    "installation": plugin.get("installation", "AVAILABLE"),
                    "authentication": plugin.get("authentication", "ON_INSTALL"),
                },
                "category": plugin["category"],
            }
        )
    return {
        "name": catalog["name"],
        "interface": catalog["interface"],
        "plugins": plugins,
    }


def build_claude_marketplace(catalog: dict) -> dict:
    plugins = []
    for plugin in catalog["plugins"]:
        plugins.append(
            {
                "name": plugin.get("claude_name", plugin["name"]),
                "source": f"./plugins/{plugin['name']}",
                "description": plugin["description"],
                "category": plugin.get("claude_category", "development"),
                "tags": plugin.get("tags", []),
            }
        )
    return {
        "name": catalog["name"],
        "owner": catalog["owner"],
        "metadata": catalog["metadata"],
        "plugins": plugins,
    }


def build_claude_plugin_manifest(codex_manifest: dict, claude_name: str) -> dict:
    manifest = {
        "name": claude_name,
        "version": codex_manifest["version"],
        "description": codex_manifest["description"],
        "author": codex_manifest.get("author", {}),
    }
    for field in ("homepage", "repository", "license", "keywords"):
        if field in codex_manifest:
            manifest[field] = codex_manifest[field]
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description="Build Codex and Claude marketplace metadata.")
    parser.add_argument("--check", action="store_true", help="Fail if generated files are out of date.")
    args = parser.parse_args()

    catalog = load_json(CATALOG_PATH)
    changed = False

    codex_marketplace = build_codex_marketplace(catalog)
    changed |= write_if_changed(
        ROOT / ".agents" / "plugins" / "marketplace.json",
        dump_json(codex_marketplace),
        args.check,
    )

    claude_marketplace = build_claude_marketplace(catalog)
    changed |= write_if_changed(
        ROOT / ".claude-plugin" / "marketplace.json",
        dump_json(claude_marketplace),
        args.check,
    )

    for plugin in catalog["plugins"]:
        plugin_root = ROOT / "plugins" / plugin["name"]
        portable_manifest = load_json(plugin_root / "plugin.json")
        claude_manifest = build_claude_plugin_manifest(
            portable_manifest,
            plugin.get("claude_name", plugin["name"]),
        )
        changed |= write_if_changed(
            plugin_root / ".claude-plugin" / "plugin.json",
            dump_json(claude_manifest),
            args.check,
        )

    if args.check and changed:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
