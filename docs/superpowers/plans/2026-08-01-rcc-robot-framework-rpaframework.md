# RCC Robot Framework and RPA Framework Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add source-backed, evaluation-tested RCC skills for Robot Framework acceptance testing and task-oriented RPA Framework automation.

**Architecture:** Keep the RCC plugin skills-only and add two specialists with distinct trigger boundaries. Each skill keeps routing and operating rules in a concise `SKILL.md`, places detailed guidance in one-level references, and publishes UI metadata through `agents/openai.yaml`; the existing RCC router, source map, plugin manifest, and marketplace catalog connect them to the bundle.

**Tech Stack:** Markdown Agent Skills, YAML skill UI metadata, JSON plugin/marketplace manifests, Robot Framework 7.x documentation, RPA Framework 32.x documentation, Python repository generators, unittest-based repository validation.

## Global Constraints

- Author canonical skill content only under `plugins/rcc/skills/<skill>/`.
- Do not manually edit `skills/`, `.agents/skills/`, generated Hermes shims, or generated Claude manifests.
- Keep RCC as the runtime and containment boundary; upstream Robot Framework and RPA Framework sources provide syntax and library evidence.
- Keep each `SKILL.md` concise and move detailed recipes into one-level `references/` files.
- Do not add an MCP server, custom UI, hooks, executable helpers, or a duplicated upstream API corpus.
- Do not copy dependency pins from prose examples; distinguish released versions from upcoming release notes.
- Develop and validate `rcc-robot-framework` completely before starting `rcc-rpaframework`.
- Preserve the unrelated untracked `.serena/` directory.

## File Map

### New canonical files

- `plugins/rcc/skills/rcc-robot-framework/SKILL.md`: trigger boundary, first inspection, operating rules, and reference routing for `.robot` suites and RCC acceptance tests.
- `plugins/rcc/skills/rcc-robot-framework/agents/openai.yaml`: UI metadata and explicit invocation prompt.
- `plugins/rcc/skills/rcc-robot-framework/references/authoring-and-execution.md`: Robot suite/resource syntax, fixtures, CLI selection, and output artifacts.
- `plugins/rcc/skills/rcc-robot-framework/references/rcc-acceptance-tests.md`: pinned `joshyorko/rcc/robot_tests` patterns.
- `plugins/rcc/skills/rcc-robot-framework/references/results-and-ci.md`: Robot results, Rebot, reruns, CI, and safe parallelism.
- `plugins/rcc/skills/rcc-rpaframework/SKILL.md`: trigger boundary, task classification, operating rules, and reference routing for `RPA.*` work.
- `plugins/rcc/skills/rcc-rpaframework/agents/openai.yaml`: UI metadata and explicit invocation prompt.
- `plugins/rcc/skills/rcc-rpaframework/references/library-selection.md`: task-to-library map and authoritative source links.
- `plugins/rcc/skills/rcc-rpaframework/references/task-recipes.md`: compact Robot Framework recipes for high-value RPA tasks.
- `plugins/rcc/skills/rcc-rpaframework/references/interoperability-and-safety.md`: `RPA_*`/`RC_*` boundaries, `robocorp.*` alternatives, browser/archive/secrets safety.
- `docs/superpowers/evals/2026-08-01-rcc-robot-framework.md`: raw baseline/forward evaluation evidence and rubric.
- `docs/superpowers/evals/2026-08-01-rcc-rpaframework.md`: raw baseline/forward evaluation evidence and rubric.

### Modified canonical files

- `plugins/rcc/skills/rcc/SKILL.md`: route Robot Framework and RPA Framework work to the new specialists.
- `plugins/rcc/skills/rcc/agents/openai.yaml`: update router UI copy to include the new specialists.
- `plugins/rcc/skills/rcc-core/SKILL.md`: hand RCC `robot_tests` authoring to `rcc-robot-framework` while retaining core implementation ownership.
- `plugins/rcc/skills/rcc-robots/SKILL.md`: hand `.robot` authoring and `RPA.*` selection to the new skills while retaining `robot.yaml` runtime ownership.
- `plugins/rcc/skills/rcc/references/source-map.md`: add current official docs and the pinned RCC acceptance-test source snapshot.
- `plugins/rcc/.codex-plugin/plugin.json`: refresh RCC discovery copy, keywords, and three starter prompts.
- `marketplaces/catalog.json`: refresh RCC catalog description and tags.

