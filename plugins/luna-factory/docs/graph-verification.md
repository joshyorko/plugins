# LF-01 through LF-06 verification

The first delivery slice implements candidate engineering graphs in the existing Rust Factory ledger and MCP Apps workbench. It does not qualify a new execution backend or perform live ChatGPT acceptance. The draft branch is stacked on PR #60; no production configuration, preserved unknown execution, worker, deployment or release was changed.

## Architecture decisions verified before implementation

All four heads were fetched and rechecked on 2026-10-08. The handoff revisions remain current:

| Source | Exact revision |
| --- | --- |
| plugins PR #60 | `1ccac51591d0466250448531632b82732ddac3fe` |
| plugins main | `5c20e80cc136a982949b516b1672bb331da6de7b` |
| review self-hosted | `dc5d35a42e689a697a1694301d59a4ee7caced99` |
| codex-action-server main | `bf0b3823e033b9b5abd86904e0a565d6b3586206` |
| actions community | `7c98236069171f57031218f938963238986293bd` |

- Existing SQLite runs, control events, revision checks, admission, generations, claims and proof remain authoritative. Planning-only records add no second task database or scheduler and cannot dispatch.
- Repository binding includes an immutable opaque identity derived from the canonical root and common Git directory. Source and authority are rechecked at apply. Revoked bindings retain a read-only, unverified historical projection without hiding unrelated held runs or rewriting stored evidence.
- Both interfaces invoke the same persisted proposal/apply operations. The server binds payload fingerprint, expected revision, source subject and the existing trusted local operator actor. This is not a new multi-user authentication system.
- Imports and target preferences create metadata only. They cannot grant READY admission, move active/unknown execution, release a claim, create effects or change acceptance/budgets.
- The existing workbench and native RPC already provide the foundation. The missing work was graph operations and host contracts, not another runtime. Native settings needed its documented wrapper, complete outputs and atomic partial updates; model context needed a production lifecycle implementation.
- Capability discovery reports configuration and unknown entitlement honestly. New adapter execution remains disabled. Persisting backend execution identities, account observation and subscription qualification belong to LF-08; no API-billed fallback was added.

