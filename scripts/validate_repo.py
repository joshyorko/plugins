#!/usr/bin/env python3
from __future__ import annotations

import ast
import hashlib
import ipaddress
import json
import re
import sys
from pathlib import Path
from urllib.parse import urlparse


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = ROOT / "marketplaces" / "catalog.json"
AGENT_PLUGIN_SCHEMA_ID = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
AGENT_MCP_SCHEMA_ID = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"
AGENT_PLUGIN_SCHEMA_PATH = ROOT / "schemas" / "agent-plugins" / "1.0.0" / "plugin.schema.json"
AGENT_MCP_SCHEMA_PATH = ROOT / "schemas" / "agent-plugins" / "1.0.0" / "mcp.schema.json"
AGENT_SCHEMA_DIGESTS = {
    "plugin.schema.json": "67c18151e76b99bf2bfc422092435f1fbcd0fb17438b00d4051fdfd93f6c954e",
    "mcp.schema.json": "1f21ce7c09c5e215d0d53207827feadba2785837116a018524f9990df0a2102c",
}
AGENT_SKILL_FIELDS = {
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
}
PORTABLE_METADATA_FIELDS = (
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
)
SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
HTTP_HEADER_NAME_RE = re.compile(r"^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$")
GUIDED_37SIGNALS_PLUGINS = {"37signals"}
GUIDED_37SIGNALS_MAX_WORDS = 650
GUIDED_37SIGNALS_MAX_ACTIVE_SKILLS = 10
GUIDED_37SIGNALS_ACTIVE_MANIFEST = "skills.active.yml"
GUIDED_37SIGNALS_SOURCE_INDEX = "references/source-index.yml"
GUIDED_37SIGNALS_EVALS = "evals"
GENERIC_37SIGNALS_SKILL_NAMES = {
    "37signals-active-record-tenanted",
    "37signals-api",
    "37signals-auth",
    "37signals-caching",
    "37signals-concerns",
    "37signals-crud",
    "37signals-events",
    "37signals-implement",
    "37signals-jobs",
    "37signals-kamal",
    "37signals-mailer",
    "37signals-migration",
    "37signals-model",
    "37signals-multi-tenant",
    "37signals-refactor",
    "37signals-review",
    "37signals-rework",
    "37signals-state-records",
    "37signals-stimulus",
    "37signals-test",
    "37signals-turbo",
}


def fail(message: str) -> None:
    raise SystemExit(message)


def load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def validate_json_schema(document: dict, schema_path: Path, document_path: Path) -> None:
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        fail("install validation dependencies with: python3 -m pip install -r requirements-dev.txt")

    schema = load_json(schema_path)
    canonical_schema = json.dumps(schema, sort_keys=True, separators=(",", ":")).encode()
    actual_digest = hashlib.sha256(canonical_schema).hexdigest()
    if actual_digest != AGENT_SCHEMA_DIGESTS.get(schema_path.name):
        fail(
            f"{schema_path} does not match the published Agent Plugins 1.0.0 schema"
        )
    errors = sorted(
        Draft202012Validator(schema).iter_errors(document),
        key=lambda error: tuple(str(part) for part in error.absolute_path),
    )
    if errors:
        fail(f"{document_path}: {errors[0].message}")


def validate_portable_manifest(plugin_root: Path) -> dict:
    path = plugin_root / "plugin.json"
    if not path.exists():
        fail(f"missing Agent Plugins manifest: {path}")
    if not path.resolve().is_relative_to(plugin_root.resolve()):
        fail(f"Agent Plugins manifest escapes plugin root: {path}")
    manifest = load_json(path)
    if manifest.get("$schema") != AGENT_PLUGIN_SCHEMA_ID:
        fail(f"{path} must declare the Agent Plugins 1.0.0 schema: {AGENT_PLUGIN_SCHEMA_ID}")
    validate_json_schema(manifest, AGENT_PLUGIN_SCHEMA_PATH, path)
    return manifest


