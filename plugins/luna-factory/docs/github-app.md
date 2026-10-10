# Luna's GitHub App: source-bound intake and delivery telemetry

Issues: #76 (issue-graph intake) and #78 (PR, check-run, review and diff telemetry). Parent: #67.

Luna Factory can read GitHub through its own private, read-only GitHub App. This is off by
default. When it isn't configured, both GitHub tools answer `unavailable` with the reason
`github_app_not_configured`, and the workbench shows nothing from GitHub.

What it adds:

- **`inspect_factory_issue_graph {repository, parent}`.** Reads one parent issue and returns
  candidate graph nodes, with nothing written. Model and app.
- **`read_factory_delivery {run_id}`.** Reads PR, check-run, review and diff data for graph nodes
  that have GitHub sources. Model and app.
- **`capabilities.github`.** `{configured, reason}` only.

What it never does:

- Write to GitHub, merge, push, approve, comment or label.
- Write to the Factory ledger. Importing still goes through `create_factory_graph`, then
  `propose_factory_change`, then `apply_factory_change`, with explicit user confirmation.
- Satisfy a criterion or count as proof. It never changes `presentation.result`, actions, attention
  or **Needs you**. Everything returned carries `reported_by: "github"`.
- Show the private key, its path, an app JWT or an installation token to a tool, the UI, a
  receipt, a log or an error string.

## Why a GitHub App, not a PAT or the ChatGPT connector

- **ChatGPT's GitHub connector stays the conversational research tool.** It acts as the person
  chatting, and only while they chat. Luna needs durable, server-side reads it can repeat while
  nobody is chatting, such as polling check runs for a plan.
- **A personal access token** carries a person's identity and usually far more scope than Luna
  needs. Fine-grained PATs are better, but they're still tied to a user, they expire on a person's
  schedule, and they can't be narrowed per request.
- **A GitHub App** has its own identity and its own permission set, chosen in the app's settings,
  and the owner can install it on selected repositories only. Luna mints a short-lived
  installation token for each read and narrows it further: to one repository and only the read
  scopes that read needs.

## Permissions and least privilege

App **5266798** (Client ID `Iv23lipLrpN81zpCD7kA`) is registered with these repository
permissions, all read-only. Webhooks are off.

| Permission | Level | Why Luna needs it | Requested by |
| --- | --- | --- | --- |
| Metadata | read | Required by GitHub for every app. Repository identity. | intake, delivery |
| Issues | read | Parent issue, sub-issues, "blocked by" relations, task-list references | intake, delivery |
| Pull requests | read | Closing pull requests, state, draft, head SHA, review decision, diff stats | delivery |
| Checks | read | Check runs on the PR head commit | delivery |
| Commit statuses | read | Combined status and status contexts | delivery |
| Actions | read | Registered, but **Luna never requests it**. Check runs cover CI state, and workflow logs aren't read. You may remove it from the app. | none |

Defense in depth, enforced in code and tested against a loopback fake GitHub:

1. **Local eligibility first.** No request (no JWT, no installation lookup) is made unless both of
   these are true. Otherwise the result is `ineligible` with the reason.
   - The repository is in `github_app.allowed_repositories`.
   - An approved Luna repository (in `repositories`, or an approved registration) has a Git remote
     naming the same `owner/repo` on GitHub. The remote is read with `git config` from the
     approved root.
2. **Read-only installation.** `GET /repos/{owner}/{repo}/installation` must return an installation
   whose permissions are all `read`. Otherwise: `github_app_not_read_only`.
3. **Pinned installation.** If you set `github_app.installation_id`, the installation that covers the
   repository must match it. Otherwise: `github_installation_mismatch`.
4. **Narrowed tokens.** Every `POST /app/installations/{id}/access_tokens` body is
   `{"repositories":["<name>"],"permissions":{...read scopes for this call...}}`. The response must
   grant exactly that one repository and no permission beyond those requested, all `read`.
   Otherwise the token is discarded: `github_token_scope_mismatch`.
5. **Tokens live in memory only.** They're cached per repository and scope, and refreshed five
   minutes before they expire. They're never persisted, serialized or logged.
6. **The app JWT is RS256.** `iat` is set 60 seconds in the past, `exp` is 9 minutes ahead (10
   minutes in total), and `iss` is the app ID. The key is reread for each JWT, so rotating it
   needs no restart, and it is never cached.

