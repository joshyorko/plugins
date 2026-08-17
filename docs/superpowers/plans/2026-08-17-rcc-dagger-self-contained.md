# Self-contained RCC Dagger MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the marketplace-installed RCC plugin expose its RCC Dagger methods without external path configuration.

**Architecture:** Vendor the small, versioned RCC Dagger module inside the plugin and make the launcher select it by default. Preserve explicit checkout overrides and test argument selection without Docker.

**Tech Stack:** Bash, Python pytest, Dagger Go SDK module, Agent Plugins 1.0 manifests.

## Global Constraints

- Preserve `RCC_DAGGER_REPO` and `RCC_DAGGER_MODULE` override compatibility.
- Do not clone or download source during MCP startup.
- Do not fall back to generic privileged no-module mode.
- Record the exact vendored RCC source commit.

---

### Task 1: Launcher contract

**Files:**
- Create: `tests/test_rcc_dagger_mcp.py`
- Modify: `plugins/rcc/skills/rcc/scripts/rcc-dagger-mcp`
- Create: `plugins/rcc/dagger/dagger.json`
- Create: `plugins/rcc/dagger/.dagger/*`

**Interfaces:**
- Consumes: `RCC_DAGGER_REPO`, `RCC_DAGGER_MODULE`, and plugin-relative paths.
- Produces: `dagger --silent mcp --stdio --mod <resolved-module>` through the existing filter.

- [ ] Write tests for bundled default selection, override precedence, and invalid override rejection.
- [ ] Run `pytest -q tests/test_rcc_dagger_mcp.py` and confirm the bundled-default test fails.
- [ ] Vendor the pinned module and implement plugin-relative default selection.
- [ ] Run `pytest -q tests/test_rcc_dagger_mcp.py` and confirm all launcher tests pass.

### Task 2: Distribution and documentation

**Files:**
- Modify: `plugins/rcc/skills/rcc/references/dagger-mcp.md`
- Modify: `plugins/rcc/skills/rcc/SKILL.md`
- Modify: RCC plugin version metadata and generated views selected by repository scripts.

**Interfaces:**
- Consumes: the launcher contract from Task 1.
- Produces: accurate install/runtime guidance and a cache-busting plugin release.

- [ ] Replace checkout-dependent default guidance with the bundled-module contract and recovery checks.
- [ ] Bump RCC plugin metadata to `0.1.2` and rebuild generated views.
- [ ] Run `bin/check` and `git diff --check`.
- [ ] Commit only the plugin-repository changes and push `main` to `origin`.