def contained_plugin_path(plugin_root: Path, value: str, field: str) -> Path:
    if not value.startswith("./"):
        fail(f"{field} must start with './': {value}")
    resolved = (plugin_root / value[2:]).resolve()
    if not resolved.is_relative_to(plugin_root.resolve()):
        fail(f"{field} escapes the plugin root: {value}")
    return resolved


def validate_http_headers(headers: dict[str, str], server_name: str) -> None:
    seen_names: set[str] = set()
    for name, value in headers.items():
        if not HTTP_HEADER_NAME_RE.fullmatch(name):
            fail(f"MCP server {server_name} has invalid HTTP header name: {name}")
        normalized_name = name.lower()
        if normalized_name in seen_names:
            fail(f"MCP server {server_name} has duplicate case-insensitive HTTP header: {name}")
        seen_names.add(normalized_name)
        if any(ord(character) < 32 and character != "\t" or ord(character) == 127 for character in value):
            fail(f"MCP server {server_name} has invalid HTTP header value for {name}")


def validate_remote_server(server: dict, server_name: str) -> None:
    value = server["url"]
    parsed = urlparse(value)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        fail(f"MCP server {server_name} URL must be absolute HTTP(S): {value}")
    if parsed.username or parsed.password or parsed.fragment:
        fail(f"MCP server {server_name} URL must not contain user info or a fragment: {value}")
    host = (parsed.hostname or "").lower()
    loopback = host == "localhost"
    try:
        loopback = loopback or ipaddress.ip_address(host).is_loopback
    except ValueError:
        pass
    if not loopback and parsed.scheme != "https":
        fail(f"non-loopback MCP server {server_name} must use HTTPS: {value}")
    validate_http_headers(server.get("headers", {}), server_name)


def validate_stdio_server(plugin_root: Path, server_name: str, server: dict) -> None:
    command = server["command"]
    if command.startswith("./"):
        contained_plugin_path(plugin_root, command, f"MCP server {server_name} command")
    elif any(character.isspace() for character in command) or "/" in command or "\\" in command:
        fail(
            f"MCP server {server_name} command must be a bare executable or start with './': "
            f"{command}"
        )

    cwd = server.get("cwd")
    if cwd and cwd.startswith("./"):
        contained_plugin_path(plugin_root, cwd, f"MCP server {server_name} cwd")
    elif cwd:
        for prefix in ("${PLUGIN_ROOT}", "${PLUGIN_DATA}"):
            if cwd.startswith(prefix):
                suffix = cwd.removeprefix(prefix).removeprefix("/")
                if ".." in Path(suffix).parts:
                    fail(f"MCP server {server_name} cwd escapes {prefix}: {cwd}")
                break


def validate_portable_mcp(plugin_root: Path) -> dict | None:
    path = plugin_root / "mcp.json"
    if not path.exists():
        return None
    if not path.resolve().is_relative_to(plugin_root.resolve()):
        fail(f"Agent Plugins MCP configuration escapes plugin root: {path}")
    mcp = load_json(path)
    if mcp.get("$schema") != AGENT_MCP_SCHEMA_ID:
        fail(f"{path} must declare the Agent Plugins 1.0.0 schema: {AGENT_MCP_SCHEMA_ID}")
    validate_json_schema(mcp, AGENT_MCP_SCHEMA_PATH, path)
    for server_name, server in mcp["mcpServers"].items():
        if server["type"] == "stdio":
            validate_stdio_server(plugin_root, server_name, server)
        else:
            validate_remote_server(server, server_name)
    return mcp


