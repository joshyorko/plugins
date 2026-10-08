# LF-07 bounded continuation verification

LF-07 implements issue #61's bounded same-owner continuation in the existing Rust Factory. It is stacked on the draft graph slice. No native worker, production configuration, preserved unknown execution, deployment, release or merge was used for this work.

## Contract

An explicit owner report directive requests one next task or one same-task repair. The live completion boundary verifies cessation, current source and repository stamp, original authority, necessary task admission, dependency proof, claim ownership, unknown effects, deadline and finite attempt/repair budgets. Routine unattempted READY work does not consume repair allowance. Repair requires a current independently observed failure bound to the latest task/attempt and a server-labelled diagnosis. The original goal, acceptance, deadline, intent lineage and budgets are retained.

A valid report still needs semantic judgment from the owner; the Factory neither invents necessity nor turns a passed process into accepted work. It does not scan GitHub for new tasks or dispatch planning-only graphs. CAS/Cloud qualification remains separate.

Each new attempt is recorded before contacting the same native owner. Direct startup and reconciliation reads cannot replay a continuation directive. After read-only correlation reattaches an observer to an active dispatch, a genuinely subsequent live completion can request a new attempt under all guards. Genuine decisions still pause; exact answer retries return the same run without duplicate inference. Unknown effects/liveness or acknowledgement retain the claim.

See the [wire contract](control-wire.md#bounded-owner-continuation-lf-07) and [design](../../../../docs/superpowers/specs/2026-10-08-luna-continuation-design.md).

## Exact source and tests

Implementation: `37e513795127289796fe4032c5578671d4d21229`. Base: PR #62 context-fix head `84856562e9a0aaac4dbedf82fe60c44d38333188` (on PR #60 `1ccac51591d0466250448531632b82732ddac3fe`).

| Gate | Result |
| --- | --- |
| `cargo test --locked` | 163 passed, 0 failed; one live-native audit intentionally ignored. |
| Dedicated continuation integration / policy | 9 integration and 4 policy/schema tests passed, included above. |
| Rust formatting, Clippy `-D warnings`, build | Passed. |
| `npm run typecheck`, `npm run build` | Passed; bundled UI reproduces without diff. |
| `LUNA_GRAPH_E2E=1 npm test` | 123 passed across 10 files, no skips, against the compiled current daemon. |
| Packaging tests | 29 passed. |
| `bin/check` | 53 passed; repository structure validated. |

[Full command output](continuation-verification-results.txt) retains exact test names and transcript hashes. Environment: Rust/Cargo 1.99.0, Node 24.19.0, npm 11.9.0, Python 3.12.14. Rust minimum 1.88 was not separately exercised.

The initial two-task integration regression failed because generation 2 never started after the explicit next directive; it passed after implementation. The context-clear fix separately reproduced both structured/text failures before passing its new ordering tests. Independent review added malformed-report and diagnosis-invalidation checks before final acceptance.

The dedicated `fake_continuation.py` is a labelled subprocess transport fixture. It records native-shaped calls and persistent dispatch IDs, emits notifications before acknowledgements, and simulates lost replies and uncertain liveness. None of its results proves real Codex execution or actual ChatGPT acceptance. Existing Rust cancellation, child/process cessation, routing, claim and recovery regressions remain part of the required suite.

The implementation was independently reviewed for claims, generations, source/evidence binding, strict native schema, replay safety and event-listener lifetime. Review found malformed blocker/nullable-diagnosis cases; the final guards reject them. Manual legacy continuation remains available, while automatic work requires the new immutable repository stamp.

## PR #62 audit actions

- `1045ebb1407eaf995459e62c9b48fdcb6008d832` enforces the compiled graph demo in hosted CI and marks LF-05 execution qualification deferred. All four checks passed on that head.
- `84856562e9a0aaac4dbedf82fe60c44d38333188` fixes the reproduced in-flight context-clear race. Local typecheck/build and 123 UI/host/demo tests passed. A post-acknowledgment bare null remains an uncorrelated removal, as documented in the host contract.
- Both changes remain in draft PR #62; LF-07 is a separate stacked draft.

## Remaining acceptance and rollback

Live ChatGPT context/interaction qualification and a real authenticated execution canary remain unperformed. They require an available supported host and separate authorization that preserves the unknown owner. Fixture assertions, hosted CI and the host SDK cannot substitute for those receipts.

Use isolated copied ledgers for any later evaluation. Existing rollback guidance still applies: new graph/diagnosis records require a matching ledger and binary/UI pair; an older binary alone is not a supported rollback. LF-08 CAS contract and fixture integration is the next READY implementation node; target entitlement, durable callback ownership, exact identities and cessation remain execution-qualification gates.