### Generated files and views

- `.agents/plugins/marketplace.json`
- `.claude-plugin/marketplace.json`
- `.agents/skills/rcc-robot-framework`
- `.agents/skills/rcc-rpaframework`
- `skills/rcc-robot-framework`
- `skills/rcc-rpaframework`
- `plugins/rcc/.claude-plugin/plugin.json`
- `plugins/rcc/plugin.yaml`
- `plugins/rcc/__init__.py`

---

### Task 1: Build and prove `rcc-robot-framework`

**Files:**
- Create: `plugins/rcc/skills/rcc-robot-framework/SKILL.md`
- Create: `plugins/rcc/skills/rcc-robot-framework/agents/openai.yaml`
- Create: `plugins/rcc/skills/rcc-robot-framework/references/authoring-and-execution.md`
- Create: `plugins/rcc/skills/rcc-robot-framework/references/rcc-acceptance-tests.md`
- Create: `plugins/rcc/skills/rcc-robot-framework/references/results-and-ci.md`
- Create: `docs/superpowers/evals/2026-08-01-rcc-robot-framework.md`

**Interfaces:**
- Consumes: Robot Framework official documentation; `joshyorko/rcc` commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`; existing `rcc-core` and `rcc-robots` ownership boundaries.
- Produces: skill name `rcc-robot-framework`; references loaded directly from its `SKILL.md`; UI prompt containing `$rcc-robot-framework`.

- [ ] **Step 1: Run two fresh-context baseline evaluations without the new skill**

Dispatch independent agents with no forked conversation context and these exact prompts:

```text
In joshyorko/rcc, add a Robot Framework regression test for `rcc ht hash` returning exit code 2. The diagnostic is on stderr while stdout must remain valid JSON. Explain the files and assertions you would use and the focused command you would run.
```

```text
Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Record each response verbatim in `docs/superpowers/evals/2026-08-01-rcc-robot-framework.md`, followed by a rubric with these exact rows: correct owner/skill, exit-code contract, stdout/stderr separation, JSON structural assertion, RCC state isolation, safe parallelism, focused command, and source grounding. Mark each row `PASS` or `FAIL` and quote the response fragment that supports the verdict.

- [ ] **Step 2: Verify the baseline exposes real gaps**

Expected: at least one evaluation fails two or more rubric rows. If both pass every row, stop this task and reduce the skill to only the missing source-navigation guidance demonstrated by the baseline; do not author redundant instructions.

- [ ] **Step 3: Create the skill metadata and core workflow**

Create `SKILL.md` with this frontmatter and section contract:

```markdown
---
name: rcc-robot-framework
description: Use when authoring, reviewing, running, or debugging Robot Framework .robot suites, resources, custom Python libraries, Robot CLI or Rebot results, or RCC's robot_tests acceptance harness.
---

# RCC Robot Framework

## First Inspection
## Operating Rules
## Boundary With Other RCC Skills
## References
```

The body must require config/source inspection before editing, preserve return code/stdout/stderr separately, isolate mutable RCC state, parse JSON structurally, review golden diffs, and route runtime/core failures to their existing owners. The References section must directly name all three reference files and state when each is required.

Create `agents/openai.yaml` exactly in this shape:

```yaml
interface:
  display_name: "RCC Robot Framework"
  short_description: "Robot Framework suites and RCC acceptance tests."
  default_prompt: "Use $rcc-robot-framework to author, run, or debug a Robot Framework suite or RCC robot_tests acceptance test."
```

- [ ] **Step 4: Write `authoring-and-execution.md` from current official sources**

