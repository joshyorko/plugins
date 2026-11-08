"""RCC installs expose skills without starting an MCP runtime."""

import json
import runpy
import unittest
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / "plugins" / "rcc"


class RccDistributionTests(unittest.TestCase):
    def test_manifests_do_not_register_or_advertise_dagger(self):
        for relative in ("plugin.json", ".codex-plugin/plugin.json",
                         ".claude-plugin/plugin.json", "plugin.yaml"):
            with self.subTest(manifest=relative):
                manifest = json.loads((PLUGIN / relative).read_text())
                self.assertNotIn("mcpServers", manifest)
                self.assertNotIn("dagger", json.dumps(manifest).lower())
        codex = json.loads((PLUGIN / ".codex-plugin/plugin.json").read_text())
        self.assertEqual(codex["interface"]["displayName"], "RCC Automation")
        for relative in ("mcp.json", ".mcp.json", "dagger",
                         "skills/rcc/scripts/rcc-dagger-mcp",
                         "skills/rcc/scripts/rcc-dagger-mcp-filter.py"):
            with self.subTest(path=relative):
                self.assertFalse((PLUGIN / relative).exists())

    def test_marketplaces_keep_the_rcc_install_path_without_dagger_tags(self):
        marketplace = json.loads((ROOT / ".agents/plugins/marketplace.json").read_text())
        rcc = next(p for p in marketplace["plugins"] if p["name"] == "rcc")
        self.assertEqual(rcc["source"]["path"], "./plugins/rcc")
        for relative in ("marketplaces/catalog.json", ".claude-plugin/marketplace.json"):
            marketplace = json.loads((ROOT / relative).read_text())
            rcc = next(p for p in marketplace["plugins"] if p["name"] == "rcc")
            self.assertNotIn("dagger", json.dumps(rcc).lower())

    def test_hermes_registers_canonical_skills_and_verifier(self):
        registered = []
        context = SimpleNamespace(register_skill=lambda **skill: registered.append(skill))
        runpy.run_path(str(PLUGIN / "__init__.py"))["register"](context)
        expected = {"rcc", "rcc-core", "rcc-robots", "rcc-workitems",
                    "rcc-robot-framework", "rcc-rpaframework", "action-server",
                    "rcc-ci-maintenance", "verify-rcc"}
        self.assertEqual({skill["name"] for skill in registered}, expected)
        for skill in registered:
            self.assertTrue(skill["path"].is_relative_to(PLUGIN / "skills"))
            self.assertTrue(skill["description"])


if __name__ == "__main__":
    unittest.main()
