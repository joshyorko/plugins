# Agent Plugins 1.0 Compliance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every packaged plugin conform to Agent Plugins 1.0.0 and visibly expose bundled MCP servers in Codex without redesigning the marketplace.

**Architecture:** Add the portable root `plugin.json` and `mcp.json` surfaces required by Agent Plugins while retaining client-owned manifests as isolated compatibility layers. Extend the existing validator so the official JSON Schemas, Agent Skills reference validator, semantic path rules, generated views, and Codex MCP advertising are checked together.

**Tech Stack:** JSON, Python 3.11, `jsonschema`, `skills-ref`, `unittest`, GitHub Actions.

## Global Constraints

- Target Agent Plugins Specification 1.0.0 using the canonical `https://agent-plugins.org/schemas/1.0.0/*.schema.json` identifiers.
- Keep portable metadata in root `plugin.json`; keep Codex, Claude, and Hermes configuration isolated in their existing client-owned files.
- Discover skills only at `skills/<skill>/SKILL.md` and portable MCP servers only at root `mcp.json`.
- Preserve existing plugin behavior and generated-view ownership.

---

### Task 1: Portable plugin manifests

**Files:**
- Create: `plugins/37signals/plugin.json`
- Create: `plugins/fizzy/plugin.json`
- Create: `plugins/rcc/plugin.json`
- Test: `tests/test_validate_repo.py`

**Interfaces:**
- Consumes: canonical metadata currently stored in `.codex-plugin/plugin.json`.
- Produces: closed Agent Plugins 1.0.0 root manifests with matching names and versions.

- [ ] Add failing validator tests for missing `$schema`, unknown portable fields, and vendor/root metadata drift.
- [ ] Run `python3 -m unittest tests.test_validate_repo` and confirm the new tests fail for the missing root manifests.
- [ ] Add the three minimal full-metadata root manifests.
- [ ] Run the focused tests and confirm they pass.

### Task 2: Portable and Codex MCP discovery

**Files:**
- Create: `plugins/fizzy/mcp.json`
- Create: `plugins/rcc/mcp.json`
- Create: `plugins/rcc/.mcp.json`
- Modify: `plugins/fizzy/.mcp.json`
- Modify: `plugins/fizzy/.codex-plugin/plugin.json`
- Modify: `plugins/rcc/.codex-plugin/plugin.json`
- Modify: `marketplaces/catalog.json`
- Test: `tests/test_validate_repo.py`
- Test: `tests/test_build_marketplaces.py`

**Interfaces:**
- Consumes: Agent Plugins typed MCP configuration and Codex's `mcpServers: "./.mcp.json"` compatibility pointer.
- Produces: portable `streamable-http` Fizzy and `stdio` RCC Dagger entries plus explicit marketplace advertising and user-toggle discovery.

- [ ] Add failing tests that require matching portable/Codex MCP server names and an RCC `rcc-dagger` entry.
- [ ] Confirm the focused tests fail because RCC is undiscoverable and Fizzy uses the stale dotfile shape.
- [ ] Add the standard MCP documents and the smallest Codex compatibility maps.
- [ ] Update RCC/Fizzy Codex manifests and marketplace descriptions to advertise their MCP servers.
- [ ] Rebuild generated marketplace views and confirm focused tests pass.

### Task 3: Spec-backed validation and CI drift prevention

**Files:**
- Create: `schemas/agent-plugins/1.0.0/plugin.schema.json`
- Create: `schemas/agent-plugins/1.0.0/mcp.schema.json`
- Create: `requirements-dev.txt`
- Modify: `scripts/validate_repo.py`
- Modify: `bin/check`
- Modify: `.github/workflows/bootstrap-smoke.yml`
- Modify: `.github/workflows/release-artifacts.yml`
- Test: `tests/test_validate_repo.py`

**Interfaces:**
- Consumes: official Draft 2020-12 schemas and `skills-ref` 0.1.1.
- Produces: deterministic local/CI validation of portable manifests, MCP semantics, Agent Skills, and vendor isolation.

- [ ] Add failing tests for schema, transport, command path, version-match, and skill-validator failures.
- [ ] Confirm those tests fail against the current validator.
- [ ] Vendor the published schemas verbatim and add pinned development validators.
- [ ] Implement schema plus semantic/layout validation and call it from `bin/check`.
- [ ] Install the pinned validators in Linux, Windows, and release CI before running checks.
- [ ] Run focused tests and the full check suite.

### Task 4: Documentation and final conformance proof

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: final portable/client-specific layout and runtime behavior.
- Produces: accurate authoring, Dagger availability, toggle, and validation guidance.

- [ ] Update the structure and RCC sections without changing product scope.
- [ ] Run the official `skills-ref validate` command across every canonical skill.
- [ ] Run Draft 2020-12 validation against every root `plugin.json` and `mcp.json`.
- [ ] Run `bin/check`, shell syntax checks, generated-view checks, and inspect the final diff/status.