**Install scope.** You can install the app on "All repositories" or "Only select repositories".
Only select is recommended: it lets GitHub enforce the boundary too. Luna narrows every token to
one allowlisted, Luna-approved repository either way, so an all-repositories installation doesn't
widen what Luna reads.

## Operator runbook

1. **Generate a private key** in the app's settings: *Private keys → Generate a private key*.
   GitHub downloads a PKCS#1 PEM (`-----BEGIN RSA PRIVATE KEY-----`), which is the format Luna
   accepts. PKCS#8 and encrypted keys are refused with `github_app_key_format_unsupported`.
2. **Place the key in the operator config directory**, never in a repository or the state
   directory. For the OCI preview that's `LUNA_CONFIG_DIR`, mounted read-only at `/run/luna-config`.
   ```sh
   install -m 600 ~/Downloads/<app>.private-key.pem "$LUNA_CONFIG_DIR/luna-github-app.pem"
   ```
   - The file must be a regular file, not a symlink, owned by the service user (the container
     runs as `65532:65532` with `keep-id`). It must not be group- or world-readable, otherwise:
     `github_app_key_permissions_too_open`.
   - Don't commit it, paste it into chat or pass it as an environment variable.
3. **Install the app** on the repositories Luna should read. "Only select repositories" is
   recommended.
4. **Set `github_app` in `operator.json`.** The key is referenced by an absolute path inside the
   service's view of the filesystem.
   ```json
   "github_app": {
     "app_id": 5266798,
     "installation_id": 170065829,
     "private_key_path": "/run/luna-config/luna-github-app.pem",
     "api_base": "https://api.github.com",
     "allowed_repositories": ["joshyorko/plugins"]
   }
   ```
   - `api_base` is optional and defaults to `https://api.github.com`. It must be an HTTPS origin
     with no path. GHE.com origins such as `https://api.<tenant>.ghe.com` work. GHES `/api/v3`
     paths aren't supported. Plain `http://` is accepted only for a literal loopback IP with a
     port, which exists for the recorded-fixture test server.
   - `installation_id` is optional. When it's unset, the installation is resolved at runtime with
     the app JWT.
   - `allowed_repositories` takes 1 to 32 entries of `owner/repo`, compared case-insensitively.
   - Unknown fields are rejected. `container/operator.example.json` deliberately doesn't include
     this block. JSON has no comments, and adding it would enable the integration by default.
5. **Approve the local repository in Luna** as usual: an operator `repositories` entry or an
   approved registration. Its Git remote must name `github.com/owner/repo`, for example
   `origin https://github.com/joshyorko/plugins.git`.
6. **Choose the egress option** (see below) and restart the service. Restarting is an operator
   action, and this change doesn't perform it.
7. **Verify.** `get_factory_capabilities` reports `github: {configured: true, reason:
   "configured_reachability_unverified"}`. Reachability is reported per read. Then ask ChatGPT to
   inspect a parent issue, or run the live check below.

### Egress: an owner decision is required

`container/compose.yaml` puts the service on an `internal: true` network, so the OCI container has
**no egress**, and this change deliberately doesn't alter that. Configured inside the container
today, every read reports `github_unreachable`. Pick one option:

- **Option A: allow egress to `api.github.com` only.** For example:
  - Add an egress proxy on a second, non-internal network that permits only `CONNECT
    api.github.com:443`, and set `HTTPS_PROXY` for the service. Luna honours the standard proxy
    variables for HTTPS origins only. The proxy sees the hostname but not the TLS-protected token.
  - Or apply host firewall rules limited to GitHub's published API ranges.

  Either way it's a compose and network change the owner must review. Luna itself still refuses
  redirects and non-HTTPS origins.
- **Option B: run the reader on the host.** Configure `github_app` only on a host-run
  `luna-factoryd`, the local service, which already has the network. The OCI container stays fully
  internal and keeps reporting `github_app_not_configured`. This needs no container change, but
  intake and telemetry then live on the host service's ledger.

Until the owner chooses, leave `github_app` unset in the OCI `operator.json`.

## `inspect_factory_issue_graph` (#76)

Input: `{repository: "owner/repo", parent: <issue number>}`. Read-only, model and app.

It reads:

- The parent issue.
- Its sub-issues, via the GitHub sub-issues API: 25 per page, at most two pages.
- For each candidate, its **blocked-by** relations from the issue dependencies API. These are
  skipped when GitHub's `issue_dependencies_summary` reports none.
- **Task-list references** (`- [ ] #12`, `owner/repo#3`, issue URLs) in the parent body, within
  the same repository. At most 16 are fetched.