Cover these concrete sections: source hierarchy; project structure (`__init__.robot`, suite files, resources, Python libraries); Settings/Variables/Test Cases-or-Tasks/Keywords; suite/test setup and teardown; scalar/list/dict/environment variables; `IF`/`FOR`/`TRY`; templates for command matrices; tags plus runtime `Skip`; `robot --dryrun`; file/suite/test/tag selection; `--outputdir`; and output/report/log/xUnit artifacts.

Use and link these sources:

```text
https://robotframework.org/robotframework/latest/RobotFrameworkUserGuide.html
https://docs.robotframework.org/docs/examples/project_structure
https://docs.robotframework.org/docs/style_guide
https://robotframework.org/robotframework/latest/libraries/Process.html
https://robotframework.org/robotframework/latest/libraries/OperatingSystem.html
https://docs.robotframework.org/docs/extending_robot_framework/custom-libraries/python_library
```

Include one compact `.robot` example using a resource keyword, explicit expected exit code, and separate stdout/stderr assertions. Do not present host `pip install` as the default for an RCC-contained project.

- [ ] **Step 5: Write `rcc-acceptance-tests.md` from the pinned RCC snapshot**

Pin the reference to commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330` and link the tree, `resources.robot`, `supporting.py`, `exitcodes.robot`, `ht_hash.robot`, `uv_native.robot`, `robot_bundle.robot`, and `tasks.py` at that commit.

Document the exact local commands:

```bash
python3 -m robot -L DEBUG -d tmp/output robot_tests
python3 -m robot -L DEBUG -d tmp/output robot_tests/robot_bundle.robot
```

Cover root suite setup/teardown, shared `Step`/stream/assertion DSL, expected nonzero exits, `Fire And Forget` limits, `ROBOCORP_HOME=tmp/robocorp`, activation-environment scrubbing, Windows executable normalization, fixtures, structural JSON parsing, reviewed golden files with newline normalization, and adversarial fixtures built in Python helpers.

- [ ] **Step 6: Write `results-and-ci.md` with safe state handling**

Cover `output.xml`, `log.html`, `report.html`, xUnit, `ExecutionResult`, `ResultVisitor`, `rebot`, `--rerunfailed`, `rebot --merge`, Pabot suite/test splitting, PabotLib locks, CI artifact upload, and focused reruns.

Use and link:

```text
https://docs.robotframework.org/docs/parsing_results
https://docs.robotframework.org/docs/flaky_tests
https://docs.robotframework.org/docs/parallel
https://docs.robotframework.org/docs/using_rf_in_ci_systems/ci/github-actions
```

State explicitly that RCC acceptance suites may mutate `ROBOCORP_HOME`, holotree, and `tmp/`; Pabot is unsafe until every worker receives isolated homes and temporary roots. Flag the official GitHub Actions sample as conceptual because its package/action pins and `continue-on-error` behavior may be stale or mask failures.

- [ ] **Step 7: Validate the skill metadata before forward evaluation**

Run:

```bash
python3 /home/vscode/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/rcc/skills/rcc-robot-framework
```

Expected: validation succeeds with no frontmatter, naming, or metadata errors.

- [ ] **Step 8: Run forward evaluations with the completed skill**

Dispatch fresh agents with each baseline prompt prefixed by:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.
```

Append responses verbatim to the evaluation file and score the same rubric. Expected: all rows pass. If a row fails, make the smallest guidance change that addresses the observed failure and rerun only the failing scenario.

- [ ] **Step 9: Deploy and verify the generated runtime views**

```bash
python3 scripts/build_marketplaces.py
python3 scripts/build_runtime_views.py
python3 scripts/build_hermes_plugins.py
bin/check
```

Expected: `skills/rcc-robot-framework` and `.agents/skills/rcc-robot-framework` are symlinks to the canonical skill and all repository checks pass.

- [ ] **Step 10: Commit the complete validated skill**

```bash
git add plugins/rcc/skills/rcc-robot-framework docs/superpowers/evals/2026-08-01-rcc-robot-framework.md skills/rcc-robot-framework .agents/skills/rcc-robot-framework
git commit -m "feat: add RCC Robot Framework skill"
```

