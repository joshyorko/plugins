# RCC Robot Framework and RPA Framework Coverage Design

## Context

The RCC plugin currently covers `robot.yaml` projects, holotree environments, Python automation libraries, work items, Action Server, and CI maintenance. Robot Framework and RPA Framework appear mostly as compatibility notes. The plugin does not yet teach agents how to author `.robot` suites, maintain RCC's own `robot_tests` acceptance harness, select `RPA.*` libraries, or apply those libraries safely in RCC-managed projects.

The new coverage must preserve RCC as the runtime and containment boundary. Upstream Robot Framework and RPA Framework sources provide syntax and library evidence; they do not redefine the plugin around hosted Robocorp or Sema4AI products.

## Goals

- Add focused Robot Framework guidance for authoring, running, debugging, and maintaining `.robot` suites in RCC-family work.
- Capture the recurring acceptance-test patterns in `joshyorko/rcc/robot_tests`.
- Add task-oriented RPA Framework guidance that helps agents select and use appropriate `RPA.*` libraries without reproducing upstream API documentation.
- Explain when to retain RPA Framework, when to use modern `robocorp.*` packages, and how to avoid mixing incompatible environment-variable families accidentally.
- Improve RCC plugin discovery metadata and starter prompts using current OpenAI plugin and skill guidance.
- Validate both skills through baseline and forward evaluations, generated-view rebuilds, and repository checks.

## Non-Goals

- Do not add an MCP server, custom UI, hooks, or a new runtime product.
- Do not move `robot.yaml`, `conda.yaml`, holotree, or artifact ownership out of `rcc-robots`.
- Do not copy the full Robot Framework or RPA Framework keyword/API corpus into the plugin.
- Do not treat hosted Robocorp or Sema4AI services as the default architecture for local or CI automation.
- Do not replace intentionally old compatibility fixtures or pins in RCC's test suite merely because newer packages exist.

## Source Hierarchy

Use sources in this order for factual claims:

1. Current OpenAI Codex/plugin documentation for skill boundaries, packaging, metadata, and progressive disclosure.
2. `joshyorko/rcc` at a recorded commit for RCC-specific acceptance-test conventions.
3. Current Robot Framework User Guide and standard-library documentation for canonical syntax and runtime behavior.
4. `docs.robotframework.org/docs` for practical guides, with stale examples called out when necessary.
5. RPA Framework documentation, release notes, PyPI metadata, and checked source for library behavior and current compatibility claims.
6. Existing RCC plugin references for local project, environment, work-item, and CI conventions.

Record fetch dates or immutable commits for source snapshots. Never lift dependency pins from prose examples without checking current release metadata. Distinguish released versions from upcoming release notes.

## Plugin Shape

Keep RCC as a skills-only plugin. Add two specialist skills because the workflows have different triggers, inputs, and success criteria:

- `rcc-robot-framework`: Robot Framework suite authoring, execution, result handling, custom libraries, CI reliability, and RCC CLI acceptance tests.
- `rcc-rpaframework`: task-oriented RPA Framework library selection, recipes, safety, and interoperability with modern `robocorp.*` packages.

Keep detailed material in one-level `references/` files. Each `SKILL.md` remains a concise router and operating workflow. Each skill receives `agents/openai.yaml` with a focused display name, short description, and default prompt that explicitly invokes the skill.

## `rcc-robot-framework`

### Responsibilities

- Recognize `.robot`, `resources.robot`, `__init__.robot`, Robot CLI, Rebot, Robot result files, custom Python libraries, and RCC `robot_tests` work.
- Teach suite structure: Settings, Variables, Test Cases or Tasks, Keywords, resources, libraries, setup/teardown, variables, control flow, templates, tags, and runtime skips.
- Teach contained execution, focused suite/test selection, output directories, `output.xml`, `log.html`, `report.html`, xUnit output, Rebot merging, rerun-failed workflows, and result-model parsing.
- Teach process assertions that preserve return code, stdout, and stderr as separate contracts.
- Teach state isolation for `ROBOCORP_HOME`, temporary projects, holotree state, fixtures, and platform-specific execution.
- Teach custom Python helpers for subprocess boundaries, environment scrubbing, JSON parsing, golden normalization, and adversarial fixture creation.

### References

- `references/authoring-and-execution.md`: suite/resource structure, Robot syntax, fixtures, templates, tags, CLI selection, dry runs, and output artifacts.
- `references/rcc-acceptance-tests.md`: pinned `joshyorko/rcc/robot_tests` conventions, shared command DSL, stream assertions, exit-code contracts, isolated state, golden files, platform behavior, and representative suites.
- `references/results-and-ci.md`: Rebot/result APIs, rerun-failed handling, CI artifacts, Pabot guidance, and the requirement to isolate RCC state before parallel execution.

### Boundary Rules

- Route changes under RCC's `robot_tests/` here after identifying the RCC behavior under test.
- Route RCC implementation, build, install, endpoint, and holotree internals to `rcc-core`.
- Route project packaging and environment failures to `rcc-robots` when the `.robot` suite itself is not the failing surface.
- Treat `Fire And Forget`-style helpers as setup/cleanup exceptions, never as a way to ignore failures in behavior under test.
- Parse JSON structurally; use stable fragment assertions or reviewed golden files for textual contracts.