It returns:

- **`nodes`.** At most 32, open issues only, `id: "issue-<n>"`, with `title`, `dependencies`
  (only edges between candidates) and `relation` (`sub_issue` or `task_list`). Each also has:
  - `source.provider`: `"github"`.
  - `source.repository_id`: `null`, because the graph identity is assigned at import.
  - `source.repository`: `owner/repo`.
  - `source.repository_node_id`: the repository's node ID.
  - `source.item_id`: `"<numeric repository id>:<issue node id>"`.
  - `source.revision`: `sha256:` of the bounded issue fields (number, node ID, title, state and
    the first 64 KiB of the body).
  - `source.display`: `{number, url}`.
- **`acceptance_candidates`.** List items under an "Acceptance" heading in the parent or a
  candidate body, labelled `derived_from_issue`. Text is redacted with the same `safe_summary`
  rule as other summaries: items that look like credentials are dropped.
- **`cycles`.** Strongly connected dependency groups, each `status: "rejected"`. Edges are never
  dropped silently. `import.ready` is false until the user decides which edge to break.
- **`external_dependencies`.** Blockers in the same repository that aren't candidates, for
  example closed issues. Not imported.
- **`cross_repository`.** Sub-issues, task-list references and blockers in other repositories.
  Reported, never imported.
- **`omitted`.** Closed issues, pull requests, deleted or moved issues (`issue_unavailable`,
  `issue_moved`) and anything over a limit.
- **`truncated`.** Flags for `sub_issues`, `task_list`, `dependencies` and `nodes`.
- **`eligibility`.** `{eligible, allowed_by_github_app, approved_aliases, reasons}`.
- **`import`.** `{ready, blockers, decisions_needed, steps, repository_id,
  confirmation: "explicit_user_confirmation_required"}`.

**"luna yolo owner/repo#N"** means:

1. ChatGPT calls `inspect_factory_issue_graph`.
2. It summarises the nodes, cycles, external and cross-repository references, and derived
   acceptance, and it names the decisions needed.
3. It then proposes the import:
   1. `create_factory_graph` on an `approved_aliases` entry, with acceptance the user confirms.
   2. `propose_factory_change` with `import_candidates`. Each source becomes
      `{provider, repository_id: graph.repository.identity, item_id, revision}`, and `display` is
      dropped.
   3. After the user confirms, `apply_factory_change`.

It's never an implicit apply. The existing graph rules still hold:

- `foreign_graph_source` when `repository_id` isn't the graph identity.
- `duplicate_graph_source` when the same GitHub item is imported again, at any revision.
- Exact retries replay.

## `read_factory_delivery` (#78)

Input: `{run_id}`. Read-only, model and app.

It reads graph nodes whose source provider is `github`. The node's GitHub repository is resolved
from the run's approved repository remote, intersected with the allowlist. The numeric repository
ID in `item_id` must match the token's repository, otherwise: `source_repository_mismatch`.

There is one GraphQL query per node: `node(id)` on the issue, with its `closedByPullRequestsReferences`
(up to 5, including closed). For each pull request it returns:

