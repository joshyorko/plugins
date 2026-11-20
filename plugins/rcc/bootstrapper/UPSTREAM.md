# Upstream Provenance (`rcc-actions-bootstrapper`)

- **Canonical Home**: `joshyorko/plugins` (`plugins/rcc/bootstrapper/`)
- **Migrated From**: [`joshyorko/actions-robot-boostrapper`](https://github.com/joshyorko/actions-robot-boostrapper)
- **Audited Upstream Commit**: `2769fb7a939af7db6111428dd27153a1a15e4c53` (`2024-03-10`)
- **Original License**: Apache License 2.0 (preserved in [`LICENSE`](file:///home/kdlocpanda/plugins/plugins/rcc/bootstrapper/LICENSE))
- **Architecture Decision**: [`../docs/adr-0001-actions-bootstrapper-mcp.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/adr-0001-actions-bootstrapper-mcp.md)
- **Legacy 36-Action Disposition**: [`../docs/legacy-action-inventory.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/legacy-action-inventory.md)
- **Version & Qualification Matrix**: [`../docs/capability-and-version-matrix.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/capability-and-version-matrix.md)

## Summary of Changes from Upstream `2769fb7a`

1. Replaced the 36 legacy actions (`src/actions_bootrapper/actions.py` and `src/robot_bootrapper/robot_actions.py`) with 8 typed MCP tools/actions (`bootstrapper_discover`, `bootstrapper_plan`, `bootstrapper_scaffold`, `bootstrapper_validate`, `bootstrapper_run`, `bootstrapper_status`, `bootstrapper_cancel`, `bootstrapper_results`), 2 MCP resources, and 1 MCP prompt.
2. Migrated `package.yaml` to `spec-version: v2` with `actions-core=1.1.0` and `actions-work-items=0.5.0`.
3. Delegated project scaffolding to the real published `action-server new --template <basic|mcp-v2-showcase|workflow-producer-consumer|canvas-query>` and `rcc robot initialize -t <template> -d <dir>` commands inside an atomic staging directory, fixing the legacy zero-byte file wipe (`open(..., "w").write("")`), fixing `actions.py` module shadowing, and normalizing `package.yaml` to released `actions-core` / `actions-work-items` pins.
4. Enforced canonical approved roots (`RCC_BOOTSTRAPPER_APPROVED_ROOTS`), non-empty directory overwrite protection, symlink/path-traversal defense, secret redaction, process-group ownership, and crash-reconciled run status (`unknown_retained`).
