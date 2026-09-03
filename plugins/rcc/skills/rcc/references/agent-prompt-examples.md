# Agent Prompt Examples

Use these short prompts when handing RCC-family work to future Codex agents.

## Inspect RCC Itself

```text
Use $rcc-core. Inspect RCC before debugging robot code: installed binary/version, configuration diagnostics, ROBOCORP_HOME, endpoint overrides, holotree state, and the relevant source/docs in github.com/joshyorko/rcc or github.com/robocorp/rcc.
```

```text
Use $rcc-core for RCC holotree, freeze/export, endpoint, prebuilt environment, rccremote, or public runtime webhook questions. Treat public on-demand runtime examples as public contract guidance and architecture inference, not unreleased Control Room backend fact.
```

```text
Use $rcc-core for Environment Artifacts. Confirm RCC v18.19.3, then separate the canonical Artifact digest, platform compatibility, provider, trust carrier/policy, local materialization, and execution lease. Inspect lifecycle state before repair and preserve strict remote trust.
```

```text
Use $rcc-core for provider or cache-server work. Inspect the named profile without printing credential values, keep rcc cache serve loopback-only, test the configured proxy/no-proxy/custom-CA boundary, and do not confuse Manifest v1 providers with legacy rccremote.
```

```text
Use $rcc-core for env coordinate or prewarm. Read the shipped v18.19.3 command help and build-coordination reference, use external timeouts, keep verified Artifact closure authoritative, and report released-binary CLI evidence separately from internal/source tests.
```

```text
Use $rcc when exposing the plugin's bundled RCC Dagger module to agents through MCP. Set RCC_DAGGER_REPO only when a different checkout is requested. If Docker or Dagger is unavailable, fall back to $rcc-core and use the regular rcc binary directly in the active project context.
```

## Inspect A Robot

```text
Use $rcc-robots. Inspect this robot root before editing: robot.yaml, conda.yaml, devdata env files, freeze files, and task entry points. Run rcc ht vars first if RCC is available, then separate environment failures from Python/task failures.
```

```text
Use $rcc-robots for robot.yaml, conda.yaml, robocorp.tasks, robocorp.browser, robocorp.vault, robocorp.storage, RPA.Assistant, or RPA Framework questions. Separate RCC environment resolution from task runtime/library behavior before editing.
```

## Fix A Dependency Failure

```text
Use $rcc-core first if the failure is holotree/cache/endpoint/install related; otherwise use $rcc-robots. Reproduce with rcc ht vars -r robot.yaml, inspect conda.yaml and environmentConfigs, then change the smallest dependency/config surface. Do not edit generated output or freeze files unless the project intentionally tracks them.
```

## Add A Work Item Consumer

```text
Use $rcc-workitems. Add a consumer task that uses the project's existing work item API and adapter env files. Preserve producer/consumer/reporter queue names, use item context managers for release, and validate with the SQLite/file local backend before distributed services.
```

```text
Use $rcc-workitems for producer/consumer/reporter, robocorp-adapters-custom, actions-work-items, Redis, DocumentDB, or queue adapter questions. Start at the queue boundary and adapter env files before changing task code.
```

## Create An Action Package

```text
Use $action-server. Create or update an Action Server package with package.yaml spec-version v2, typed sema4ai.actions responses, Secret/OAuth2Secret for sensitive inputs, and dev-tasks for tests/lint. Validate action-server start and OpenAPI/MCP endpoints when the dependency is available.
```

```text
Use $action-server for package.yaml v2, sema4ai-actions, sema4ai-mcp, /mcp, or Action Server work-items template questions. Validate package behavior through Action Server commands and endpoint checks when available.
```

## Review Template Pins

```text
Use $rcc-robots. Before changing package pins, fetch PyPI or the owning repo metadata in this run. If uv or shared Robocorp pins change, update all RCC templates consistently and record sources in references/source-map.md.
```

## Diagnose CI Cache Issues

```text
Use $rcc-ci-maintenance. Check ROBOCORP_HOME, holotree cache keys, OS/architecture freeze files, and rcc ht vars output. Prefer cache-key invalidation over broad cache deletion, and keep output/work item runtime files as artifacts.
```

```text
Use $rcc-ci-maintenance for Environment Artifact promotion. Pin RCC and its asset SHA, isolate producer/provider/consumer/coordinator roots, verify cold and provider-dead warm paths, retain exact commit/binary/artifact receipts, and report four-platform lifecycle separately from Linux-only prewarm.
```
