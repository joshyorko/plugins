[![Bootstrap Smoke](https://github.com/joshyorko/plugins/actions/workflows/bootstrap-smoke.yml/badge.svg)](https://github.com/joshyorko/plugins/actions/workflows/bootstrap-smoke.yml)

# Agent Skills

A plugin-first skill repo for three jobs:

- `plugins/37signals/` for Rails, Hotwire, product refresh/design, shaping, work, and DHH judgment inspired by public 37signals/Basecamp sources
- `plugins/fizzy/` for self-hosted Fizzy workflows via the upstream CLI
- `plugins/rcc/` for RCC automation, isolated environments, and robot scaffolding

The canonical source of truth lives under `plugins/`. The repo exposes a generated top-level `skills/` view for humans and standalone installers, plus `.agents/skills/` for in-repo agent discovery.

## Structure

```text
plugins/
├── AGENTS.md
├── README.md
├── .agents/
│   ├── plugins/
│   │   └── marketplace.json
│   └── skills/
├── .claude-plugin/
│   └── marketplace.json
├── marketplaces/
│   └── catalog.json
├── plugins/
│   ├── 37signals/
│   │   ├── .codex-plugin/
│   │   ├── .claude-plugin/
│   │   ├── plugin.yaml        # generated Hermes compatibility
│   │   ├── __init__.py        # generated Hermes skill registration
│   │   ├── references/
│   │   └── skills/
│   ├── fizzy/
│   │   ├── .codex-plugin/
│   │   ├── .claude-plugin/
│   │   └── skills/
│   └── rcc/
│       ├── .codex-plugin/
│       ├── .claude-plugin/
│       └── skills/
├── scripts/
│   ├── build_hermes_plugins.py
│   ├── build_marketplaces.py
│   ├── build_runtime_views.py
│   └── validate_repo.py
└── skills/  # generated standalone symlinks
```

## Skills

### 37signals

The `plugins/37signals/` plugin is now one compact active skill suite instead of many recipe-sized skills. The active surface is listed in [`plugins/37signals/skills.active.yml`](plugins/37signals/skills.active.yml); detailed tactics live as recipe references under [`plugins/37signals/references/recipes/`](plugins/37signals/references/recipes/). The skills are community-maintained, source-grounded, and not official 37signals/Basecamp guidance.

The suite separates durable entrypoints from lower-level implementation recipes. Rails work starts with implement, review, refactor, Hotwire, or DHH Rails judgment. Product work starts with product refresh, product design, Shape Up, scope judgment, or communication. When a lower-level topic is needed, the active skill loads a recipe such as auth, tenancy, migrations, jobs, caching, tests, Turbo, Stimulus, Kamal, delegated types, product refresh, or product copy.

**Active entrypoints**

- [`37signals-rails-implement`](plugins/37signals/skills/37signals-rails-implement/SKILL.md) - end-to-end Rails feature implementation with recipe selection for models, controllers, tenancy, jobs, mailers, APIs, tests, and deployment.
- [`37signals-rails-review`](plugins/37signals/skills/37signals-rails-review/SKILL.md) - Rails code review focused on behavioral risk, missing tests, coupling, tenancy leaks, and convention drift.
- [`37signals-rails-refactor`](plugins/37signals/skills/37signals-rails-refactor/SKILL.md) - behavior-preserving Rails refactoring toward clearer domain objects, CRUD resources, lifecycle records, and local conventions.
- [`37signals-hotwire`](plugins/37signals/skills/37signals-hotwire/SKILL.md) - Turbo, Stimulus, server-rendered UI, and SPA-resistance decisions for Rails interfaces.
- [`37signals-product-refresh`](plugins/37signals/skills/37signals-product-refresh/SKILL.md) - existing product, screen, workflow, onboarding, or settings refresh without vague redesign or rewrite.
- [`37signals-product-design`](plugins/37signals/skills/37signals-product-design/SKILL.md) - interface-first product UI, frontend flows, copy, empty states, onboarding, and working screens.
- [`37signals-shape-up`](plugins/37signals/skills/37signals-shape-up/SKILL.md) - shaped pitches with appetite, problem, solution sketch, rabbit holes, no-gos, and scope boundaries.
- [`37signals-scope-judgment`](plugins/37signals/skills/37signals-scope-judgment/SKILL.md) - REWORK, Getting Real, and calm-work simplification for product plans, roadmaps, process, and team communication.
- [`37signals-communication`](plugins/37signals/skills/37signals-communication/SKILL.md) - concise product/team writing: pitches, decisions, launch notes, status updates, and async explanations.
- [`dhh-rails-judgment`](plugins/37signals/skills/dhh-rails-judgment/SKILL.md) - DHH-inspired Rails architecture judgment around monoliths, Hotwire, omakase defaults, and conceptual compression.

**Recommended 37signals flows**

- Refresh existing product: start with `$37signals-product-refresh` for stale, bloated, confusing, or outdated screens/workflows. Ask for a Refresh Brief with baseline, pain, appetite, epicenter, what stays, cuts, states/copy, Rails/Hotwire impact, validation, and build/shape/reject recommendation.
- Shape a new bet: use `$37signals-shape-up` for raw ideas, feature requests, spikes, or projects that need a pitch with appetite, rabbit holes, no-gos, and circuit breaker.
- Make broad work smaller: use `$37signals-scope-judgment` when a plan sprawls across dashboards, settings, roles, alerts, meetings, or automation before the core outcome is sharp.
- Build in Rails: use `$37signals-rails-implement` after the product workflow is clear; it routes to recipes for models, resources, migrations, jobs, mailers, tenancy, delegated types, Hotwire, tests, and deployment.
- Review or clean Rails code: use `$37signals-rails-review` for PR/code review and `$37signals-rails-refactor` for behavior-preserving cleanup.
- Decide frontend weight: use `$37signals-hotwire` before accepting SPA or heavy client-side architecture inside a Rails app.
- Ask DHH/Rails doctrine questions: use `$dhh-rails-judgment` only for explicit Rails architecture tradeoffs.
- Write the decision: use `$37signals-communication` for kickoff notes, decision memos, release notes, and async status artifacts.

Example prompts:

- `Use $37signals-product-refresh to refresh this stale onboarding flow without turning it into a rewrite.`
- `Use $37signals-shape-up to turn this raw idea into a bounded pitch.`
- `Use $37signals-rails-review to review this PR through 37signals-inspired Rails conventions.`
- `Use $37signals-hotwire to decide if this interaction needs Turbo, Stimulus, or a heavier frontend.`
- `Use $37signals-communication to write a short launch note for this refresh.`

Shared evidence and caveats live in [`plugins/37signals/references/source-index.yml`](plugins/37signals/references/source-index.yml) and [`plugins/37signals/references/caveats.md`](plugins/37signals/references/caveats.md). Regression cases live under [`plugins/37signals/evals/`](plugins/37signals/evals/) and are validated by `bin/check`.

### Fizzy

This repo standardizes on a CLI-only Fizzy workflow for the hosted instance at `https://fizzy.joshyorko.com`.

Rule: use the installed [`fizzy` CLI](https://github.com/basecamp/fizzy-cli) only. Do not fall back to raw API calls, endpoint probing, or HTML scraping.

Install the CLI from the self-managed tap:

```bash
brew install joshyorko/tools/fizzy-cli-master
```

The formula name stays unique as `fizzy-cli-master`, but the installed executable is still `fizzy`. If you want a repo-local helper for that same Homebrew flow, use:

```bash
bash plugins/fizzy/skills/fizzy/scripts/install.sh
```

Once the CLI is installed, the intended flow is:

```bash
fizzy doctor
fizzy config show
fizzy skill
fizzy setup --api-url "https://fizzy.joshyorko.com"
```

Or, for token-based setup:

```bash
export FIZZY_API_URL=https://fizzy.joshyorko.com
export FIZZY_TOKEN=fizzy_your_token_here
fizzy auth login "$FIZZY_TOKEN" --api-url "$FIZZY_API_URL"
fizzy identity show --api-url "$FIZZY_API_URL" --markdown
fizzy board list --api-url "$FIZZY_API_URL" --limit 5 --markdown
```

Primary docs:

- [Fizzy skill](plugins/fizzy/skills/fizzy/SKILL.md)
- [Fizzy install helper](plugins/fizzy/skills/fizzy/scripts/install.sh)

### RCC

The `plugins/rcc/` distribution is split into focused RCC-family skills.

- [`rcc`](plugins/rcc/skills/rcc/SKILL.md) — router for choosing the right RCC specialist.
- [`rcc-core`](plugins/rcc/skills/rcc-core/SKILL.md) — RCC itself: CLI/source orientation, holotree/cache internals, endpoints, templates, bundles, and remote cache/client behavior.
- [`rcc-robots`](plugins/rcc/skills/rcc-robots/SKILL.md) — RCC CLI, `robot.yaml`, `conda.yaml`, holotree, templates, freezes, bundles, and environment validation.
- [`rcc-workitems`](plugins/rcc/skills/rcc-workitems/SKILL.md) — classic `robocorp.workitems`, `actions-work-items`, producer/consumer/reporter flows, local queues, and custom adapters.
- [`action-server`](plugins/rcc/skills/action-server/SKILL.md) — ordinary Action Server packages, Josh's `actions` community branch, `package.yaml` v2, `sema4ai-actions`, `sema4ai-mcp`, secrets, dev tasks, and OpenAPI/MCP checks.
- [`rcc-ci-maintenance`](plugins/rcc/skills/rcc-ci-maintenance/SKILL.md) — RCC in GitHub Actions, `ROBOCORP_HOME` caching, pinned RCC installs, scheduled maintenance robots, allowlists, and bot PR workflows.

## Quick Start

### Codex Bootstrap

Install this repo once per machine and expose its plugins/skills globally.

macOS/Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/joshyorko/plugins/main/install.sh | bash
```

Pinned macOS/Linux install:

```bash
curl -fsSL https://raw.githubusercontent.com/joshyorko/plugins/main/install.sh | bash -s -- --ref v1.2.3
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/joshyorko/plugins/main/install.ps1 | iex
```

Pinned Windows install:

```powershell
$env:AGENT_SKILLS_REF = "v1.2.3"
irm https://raw.githubusercontent.com/joshyorko/plugins/main/install.ps1 | iex
```

The remote entrypoints prefer a git checkout when available and fall back to verified release archives when git is unavailable. See [docs/codex-bootstrap.md](docs/codex-bootstrap.md) for options, manual fallback, uninstall flow, and devcontainer usage.

### Build Generated Views

After editing anything under `plugins/` or `marketplaces/catalog.json`, rebuild the generated views and marketplaces:

```bash
python3 scripts/build_marketplaces.py
python3 scripts/build_runtime_views.py
python3 scripts/build_hermes_plugins.py
bin/check
```

### Skill Views

The repo emits two generated skill views:

- `skills/` for a flat, agent-agnostic standalone view
- `.agents/skills/` for standard in-repo agent discovery
- `.agents/plugins/marketplace.json` for the repo-local Codex marketplace manifest used by the installer's Codex marketplace registration command

If you're unsure where to start, use `37signals-product-refresh` for existing products/screens/workflows, `37signals-shape-up` for raw bets, and `37signals-rails-implement`, `37signals-rails-refactor`, or `37signals-rails-review` for Rails work.

For Fizzy specifically, install the CLI first with:

```bash
brew install joshyorko/tools/fizzy-cli-master
```

Or use the repo helper for the same tap-driven install:

```bash
bash plugins/fizzy/skills/fizzy/scripts/install.sh
```

Then install the upstream skill from the CLI if desired:

```bash
fizzy skill
```

### Claude Compatibility

The repo now also emits Claude-compatible marketplace metadata:

- `.claude-plugin/marketplace.json`
- `plugins/*/.claude-plugin/plugin.json`

That keeps the same plugin-owned source tree usable from both generic skill tooling and Claude-style plugin ecosystems.

### Hermes Compatibility

The repo also emits Hermes-compatible plugin shims for each plugin:

- `plugins/*/plugin.yaml`
- `plugins/*/__init__.py`

Install a plugin directly from its subdirectory and enable it:

```bash
hermes plugins install joshyorko/plugins/plugins/37signals --enable
hermes plugins install joshyorko/plugins/plugins/fizzy --enable
hermes plugins install joshyorko/plugins/plugins/rcc --enable
```

Hermes plugin skills are explicit-load and namespaced. They do not appear in the flat `hermes skills list` output or the system prompt skill index. Load them with the plugin prefix, for example:

```bash
/skill 37signals:37signals-rails-implement
/skill fizzy:fizzy
/skill rcc:rcc
```

The Hermes shims are generated by `scripts/build_hermes_plugins.py`; do not hand-edit them.

## License

See the relevant upstream projects and individual skill directories for licensing context.