See the independent [host baseline](graph-host-evidence.md), [backend and official-documentation evidence](graph-backend-evidence.md), and current [wire contract](control-wire.md#planning-graph-contract-lf-01-through-lf-06).

## Implemented scope

| Node | Delivered |
| --- | --- |
| LF-01 | Preserved safety regression suite, repository authority/rebinding tests, no-dispatch planning guards, production host contract tests and separate evidence levels. |
| LF-02 | Output schemas for all 20 tools, 13 model-visible tools, strict settings envelope and atomic effective settings. |
| LF-03 | Repository-bound candidate graphs, source revisions, dependencies, proof/attempt/claim projection in the existing ledger. |
| LF-04 | Node inspection and selection, negotiated Model–App Context, validated restore, explicit-clear suppression, stale-response protection and independent instances. |
| LF-05 | Five backend descriptors distinguish advertised/enabled/qualified operations and unknown authentication/entitlement. Only configured native-local is selectable as a planning preference; execution qualification remains deferred. |
| LF-06 | Revision-fenced, fingerprinted, idempotent proposal/apply operations shared by model tools and UI, with audit parity and no execution effects. |

Implementation commit: `5491033cd1ee4fd6a1676a0d0e69e70eae60c01c`.

## Verification

Final command results and environment versions are retained in `graph-verification-results.txt`. Rust fixtures include fake native processes; none uses an authorized production worker. The ignored live-native audit remains ignored. Tests in the other three repositories were inspected as architectural evidence, not run or changed in this effort.

| Gate | Result |
| --- | --- |
| `cargo test --locked` | 150 passed; 1 intentionally ignored live-native audit; 0 failed. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets --locked -- -D warnings` | Passed. |
| `cargo build --locked` | Passed; compiled binary used by the demo. |
| `npm run typecheck` and `npm run build` | Passed; checked-in UI reproduces without diff. |
| `LUNA_GRAPH_E2E=1 npm test` | 118 passed across 10 files; 0 skipped. |
| `python3 -m unittest discover -s plugins/luna-factory/tests -v` | 29 passed. |
| `bin/check` | 53 passed; repository structure validated. |

Environment: Rust/Cargo 1.99.0, Node 24.19.0, npm 11.9.0, Python 3.12.14. Minimum supported Rust 1.88 was not separately exercised.

The settings envelope, missing output schemas, graph mutation annotation, model node-context wiring and service-lock inheritance each had an observed failing regression before the corresponding fix. The lock failure was reproduced deterministically: a child paused between fork and exec retained the inherited open file description after the final Factory was dropped. A final-owner lease guard now unlocks explicitly, while live clones retain exclusivity. No lock-acquisition retry was added.

## Smallest end-to-end demonstration

`ui/test/graph_e2e.test.ts` uses the compiled Rust daemon, temporary SQLite database, isolated clone of this real repository, actual HTTP MCP tools, production controller/renderer/HostBridge and official App/AppBridge SDK transport. Its source node is the captured public [issue #61](https://github.com/joshyorko/plugins/issues/61), with immutable repository/item IDs and a SHA-256 snapshot revision.

The model-filtered tool path creates and reads the graph; workbench selection attaches `issue-61` and its observed graph revision to model context; the workbench proposes and applies `native-local`; model reads observe the identical revised graph and audit. A subsequent model dependency edit is reflected by workbench refresh. Final graph revision is 6, with three applied changes, no attempts and no held claim. The fixture config points at a nonexistent native binary. The [receipt](graph-demo-evidence.json) records the tool sequence and final context/state.

This proves the complete local protocol path using a synthetic host. It does **not** prove that an actual ChatGPT session understood a click, browser pointer interaction, deployed catalog filtering, or desktop/mobile rendering. No live supported host connection is available in this environment, and replacing the preserved live service remains outside this task's authority.

Reproduce from the implementation checkout:

```sh
cd plugins/luna-factory/server
cargo build --locked
cd ../ui
npm ci
npm run build
LUNA_GRAPH_E2E=1 npm test
```

## Rollback and remaining gates

No production state was migrated. Keep any test deployment on a copied configuration and ledger. Before a future authorized rollout, stop only a proven-settled service and take a consistent SQLite backup including WAL state, plus its configuration and binary/UI pair. Do not replace the preserved unknown owner merely to try this slice.

The old binary can read untouched old data, but its closed Control deserializer cannot read newly persisted graph fields. A binary-only rollback after graph writes is therefore unsupported. Restore the matching pre-rollout ledger/configuration and binary/UI pair; retain the newer ledger separately for audit. Do not delete graph fields or rewrite unknown execution evidence to force compatibility.

Live host acceptance and deployed tunnel/Executor catalog pins remain open (20 total tools, 13 model-visible). Desktop/Work web and intended mobile hosts need context restore/removal, settings and navigation acceptance. The host spec's support table is an expectation, not that evidence. Legacy unstamped runs remain readable; new graph edits require a stamped, authorized repository and settled execution.

Next READY engineering work is LF-07, using existing issue #61 and the same-owner bounded continuation boundary, then LF-08 contract/fixture work for CAS. Neither status authorizes a real worker. CAS execution qualification is blocked on exact target identity, subscription evidence, retained callback ownership and reconciliation/cessation. Cloud CLI, newer Cloud and GitHub-mediated execution have separate documented limits; see the [refined dependency graph](delivery-graph.json).

## Hosted CI follow-up

At PR head `973960d36b9c3d3c2d95913af34f284305bb94df`, all four hosted checks passed: Rust and UI in [Luna Factory run 37844622972](https://github.com/joshyorko/plugins/actions/runs/37844622972), and Linux/Windows in [Bootstrap Smoke run 37844623015](https://github.com/joshyorko/plugins/actions/runs/37844623015). These original workflows did not enable the compiled-server demo. The follow-up adds that opt-in test after the release build, using the built release binary and official AppBridge fixture. It still does not claim live ChatGPT acceptance. LF-05's delivery status now explicitly separates verified planning discovery from deferred execution qualification.
