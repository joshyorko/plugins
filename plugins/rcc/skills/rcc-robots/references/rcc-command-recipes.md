# RCC Command Recipes

Use this guide for RCC command selection, robot configuration, environment prebuilds, and cache hygiene.

For RCC source, endpoint, holotree internals, or cache behavior not tied to a specific robot, switch to `$rcc-core`.

For cross-source Python library evidence, current example gaps, and package/source refresh commands, see `../../rcc/references/python-library-audit.md`.

## Runtime Worker Environment Variables

Public on-demand runtime examples set worker/linking variables such as:

```text
RC_WORKER_NAME
RC_WORKER_LINK_TOKEN
RC_AGENT_TERMINATE_AFTER_RUN_MS
```

They may also set `ROBOCORP_HOME` to place RCC state inside the worker. Keep that distinction clear: `ROBOCORP_HOME` is still RCC's home/cache boundary, while the `RC_*` values belong to worker lifecycle/linking context. Do not confuse worker/linking variables with RCC dependency resolution inputs such as `robot.yaml`, `conda.yaml`, and holotree cache state.

## Create Or Locate A Robot

```bash
rcc robot init --json
rcc robot init -t python -d my-robot
rcc create
rcc pull github.com/joshyorko/template-python-browser
```

Before editing task code, inspect:
- `robot.yaml`: tasks, `devTasks`, `environmentConfigs`, `artifactsDir`, `PATH`, `PYTHONPATH`.
- `conda.yaml`: channels, Python, `uv`, pip dependencies, `rccPostInstall`.
- `devdata/*.json`: env files passed with `rcc run -e`.
- freeze files: `environment_linux_amd64_freeze.yaml`, `environment_windows_amd64_freeze.yaml`, `environment_darwin_amd64_freeze.yaml`.

## robot.yaml Pattern

```yaml
tasks:
  Main:
    shell: python -m robocorp.tasks run tasks.py -t main

devTasks:
  Test:
    shell: pytest tests -v

environmentConfigs:
  - environment_linux_amd64_freeze.yaml
  - environment_windows_amd64_freeze.yaml
  - environment_darwin_amd64_freeze.yaml
  - conda.yaml

artifactsDir: output
PATH:
  - .
PYTHONPATH:
  - .
  - src
ignoreFiles:
  - .gitignore
```

Use `environmentConfigs` when production repeatability matters. Use single `condaConfigFile: conda.yaml` only for simple local projects.

## conda.yaml Pattern

```yaml
channels:
  - conda-forge

dependencies:
  - python=3.12.11
  - uv=0.11.8
  - pip:
      - robocorp==3.1.1
      - requests==2.32.5

# robocorp.browser projects often need:
# rccPostInstall:
#   - python -m robocorp.browser install chromium --isolated
#
# Robot Framework Browser projects often need:
# rccPostInstall:
#   - rfbrowser init
```

RCC templates in this skill use `uv` for faster pip dependency installation. Package metadata was refreshed from PyPI during the 2026-05-23 skill refresh; recheck exact pins before bumping.

For uv-native mode, declare exact Python and uv versions. RCC v18.18+ strips inherited `UV_*` configuration, forces `UV_NO_CONFIG=1`, disables implicit Python downloads after the chosen interpreter is staged, rejects ambiguous or escaping Python symlinks, and requires the pinned uv dependency inventory and strict `pip check` to succeed before recording Holotree layers. Diagnose these as RCC environment failures before changing application imports.

## Prebuild And Inspect Holotree

```bash
rcc ht vars -r robot.yaml
rcc ht vars -r robot.yaml --json
rcc ht vars -r robot.yaml --space dev
rcc diagnostics --robot robot.yaml --json
rcc robot diagnostics -r robot.yaml --json
rcc task shell -r robot.yaml
rcc task script -r robot.yaml --silent -- python -m pip list
```

`rcc ht vars` is the first diagnostic when a robot fails before Python starts. It proves whether RCC can resolve the environment and exposes `ROBOT_ROOT`, `ROBOT_ARTIFACTS`, Python path, and other runtime variables.

## Run Tasks

```bash
rcc run -r robot.yaml -t Main
rcc run -r robot.yaml -t Main --silent
rcc run -r robot.yaml --dev -t Test
rcc run -r robot.yaml -t Consumer -e devdata/env-sqlite-consumer.json
```

