# Issue #102 source acceptance

The approved backend repairs and all five UX slices are implemented on
`luna-factory/qualification-gate`, in draft PR #91. This checkpoint does not
authorize merge, release, real qualification, or native inference.

## Backend

- Full security checkpoint: 318 Rust tests passed, 0 failed, 2 live tests ignored.
- Final security clock fixes: formatting and all-target Clippy passed;
  qualification unit tests 8/8 and integration tests 20/20 passed.
- Atomic task edit and canonical-title repair: formatting and all-target Clippy
  passed; graph tests 25/25, MCP contract 15/15, output schemas 5/5 and the
  objective/child title-bound control regression passed.
- Compiled Rust/SQLite/HTTP plus production UI controller integration passed,
  including one nullable-title atomic edit, canonical-title preservation, source
  and revision fencing, restart persistence, and zero native dispatch.
- Independent backend review accepted the security and atomic-edit changes.

`update_task.title: null` preserves the exact server title at the fenced revision.
An explicit string replaces it. `target_id: null` clears the planning preference.
Display titles keep their existing bound; safe editor text is additive, and
withheld text is never replaced with a redacted display placeholder.

## UX

The implementer and independent reviewer inspected all 35 final captures.
Each approved slice received PASS:

1. Tokens, shell, header and initial focus.
2. Plain vocabulary and the banned-copy regression.
3. Home composer, unique plan rows, repository menu and options.
4. Plan primary action, map cards, task edit and Before/After proposal review.
5. Activity, inline cards, exclusive error/loading, offline and mobile states.

The captured UI source SHA-256 is
`9ce6743a5119b8c867837e43f71948c0b53a1d6ab1e74da2c0afb6c65d6bdc28`.
The bundle SHA-256 is
`ef54cd45350a9d1bee2ac0f426d3743ffbdd6fa4bc8b658ef31f494131b9a36c`.
The manifest is `plugins/luna-factory/ui/test/dogfood-visual-snapshots/manifest.json`.

Typecheck and production build passed. Vitest passed 281 tests with one opt-in
integration skipped; the compiled integration was run separately and passed.
The basic-host capture suite passed 35/35 with zero automatic messages, mutation
calls, or clipped controls. These are synthetic MCP Apps host captures, not
authenticated ChatGPT Desktop acceptance. Fit controls are not added in this
checkpoint; the explicit five-slice acceptance criteria remain satisfied.

## Runtime checkpoint and remaining gates

Backend commit `c179e60313850ace5135ef87c2f6f2657d67e369` was deployed and verified
before the UX checkpoint. Health, binary/UI identity, 32-tool discovery, the saved
run and graph, Dakota's existing Codex 0.165.0 transport, current-host MCP calls
and actual OMP 18.13.3 discovery passed. The active planning configuration was
preserved. All hosted checks on that deployed commit passed.

The matching final backend/UI deployment and final-head hosted checks are tracked
in PR #91. Persisted `update_task` records require a compatible runtime; do not
restore an old database backup over newer plans to roll back.

Trusted daemon identity is unavailable in the Codex 0.165.0 initialize response.
Grant recording and ordinary/canary inference remain denied. Campaign promotion
also remains a static refusal. OMP currently exposes the nine app-only tools;
its discovery success is not correct visibility-filtering proof. Live operator
canary evidence, independent native checks, full cessation and authenticated
ChatGPT acceptance remain outside the completed source/fixture evidence.
