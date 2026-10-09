# Luna Factory stack: integration and merge readiness

Assessment date: October 8, 2026. **Feature development is frozen after bounded LF-08.** Code acceptance and live-deployment acceptance are separate. No merge, tag, deployment, production configuration change, real worker dispatch or preserved-owner reconciliation was performed.

**Maintainer update, October 9, 2026:** the maintainer explicitly authorized finishing release-blocking fixes, merging #60 → #62 → #63 → #64 after exact-source checks and independent review, publishing the installable `0.2.0` package from `main`, and coordinating safe Dakota/Cutover activation. This supersedes the historical approval and draft-retention restrictions in this assessment; see the [audited decision in #59](https://github.com/joshyorko/plugins/issues/59#issuecomment-6089593813). It does not broaden the Factory owner's runtime authority or waive execution ownership, backup, callback, host-acceptance, security, or CI gates. The October 8 evidence below remains a historical snapshot; release and activation receipts must identify their actual later source and outcomes.

## Stack and code-merge gates

| PR | Exact implementation/head inspected | Base | Exact-head evidence |
| --- | --- | --- | --- |
| [#60](https://github.com/joshyorko/plugins/pull/60) | `1ccac51591d0466250448531632b82732ddac3fe` | main `5c20e80cc136a982949b516b1672bb331da6de7b` | 4/4 successful: [Rust/UI](https://github.com/joshyorko/plugins/actions/runs/37526894923), [Linux/Windows](https://github.com/joshyorko/plugins/actions/runs/37526895181). |
| [#62](https://github.com/joshyorko/plugins/pull/62) | `84856562e9a0aaac4dbedf82fe60c44d38333188` | #60 exact head | 4/4 successful: [Rust/UI](https://github.com/joshyorko/plugins/actions/runs/37846183865), [Linux/Windows](https://github.com/joshyorko/plugins/actions/runs/37846183743). |
| [#63](https://github.com/joshyorko/plugins/pull/63) | `d97e8f263bed9b710301d1d9804611d3e2df39b7` | #62 exact head | 4/4 successful: [Rust/UI](https://github.com/joshyorko/plugins/actions/runs/37846900883), [Linux/Windows](https://github.com/joshyorko/plugins/actions/runs/37846900870). |
| [#64 / LF-08](https://github.com/joshyorko/plugins/pull/64) | Code `35fdf6e714d5ae606579fdfd884b9fb8ecee35f0`; subsequent documentation head must be checked separately | #63 exact head | 4/4 successful on code head: [Rust/UI](https://github.com/joshyorko/plugins/actions/runs/37850336021), [Linux/Windows](https://github.com/joshyorko/plugins/actions/runs/37850336003). Final exact-head results are reported in the delivery response; do not substitute these earlier runs for a later head. |

All remain open drafts. Exact ancestry was checked with Git merge bases, not inferred from PR labels. [Machine-readable CI evidence](stack-ci-evidence.json) retains check names, exact head SHAs, URLs and merge trees for this implementation snapshot; every CI merge tree equals its PR head tree. The remote LF-08 implementation tree `1138cb24641518b501046079411d3ab49679dd98` equals the locally tested tree; publication through the connected GitHub API changed commit metadata, not content. The original local commit `2edd2307410d1f678a9b205fabaa1427d58c6ef0` is retained on a local evidence branch.

The merge recommendation is **conditional code readiness**: accept these as a reviewable stack after final-head checks and maintainer review, not as a production qualification claim. No submitted human approval was present in the inspected review lists. Keep draft status until the owner chooses to advance it. Merge order, if explicitly approved later, is #60 → #62 → #63 → #64. After any base merge, squash/rebase or retarget, verify the remaining diff, ancestry and resulting CI again.

## Contract and data compatibility

The existing Rust control plane and SQLite ledger remain authoritative. No second task database, scheduler or Cloud orchestrator was added. LF-01–06 implement planning graph operations and UI/context synchronization; LF-07 continues bounded, admitted native tasks on the same owner. Planning target preferences do not select an execution backend. LF-08 adds opt-in CAS reads and an internal durable planning seam; it cannot start or continue CAS work.

The final model/app catalog has 21 tools, with concrete output schemas. Existing tool request shapes remain compatible; `inspect_factory_cas` is additive and the existing UI ignores additive backend alias metadata. SDK versions stay pinned. Context-clear ordering is covered by fixtures, but a later uncorrelated bare-null notification still means removal. Actual host behavior requires the canary below.

SQLite schema remains version 2. Schema 0/1 opening migrates transactionally; raw payload journal hashes are checked before deserialization/default insertion. PR62 adds planning/graph fields; PR63 adds `observed_failure` diagnosis semantics; LF08 adds a default-empty, skip-empty CAS map and optional operator configuration. Regression tests open a pre-CAS raw payload/journal without rewriting its bytes/hash, preserve immutable prepared records across reopening, and leave an unrelated unknown held run/claim unchanged during CAS reads.

**Binary-only downgrade is not supported.** PR60's closed Control decoder cannot read newly written graph fields. PR62 does not understand PR63 failure-diagnosis semantics. PR63 cannot read nonempty LF08 CAS records or `cas_targets` configuration. An unchanged pre-CAS ledger/config remains readable; this is not permission to run an older daemon over upgraded state.

Rollback requires a matching binary, UI, skill, configuration and consistent SQLite backup, including committed WAL contents. Use SQLite backup or a verified quiescent snapshot; do not copy just a live `.sqlite` file. Preserve newer state separately and never overwrite intervening runs/claims with an old backup. Test migrations only against disposable consistent copies with a nonexistent native binary/socket. Even `status` calls `Store::open` and may migrate; it is not an untouched-file audit. **Never serve a copied production ledger:** startup can contact copied owner IDs and attach deadline watchers. A runtime preview needs a new empty ledger.

An [offline three-binary rollback rehearsal](rollback-verification.json) passed using archived PR60, PR63 and current LF08 binaries. Genuine PR60 schema-2 records open unchanged under all three; a genuine PR63 graph opens unchanged under LF08 but is rejected by PR60. PR63 rejects both LF08 CAS configuration and a nonempty synthetic CAS record with a matching latest journal hash. Restoring a consistent pre-CAS backup to a separate database is accepted unchanged by PR63 and LF08. The synthetic row tests compatibility, not dispatch. Restoration covered a disposable history with no intervening work; it does not authorize overwriting production history. Exact binary hashes and expected errors are retained in the evidence. All fixture services stopped and the tested LF08 binary was restored byte-identically.

## Isolated acceptance already performed

- Real compiled Rust service, temporary SQLite and actual MCP transport with the workbench host-contract fixture: graph import, node selection, context projection, revision-fenced target preference and reverse-direction dependency update agree on authoritative state. No attempts, dispatches or claims. This uses a captured issue #61 snapshot, not live GitHub ingestion or actual ChatGPT.
- Native protocol fixture: two necessary tasks continue without external Resume on one owner; original deadline, authority and lineage remain fixed. Independent scoped proof, repair, decision, stale source, lost acknowledgment and unknown effects/liveness regressions pass. This is not real Codex inference.
- CAS HTTP fixture: durable request reopening, exact route/body/schema checks, all receipt states, identity/source drift, malformed responses and immutable records. Missing or completed receipts never settle the graph or release a claim.
- Compiled Factory MCP → real Actions Runtime 1.0.1/CAS pinned source: target inspection succeeds against a new empty disposable ledger and absent native socket. It reports execution false and identity/entitlement blockers. The host is a Python MCP client, not ChatGPT. See [LF-08 evidence](cas-verification.md).

## Smallest safe real dogfood procedure

This is a procedure for a separately authorized canary, not an instruction to activate services now. It requires no changes to the preserved instance, owner, claim, ledger, tunnel mapping or production configuration.

1. Stage the tested binary/UI/canonical skill in a new directory. Allocate a private temporary state directory, new empty database, unused loopback port and disposable Git repository. Cap finish at `local_candidate`, capacity at 1, repair at 0 initially and wall time at a short finite bound. Record source SHAs and artifact hashes. Use an absent native binary for the planning stage; never use a production-ledger copy.
2. Create a separate private ChatGPT developer connection/tunnel to that isolated port, using the supported host flow. Do not replace the preserved tunnel. Confirm the model-filtered tool catalog and packaged app resource, then create/import the small graph with an explicitly captured source revision.
3. Ask ChatGPT to inspect it; select a node in the actual workbench; ask the model to identify the selected node and revision. Propose and apply an authorized target-preference change at the current revision; repeat the same request to prove idempotency. Confirm both interfaces display the same revised state. Exercise reverse-direction UI mutation, context clear/remount, reconnect and a stale revision rejection. Retain the actual host transcript/receipts. Require zero native attempts, claims and dispatches.
4. For a CAS preview, configure a separately isolated CAS observe deployment and explicit package/target/CWD. Call only `inspect_factory_cas`; require visible qualification blockers and no native dispatch. This is a useful integration preview. **Real CAS execution cannot be dogfooded through this slice** until its blockers are closed; do not substitute direct ad hoc CAS calls for Factory acceptance.
5. If separately authorized, use a second fresh ledger/repository and an explicitly identified, already-running isolated native daemon/socket. Verify exact native version/schema, canonical skill digest, approved subscription-compatible authentication/provider/model route and no API-key fallback. Existing model-list/provider metadata alone is insufficient; any inference used to establish entitlement is part of the approved canary.
6. Run one bounded local-candidate objective. For LF07 acceptance, make it two small necessary tasks with independent file predicates, no external side effects, no children initially, and a zero repair budget. Require one owner, automatic continuation without external Resume, exact task/attempt/generation proof, unchanged deadline and verified final settlement before release. Record zero unapproved model/provider/target changes. Test callback, restart/reconnect, deadline, cancellation and descendants separately before broader deployment. If any effect or cessation is unknown, retain the isolated claim and stop; never clear it to make the canary pass.

Stop and clean up only processes/resources created for this canary and proven ceased. No merge, production worker launch, deployment or tag is authorized by this document. The existing unknown owner's safe settlement is a separate live-cutover gate, not a dependency of a fully isolated preview.

## LF-09 and remaining gates

**Do not start LF-09 before the first useful preview.** Planning plus actual ChatGPT interaction, CAS inspection, and the existing local-native proof path can deliver useful acceptance without remote artifact intake. LF09 is required before remote CAS/Devsy outputs are promoted to verified integration/completion. It cannot fix missing callback ownership, subscription evidence or owner protocol parity by itself.

| Gate | Required for |
| --- | --- |
| Exact final-head green CI, reviewed stack diff/ancestry, maintainer disposition and explicit merge approval | Code merge. |
| Actual ChatGPT selection/context/write/reconnect transcript, matching packaged hashes and zero-dispatch proof | Useful conversational planning preview. |
| Isolated native version/auth/route qualification and bounded same-owner canary with current scoped proof | Real local execution preview. |
| Supported callback ownership, canonical skill/output-schema/client-correlation contract, immutable workspace/source identity, scoped receipts and read-only recovery | CAS execution admission. |
| Subscription entitlement evidence without API-billed fallback; complete owner/child/terminal observation and cessation | Every enabled execution target. |
| Remote output digests bound to task/attempt/generation/source, independent integration verification (LF09) | Remote artifact acceptance, not planning/CAS inspection. |
| Matching backup/restore rehearsal, actual target/host qualification, preserved-owner safe settlement, explicit deployment approval | Production replacement/live cutover. |

The next READY work is acceptance and review, not another feature slice. Cloud/GitHub execution remains a separate unqualified investigation; no new API or Agents API fallback is assumed.