Use `--silent` for agent-readable output. Add `--debug`, `--trace`, or `--timeline` when diagnosing RCC behavior.

## Bundles

```bash
rcc robot bundle --robot robot.yaml --output my-robot.py
rcc robot run-from-bundle my-robot.py --task Main
```

Environment Artifact bundles can add `--artifact-archive` and `--artifact-index`; inspect the exact v18.19.3 help before creating one. Platform selection is exact and must reject incompatible workers. Hardened unpacking rejects traversal, archive/project symlinks, and unsafe destinations; `--force` stages and replaces the complete destination instead of merging into it. Bundle after the robot validates locally. Do not commit `output/`, transient bundle outputs, or generated freeze files unless the project intentionally tracks them.

For canonical `.rcca` layout, trust attachments, and archive limits, read `../../rcc-core/references/environment-artifacts.md`.

## Freeze Files And Dependency Exports

RCC writes platform freeze files during a real run. Copy the freeze file from `output/` only when the project intentionally tracks reproducible environment locks:

```bash
rcc run -r robot.yaml -t Main --silent
cp output/environment_linux_amd64_freeze.yaml .
rcc robot dependencies -r robot.yaml --space user --export
```

`rcc robot dependencies --export` writes dependency export data such as `dependencies.yaml`; it does not create the platform freeze file. Keep freeze-first fallback order in `environmentConfigs`:

```yaml
environmentConfigs:
  - environment_linux_amd64_freeze.yaml
  - environment_windows_amd64_freeze.yaml
  - environment_darwin_amd64_freeze.yaml
  - conda.yaml
```

## Cache And Home Directories

Set `ROBOCORP_HOME` in CI or experiments to keep RCC state isolated:

```bash
export ROBOCORP_HOME="$PWD/.cache/robocorp"
rcc ht vars -r robot.yaml
```

Common cache commands:

```bash
rcc holotree list
rcc holotree delete --space <space>
rcc holotree check --retries 5
rcc holotree shared --enable
rcc diagnostics --quick --json
```

Use targeted deletes by space. Avoid broad cache deletion in shared developer machines or CI caches unless the cache is known corrupt.

Environment Artifact state under `$ROBOCORP_HOME/artifacts/v1`, provider storage, coordinator roots, and legacy Holotree are separate boundaries. Set `RCC_HOLOTREE_MODE=private` when an artifact worker needs private-home lifecycle despite a machine shared-Holotree marker. Use `rcc env lifecycle inspect|verify|repair` before considering deletion.

## Environment Artifact Robot Flow

```bash
rcc env publish --robot robot.yaml --provider <reference> --json
rcc env acquire --artifact sha256:<64-hex> --provider <reference> --json
rcc env exec --artifact sha256:<64-hex> --provider <reference> \
  --json -- python -m robocorp.tasks run tasks.py -t Main
```

For long-lived/interactive commands, use `--inherit-streams --receipt-file <path>` and parse the receipt only after the child exits. A valid warm materialization can run with the provider dead and without package-network access; it must still pass compatibility and trust checks. See the RCC core artifact and provider references before using remote publication.

## Legacy RCC Remote

Josh's `rccremote-docker` repo uses `RCC_REMOTE_ORIGIN` for clients:

```bash
export RCC_REMOTE_ORIGIN=https://rccremote.example.com
rcc holotree pull -r robot.yaml
rcc holotree pull -r robot.yaml --origin https://rccremote.example.com
```

For self-hosted remote caches, validate server bootstrapping from the deployment repo first, then validate client connectivity with catalog/list/pull commands before changing robot dependencies.

This is the legacy v12 `rccremote` protocol, not a Manifest v1 named provider or `rcc cache serve`. Keep their roots, endpoints, credentials, and diagnostics separate.

## CI Cache Hygiene

- Pin `ROBOCORP_HOME` to a job cache directory.
- Cache holotree data by OS, architecture, Python version, and dependency hash.
- Run `rcc ht vars -r robot.yaml` before task execution so cache/environment failures are isolated.
- Keep `output/`, logs, and work item files as artifacts, not source changes.
- When dependency resolution changes, invalidate cache keys rather than deleting caches ad hoc.