### Task 2: Build and prove `rcc-rpaframework`

**Files:**
- Create: `plugins/rcc/skills/rcc-rpaframework/SKILL.md`
- Create: `plugins/rcc/skills/rcc-rpaframework/agents/openai.yaml`
- Create: `plugins/rcc/skills/rcc-rpaframework/references/library-selection.md`
- Create: `plugins/rcc/skills/rcc-rpaframework/references/task-recipes.md`
- Create: `plugins/rcc/skills/rcc-rpaframework/references/interoperability-and-safety.md`
- Create: `docs/superpowers/evals/2026-08-01-rcc-rpaframework.md`

**Interfaces:**
- Consumes: RPA Framework official docs/release notes, current package metadata, existing `rcc-robots` and `rcc-workitems` boundaries.
- Produces: skill name `rcc-rpaframework`; a task-oriented library selector; UI prompt containing `$rcc-rpaframework`.

- [ ] **Step 1: Run two fresh-context baseline evaluations without the new skill**

Use these exact prompts:

```text
Build an RCC-managed Robot Framework task that reads a CSV into a table, validates JSON, creates a ZIP artifact, and uploads through a browser. Choose the RPA Framework libraries and show robot.yaml/conda.yaml implications without copying stale dependency pins.
```

```text
Review an RCC robot that combines RPA.Robocorp.WorkItems, robocorp.workitems, RPA.Robocorp.Vault, and robocorp.vault in one run. Explain which environment contracts conflict, what should remain for maintenance, and what a safe migration boundary looks like.
```

Record verbatim responses in `docs/superpowers/evals/2026-08-01-rcc-rpaframework.md`. Score these rows: correct library selection, RCC environment boundary, current-version verification, archive safety, browser initialization, `RPA_*`/`RC_*` separation, hosted-adapter framing, and migration restraint.

- [ ] **Step 2: Verify the baseline exposes real gaps**

Expected: at least one evaluation fails two or more rubric rows. If both pass every row, author only the source-routing and compatibility guidance the baseline still lacks.

- [ ] **Step 3: Create the skill metadata and task-classification workflow**

Create `SKILL.md` with:

```markdown
---
name: rcc-rpaframework
description: Use when selecting, authoring, reviewing, or debugging RPA Framework libraries and RPA.* keywords in RCC-managed Robot Framework or Python automation projects.
---

# RCC RPA Framework

## First Inspection
## Select By Task
## Operating Rules
## Boundary With Modern Robocorp Libraries
## References
```

Require inspection of `robot.yaml`, dependency config, imports, environment families, and platform needs before recommending libraries. The References section must directly route library choice, recipes, and safety/interoperability to the three files.

Create `agents/openai.yaml`:

```yaml
interface:
  display_name: "RCC RPA Framework"
  short_description: "RPA Framework libraries in RCC automation."
  default_prompt: "Use $rcc-rpaframework to choose and apply RPA Framework libraries safely in an RCC-managed automation project."
```

- [ ] **Step 4: Write `library-selection.md` as a task map**

Create a decision table with columns `Task`, `Primary library`, `Package/platform needs`, `Prefer instead when`, and `Source`. Include Filesystem, JSON, Tables, Archive, HTTP, Excel, PDF, Email, Database, browser choices, Desktop/OCR, RobotLogListener, Vault, WorkItems, Storage, and Assistant.

Link the RPA Framework library index and the specific Filesystem, JSON, Tables, Archive, RobotLogListener, Browser Playwright, and release-note pages. State that RPA Framework 32.0.1 was the latest released version observed on 2026-08-01 while 32.0.2 appeared as upcoming; require a live metadata check before any current pin claim.

- [ ] **Step 5: Write `task-recipes.md` with one strong example per pattern**

Include concise Robot Framework recipes for: CSV-to-table transformation; JSON validation; safe archive creation/extraction; HTTP download to an RCC artifact; browser initialization plus screenshot/download; Excel/PDF/Email/Database selection; and secret-safe logging with RobotLogListener.

