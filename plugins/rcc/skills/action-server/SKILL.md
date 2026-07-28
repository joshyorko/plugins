---
name: action-server
description: Use for Action Server packages, package.yaml, sema4ai-actions, sema4ai-mcp, secrets, tests, and MCP validation.
---

# Action Server

Use this skill for normal Action Server package authoring and debugging across Josh's `actions` community branch and Sema4AI-compatible package APIs.

## Scope

- Build or repair action packages using `package.yaml` spec-version v2.
- Work with `sema4ai-actions`, `sema4ai-mcp`, typed responses, `Secret`, `OAuth2Secret`, tests, lint dev tasks, OpenAPI, and local MCP endpoint validation.
- Use `actions-work-items` only when the action package itself needs producer/consumer work item behavior; switch to `$rcc-workitems` for adapter-heavy queue design.
- If an action package uses `actions-work-items` with Redis, DocumentDB, retry/outbox, or cross-job queues, read the RCC work-items adapter references before changing action code.

## Non-Goals

- Do not prototype an plugins marketplace server.
- Do not create a new MCP server, action server, daemon, web service, or marketplace runtime product for this repo.
- Do not make plugin distribution sound hosted; this skill is for ordinary Action Server action packages.

## First Inspection

1. Locate `package.yaml`, `actions.py`, `src/**`, `tests/**`, and `.action-server/` or datadir settings if present.
2. Inspect dependencies, `pythonpath`, `dev-dependencies`, and `dev-tasks` before changing action code.
3. Check secrets and typed inputs/outputs before writing sample payloads.
4. Validate local endpoints only when Action Server is installed and intended for the project.

## References

- `references/action-server-recipes.md`: package v2 patterns, actions, MCP decorators, secrets, dev tasks, endpoint validation, and work-item-in-actions examples.
- `../rcc/references/python-library-audit.md`: cross-source Python library map, action/MCP example gaps, and source refresh workflow.
- `../rcc-workitems/references/workitems-adapters.md`: queue/adapters details when action packages use `actions-work-items`.
- `../rcc-workitems/references/docdb-rpa-patterns.md`: DocDB queue/outbox/retry patterns to reuse only when an action package has real durable work-item semantics.
- `../rcc-robots/references/troubleshooting-validation.md`: shared failure triage and validation commands.
- `../rcc-robots/assets/templates/package.yaml`: starter package template.
- `../rcc/references/source-map.md`: source evidence for Action Server and Sema4AI recipes.
