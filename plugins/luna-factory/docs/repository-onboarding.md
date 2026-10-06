# Add a local repository

Repository access remains trusted operator policy. Safe defaults cannot add an
alias or increase finish authority. Luna Factory does not clone repositories or
use a GitHub connector.

An operator can add dedicated local discovery roots to the private JSON config:

```json
"discovery_roots": {
  "acceptance": {
    "root": "/absolute/path/to/disposable-repositories",
    "max_finish": "local_candidate"
  }
}
```

The root must be an existing canonical directory. The state database must remain
outside it. Prefer a dedicated parent of the repositories you want to offer;
do not use a home directory or a filesystem root. Roots are optional and existing
explicit `repositories` entries continue to work.

## Workbench flow

Choose **Add repository** from the empty state or **Settings**. The app reads the
bounded discovery catalog, then lets you choose a candidate, alias and explicit
finish authority. It sends an opaque candidate ID, never a local path. Choosing
**Request access** persists a pending request. It does not authorize a run.

The operator reviews requests on Dakota and approves the exact request locally:

```bash
luna-factoryd repository-requests --config /absolute/path/to/private/operator.json
luna-factoryd approve-repository --config /absolute/path/to/private/operator.json --request-id REQUEST_ID
```

Return to **Add repository**, then choose **Refresh repositories**. The approved
alias becomes available in **New run** without restarting the service. Explicit
operator-configured aliases take precedence. The original database stores requests
and approvals durably, so disconnect and restart do not reset access decisions.

Local approval is deliberate: app-only visibility and destructive annotations
guide compatible hosts but do not authenticate a human approval to the daemon.
This custom MCP connection has no proved, enforceable host approval contract.
The minimum safe implementation therefore keeps the grant in a local-only CLI,
instead of trusting a remote `approved: true` field or inventing host metadata.
The request tool is app-only, marked as a mutation, and rejects paths and approval
fields. There is no approval or trust-root mutation tool in MCP.

## Bounds and revocation

Discovery does not invoke Git, read repository contents, follow symlinks, access
credentials, clone, start a native process or perform inference. It examines
ordinary directory and Git marker metadata under at most eight roots, four
directory levels and 2,048 entries per request. The catalog and durable registry
each hold at most 100 repositories. Exceeding a bound fails closed. Hidden
directories, `node_modules`, `target`, bare repositories and linked-worktree
`.git` files are excluded. Operators can continue to configure worktree aliases
explicitly through the existing `repositories` contract.

Approval rechecks the candidate, directory identity and current root finish cap.
Replacing a candidate, changing its Git directory, removing/remapping its root,
or changing the root cap invalidates its dynamic alias. Admission and resume use
the same current policy. Approved aliases cannot override explicit operator aliases
or increase their authority. Neither a pending request nor a stale approval grants
access to a changed location. Names shown in the app are relative to a trusted
root; absolute roots and filesystem identities remain private.

## Branding

The package includes the workbench crescent as `assets/logo.svg` and a 512 by 512
PNG at `assets/logo.png`. Both canonical OpenAI `logo` and `composerIcon` fields
reference the PNG; generated metadata and staging preserve those references and
files. The server advertises that same PNG as a standard self-contained `data:`
icon in `serverInfo.icons`, supported by rmcp 3.5.0 and MCP 2025-11-25 and later.
No public image server or loopback URL is needed by the remote host.

Packaged plugins carry branding automatically. Custom MCP hosts may ignore server
icons or retain a cached icon. Refresh or recreate the custom connection to repeat
discovery; if the host still does not render standard server icons, a one-time
manual icon may be necessary. Server metadata does not prove host rendering.