- `state`: `open`, `closed` or `merged`.
- `draft`, `head_sha` and `review_decision` (GitHub's computed decision).
- `diff`: `{files_changed, additions, deletions}`.
- `checks`:
  - `rollup`: GitHub's combined state.
  - Up to 25 contexts, split into `runs` (name, status, conclusion, `started_at`, `completed_at`,
    and a bounded, redacted title or summary) and `statuses`.
- `ready_for_review`.
- `merge_authority: false`.
- `observed_at` and `reported_by: "github"`.

The result also carries `proof: "none"` and `merge_capability: "none"`.

- **Ready for review** means:
  - the PR is open,
  - it isn't a draft,
  - GitHub's combined rollup is `success`.

  The rollup covers every reported check, so it's stricter than "required checks green". Reading
  which checks are *required* needs administration permission, which the app deliberately lacks.
  It implies nothing about merge. There's no merge capability, and "Ready to integrate" stays a
  separate, unimplemented state (#81).
- **Bounded polling.** Each node is fetched at most once per 60 seconds, including after a
  failure. Each call allows at most 16 node fetches, 24 requests and 30 seconds, and nodes beyond
  that are `deferred`. The cache is in memory and is bounded to 512 entries. It's not written to
  SQLite, so a restart forgets it, and it can never enter control, criteria or presentation.
- **Freshness.** Each node is `fresh`, `cached` or `stale`:
  - `stale` keeps the last observation and its `observed_at` after a failed refresh.
  - `unavailable` and `deferred` carry a stable `reason`.

  The workbench computes the age from the server's read time.
- **Possible gap.** If GitHub requires `contents: read` for commit rollups through GraphQL,
  rollups come back empty and the node is marked `partial`. Luna deliberately doesn't request
  `contents`. The live check below shows which case applies.

## Bounds

| Limit | Value |
| --- | --- |
| Request timeout / connect timeout / per-call deadline | 10 s / 5 s / 30 s |
| Response size | 2 MiB, streamed and checked |
| Redirects | None followed (`github_redirect_refused`); server-supplied `Link` URLs are never followed |
| Intake requests per call | 64 |
| Sub-issue pages | 2 × 25 |
| Task-list fetches / candidate nodes / reported items | 16 / 32 / 64 |
| Acceptance candidates | 8 per issue, 64 total, 1000 chars each |
| Delivery fetches per call / requests per call | 16 / 24 |
| Delivery minimum interval per node | 60 s |
| Pull requests per node / check contexts per PR | 5 / 25 |
| Rate limits | `403`/`429` with `x-ratelimit-remaining: 0`, `retry-after` or GraphQL `RATE_LIMITED` become `rate_limited` with `retry_at`, capped at one hour; no request is sent before `retry_at` |

## Reasons

| Reason | Meaning |
| --- | --- |
| `github_app_not_configured` | No `github_app` block. Default. |
| `github_app_config_invalid`, `github_client_unavailable` | The block failed validation at use, or the HTTPS client couldn't be built |
| `github_app_key_unreadable`, `github_app_key_not_regular_file`, `github_app_key_permissions_too_open`, `github_app_key_invalid`, `github_app_key_format_unsupported` | Fix the key file (see step 2) |
| `repository_not_in_github_app_allowlist`, `repository_not_approved_in_luna` | Ineligible. No request was made |
| `github_unreachable`, `github_timeout` | No route to GitHub, for example the OCI internal network (see Egress) |
| `rate_limited` | Wait until `retry_at` |
| `github_app_not_installed`, `github_installation_mismatch`, `github_app_not_read_only`, `github_token_scope_mismatch` | Installation or permission problem. Fix it in the app settings |
| `github_authentication_failed`, `github_permission_denied`, `github_not_found`, `github_gone`, `github_redirect_refused`, `github_service_unavailable`, `github_invalid_response`, `github_response_too_large` | Request-level failure |
| `parent_issue_unavailable`, `parent_is_pull_request`, `parent_issue_moved`, `issue_unavailable`, `source_repository_mismatch`, `source_not_an_issue`, `unrecognized_github_item_id`, `no_github_sources`, `polling_budget_deferred` | Item-level outcome |

The UI maps these to plain sentences and never shows the raw code.

## Verification

- Recorded fixtures only. `server/tests/github_app.rs` runs a fake GitHub API on loopback.
  - Shapes come from read-only `gh api` calls against joshyorko/plugins: issue #67's sub-issues
    and PR #68's rollup, in `server/tests/fixtures/github/`.
  - Throwaway RSA keys are generated per test run with `openssl`, and none is committed.
  - It covers JWT claims, single-repository token bodies, token caching and expiry, pagination
    limits, sub-issues, task lists, cycles, cross-repository and deleted issues, revision changes
    and idempotent re-import, rate limiting, the unconfigured, unreachable, unsafe-key, mismatch
    and write-permission states, redaction, and the contract that GitHub "success" never changes
    criteria, presentation result, actions or attention.
- Opt-in live check, run by the integrator. It's read-only, uses a disposable in-process server
  and prints only counts and statuses:
  ```sh
  cd plugins/luna-factory/server
  LUNA_GITHUB_LIVE=1 LUNA_GITHUB_KEY=/absolute/path/to/luna-github-app.pem \
    cargo test --locked --test github_live -- --ignored --nocapture
  ```
  It needs network access to `api.github.com` from where it runs, which means Option B or a host
  shell. It never prints the key, the JWT or tokens.

## Not supported in this slice

- Webhooks.
- Persisted telemetry.
- GHES path-prefixed APIs.
- PKCS#8 or encrypted keys.
- Required-check configuration.
- The "protected-path hits" idea from #76's scope: there is no protected-path policy source to
  check against yet.
- Any write.
