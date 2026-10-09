# Luna graph implementation plan

> For agentic workers: use superpowers:executing-plans or subagent-driven-development, with isolated file ownership and one integration owner.

**Goal:** Implement LF-01 through LF-06 as a nondispatching graph operated by MCP and the workbench.

**Architecture:** Extend the existing run ledger and event journal. Planning metadata and candidate imports cannot expand execution authority. Both interaction paths call identical Factory methods.

**Tech stack:** Rust/rmcp/SQLite and TypeScript/MCP Apps.

**Spec:** ../specs/2026-10-08-luna-graph-design.md

## Global constraints

No production state/configuration changes, real worker dispatch, merge, release or deployment. Preserve ownership, generations, budgets, READY gating, idempotency, claims and verification. Actor authority comes from the existing trusted local service boundary. No new scheduler, database, API-billed execution or undocumented Cloud interface.

## Review focus

- Source/alias replacement between proposal and apply must fail without changing held claims.
- Duplicate apply after reply loss must not add another event or native action.
- Imported candidates and planning runs must never enter execution through legacy tools.
- Host clear/remount/late responses must not invent current selection or authorization.
- Concurrent settings patches and UI/model graph edits must retain unrelated data or reject stale revision.

## Tasks and file ownership

1. Integration owner: mcp.rs/http.rs/schemas.rs and host-contract tests. Write failing tests for native settings envelope, unavailable profile and required output schemas; observe failures, implement strict contracts, validate actual response variants. Wire the graph and capability APIs after core types stabilize. Run targeted tests and full gates.
2. Graph agent: graph.rs/control.rs/store.rs/lifecycle.rs/presentation.rs/lib.rs and graph.rs tests. Establish boundaries before edits. Test planning creation, source-bound imports, dependency cycles, revision fences, event replay, unknown ownership retention and no native calls. Implement through existing Store and journal only. Root exclusively edits the settings-method tail of lifecycle.rs.
3. Backend agent: backends.rs and backends.rs tests. Implement capabilities(&Config)->Value and validate_preference(&Config,&str)->Result<()>; distinguish planning eligibility from subscription execution qualification. No credential/filesystem/native probes.
4. UI agent: ui/src, ui/test and generated ui/dist. Test and implement production host bridge, restored/cleared instance context, graph selection/inspection and proposal/apply flow. Use {set} settings and authoritative revision ordering. Graph API types are shared with task 2.
5. Integration owner: review all diffs, run actual MCP + App bridge demonstration against temporary ledger, full suites and fresh independent review. Fix material findings with regression tests. Record exact commits, commands, results and live-host limitations. Refine LF-07 onward and prepare a draft PR stacked on PR60.

Each implementation task follows tests RED, code, tests GREEN, review. One integration owner commits coherent slices after verifying shared interfaces. User explicitly authorized continuous phases; no planning-only approval handoff is required.
