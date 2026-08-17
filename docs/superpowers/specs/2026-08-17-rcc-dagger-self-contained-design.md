# Self-contained RCC Dagger MCP Design

## Goal

An installed RCC plugin must expose the RCC Dagger module from any Codex working directory without requiring `RCC_DAGGER_REPO` or a separate RCC checkout.

## Design

Vendor the module boundary from `joshyorko/rcc` commit `0e3a5e4d97291b880ad97d235a2f730ae8a4e251` under `plugins/rcc/dagger/`: `dagger.json` plus `.dagger/`. The launcher resolves modules in this order: `RCC_DAGGER_REPO`/`RCC_DAGGER_MODULE`, the plugin-bundled module, then a clear startup error. It no longer silently exposes Dagger's generic privileged no-module surface.

The override remains available for RCC module development. Normal marketplace installs use the bundled, versioned module. Tests execute the launcher from an unrelated directory with a fake `dagger` binary and assert the exact `--mod` selection without starting Docker.

## Validation

Repository tests must prove default bundled-module selection, explicit override precedence, and invalid override failure. `bin/check` validates manifests, generated runtime views, skills, and tests. A manual Dagger method listing should expose `rcc`, `rcc-with-output`, and `run-robot-tests` after reinstall and a fresh Codex session.
