from __future__ import annotations

import ast
import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import build_hermes_plugins


class FakeHermesContext:
    def __init__(self) -> None:
        self.skills: list[tuple[str, Path, str]] = []

    def register_skill(self, name: str, path: Path, description: str = "") -> None:
        self.skills.append((name, path, description))


class HermesPluginBuilderTest(unittest.TestCase):
    def make_plugin(self, root: Path, name: str = "demo") -> Path:
        plugin_root = root / "plugins" / name
        manifest_dir = plugin_root / ".codex-plugin"
        skill_dir = plugin_root / "skills" / f"{name}-skill"
        manifest_dir.mkdir(parents=True)
        skill_dir.mkdir(parents=True)
        (manifest_dir / "plugin.json").write_text(
            json.dumps(
                {
                    "name": name,
                    "version": "0.2.0",
                    "description": f"{name} Codex description.",
                    "skills": "./skills/",
                }
            ),
            encoding="utf-8",
        )
        (plugin_root / "plugin.json").write_text(
            json.dumps(
                {
                    "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                    "name": name,
                    "version": "0.2.0",
                    "description": f"{name} portable description.",
                }
            ),
            encoding="utf-8",
        )
        (skill_dir / "SKILL.md").write_text(
            "---\n"
            f"name: {name}-skill\n"
            f"description: Use when testing {name} skill registration.\n"
            "---\n"
            "# Demo\n",
            encoding="utf-8",
        )
        return plugin_root

    def test_hermes_plugin_yaml_uses_portable_manifest_metadata(self) -> None:
        root = Path(tempfile.mkdtemp())
        plugin_root = self.make_plugin(root, "fizzy")
        catalog_plugin = {"name": "fizzy", "description": "Catalog description"}
        manifest = build_hermes_plugins.plugin_manifest(plugin_root)

        yaml_text = build_hermes_plugins.hermes_plugin_yaml(catalog_plugin, manifest)
        data = json.loads(yaml_text)

        self.assertEqual("fizzy", data["name"])
        self.assertEqual("0.2.0", data["version"])
        self.assertEqual("fizzy portable description.", data["description"])
        self.assertEqual("standalone", data["kind"])

    def test_generated_init_compiles_and_keeps_frontmatter_separator_escaped(self) -> None:
        tree = ast.parse(build_hermes_plugins.INIT_TEMPLATE)
        self.assertIsNotNone(tree)
        self.assertIn('text.find("\\\\n---", 3)', repr(build_hermes_plugins.INIT_TEMPLATE))
        self.assertNotIn('text.find("\n---", 3)', build_hermes_plugins.INIT_TEMPLATE)

    def test_generated_init_registers_skill_descriptions_with_fake_hermes_context(self) -> None:
        root = Path(tempfile.mkdtemp())
        plugin_root = self.make_plugin(root, "rcc")
        init_path = plugin_root / "__init__.py"
        init_path.write_text(build_hermes_plugins.INIT_TEMPLATE, encoding="utf-8")

        spec = importlib.util.spec_from_file_location("rcc_hermes_plugin", init_path)
        if spec is None or spec.loader is None:
            self.fail("generated Hermes plugin __init__.py should be importable")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        ctx = FakeHermesContext()
        module.register(ctx)

        self.assertEqual(1, len(ctx.skills))
        name, path, description = ctx.skills[0]
        self.assertEqual("rcc-skill", name)
        self.assertEqual(plugin_root / "skills" / "rcc-skill" / "SKILL.md", path)
        self.assertEqual("Use when testing rcc skill registration.", description)

    def test_check_mode_reports_stale_hermes_files_without_writing(self) -> None:
        root = Path(tempfile.mkdtemp())
        self.make_plugin(root, "demo")
        (root / "marketplaces").mkdir()
        (root / "marketplaces" / "catalog.json").write_text(
            json.dumps({"plugins": [{"name": "demo", "description": "Demo"}]}),
            encoding="utf-8",
        )

        with mock.patch.object(build_hermes_plugins, "ROOT", root), mock.patch.object(build_hermes_plugins, "CATALOG_PATH", root / "marketplaces" / "catalog.json"):
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(1, build_hermes_plugins.main(["--check"]))
            self.assertIn("stale: plugins/demo/plugin.yaml", stdout.getvalue())

        self.assertFalse((root / "plugins" / "demo" / "plugin.yaml").exists())
        self.assertFalse((root / "plugins" / "demo" / "__init__.py").exists())


if __name__ == "__main__":
    unittest.main()