Recipes must use RCC artifact paths, show only imports and keywords needed for the task, and tell readers to resolve dependencies through the project's `conda.yaml` or declared freeze rather than host installation.

- [ ] **Step 6: Write `interoperability-and-safety.md`**

Include an explicit matrix for legacy `RPA_WORKITEMS_*`/`RPA_SECRET_*` with `RPA.Robocorp.WorkItems`/`RPA.Robocorp.Vault` versus modern `RC_WORKITEM_*`/`RC_VAULT_*` with `robocorp.workitems`/`robocorp.vault`.

Cover intentional bridge requirements, hosted adapter/auth boundaries, local mock-secret limits, redaction through RobotLogListener or modern logging APIs, RPA.Archive traversal risk and current-release verification, stale Browser Playwright examples, `rfbrowser init` only for Robot Framework Browser projects, platform dependencies for desktop/OCR, and migration restraint for maintained legacy robots.

- [ ] **Step 7: Validate and forward-test the complete skill**

Run:

```bash
python3 /home/vscode/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/rcc/skills/rcc-rpaframework
```

Then rerun both baseline prompts with:

```text
Use $rcc-rpaframework at plugins/rcc/skills/rcc-rpaframework/SKILL.md.
```

Expected: every rubric row passes. Make only failure-driven revisions and rerun the affected scenario.

- [ ] **Step 8: Deploy and verify the generated runtime views**

```bash
python3 scripts/build_marketplaces.py
python3 scripts/build_runtime_views.py
python3 scripts/build_hermes_plugins.py
bin/check
```

Expected: `skills/rcc-rpaframework` and `.agents/skills/rcc-rpaframework` are symlinks to the canonical skill and all repository checks pass.

- [ ] **Step 9: Commit the complete validated skill**

```bash
git add plugins/rcc/skills/rcc-rpaframework docs/superpowers/evals/2026-08-01-rcc-rpaframework.md skills/rcc-rpaframework .agents/skills/rcc-rpaframework
git commit -m "feat: add RCC RPA Framework skill"
```

### Task 3: Integrate routing and source evidence

**Files:**
- Modify: `plugins/rcc/skills/rcc/SKILL.md`
- Modify: `plugins/rcc/skills/rcc/agents/openai.yaml`
- Modify: `plugins/rcc/skills/rcc-core/SKILL.md`
- Modify: `plugins/rcc/skills/rcc-robots/SKILL.md`
- Modify: `plugins/rcc/skills/rcc/references/source-map.md`

**Interfaces:**
- Consumes: skill names and boundaries completed in Tasks 1–2.
- Produces: deterministic routing among `rcc-core`, `rcc-robots`, `rcc-robot-framework`, and `rcc-rpaframework`; shared source provenance.

- [ ] **Step 1: Add direct router entries for both skills**

In `rcc/SKILL.md`, add route bullets with these meanings:

```text
rcc-robot-framework owns .robot suites, resources, Robot CLI/Rebot, custom Robot libraries, results, and RCC robot_tests acceptance work.
rcc-rpaframework owns RPA.* library selection, keyword recipes, package/platform needs, and interoperability with robocorp.* libraries.
```

Add boundary examples: a failing `rcc ht vars` before suite execution starts in `rcc-core`; a broken `robot.yaml` environment starts in `rcc-robots`; a wrong assertion in `robot_tests/ht_hash.robot` starts in `rcc-robot-framework`; choosing `RPA.Tables` versus a Python library starts in `rcc-rpaframework`.

- [ ] **Step 2: Tighten specialist handoffs**

In `rcc-core/SKILL.md`, route `robot_tests/` authoring/execution to `rcc-robot-framework` while keeping the RCC behavior under test and Go implementation in `rcc-core`.

In `rcc-robots/SKILL.md`, retain `robot.yaml`, `conda.yaml`, holotree, freeze, task runtime, and artifacts; route `.robot` suite mechanics to `rcc-robot-framework` and `RPA.*` selection to `rcc-rpaframework`.

