---
name: rcc-core
description: "Use for RCC itself: commands, install, source tree, holotree/cache, endpoints, profiles, templates, and bundles."
---

# RCC Core

Use this skill when the task is about RCC itself rather than a specific RCC-backed automation project, work item adapter, Action Server package, or CI wrapper.

## First Inspection

1. Identify the active context: installed `rcc` binary, Josh's fork at `github.com/joshyorko/rcc`, upstream `github.com/robocorp/rcc` as historical/reference context, or an automation project merely using RCC.
2. Run read-only CLI checks first when a binary is available: `rcc version`, `rcc diagnostics --quick`, `rcc docs recipes`, and `rcc docs changelog`.
3. For source work, inspect `README.md`, `developer/toolkit.yaml`, `docs/recipes.md`, `docs/holotree.md`, `docs/troubleshooting.md`, `cmd/rcc`, `robot/`, `conda/`, `htfs/`, `remotree/`, `settings/`, and `templates/`.
4. If a failure reaches Python task code, switch to `$rcc-robots`; route `robot_tests/` authoring and execution to `$rcc-robot-framework` while keeping the RCC behavior under test and Go implementation here; if it reaches queue behavior, DocDB helpers, retry/outbox, or adapter reservation, switch to `$rcc-workitems`.

## Operating Rules

- On Josh's Bluefin host, prefer repo-native/devcontainer paths or Homebrew for host `rcc`; do not suggest host package layering unless there is a clear reason.
- Treat `ROBOCORP_HOME` as the primary RCC home/cache boundary. Older notes may mention `RCC_HOME`; verify current source/config behavior before relying on it.
- Use `rcc ht vars` and `rcc task script` to prove environment resolution before debugging task imports.
- When CI runs helper scripts through `rcc task script` or a declared `robot.yaml` task, prove the RCC environment boundary first; do not replace it with host Python unless the project explicitly owns a host-Python path.
- Delete holotree spaces surgically. Avoid broad cache cleanup on a shared workstation.
- For Josh's fork, remember telemetry is intentionally disabled and endpoint overrides are first-class.
- Treat Josh's fork as the authoritative source for current RCC behavior in this stack. Use upstream Robocorp/Sema4.ai repositories for dependency/API history or explicit compatibility checks, not for feature-direction assumptions.

## References

- `references/rcc-source-recipes.md`: RCC command map, source tree orientation, holotree/cache, endpoints, remote cache, and fork development recipes.
- `../rcc-robots/references/rcc-command-recipes.md`: robot-facing commands and environment recipes.
- `../rcc-robots/references/troubleshooting-validation.md`: failure-splitting playbook.
- `../rcc-workitems/references/docdb-rpa-patterns.md`: why some helper scripts intentionally stay behind an RCC task boundary in DocDB-backed robots.
- `../rcc/references/source-map.md`: evidence map for current RCC-owned sources plus upstream dependency/interface-history sources.
