# Luna Factory planning graph design

The requested outcome is one repository-bound engineering graph operated through model tools and the existing MCP Apps workbench. This slice creates no execution and does not touch the preserved unknown owner or its claim. User authorization covers validation, implementation, tests, reviewable branches and draft PRs; it excludes merge, release, deployment, production configuration and worker dispatch.

## Independently established boundaries

Fetched heads on October 8, 2026:

| Repository/ref | Commit |
| --- | --- |
| plugins PR 60 | `1ccac51591d0466250448531632b82732ddac3fe` |
| plugins main | `5c20e80cc136a982949b516b1672bb331da6de7b` |
| review self-hosted | `dc5d35a42e689a697a1694301d59a4ee7caced99` |
| codex-action-server main | `bf0b3823e033b9b5abd86904e0a565d6b3586206` |
| actions community | `7c98236069171f57031218f938963238986293bd` |

The handoff pins remain current. PR 60 remains draft/unmerged. The Rust Store already owns run snapshots, claim rows, immutable run contracts, compare-and-swap revisions and event fingerprints. Control already owns task admission, dependencies, attempts, current proof and fail-closed execution settlement. Factory methods are the common effect boundary for both MCP audiences. These contracts remain authoritative.

Reject a second graph database or scheduler. Reject simply calling start_factory to obtain a graph: that starts native execution. Reject treating a configured target, executable or rate-limit response as subscription qualification. CAS account/read is not exposed at the inspected head. Review pump/drain is a future behavioral donor; its ownership recovery cannot be transplanted. Actions Work Items remains a separate queue.

## Additive graph architecture

Store planning-only runs in the existing runs/control_events ledger. Their immutable planning_only marker defaults false for old data. Creation validates an approved repository, stores source identity and acceptance, creates candidate tasks without effect authority, and acquires no claim. Planning records cannot enter native start/resume/steer/cancel/reconcile paths. Existing execution runs retain their current behavior.

Project Control.tasks, criteria, attempts and claims into a bounded graph. Repository identity is an opaque digest, with alias, base head and exact current source subject. Imported nodes carry provider, repository identity, stable item identity and source revision. Imports create candidates; they do not confer necessity, READY admission, execution authority or acceptance. Explicit dependencies must reference known nodes and remain acyclic.

Graph changes use persisted proposals and apply operations in the same event journal. Each proposal binds the canonical payload fingerprint, local operator authority, original source and expected revision. Apply rechecks repository authority, source, revision, graph legality and target eligibility. Identical replay returns current state without another event; conflicting identities fail. Graph edits cannot alter attempt identities, budgets, generations, acceptance or held claims. Active/unknown execution cannot be reassigned through planning tools.

Tools: create_factory_graph with the existing bounded StartRequest shape, get_factory_graph, propose_factory_change, apply_factory_change, get_factory_backends. Both model and app use these methods. Source text and model context are data, never authorization. The existing service is a trusted local operator boundary; this slice does not claim multi-user authenticated actor identity. Audit actor is assigned by the server, not accepted from input.

## Host contracts and workbench

Correct native settings input to the documented strict {set:{changed fields}} wrapper. Validate before one atomic read/merge/write; return all effective values. Omit profile when none is configured. Revalidate persisted UI defaults against current operator limits. Declare output schemas for every structured tool response and validate actual results in tests.

Keep graph/node selection local to each App instance. Read modelContext.getCurrent after connection and on host context changes. Validate restored IDs against authoritative graph reads. Publish bounded IDs, revision and node details through negotiated structured/text support. Explicit clear suppresses automatic reattachment; opaque update IDs identify echoes, not ordering. Context does not carry workflow authority. Different app instances have independent context. Complete graphical/headless fallback remains available where extensions are unsupported.

Official contract pin: openai/mcp-extensions `7e1be49daea03d7ec46ed2472f410099db2743d6`, docs/spec.md and typescript/src/app/model-context.ts. Current published SDK 0.1.0 matches the repository. The spec support table is expected support, not a live acceptance result; Web refers to Work browser, excluding classic ChatGPT.

## Backend evidence and limits

Discovery reads configuration only and launches nothing. Native-local is selectable as a planning preference; execution remains unqualified and entitlement unknown. CAS, cloud CLI, newer Cloud and GitHub remain separately identified unsupported routes in this slice. No Agents API fallback. Future dispatch must qualify exact backend, target, workspace, native identity, authentication, subscription entitlement and required operations at its effect boundary.

## Proof and rollback

Test production Factory/SQLite/MCP paths with temporary Git repositories and absent native binary. Test schemas, replay/stale conflicts, source/alias drift, cycles, non-admission, unknown claims, immutable contracts, restart, UI selection/context races and equivalent model/app operations. Run Rust fmt/clippy/tests, UI tests/typecheck/build, package tests and repository checks. A synthetic App host can prove transport and context behavior; it cannot prove actual ChatGPT availability. Live acceptance stays explicitly unperformed without a deployed compatible host.

Keep changes on feat/luna-factory-graph-slice based on PR60. No production migration is run. Preserve the old package/database backup if an operator later deploys; do not roll old code over new metadata without an explicit compatible rollback or restoring a pre-cutover backup with no intervening work. Never replace an unknown active writer.
