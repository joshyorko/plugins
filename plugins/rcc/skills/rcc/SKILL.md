---
name: rcc
description: Route RCC-family work across core, Environment Artifacts, providers, robots, work items, Action Server, CI/cache, and maintenance automation.
---

# RCC Router

Use this skill to triage RCC-family tasks. Load the specialist skill that matches the actual work, then continue there.

Treat RCC as the center of gravity for this stack. Frame the domain as RCC-managed contained automation built around `rcc`, `robot.yaml`, holotree environments, work item adapters, and local/CI execution. Keep literal dependency and API names such as `robocorp.tasks`, `robocorp.workitems`, `robocorp-browser`, and `ROBOCORP_HOME` when current code uses them, but treat Robocorp/Sema4.ai references as upstream dependency or interface history unless the task explicitly targets those upstream services or packages. Do not assume upstream Robocorp/Sema4.ai defines new feature direction or active open-source leadership for this stack.

## Route By Task

- Use `$rcc-core` for RCC itself: command selection, install/source orientation, Holotree/cache internals, Environment Artifacts, providers, trust, lifecycle/leases, build coordination/prewarm, endpoint/profile configuration, templates, bundles, remote cache/client behavior, and failures before a robot task starts.
- Use `$rcc-robots` to bootstrap an empty directory with `rcc robot initialize`, or for existing RCC-backed automation projects using `robot.yaml`: `conda.yaml`, environment configs, freeze files, task runtime, templates, artifacts, and task debugging. Python `@task`, Robot Framework `.robot`, and Actions Core `@action` / `package.yaml` are different project shapes; choose one from the request and existing project.
- Use `$rcc-robot-framework` for `.robot` suites and resources, Robot CLI/Rebot, custom Robot libraries, results, and RCC `robot_tests` acceptance work.
- Use `$rcc-rpaframework` for `RPA.*` library selection, keyword recipes, package/platform needs, and interoperability with `robocorp.*` libraries.
- Use `$rcc-workitems` for classic `robocorp.workitems`, `actions-work-items`, producer/consumer/reporter flows, local SQLite/file queues, custom adapters, Redis, DocumentDB, and Yorko Control Room adapters.
- Use `$action-server` for `joshyorko/actions` packages, `package.yaml` v2, PyPI `actions-core` and `actions-runtime`, `actions.mcp`, Docker/Compose builds and deployments, OpenAPI/MCP exposure, secrets, dev tasks, action tests, and typed responses.
- Use `$rcc-ci-maintenance` for RCC in GitHub Actions, `ROBOCORP_HOME` cache, pinned RCC install, scheduled maintenance automation, allowlist dependency maintenance, bot PR flows, and CI cache hygiene.

## Quick operational recipes

Load the owning specialist, then the linked recipe. These are entry points into the existing references.

| Request | Owner and recipe |
| --- | --- |
| "What's the fastest way to check RCC health?" | `$rcc-core`: [RCC health](../rcc-core/references/rcc-source-recipes.md#recipe-rcc-health) |
| "Create a new robot" | `$rcc-robots`: [New robot](../rcc-robots/references/rcc-command-recipes.md#recipe-new-robot) |
| "How do I run this robot?" | `$rcc-robots`: [Run robot](../rcc-robots/references/rcc-command-recipes.md#recipe-run-robot) |
| "Why isn't my RCC environment starting?" | `$rcc-core` first for RCC/Holotree failures: [Debug environment](../rcc-robots/references/troubleshooting-validation.md#recipe-debug-environment); `$rcc-robots` for project dependency/configuration failures |
| "Inspect this Environment Artifact" | `$rcc-core`: [Inspect Environment Artifact](../rcc-core/references/environment-artifacts.md#recipe-inspect-environment-artifact) |
| "Why did the CI cache or freeze stop working?" | `$rcc-ci-maintenance`: [CI/cache troubleshooting](../rcc-ci-maintenance/references/environment-artifact-ci.md#recipe-cicache-troubleshooting) |
| "Give me the TLDR for RCC" | `$rcc-core`: [human page and optional Tealdeer setup](../rcc-core/references/tldr/README.md) |
| "Enable or remove tldr rcc" | `$rcc-core`: [managed daily use](../rcc-core/references/tldr/README.md#managed-daily-use) |

## Router rules

- Do not keep detailed command recipes in this router. Load one specialist skill and its references.
- Route `rcc env`, `rcc provider`, and `rcc cache serve` to `$rcc-core`. If the artifact problem is specifically a robot/package configuration or task-runtime problem, pull in `$rcc-robots` after the RCC lifecycle boundary is clear.
- If a task crosses boundaries, start with the skill that owns the failing surface. Examples: a failing `rcc ht vars` before suite execution starts belongs to `$rcc-core`; a broken `robot.yaml` environment belongs to `$rcc-robots`; a wrong assertion in `robot_tests/ht_hash.robot` belongs to `$rcc-robot-framework`; choosing `RPA.Tables` versus a Python library belongs to `$rcc-rpaframework`.
- Treat artifact publication, provider-profile replacement/removal, lifecycle repair, cache-provider administration, and coordinator release as state-changing operations. Inspect first and keep credentials out of command lines, files, logs, receipts, and provenance.
- For DocDB-backed RPA systems that mix queue naming, helper scripts, retry, outbox, dashboards, or GitHub Actions matrix workers, start with `$rcc-workitems`; pull in `$rcc-robots` or `$rcc-ci-maintenance` only after the queue boundary is clear.
- Keep canonical edits under `plugins/rcc/skills/<skill>/`. Treat top-level `skills/` and `.agents/skills/` as generated views.
- Do not prototype marketplace servers, MCP servers, web services, daemons, or new runtime products from this plugin. `action-server` covers normal action-package work only.
- Use the `rcc` binary directly in the active project context; start with `$rcc-core` for binary/version/install checks. Do not silently substitute host Python when RCC environment resolution fails.
- Prefer our `actions-work-items` and user-owned adapters for portable queue workflows. Upstream Vault/Storage are compatibility APIs with hosted defaults, not shipped self-hosted services. Action Server's intended future control-plane role is not implemented by this plugin.

## Shared References

- `references/python-library-audit.md`: cross-source Python library evidence, example gaps, and refresh commands for RCC-family recipes.
- `references/source-map.md`: source evidence for RCC plugin refreshes.
- `../rcc-core/references/environment-artifacts.md`: Environment Artifact identity, compatibility, lifecycle, archives, leases, repair, and warm reuse.
- `../rcc-core/references/providers-and-trust.md`: providers, `cache serve`, enterprise transport, trust carriers/policy, receipts, and legacy `rccremote` boundaries.
- `../rcc-core/references/build-coordination.md`: shipped coordination/prewarm CLI, state model, limitations, and evidence boundaries.
- `references/agent-prompt-examples.md`: short prompts that point future agents at the right specialist skill.
- `../rcc-workitems/references/docdb-rpa-patterns.md`: production DocDB/RPA queue, helper, retry, outbox, artifact, and CI patterns from the local BPS example.