Update `rcc/agents/openai.yaml` so the short description and default prompt name both new skill surfaces without exceeding a single readable sentence.

- [ ] **Step 3: Add source snapshots to `source-map.md`**

Add a dated `2026-08-01 Robot Framework / RPA Framework Coverage` section with:

```text
OpenAI plugin skills: https://developers.openai.com/plugins/build/skills
OpenAI plugin packaging: https://developers.openai.com/plugins/build/plugins
OpenAI plugin architecture: https://developers.openai.com/plugins/concepts/plugins
RCC robot_tests: https://github.com/joshyorko/rcc/tree/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests
Robot Framework User Guide: https://robotframework.org/robotframework/latest/RobotFrameworkUserGuide.html
Robot Framework practical docs: https://docs.robotframework.org/docs
RPA Framework docs: https://rpaframework.org/
RPA Framework release notes: https://rpaframework.org/releasenotes.html
```

Record Robot Framework 7.4.2 and RPA Framework 32.0.1 as observed source snapshots, not permanent recommended pins. Note that RPA Framework 32.0.2 was listed as upcoming when checked.

- [ ] **Step 4: Run focused link and routing checks**

```bash
rg -n "rcc-robot-framework|rcc-rpaframework|d5942d90994d|Robot Framework 7.4.2|RPA Framework 32.0.1" plugins/rcc/skills/rcc plugins/rcc/skills/rcc-core plugins/rcc/skills/rcc-robots
```

Expected: both skill names appear in the router and boundary owners; source versions and pinned commit appear in `source-map.md`.

- [ ] **Step 5: Commit routing and evidence**

```bash
git add plugins/rcc/skills/rcc plugins/rcc/skills/rcc-core/SKILL.md plugins/rcc/skills/rcc-robots/SKILL.md
git commit -m "docs: route RCC Robot Framework work"
```

### Task 4: Refresh plugin and marketplace discovery metadata

**Files:**
- Modify: `plugins/rcc/.codex-plugin/plugin.json`
- Modify: `marketplaces/catalog.json`

**Interfaces:**
- Consumes: completed specialist names and router language.
- Produces: install-surface copy that advertises both skills and stays within current OpenAI limits.

- [ ] **Step 1: Update the RCC plugin manifest**

Use this plugin description:

```text
RCC-family skills for RCC core, Robot Framework acceptance tests, RPA Framework automation, robot projects, work items, Action Server, and CI maintenance.
```

Add `rpaframework`, `rpa-framework`, and `robot-tests` to `keywords`. Update `interface.shortDescription` and `interface.longDescription` with the same boundaries. Replace the existing six `defaultPrompt` entries with exactly these three:

```json
[
  "Maintain RCC Robot Framework acceptance tests.",
  "Build an RCC automation with RPA Framework.",
  "Debug an RCC robot environment or task."
]
```

- [ ] **Step 2: Update authored marketplace metadata**

In the RCC entry in `marketplaces/catalog.json`, use the manifest description and add `robot-framework`, `robot-tests`, `rpaframework`, and `rpa-framework` tags. Preserve plugin order, category, and Claude category.

- [ ] **Step 3: Validate canonical metadata before generation**

```bash
python3 /home/vscode/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py plugins/rcc
```

Expected: plugin validation succeeds; the manifest has a valid name/version, real asset paths, and no unsupported or placeholder fields.

- [ ] **Step 4: Commit authored metadata**

```bash
git add plugins/rcc/.codex-plugin/plugin.json marketplaces/catalog.json
git commit -m "chore: refresh RCC plugin discovery metadata"
```

### Task 5: Rebuild generated views and validate the bundle

**Files:**
- Generate: `.agents/plugins/marketplace.json`
- Generate: `.claude-plugin/marketplace.json`
- Generate: `plugins/rcc/.claude-plugin/plugin.json`
- Generate: `plugins/rcc/plugin.yaml`
- Generate: `plugins/rcc/__init__.py`

**Interfaces:**
- Consumes: all canonical files from Tasks 1–4.
- Produces: synchronized Codex, Claude, Hermes, and runtime skill views.

