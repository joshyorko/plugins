# Contract: pick a GitHub repo or start a new one (owner decision, 2026-10-14)

The owner wants a simple flow: open Luna, pick a repo (from the GitHub App they already installed, or a brand new local one) and say what they want. The App exists: app_id 5266798, installation 170065829 on the owner's personal account (joshyorko), currently "All repositories", read-only (actions, checks, statuses, issues, metadata, pull requests). The owner chose the GitHub App route and will add **Contents: read** to the App (they do that in GitHub; nobody else touches App settings or keys).

## Three new tools (all `_meta.ui.visibility: ["model","app"]`)

1. `list_factory_github_repositories`: read-only (`readOnlyHint:true, destructiveHint:false, idempotentHint:true, openWorldHint:true`).
   - Input: `{cursor?: string(1..512)}`. `additionalProperties:false`.
   - Output: `{schema_version:1, reported_by:"github", provenance:"github_app", configured:boolean, available:boolean, reason:string|null, repositories:[{full_name:"owner/repo" (lowercase), private:boolean, default_branch:string|null, description:string|null (safe_summary, max 280), connected_alias:string|null, connectable:boolean, blocked_reason:string|null}] (max 100), next_cursor:string|null, observed_at:integer}`.
   - Lists repos visible to the installation (`GET /installation/repositories`), marks the ones already connected and whether each can be connected (allowlist, permission).
2. `connect_factory_github_repository`: write (`readOnlyHint:false, destructiveHint:false, idempotentHint:true, openWorldHint:true`).
   - Input: `{repository:"owner/repo", idempotency_key:string}`.
   - Output: `{schema_version:1, status:"connected"|"already_connected", alias:string, repository:{alias, identity (64 hex)}, full_name, default_branch, provenance:"github_app"}`.
   - Clones the repo into the operator-configured workspace root and registers it as an approved repository alias, so it shows up in the plan form's repository picker immediately. No push, no branch, no PR, no execution.
3. `create_factory_local_repository`: write (`readOnlyHint:false, destructiveHint:false, idempotentHint:true, openWorldHint:false`).
   - Input: `{name: "^[a-z0-9][a-z0-9._-]{0,63}$", idempotency_key:string}`.
   - Output: `{schema_version:1, status:"created"|"already_exists", alias, repository:{alias, identity}}`.
   - Greenfield: `git init` plus one empty initial commit in the workspace root, registered as an approved alias. No GitHub call.

Catalog after these: **32 tools, 23 model-visible, 9 app-only.**

## Operator config (operator.json)

- `github_app.allowed_repositories` additionally accepts `"owner/*"` (all repos of that owner that the installation can see).
- New optional `workspaces: {root: "/abs/path", connect_github: boolean, create_local: boolean}`. Both flags default to false. When a flag is false, the matching tool answers with a typed reason (`github_connect_not_enabled` / `local_create_not_enabled`) and does nothing. Turning a flag on is the operator's approval. A repo connected or created this way is approved with recorded provenance (`github_app_installation` or `local_create`), shown in the UI as "Connected from GitHub" or "Created in Luna".
- `capabilities.repositories: {connect_github:boolean, create_local:boolean}` (no paths).

## Safety rules (unchanged invariants)

- Tokens: installation token narrowed to the one repository and `contents: read` only. Pass it to `git` through environment variables only (`GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` with `http.extraHeader`), never on argv, never in `.git/config`, never in logs, errors or tool output. After cloning, the remote URL is the plain `https://github.com/owner/repo.git`.
- Clone target is `<root>/<owner>__<repo>`. Refuse it if it exists and isn't the same repo (`workspace_path_conflict`). No symlinks. Shallow clone is fine (`--depth 1` default branch).
- GitHub data is never proof. Connecting or creating never starts a run, never dispatches native work and never qualifies execution (that is #71).
- Never read, print or copy the owner's private key. Tests generate throwaway keys.
