from __future__ import annotations

import tempfile
import unittest
import json
from pathlib import Path
from unittest import mock

from scripts import validate_repo


class SkillFrontmatterTest(unittest.TestCase):
    def make_skill(self, directory_name: str, frontmatter: str) -> Path:
        root = Path(tempfile.mkdtemp())
        skill_dir = root / directory_name
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            f"---\n{frontmatter}\n---\n# Test Skill\n",
            encoding="utf-8",
        )
        return skill_dir

    def test_rejects_unquoted_colon_in_scalar_value(self) -> None:
        skill_dir = self.make_skill(
            "bad-skill",
            "name: bad-skill\n"
            "description: Use for RCC itself: command selection",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_skill_frontmatter(skill_dir)

        self.assertIn("quote frontmatter values", str(error.exception))

    def test_accepts_quoted_colon_in_scalar_value(self) -> None:
        skill_dir = self.make_skill(
            "good-skill",
            "name: good-skill\n"
            'description: "Use for RCC itself: command selection"',
        )

        validate_repo.validate_skill_frontmatter(skill_dir)

    def test_accepts_folded_description_with_colon(self) -> None:
        skill_dir = self.make_skill(
            "folded-skill",
            "name: folded-skill\n"
            "description: >-\n"
            "  Use when a long description contains: a colon",
        )

        validate_repo.validate_skill_frontmatter(skill_dir)

    def test_rejects_directory_name_mismatch(self) -> None:
        skill_dir = self.make_skill(
            "actual-name",
            "name: other-name\n"
            "description: Valid short description",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_skill_frontmatter(skill_dir)

        self.assertIn("skill name must match directory", str(error.exception))

    def test_rejects_missing_frontmatter_close(self) -> None:
        root = Path(tempfile.mkdtemp())
        skill_dir = root / "broken-skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nname: broken-skill\ndescription: Broken\n# Missing close\n",
            encoding="utf-8",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_skill_frontmatter(skill_dir)

        self.assertIn("missing YAML frontmatter close", str(error.exception))

    def test_rejects_nonstandard_agent_skill_frontmatter(self) -> None:
        skill_dir = self.make_skill(
            "vendor-skill",
            "name: vendor-skill\n"
            "description: Use when testing portable metadata\n"
            "invocable: true",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_skill_frontmatter(skill_dir)

        self.assertIn("unsupported Agent Skills frontmatter", str(error.exception))

    def test_rejects_37signals_non_trigger_description(self) -> None:
        skill_dir = self.make_skill(
            "37signals-skill",
            "name: 37signals-skill\n"
            "description: Builds Rails things",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_skill(skill_dir)

        self.assertIn("must start with 'Use when'", str(error.exception))

    def test_rejects_37signals_long_skill_body(self) -> None:
        root = Path(tempfile.mkdtemp())
        skill_dir = root / "37signals-skill"
        skill_dir.mkdir()
        long_body = " ".join(["word"] * (validate_repo.GUIDED_37SIGNALS_MAX_WORDS + 1))
        (skill_dir / "SKILL.md").write_text(
            "---\n"
            "name: 37signals-skill\n"
            "description: Use when testing length\n"
            "---\n"
            f"{long_body}\n",
            encoding="utf-8",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_skill(skill_dir)

        self.assertIn("exceeds", str(error.exception))

    def test_rejects_37signals_per_skill_references(self) -> None:
        skill_dir = self.make_skill(
            "37signals-skill",
            "name: 37signals-skill\n"
            "description: Use when testing references",
        )
        (skill_dir / "references").mkdir()

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_skill(skill_dir)

        self.assertIn("per-skill references", str(error.exception))

    def make_37signals_plugin(self) -> tuple[Path, list[Path]]:
        root = Path(tempfile.mkdtemp()) / "37signals"
        skills_root = root / "skills"
        recipes_root = root / "references" / "recipes"
        evals_root = root / "evals" / "activation"
        skills_root.mkdir(parents=True)
        recipes_root.mkdir(parents=True)
        evals_root.mkdir(parents=True)

        active = ["37signals-rails-implement", "dhh-rails-judgment"]
        (root / "skills.active.yml").write_text(
            '{\n'
            '  "max_active": 10,\n'
            '  "skills": ["37signals-rails-implement", "dhh-rails-judgment"],\n'
            '  "recipes": ["rails-models"]\n'
            '}\n',
            encoding="utf-8",
        )
        (root / "references" / "source-index.yml").write_text(
            '{\n'
            '  "sources": {\n'
            '    "rails-doctrine": {\n'
            '      "url": "https://rubyonrails.org/doctrine",\n'
            '      "scope": "dhh-rails",\n'
            '      "allowed_claims": ["omakase"],\n'
            '      "caveat": "Rails only"\n'
            '    }\n'
            '  }\n'
            '}\n',
            encoding="utf-8",
        )
        for name in active:
            skill_dir = skills_root / name
            (skill_dir / "agents").mkdir(parents=True)
            (skill_dir / "SKILL.md").write_text(
                f"---\nname: {name}\ndescription: Use when testing {name}\n---\n# {name}\n",
                encoding="utf-8",
            )
            (skill_dir / "agents" / "openai.yaml").write_text(
                f'interface:\n  default_prompt: "Use ${name}"\n',
                encoding="utf-8",
            )
        (recipes_root / "rails-models.md").write_text(
            "---\n"
            "type: recipe\n"
            "owned_by: 37signals-rails-implement\n"
            "source_ids: rails-doctrine\n"
            "claim_scope: dhh-rails\n"
            "---\n"
            "# Recipe\n",
            encoding="utf-8",
        )
        (evals_root / "cases.yml").write_text(
            '{\n'
            '  "cases": [\n'
            '    {\n'
            '      "id": "implement_model_change",\n'
            '      "prompt": "Add a Rails model change.",\n'
            '      "expect_skill": "37signals-rails-implement",\n'
            '      "reject_skills": ["dhh-rails-judgment"],\n'
            '      "must_load_recipes": ["rails-models"],\n'
            '      "may_reference_sources": ["rails-doctrine"]\n'
            '    },\n'
            '    {\n'
            '      "id": "dhh_rails_judgment",\n'
            '      "prompt": "Should this Rails app become a service?",\n'
            '      "expect_skill": "dhh-rails-judgment"\n'
            '    }\n'
            '  ]\n'
            '}\n',
            encoding="utf-8",
        )
        return root, sorted(p for p in skills_root.iterdir() if p.is_dir())

    def test_accepts_37signals_active_manifest_and_recipes(self) -> None:
        plugin_root, skill_dirs = self.make_37signals_plugin()

        validate_repo.validate_37signals_plugin(plugin_root, skill_dirs)

    def test_rejects_37signals_recipe_like_active_skill(self) -> None:
        plugin_root, skill_dirs = self.make_37signals_plugin()
        stale_dir = plugin_root / "skills" / "37signals-model"
        stale_dir.mkdir()
        skill_dirs.append(stale_dir)

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_plugin(plugin_root, skill_dirs)

        self.assertIn("active skill directories", str(error.exception))

    def test_rejects_37signals_recipe_unknown_source(self) -> None:
        plugin_root, skill_dirs = self.make_37signals_plugin()
        recipe = plugin_root / "references" / "recipes" / "rails-models.md"
        recipe.write_text(
            "---\n"
            "type: recipe\n"
            "owned_by: 37signals-rails-implement\n"
            "source_ids: missing-source\n"
            "claim_scope: rails-synthesis\n"
            "---\n"
            "# Recipe\n",
            encoding="utf-8",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_plugin(plugin_root, skill_dirs)

        self.assertIn("source_ids are unknown", str(error.exception))

    def test_rejects_37signals_changed_active_limit(self) -> None:
        plugin_root, skill_dirs = self.make_37signals_plugin()
        manifest = plugin_root / "skills.active.yml"
        manifest.write_text(
            '{\n'
            '  "max_active": 25,\n'
            '  "skills": ["37signals-rails-implement", "dhh-rails-judgment"],\n'
            '  "recipes": ["rails-models"]\n'
            '}\n',
            encoding="utf-8",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_plugin(plugin_root, skill_dirs)

        self.assertIn("max_active must stay", str(error.exception))

    def test_rejects_37signals_eval_unknown_skill(self) -> None:
        plugin_root, skill_dirs = self.make_37signals_plugin()
        eval_file = plugin_root / "evals" / "activation" / "cases.yml"
        eval_file.write_text(
            '{\n'
            '  "cases": [\n'
            '    {\n'
            '      "id": "unknown_skill",\n'
            '      "prompt": "Review a Rails change.",\n'
            '      "expect_skill": "37signals-review"\n'
            '    }\n'
            '  ]\n'
            '}\n',
            encoding="utf-8",
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_37signals_plugin(plugin_root, skill_dirs)

        self.assertIn("expect_skill is not active", str(error.exception))


class AgentPluginManifestTest(unittest.TestCase):
    def make_plugin(self, manifest: dict) -> Path:
        plugin_root = Path(tempfile.mkdtemp()) / "test-plugin"
        plugin_root.mkdir()
        (plugin_root / "plugin.json").write_text(
            json.dumps(manifest),
            encoding="utf-8",
        )
        return plugin_root

    def test_rejects_missing_portable_schema(self) -> None:
        plugin_root = self.make_plugin({"name": "test-plugin"})

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_manifest(plugin_root)

        self.assertIn("Agent Plugins 1.0.0 schema", str(error.exception))

    def test_rejects_vendor_field_in_portable_manifest(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_PLUGIN_SCHEMA_ID,
                "name": "test-plugin",
                "skills": "./skills/",
            }
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_manifest(plugin_root)

        self.assertIn("Additional properties are not allowed", str(error.exception))

    def test_rejects_drifted_vendored_official_schema(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_PLUGIN_SCHEMA_ID,
                "name": "test-plugin",
            }
        )
        schema_path = Path(tempfile.mkdtemp()) / "plugin.schema.json"
        schema = json.loads(validate_repo.AGENT_PLUGIN_SCHEMA_PATH.read_text())
        schema["title"] = "Drifted schema"
        schema_path.write_text(json.dumps(schema), encoding="utf-8")

        with mock.patch.object(validate_repo, "AGENT_PLUGIN_SCHEMA_PATH", schema_path):
            with self.assertRaises(SystemExit) as error:
                validate_repo.validate_portable_manifest(plugin_root)

        self.assertIn("does not match the published Agent Plugins 1.0.0 schema", str(error.exception))


class AgentPluginMcpTest(unittest.TestCase):
    def make_plugin(self, mcp: dict, codex_mcp: dict | None = None) -> Path:
        plugin_root = Path(tempfile.mkdtemp()) / "test-plugin"
        plugin_root.mkdir()
        (plugin_root / "mcp.json").write_text(json.dumps(mcp), encoding="utf-8")
        if codex_mcp is not None:
            (plugin_root / ".mcp.json").write_text(
                json.dumps(codex_mcp),
                encoding="utf-8",
            )
        return plugin_root

    def test_rejects_nonstandard_remote_transport(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "remote": {
                        "type": "http",
                        "url": "https://example.com/mcp",
                    }
                },
            }
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_mcp(plugin_root)

        self.assertIn("not valid under any of the given schemas", str(error.exception))

    def test_accepts_http_for_any_loopback_ip_literal(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "remote": {
                        "type": "streamable-http",
                        "url": "http://127.0.0.2/mcp",
                    }
                },
            }
        )

        validate_repo.validate_portable_mcp(plugin_root)

    def test_rejects_case_insensitive_duplicate_headers(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "remote": {
                        "type": "streamable-http",
                        "url": "https://example.com/mcp",
                        "headers": {"X-Key": "one", "x-key": "two"},
                    }
                },
            }
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_mcp(plugin_root)

        self.assertIn("duplicate case-insensitive HTTP header", str(error.exception))

    def test_rejects_malformed_header_name(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "remote": {
                        "type": "streamable-http",
                        "url": "https://example.com/mcp",
                        "headers": {"Bad Header": "value"},
                    }
                },
            }
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_mcp(plugin_root)

        self.assertIn("invalid HTTP header name", str(error.exception))

    def test_rejects_escaping_stdio_command(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "local": {
                        "type": "stdio",
                        "command": "../outside/server",
                    }
                },
            }
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_portable_mcp(plugin_root)

        self.assertIn("bare executable or start with './'", str(error.exception))

    def test_rejects_codex_server_name_drift(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "rcc-dagger": {
                        "type": "stdio",
                        "command": "./bin/rcc-dagger-mcp",
                    }
                },
            },
            {"other-server": {"command": "./bin/rcc-dagger-mcp"}},
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_mcp_surfaces(plugin_root)

        self.assertIn("server names must match", str(error.exception))

    def test_rejects_codex_server_configuration_drift(self) -> None:
        plugin_root = self.make_plugin(
            {
                "$schema": validate_repo.AGENT_MCP_SCHEMA_ID,
                "mcpServers": {
                    "rcc-dagger": {
                        "type": "stdio",
                        "command": "./bin/rcc-dagger-mcp",
                    }
                },
            },
            {"rcc-dagger": {"command": "./bin/other-server"}},
        )

        with self.assertRaises(SystemExit) as error:
            validate_repo.validate_mcp_surfaces(plugin_root)

        self.assertIn("configuration differs", str(error.exception))


if __name__ == "__main__":
    unittest.main()
