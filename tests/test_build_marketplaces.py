from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import build_marketplaces


class MarketplaceBuilderTest(unittest.TestCase):
    def test_codex_marketplace_reads_current_plugin_manifest_interface(self) -> None:
        root = Path(tempfile.mkdtemp())
        manifest_dir = root / "plugins" / "37signals" / ".codex-plugin"
        manifest_dir.mkdir(parents=True)
        manifest = {
            "name": "37signals",
            "version": "0.1.0",
            "description": "Compact 37signals-inspired skills.",
            "interface": {
                "displayName": "37signals",
                "defaultPrompt": ["Use $37signals-rails-implement"],
            },
        }
        (manifest_dir / "plugin.json").write_text(
            json.dumps(manifest),
            encoding="utf-8",
        )
        catalog = {
            "name": "plugins",
            "interface": {"displayName": "Agent Skills"},
            "plugins": [
                {
                    "name": "37signals",
                    "description": "Catalog description",
                    "category": "Developer Tools",
                    "tags": ["rails"],
                }
            ],
        }

        with mock.patch.object(build_marketplaces, "ROOT", root):
            marketplace = build_marketplaces.build_codex_marketplace(catalog)

        plugin = marketplace["plugins"][0]
        self.assertEqual("Catalog description", plugin["description"])
        self.assertEqual(
            ["Use $37signals-rails-implement"],
            plugin["interface"]["defaultPrompt"],
        )

    def test_claude_plugin_manifest_uses_updated_codex_description(self) -> None:
        manifest = {
            "version": "0.1.0",
            "description": "Compact 37signals-inspired skills.",
            "author": {"name": "Josh Yorko"},
            "keywords": ["37signals", "communication"],
        }

        claude = build_marketplaces.build_claude_plugin_manifest(manifest, "37signals")

        self.assertEqual("37signals", claude["name"])
        self.assertEqual("Compact 37signals-inspired skills.", claude["description"])
        self.assertEqual(["37signals", "communication"], claude["keywords"])


if __name__ == "__main__":
    unittest.main()