def validate_mcp_surfaces(plugin_root: Path, codex_manifest: dict | None = None) -> None:
    portable = validate_portable_mcp(plugin_root)
    if portable is None:
        return
    if codex_manifest is None and (plugin_root / ".codex-plugin" / "plugin.json").exists():
        codex_manifest = load_json(plugin_root / ".codex-plugin" / "plugin.json")

    codex_path = plugin_root / ".mcp.json"
    if codex_manifest is not None:
        configured_path = codex_manifest.get("mcpServers")
        if configured_path != "./.mcp.json":
            fail(f"Codex manifest must advertise portable MCP servers via ./.mcp.json: {plugin_root}")
        codex_path = contained_plugin_path(plugin_root, configured_path, "Codex mcpServers")
    if not codex_path.exists():
        fail(f"missing Codex MCP compatibility map: {codex_path}")

    codex = load_json(codex_path)
    codex_servers = codex.get("mcpServers", codex.get("mcp_servers", codex))
    if set(codex_servers) != set(portable["mcpServers"]):
        fail(f"portable and Codex MCP server names must match for {plugin_root}")
    for server_name, portable_server in portable["mcpServers"].items():
        codex_server = codex_servers[server_name]
        if portable_server["type"] == "stdio":
            portable_signature = {
                "type": "stdio",
                "command": portable_server["command"],
                "args": portable_server.get("args", []),
                "env": portable_server.get("env", {}),
                "cwd": portable_server.get("cwd"),
            }
            codex_signature = {
                "type": codex_server.get("type", "stdio"),
                "command": codex_server.get("command"),
                "args": codex_server.get("args", []),
                "env": codex_server.get("env", {}),
                "cwd": codex_server.get("cwd"),
            }
        else:
            codex_type = codex_server.get("type")
            if codex_type == "http":
                codex_type = "streamable-http"
            portable_signature = {
                "type": portable_server["type"],
                "url": portable_server["url"],
                "headers": portable_server.get("headers", {}),
            }
            codex_signature = {
                "type": codex_type,
                "url": codex_server.get("url"),
                "headers": codex_server.get("headers", {}),
            }
        if codex_signature != portable_signature:
            fail(f"portable and Codex MCP server configuration differs for {server_name}")