- [ ] **Step 1: Rebuild every generated surface in repository order**

```bash
python3 scripts/build_marketplaces.py
python3 scripts/build_runtime_views.py
python3 scripts/build_hermes_plugins.py
```

Expected: both new skill symlinks appear in `skills/` and `.agents/skills/`; compatibility manifests include the refreshed RCC metadata.

- [ ] **Step 2: Verify generated-view invariants**

```bash
find skills .agents/skills -mindepth 1 -maxdepth 1 ! -type l -print
test ! -e codex
```

Expected: `find` prints nothing and the `test` command exits 0.

- [ ] **Step 3: Run the complete repository validator**

```bash
bin/check
```

Expected: marketplace, runtime-view, and Hermes `--check` commands pass; all unittests pass; repository validation passes.

- [ ] **Step 4: Review the final diff and worktree**

```bash
git diff --check
git status --short
git diff --stat
```

Expected: no whitespace errors; only intended canonical/generated RCC files and evaluation evidence are changed; `.serena/` remains untracked and untouched.

- [ ] **Step 5: Commit generated compatibility views**

```bash
git add .agents/plugins/marketplace.json .claude-plugin/marketplace.json plugins/rcc/.claude-plugin/plugin.json plugins/rcc/plugin.yaml plugins/rcc/__init__.py
git commit -m "build: publish RCC framework skill views"
```

### Task 6: Run final trigger-boundary acceptance checks

**Files:**
- Modify if failures require it: `docs/superpowers/evals/2026-08-01-rcc-robot-framework.md`
- Modify if failures require it: `docs/superpowers/evals/2026-08-01-rcc-rpaframework.md`
- Modify if failures require it: the smallest affected `SKILL.md` or `agents/openai.yaml`

**Interfaces:**
- Consumes: installed/generated skill views and completed router.
- Produces: evidence that positive and negative prompts choose the intended specialist without overlap.

- [ ] **Step 1: Test direct and indirect positive prompts**

Run fresh-context agents for these prompts without naming a skill:

```text
Why is this .robot suite reading the JSON diagnostic from stdout when RCC writes it to stderr?
```

```text
Help me maintain the command matrix in robot_tests/exitcodes.robot.
```

```text
Which RPA library should I use to turn worksheet rows into a filtered table in an RCC robot?
```

```text
This legacy robot imports RPA.Robocorp.Vault; should I replace it with robocorp.vault now?
```

Expected: the first two select `rcc-robot-framework`; the last two select `rcc-rpaframework`.

- [ ] **Step 2: Test negative routing prompts**

```text
rcc ht vars fails before Python starts; diagnose the holotree environment.
```

```text
Fix the environmentConfigs and freeze files in this robot.yaml project.
```

```text
Design a DocumentDB producer/consumer/reporter queue using robocorp.workitems.
```

```text
Repair the ROBOCORP_HOME cache key in this GitHub Actions workflow.
```

Expected: route respectively to `rcc-core`, `rcc-robots`, `rcc-workitems`, and `rcc-ci-maintenance`; neither new skill should claim these requests.

- [ ] **Step 3: Apply only evidence-backed metadata fixes**

If a prompt routes incorrectly, change the smallest relevant description or boundary sentence, append the failing prompt and before/after result to the corresponding evaluation document, rerun that prompt, then rerun `bin/check`. Do not broaden both new descriptions to capture the same vocabulary.

- [ ] **Step 4: Run final verification**

```bash
python3 /home/vscode/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/rcc/skills/rcc-robot-framework
python3 /home/vscode/.codex/skills/.system/skill-creator/scripts/quick_validate.py plugins/rcc/skills/rcc-rpaframework
python3 /home/vscode/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py plugins/rcc
bin/check
git diff --check
```

Expected: all validators pass with clean output.

- [ ] **Step 5: Commit final evaluation-driven refinements if needed**

```bash
git add plugins/rcc/skills docs/superpowers/evals
git commit -m "test: verify RCC framework skill routing"
```

Skip this commit only when Task 6 produces no file changes.
