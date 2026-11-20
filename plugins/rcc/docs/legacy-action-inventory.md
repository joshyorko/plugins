# Legacy 36-Action Keep / Replace / Retire Inventory

This document records the disposition of all **36 `@action` entrypoints** from upstream `joshyorko/actions-robot-boostrapper` at commit `2769fb7a939af7db6111428dd27153a1a15e4c53` (`src/actions_bootrapper/actions.py` — 10 actions; `src/robot_bootrapper/robot_actions.py` — 26 actions).

## Summary Counts

| Source Module | Legacy Actions | Replaced by Modern Typed Tool | Retired (Unsafe / Redundant / Invalid CLI) | Total |
| --- | ---: | ---: | ---: | ---: |
| `src/actions_bootrapper/actions.py` | 10 | 5 | 5 | 10 |
| `src/robot_bootrapper/robot_actions.py` | 26 | 11 | 15 | 26 |
| **Total** | **36** | **16** | **20** | **36** |

All 16 replaced capabilities are consolidated into the **8 typed MCP tools/actions** (`bootstrapper_discover`, `bootstrapper_plan`, `bootstrapper_scaffold`, `bootstrapper_validate`, `bootstrapper_run`, `bootstrapper_status`, `bootstrapper_cancel`, `bootstrapper_results`), **2 MCP resources** (`bootstrapper://catalog/templates`, `bootstrapper://runs/{run_id}`), and the external runtime lifecycle CLI ([`plugins/rcc/scripts/bootstrapper_runtime.py`](file:///home/kdlocpanda/plugins/plugins/rcc/scripts/bootstrapper_runtime.py)).

---

## Part 1: `src/actions_bootrapper/actions.py` (10 Legacy Actions)

| # | Legacy Action Name | Disposition | Modern Replacement / Boundary | Legacy Defect / Security Finding Audited at `2769fb7a` |
| ---: | --- | --- | --- | --- |
| 1 | `bootstrap_action_package` | **Replace** | `bootstrapper_plan` + `bootstrapper_scaffold(project_kind="action")` | Ran `action-server new --name ...` via `subprocess.Popen(..., shell=True)` (command injection risk), then opened `package.yaml`, `actions.py`, and `devdata/input_.json` with `"w"` and wrote `""` (wiping all three files to 0 bytes), and created `actions.py` which shadows `actions-core`'s `actions` package. |
| 2 | `update_action_package_dependencies` | **Retire** (arbitrary file overwrite) / **Replace** validation via `bootstrapper_validate` | Initial dependency pinning in `bootstrapper_scaffold`; schema & dependency verification in `bootstrapper_validate`; host IDE/agent edits files directly | Unconditionally overwrote `package.yaml` with unvalidated raw strings inside `os.path.join(action_package_dir, action_package_name, "package.yaml")` with no path boundary or YAML schema check. |
| 3 | `update_action_package_action_dev_data` | **Retire** (arbitrary file overwrite) | Seeded atomically by `bootstrapper_scaffold`; validated by `bootstrapper_validate` | Overwrote `devdata/input_<action_name>.json` with unvalidated string content and no path traversal protection on `action_name`. |
| 4 | `start_action_server` | **Retire from MCP tool surface**; **Replace** with external CLI [`bootstrapper_runtime.py`](file:///home/kdlocpanda/plugins/plugins/rcc/scripts/bootstrapper_runtime.py) | `python3 plugins/rcc/scripts/bootstrapper_runtime.py start` (for server lifecycle) and `bootstrapper_run(project_kind="action")` (for bounded action runs) | Resolved `Path(__file__).parent / "start_action_server.py"` (file was at repo root, so path failed at runtime), passed `secrets` JSON in `sys.argv[3]`, printed `Command: ...` with plaintext secrets to stdout, enabled `RC_ADD_SHUTDOWN_API=1`, and spawned a detached server via `shell=True`. |
| 5 | `stop_action_server` | **Retire from MCP tool surface**; **Replace** with external CLI [`bootstrapper_runtime.py`](file:///home/kdlocpanda/plugins/plugins/rcc/scripts/bootstrapper_runtime.py) & `bootstrapper_cancel` | `python3 plugins/rcc/scripts/bootstrapper_runtime.py stop` (for owned runtime) and `bootstrapper_cancel` (for active runs) | Sent an unauthenticated `POST {action_server_url}/api/shutdown` to any caller-supplied URL, risking SSRF or terminating unrelated `action-server` instances. |
| 6 | `update_action_code` | **Retire** (arbitrary code overwrite) | Seeded atomically as `<package>_actions.py` by `bootstrapper_scaffold`; validated by `bootstrapper_validate` | Overwrote `actions.py` with unvalidated Python source in arbitrary directories and perpetuated the `actions.py` module-shadowing bug. |
| 7 | `open_action_code` | **Retire** | Host agent/IDE native file navigation | Executed `code <file_path>` on the host machine to launch a GUI VS Code window from inside a headless action run. |
| 8 | `get_action_run_logs` | **Replace** | `bootstrapper_results(query="run_summary" \| "failure_diagnosis")` + resource `bootstrapper://runs/{run_id}` | Performed unauthenticated `requests.get(f"{action_server_url}/api/runs/{run_id}/logs")` against an unvalidated URL (SSRF risk) with no secret redaction. |
| 9 | `get_action_run_logs_latest` | **Replace** | `bootstrapper_results(query="run_summary")` + `bootstrapper_status` | Crashed with `IndexError` when `/api/runs` returned an empty list and wrapped the inner `Response` inside a second `Response(result=Response(...))`. |
| 10 | `get_file_contents` | **Replace** (bounded metadata/summaries) / **Retire** (arbitrary host file read) | `bootstrapper_results(query="project_summary")` (approved roots only) | Joined `action_package_dir`, `action_package_name`, and `file_name` without canonical path resolution, allowing `../../` traversal reads across the host filesystem. |

---

## Part 2: `src/robot_bootrapper/robot_actions.py` (26 Legacy Actions)

All 26 legacy robot actions delegated to a module-level `_run(command: list[str])` helper that caught `subprocess.CalledProcessError` and returned `Response(result=e.output)` **without raising `ActionError` or recording a non-zero exit status**, causing every failed RCC command to appear successful to the model.

| # | Legacy Action Name | Disposition | Modern Replacement / Boundary | Legacy Defect / Security Finding Audited at `2769fb7a` |
| ---: | --- | --- | --- | --- |
| 11 | `run_shell_command` | **Retire** | Removed completely; only structured `bootstrapper_validate` and `bootstrapper_run` on approved roots are allowed | Executed arbitrary `shlex.split(command)` subprocesses on the host and swallowed non-zero exit codes as `Response(result=e.output)`. |
| 12 | `create_robot` | **Replace** | `bootstrapper_plan` + `bootstrapper_scaffold(project_kind="robot")` | Ran `rcc robot initialize -t <template> -d <dir> -f` (`-f` force-overwriting existing files!) and then opened `robot.yaml`, `conda.yaml`, and `tasks.py` with `"w"` and wrote `""`, destroying the newly initialized robot. |
| 13 | `pull_robot` | **Retire** | Curated offline-capable templates in `bootstrapper_scaffold`; skill guidance in `rcc-robots` for external git repos | Ran `rcc pull <url> -d <robot_dir>` against arbitrary remote URLs without approved-root checks or network/trust gates. |
| 14 | `list_templates` | **Replace** | `bootstrapper_discover` + resource `bootstrapper://catalog/templates` | Wrapped `rcc robot initialize --list` as unstructured text output. |
| 15 | `pull_template` | **Retire** | Consolidated into `bootstrapper_discover` / `bootstrapper_scaffold` | Exact duplicate of `pull_robot` (`rcc pull <template_url> -d <robot_dir>`). |
| 16 | `create_from_template` | **Retire** (invalid CLI) / **Replace** via `bootstrapper_scaffold` | `bootstrapper_scaffold(project_kind="robot", template=...)` | Invoked `rcc create <template> -d <robot_dir>`, which is not a valid RCC CLI command (`rcc` has no top-level `create` subcommand), and silently returned the CLI error output as success. |
| 17 | `run_robot` | **Replace** | `bootstrapper_run(project_kind="robot", task_name=...)` | Accepted `task_name: str` in its signature (`def run_robot(robot_dir: str, task_name: str)`) but **ignored `task_name` completely**, running `["rcc", "run", "-r", f"{robot_dir}/robot.yaml"]` and swallowing non-zero exit codes. |
| 18 | `task_testrun` | **Replace** | `bootstrapper_run(project_kind="robot", task_name=...)` | Ran `["rcc", "task", "testrun", "--task", task_name]` in the action server's current working directory without `-r <robot.yaml>` or `--directory`, failing whenever the server CWD was not the target robot. |
| 19 | `initialize_robot` | **Retire** (invalid CLI) / **Replace** via `bootstrapper_scaffold` | `bootstrapper_scaffold(project_kind="robot")` | Invoked `["rcc", "robot", "init", "--name", robot_name]`, which is not a valid RCC CLI command (`rcc robot initialize` takes `-t` and `-d`, not `init --name`). |
| 20 | `robot_dependencies` | **Retire** (invalid CLI) / **Replace** via `bootstrapper_validate` | `bootstrapper_validate(project_kind="robot")` | Invoked `["rcc", "robot", "dependencies"]`, which does not exist in RCC CLI, and ran without a robot path. |
| 21 | `robot_diagnostics` | **Replace** | `bootstrapper_validate(project_kind="robot")` + `bootstrapper_discover` | Ran `["rcc", "configuration", "diagnostics"]` in the server CWD without `--robot <robot.yaml>`, missing project-specific diagnostics. |
| 22 | `wrap_robot` | **Retire** | Covered by `rcc-robots` skill (`rcc robot wrap -r <robot.yaml>`) | Ran `["rcc", "robot", "wrap"]` in the server CWD without specifying which robot to wrap. |
| 23 | `unwrap_robot` | **Retire** | Covered by `rcc-robots` skill (`rcc robot unwrap -z <zip> -d <dir>`) | Ran `["rcc", "robot", "unwrap", "-z", zip_file]` without `-d` destination validation, extracting into the server CWD. |
| 24 | `run_task` | **Replace** | `bootstrapper_run(project_kind="robot", task_name=...)` | Ran `["rcc", "task", "run", "--task", task_name]` (`rcc task run` is a legacy alias) in the server CWD without `-r <robot.yaml>`. |
| 25 | `list_tasks` | **Replace** | `bootstrapper_validate` / `bootstrapper_results(query="project_summary")` (parses `robot.yaml` `tasks:` map directly) | Invoked `["rcc", "task", "list"]`, which is not a valid RCC command, and returned the CLI error string as apparent success. |
| 26 | `script_in_robot` | **Retire** | Removed completely (arbitrary code execution vector) | Executed `["rcc", "run", "--", *shlex.split(command)]` in the server CWD, allowing arbitrary shell commands inside the holotree environment. |
| 27 | `docs_list` | **Retire from MCP tool surface** | Covered by `rcc` / `rcc-core` skills and optional offline `tldr rcc` pages | Wrapped `rcc docs list` as raw text in the MCP tool list, bloating tool discovery. |
| 28 | `docs_recipes` | **Retire from MCP tool surface** | Covered by `rcc` / `rcc-core` / `rcc-robots` skills | Wrapped `rcc docs recipes` as raw text. |
| 29 | `docs_changelog` | **Retire from MCP tool surface** | Covered by `rcc-core` skill and [`capability-and-version-matrix.md`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/capability-and-version-matrix.md) | Wrapped `rcc docs changelog` as raw text. |
| 30 | `help` | **Retire from MCP tool surface** | Covered by `rcc` skill and `bootstrapper_discover` | Wrapped `rcc --help` as raw text. |
| 31 | `prebuild_holotree` | **Replace** (opt-in isolated preflight in `bootstrapper_validate`) | `bootstrapper_validate(project_kind="robot", build_env=True)` | Ran `["rcc", "holotree", "variables", conda_yaml_path, "--space", "user"]`, mutating the shared `"user"` holotree space for arbitrary `conda.yaml` paths. |
| 32 | `export_holotree_environment` | **Retire from MCP tool surface** | Covered by `rcc-core` / `rcc-ci-maintenance` skills (`rcc holotree export`) | Ran `["rcc", "ht", "prebuild", conda_yaml_path, "--export", export_path]`, writing archives to arbitrary host filesystem paths. |
| 33 | `import_holotree_environment` | **Retire from MCP tool surface** | Covered by `rcc-core` / `rcc-ci-maintenance` skills (`rcc holotree import`) | Imported arbitrary zip archives into the host holotree cache without trust verification. |
| 34 | `update_robot_task_code` | **Retire** (arbitrary code overwrite) | Seeded atomically by `bootstrapper_scaffold`; validated by `bootstrapper_validate` | Overwrote `<robot_dir>/tasks.py` with unvalidated Python source outside approved roots. |
| 35 | `update_robot_yaml` | **Retire** (arbitrary file overwrite) | Seeded atomically by `bootstrapper_scaffold`; validated by `bootstrapper_validate` | Overwrote `<robot_dir>/robot.yaml` with unvalidated string content outside approved roots. |
| 36 | `update_conda_yaml` | **Retire** (arbitrary file overwrite) | Seeded atomically by `bootstrapper_scaffold`; validated by `bootstrapper_validate` | Overwrote `<robot_dir>/conda.yaml` with unvalidated string content outside approved roots. |

---

## Regression Test Coverage Map

Every defect class identified above is locked by automated regression tests in [`plugins/rcc/tests/test_bootstrapper_unit.py`](file:///home/kdlocpanda/plugins/plugins/rcc/tests/test_bootstrapper_unit.py) and [`plugins/rcc/tests/test_bootstrapper_runtime.py`](file:///home/kdlocpanda/plugins/plugins/rcc/tests/test_bootstrapper_runtime.py):

- **Non-destructive atomic scaffolding**: Verifies `bootstrapper_scaffold` writes complete, non-empty `package.yaml` / `<pkg>_actions.py` / `devdata/` or `robot.yaml` / `conda.yaml` / `tasks.py`, refuses non-empty directories (including hidden dotfiles and `.git`), and rolls back cleanly on staging failures.
- **No `actions.py` module shadowing**: Verifies scaffolded Action packages use `<package_slug>_actions.py` and `bootstrapper_validate` flags `actions.py` shadowing.
- **No silent error swallowing**: Verifies non-zero subprocess exit codes produce structured `failed` run records and `ActionError` / structured failure diagnoses (`exit_code != 0` never masquerades as `succeeded`).
- **Task parameter fidelity**: Verifies `bootstrapper_run(project_kind="robot", task_name=...)` passes `--task <task_name>` and `-r <robot.yaml>` explicitly.
- **Approved-root & traversal enforcement**: Verifies `../` traversal, unapproved paths, and symlink escapes are rejected before any read, write, or subprocess launch.
- **Secret redaction & no argv leakage**: Verifies secrets passed via `Secret` or secret maps are never placed in subprocess `argv` and are scrubbed from stdout, stderr, error messages, and persisted run journals.