def load_json_manifest(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError as error:
        fail(f"{path} must be JSON-compatible YAML: {error}")


def csv_values(value: str) -> list[str]:
    return [item.strip() for item in value.split(",") if item.strip()]


def require_string_list(value: object, field: str, path: Path) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        fail(f"{field} must be a string list in {path}")
    return value


def parse_skill_frontmatter(path: Path) -> dict[str, str]:
    text = path.read_text()
    if not text.startswith("---\n"):
        fail(f"missing YAML frontmatter start: {path}")
    parts = text.split("---\n", 2)
    if len(parts) < 3:
        fail(f"missing YAML frontmatter close: {path}")
    frontmatter = parts[1]

    data: dict[str, str] = {}
    lines = frontmatter.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        if line.startswith((" ", "\t")):
            index += 1
            continue
        match = re.match(r"^([A-Za-z0-9_-]+):(?:\s*(.*))?$", line)
        if not match:
            fail(f"invalid frontmatter line in {path}: {line}")
        key, value = match.groups()
        value = value or ""
        if value.startswith(("|", ">")):
            block_lines = []
            index += 1
            while index < len(lines) and (
                not lines[index].strip()
                or lines[index].startswith((" ", "\t"))
            ):
                block_lines.append(lines[index].strip())
                index += 1
            data[key] = "\n".join(block_lines).strip()
            continue
        if not value.strip():
            index += 1
            while index < len(lines) and (
                not lines[index].strip()
                or lines[index].startswith((" ", "\t"))
            ):
                index += 1
            data[key] = ""
            continue
        stripped = value.strip()
        if (
            ": " in stripped
            and not stripped.startswith(("'", '"', "{", "["))
        ):
            fail(f"quote frontmatter values containing ': ' in {path}: {line}")
        data[key] = stripped.strip("'\"")
        index += 1

    return data


def validate_skill_frontmatter(skill_dir: Path) -> None:
    path = skill_dir / "SKILL.md"
    data = parse_skill_frontmatter(path)
    unsupported = set(data) - AGENT_SKILL_FIELDS
    if unsupported:
        fail(
            f"unsupported Agent Skills frontmatter in {path}: "
            f"{', '.join(sorted(unsupported))}"
        )
    name = data.get("name", "")
    description = data.get("description", "")
    if not name:
        fail(f"missing skill name in {path}")
    if name != skill_dir.name:
        fail(f"skill name must match directory for {path}: {name} != {skill_dir.name}")
    if not SKILL_NAME_RE.match(name):
        fail(f"invalid skill name in {path}: {name}")
    if not description:
        fail(f"missing skill description in {path}")
    if len(description) > 1024:
        fail(f"skill description exceeds 1024 characters in {path}")

    try:
        from skills_ref import validate as validate_agent_skill
    except ImportError:
        fail("install validation dependencies with: python3 -m pip install -r requirements-dev.txt")
    problems = validate_agent_skill(skill_dir)
    if problems:
        fail(f"Agent Skills validation failed for {path}: {'; '.join(problems)}")


def validate_hermes_plugin(plugin_root: Path, plugin: dict, codex_manifest: dict) -> None:
    plugin_yaml = plugin_root / "plugin.yaml"
    init_py = plugin_root / "__init__.py"
    if not plugin_yaml.exists():
        fail(f"missing Hermes plugin manifest: {plugin_yaml}")
    if not init_py.exists():
        fail(f"missing Hermes plugin loader: {init_py}")

    hermes_manifest = load_json_manifest(plugin_yaml)

    if hermes_manifest.get("name") != plugin["name"]:
        fail(f"Hermes plugin manifest name mismatch for {plugin['name']}")
    if hermes_manifest.get("version") != codex_manifest.get("version"):
        fail(f"Hermes plugin manifest version mismatch for {plugin['name']}")
    if hermes_manifest.get("kind") != "standalone":
        fail(f"Hermes plugin kind must be standalone for {plugin['name']}")

    try:
        ast.parse(init_py.read_text(encoding="utf-8"), filename=str(init_py))
    except SyntaxError as error:
        fail(f"Hermes plugin loader does not compile: {init_py}: {error}")


def validate_37signals_skill(skill_dir: Path) -> None:
    path = skill_dir / "SKILL.md"
    data = parse_skill_frontmatter(path)
    description = data.get("description", "").strip()
    if not description.startswith("Use when"):
        fail(f"37signals skill description must start with 'Use when': {path}")

    text = path.read_text()
    word_count = len(re.findall(r"\S+", text))
    if word_count > GUIDED_37SIGNALS_MAX_WORDS:
        fail(
            f"37signals skill exceeds {GUIDED_37SIGNALS_MAX_WORDS} words "
            f"({word_count}): {path}"
        )

    references_dir = skill_dir / "references"
    if references_dir.exists():
        fail(f"37signals per-skill references are not allowed: {references_dir}")

    stale_patterns = [
        "ThibautBaissac",
        "rails_ai_agents",
        "Historical community",
        "references/full-guide",
        "rails_style_guide",
        "original Claude",
        "Claude-oriented",
    ]
    for stale_pattern in stale_patterns:
        if stale_pattern in text:
            fail(f"stale 37signals context '{stale_pattern}' found in {path}")

    if "DHH says" in text and skill_dir.name != "dhh-rails-judgment":
        fail(f"DHH-specific claim outside DHH skill: {path}")

    agents_path = skill_dir / "agents" / "openai.yaml"
    if not agents_path.exists():
        fail(f"missing 37signals OpenAI agent metadata: {agents_path}")
    agents_text = agents_path.read_text()
    if f"${skill_dir.name}" not in agents_text:
        fail(f"37signals OpenAI default prompt must reference ${skill_dir.name}: {agents_path}")


def validate_37signals_plugin(plugin_root: Path, skill_dirs: list[Path]) -> None:
    manifest_path = plugin_root / GUIDED_37SIGNALS_ACTIVE_MANIFEST
    if not manifest_path.exists():
        fail(f"missing 37signals active manifest: {manifest_path}")

    manifest = load_json_manifest(manifest_path)
    active_skills = manifest.get("skills", [])
    max_active = manifest.get("max_active")
    if not isinstance(active_skills, list) or not all(isinstance(name, str) for name in active_skills):
        fail(f"37signals active skills must be a string list: {manifest_path}")
    if not isinstance(max_active, int) or max_active < 1:
        fail(f"37signals max_active must be a positive integer: {manifest_path}")
    if max_active != GUIDED_37SIGNALS_MAX_ACTIVE_SKILLS:
        fail(
            f"37signals max_active must stay {GUIDED_37SIGNALS_MAX_ACTIVE_SKILLS}: "
            f"{manifest_path}"
        )
    if len(active_skills) > GUIDED_37SIGNALS_MAX_ACTIVE_SKILLS:
        fail(f"37signals active skills exceed max_active {max_active}: {manifest_path}")
    if len(active_skills) != len(set(active_skills)):
        fail(f"duplicate 37signals active skill in {manifest_path}")
    for name in active_skills:
        if not SKILL_NAME_RE.match(name):
            fail(f"invalid 37signals active skill name in {manifest_path}: {name}")

    actual_skills = [skill_dir.name for skill_dir in skill_dirs]
    if set(actual_skills) != set(active_skills):
        fail(f"37signals active skill directories must match skills.active.yml: {actual_skills} != {active_skills}")
    for stale_name in GENERIC_37SIGNALS_SKILL_NAMES:
        if stale_name in actual_skills:
            fail(f"recipe-like 37signals skill must not be active: {stale_name}")

    source_index_path = plugin_root / GUIDED_37SIGNALS_SOURCE_INDEX
    if not source_index_path.exists():
        fail(f"missing 37signals source index: {source_index_path}")
    source_index = load_json_manifest(source_index_path)
    sources = source_index.get("sources", {})
    if not isinstance(sources, dict) or not sources:
        fail(f"37signals source index must define sources: {source_index_path}")
    for source_id, source in sources.items():
        if not isinstance(source, dict):
            fail(f"37signals source entry must be an object: {source_id}")
        for field in ("url", "scope", "allowed_claims", "caveat"):
            if field not in source:
                fail(f"37signals source '{source_id}' missing {field}: {source_index_path}")

    expected_recipes = set(manifest.get("recipes", []))
    recipes_root = plugin_root / "references" / "recipes"
    if not recipes_root.exists():
        fail(f"missing 37signals recipe directory: {recipes_root}")
    actual_recipes = {path.stem for path in recipes_root.glob("*.md")}
    if actual_recipes != expected_recipes:
        fail(f"37signals recipes do not match manifest: {actual_recipes} != {expected_recipes}")

    active_set = set(active_skills)
    source_ids = set(sources)
    for recipe_path in sorted(recipes_root.glob("*.md")):
        data = parse_skill_frontmatter(recipe_path)
        if data.get("type") != "recipe":
            fail(f"37signals recipe must declare type: recipe: {recipe_path}")
        owned_by = csv_values(data.get("owned_by", ""))
        recipe_sources = csv_values(data.get("source_ids", ""))
        claim_scope = data.get("claim_scope", "")
        if not owned_by:
            fail(f"37signals recipe missing owned_by: {recipe_path}")
        if not set(owned_by).issubset(active_set):
            fail(f"37signals recipe owner is not active in {recipe_path}: {owned_by}")
        if not recipe_sources:
            fail(f"37signals recipe missing source_ids: {recipe_path}")
        if not set(recipe_sources).issubset(source_ids):
            fail(f"37signals recipe source_ids are unknown in {recipe_path}: {recipe_sources}")
        if not claim_scope:
            fail(f"37signals recipe missing claim_scope: {recipe_path}")

        text = recipe_path.read_text()
        if "DHH says" in text and claim_scope != "dhh-rails":
            fail(f"DHH-specific wording outside dhh-rails scope: {recipe_path}")

    evals_root = plugin_root / GUIDED_37SIGNALS_EVALS
    if not evals_root.exists():
        fail(f"missing 37signals eval directory: {evals_root}")
    eval_files = sorted(evals_root.glob("**/*.yml"))
    if not eval_files:
        fail(f"37signals eval directory must contain .yml cases: {evals_root}")

    seen_case_ids: set[str] = set()
    covered_skills: set[str] = set()
    for eval_path in eval_files:
        eval_data = load_json_manifest(eval_path)
        cases = eval_data.get("cases")
        if not isinstance(cases, list) or not cases:
            fail(f"37signals eval file must contain non-empty cases list: {eval_path}")
        for case in cases:
            if not isinstance(case, dict):
                fail(f"37signals eval case must be an object: {eval_path}")
            case_id = case.get("id")
            prompt = case.get("prompt")
            if not isinstance(case_id, str) or not case_id:
                fail(f"37signals eval case missing id: {eval_path}")
            if case_id in seen_case_ids:
                fail(f"duplicate 37signals eval case id: {case_id}")
            seen_case_ids.add(case_id)
            if not isinstance(prompt, str) or not prompt:
                fail(f"37signals eval case missing prompt: {case_id}")

            expect_skill = case.get("expect_skill")
            if expect_skill is not None and expect_skill not in active_set:
                fail(f"37signals eval expect_skill is not active in {case_id}: {expect_skill}")
            if isinstance(expect_skill, str):
                covered_skills.add(expect_skill)
            reject_skills = case.get("reject_skills", [])
            for rejected in require_string_list(reject_skills, "reject_skills", eval_path):
                if rejected not in active_set:
                    fail(f"37signals eval reject_skills is not active in {case_id}: {rejected}")
            must_load_recipes = case.get("must_load_recipes", [])
            for recipe in require_string_list(must_load_recipes, "must_load_recipes", eval_path):
                if recipe not in expected_recipes:
                    fail(f"37signals eval must_load_recipes is unknown in {case_id}: {recipe}")
            may_reference_sources = case.get("may_reference_sources", [])
            for source_id in require_string_list(may_reference_sources, "may_reference_sources", eval_path):
                if source_id not in source_ids:
                    fail(f"37signals eval may_reference_sources is unknown in {case_id}: {source_id}")
            for text_list_field in ("must_mention", "must_not_say"):
                require_string_list(case.get(text_list_field, []), text_list_field, eval_path)
            reject_all = case.get("reject_all_37signals_static_skills")
            if reject_all is not None and not isinstance(reject_all, bool):
                fail(f"reject_all_37signals_static_skills must be a boolean in {case_id}")

    missing_eval_coverage = active_set - covered_skills
    if missing_eval_coverage:
        fail(f"37signals evals must cover every active skill: {sorted(missing_eval_coverage)}")


def main() -> int:
    catalog = load_json(CATALOG_PATH)
    plugins = catalog["plugins"]
    seen_skills: dict[str, str] = {}
    expected_standalone_entries: dict[str, Path] = {}
    expected_agent_entries: dict[str, Path] = {}

    for plugin in plugins:
        plugin_root = ROOT / "plugins" / plugin["name"]
        portable_manifest = validate_portable_manifest(plugin_root)
        codex_manifest_path = plugin_root / ".codex-plugin" / "plugin.json"
        claude_manifest_path = plugin_root / ".claude-plugin" / "plugin.json"
        if not codex_manifest_path.exists():
            fail(f"missing codex manifest: {codex_manifest_path}")
        if not claude_manifest_path.exists():
            fail(f"missing claude manifest: {claude_manifest_path}")

        codex_manifest = load_json(codex_manifest_path)
        if codex_manifest["name"] != plugin["name"]:
            fail(f"plugin manifest name mismatch for {plugin['name']}")
        if portable_manifest["name"] != plugin["name"]:
            fail(f"portable plugin manifest name mismatch for {plugin['name']}")
        for field in PORTABLE_METADATA_FIELDS:
            if codex_manifest.get(field) != portable_manifest.get(field):
                fail(f"Codex and portable plugin metadata differ for {plugin['name']}: {field}")
        if codex_manifest.get("skills") != "./skills/":
            fail(f"Codex skills path must preserve portable fixed discovery for {plugin['name']}")
        validate_mcp_surfaces(plugin_root, codex_manifest)
        validate_hermes_plugin(plugin_root, plugin, codex_manifest)

        skills_root = plugin_root / "skills"
        if not skills_root.exists():
            fail(f"missing skills path for {plugin['name']}: {skills_root}")

        skill_dirs = sorted(p for p in skills_root.iterdir() if p.is_dir())
        if plugin["name"] in GUIDED_37SIGNALS_PLUGINS:
            validate_37signals_plugin(plugin_root, skill_dirs)

        for skill_dir in skill_dirs:
            if not skill_dir.resolve().is_relative_to(plugin_root.resolve()):
                fail(f"skill directory escapes plugin root: {skill_dir}")
            if skill_dir.name in seen_skills:
                fail(f"duplicate skill name {skill_dir.name} in {plugin['name']} and {seen_skills[skill_dir.name]}")
            seen_skills[skill_dir.name] = plugin["name"]
            expected_agent_entries[skill_dir.name] = skill_dir
            expected_standalone_entries[skill_dir.name] = skill_dir
            if not (skill_dir / "SKILL.md").exists():
                fail(f"missing SKILL.md in {skill_dir}")
            if not (skill_dir / "SKILL.md").resolve().is_relative_to(plugin_root.resolve()):
                fail(f"SKILL.md escapes plugin root: {skill_dir / 'SKILL.md'}")
            validate_skill_frontmatter(skill_dir)
            if plugin["name"] in GUIDED_37SIGNALS_PLUGINS:
                validate_37signals_skill(skill_dir)

    for name, target in expected_agent_entries.items():
        link_path = ROOT / ".agents" / "skills" / name
        if not link_path.is_symlink():
            fail(f"missing skill symlink: {link_path}")
        if link_path.resolve() != target.resolve():
            fail(f"incorrect skill symlink: {link_path}")
    actual_agent_entries = {path.name for path in (ROOT / ".agents" / "skills").iterdir()}
    if actual_agent_entries != set(expected_agent_entries):
        fail("unexpected entries found in .agents/skills")

    for name, target in expected_standalone_entries.items():
        link_path = ROOT / "skills" / name
        if not link_path.is_symlink():
            fail(f"missing standalone skill symlink: {link_path}")
        if link_path.resolve() != target.resolve():
            fail(f"incorrect standalone skill symlink: {link_path}")
    actual_standalone_entries = {path.name for path in (ROOT / "skills").iterdir()}
    if actual_standalone_entries != set(expected_standalone_entries):
        fail("unexpected entries found in top-level skills view")

    if (ROOT / "codex").exists() or (ROOT / "codex").is_symlink():
        fail("legacy codex compatibility view should not exist")

    codex_marketplace = load_json(ROOT / ".agents" / "plugins" / "marketplace.json")
    claude_marketplace = load_json(ROOT / ".claude-plugin" / "marketplace.json")
    ordered_names = [plugin["name"] for plugin in plugins]
    if [entry["name"] for entry in codex_marketplace["plugins"]] != ordered_names:
        fail("Codex marketplace plugin order does not match catalog")
    if [entry["name"] for entry in claude_marketplace["plugins"]] != ordered_names:
        fail("Claude marketplace plugin order does not match catalog")

    print("repo structure validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