## `rcc-rpaframework`

### Responsibilities

- Provide a task-oriented map for Filesystem, JSON, Tables, Archive, HTTP, Excel, PDF, Email, Database, Browser, Desktop/OCR, RobotLogListener, Vault, WorkItems, Storage, and Assistant-related workflows when supported by current packages.
- Provide concise Robot Framework examples and note Python-callable APIs where that is materially useful.
- Explain package extras, platform requirements, browser initialization, artifact handling, secret-safe logging, and common environment contracts.
- Compare legacy `RPA.*` choices with modern `robocorp.*` packages using the task's existing architecture and compatibility requirements.
- Highlight security and staleness concerns, including archive traversal fixes and outdated browser examples.

### References

- `references/library-selection.md`: task-to-library decision table, package boundaries, platform constraints, and authoritative upstream links.
- `references/task-recipes.md`: compact, adaptable recipes for the highest-value local and CI automation tasks.
- `references/interoperability-and-safety.md`: `RPA_*` versus `RC_*` environment families, `robocorp.*` alternatives, secrets/logging, archive safety, browser setup, and hosted-adapter boundaries.

### Boundary Rules

- Prefer the library already used by a maintained project unless migration is requested or necessary.
- For new RCC Python robots, prefer modern `robocorp.*` packages when they directly cover the task; use RPA Framework when Robot keyword workflows, broader RPA libraries, desktop automation, or compatibility make it the better fit.
- Do not mix legacy and modern work-item or vault environment families unless an intentional bridge is documented and tested.
- Treat `RPA.Robocorp.*` libraries as hosted-platform adapters with their own authentication and environment contracts, not as default RCC-local services.

## Router and Metadata Changes

- Update `rcc` routing so `.robot` authoring and `robot_tests` work select `rcc-robot-framework`, while `RPA.*` library work selects `rcc-rpaframework`.
- Cross-link `rcc-core` and `rcc-robots` at their ownership boundaries without duplicating detailed recipes.
- Add official Robot Framework, RPA Framework, and pinned RCC acceptance-test sources to the shared source map.
- Refresh the RCC plugin description, keywords, long description, and catalog tags to name Robot Framework and RPA Framework explicitly.
- Limit plugin starter prompts to three concise examples, matching current OpenAI install-surface guidance.
- Keep canonical files under `plugins/rcc/`; regenerate compatibility manifests and runtime views rather than editing generated files.

## Evaluation-Driven Development

Develop one skill completely before starting the second.

### Baseline

Run fresh-context evaluations without the new skill and record concrete omissions or incorrect decisions. At minimum, cover:

- adding an RCC CLI regression test that must assert a nonzero exit code and stderr;
- explaining and safely running a focused `robot_tests` suite without contaminating the developer's RCC state;
- choosing libraries for a Robot Framework task that handles tables, archives, and browser automation;
- deciding between `RPA.*` and `robocorp.*` packages without mixing environment families;
- designing CI reruns or parallel execution for stateful RCC acceptance suites.

### Forward Evaluation

Run equivalent fresh-context prompts with the completed skill available. Pass raw task prompts and source artifacts, not the intended answer. Verify that agents:

- trigger the correct specialist skill;
- preserve RCC's packaging/runtime boundary;
- choose the correct source/reference file;
- separate stdout, stderr, and exit-code assertions;
- isolate mutable RCC state;
- avoid stale pins and unsafe archive guidance;
- produce concise, runnable patterns rather than an exhaustive keyword dump.

Refine descriptions and guidance from observed failures. Include direct, indirect, and negative trigger prompts so the two new skills do not compete with `rcc-core`, `rcc-robots`, or `rcc-workitems` incorrectly.

## Validation

For each skill:

1. Validate frontmatter, naming, and `agents/openai.yaml` metadata with the current OpenAI skill tooling where applicable.
2. Run the skill's baseline and forward evaluations and record results in implementation notes or test output, not inside the shipped skill.
3. Check internal links, source URLs, commands, examples, and reference routing.

After both skills and router/metadata changes:

1. Run `python3 scripts/build_marketplaces.py`.
2. Run `python3 scripts/build_runtime_views.py`.
3. Run `python3 scripts/build_hermes_plugins.py`.
4. Run `bin/check`.
5. Confirm `skills/` and `.agents/skills/` contain symlinks only, every new skill has one owner, and `codex/` does not exist.
6. Review the final diff for unintended generated or user-owned changes.

## Acceptance Criteria

- Direct requests involving `.robot`, Robot Framework execution/results, or `joshyorko/rcc/robot_tests` reliably route to `rcc-robot-framework`.
- Direct requests involving `RPA.*` library selection or use reliably route to `rcc-rpaframework`.
- Negative trigger prompts continue to route RCC runtime, work-item, Action Server, and CI-maintenance work to their existing owners.
- The skills provide usable RCC-first guidance for the representative evaluation scenarios.
- Current official sources and pinned repository evidence support version-sensitive and behavioral claims.
- No unnecessary MCP server, UI, hooks, scripts, duplicated API corpus, or generated-file hand edits are introduced.
- All repository generators and `bin/check` pass.
