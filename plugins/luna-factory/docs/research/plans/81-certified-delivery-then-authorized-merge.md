# #81 staged plan: certified push, then open PR for review, then merge as a separately granted authority (no #79 dependency)

**Baseline.** `origin/main` has moved to `e194b01` (#87), not `57dbdfb`. All line numbers below refer to `e194b01`, in `plugins/luna-factory/server/src`. Both reports were checked against this code. #79 is not needed for any stage. #71 (qualification) gates any live use. #78 (bound PR/CI observations) gates Stages 2–3. #76 supplies the GitHub credential.

## What the code does today

- **Finish ranks:** `local_candidate`/`push`/`pr` (config.rs:58-65). They are capped at admission (store.rs:393-396) and by registration (repositories.rs:198, 249).
- **Effects:** an effect is only `{id, kind, status}` (control.rs:131-135). `observe_effect` treats two effects as the same if their kind matches; it does not compare payloads (control.rs:502-517). `EffectSettled` refuses Push/Pr (control.rs:876-886).
- **Delivery is never certified.** Push/PR is only noticed in the owner's shell output (evidence.rs:517-539) or made up after the fact as `delivery:<dispatch>` (lifecycle.rs:1868-1882). A non-local finish is forced to Blocked (lifecycle.rs:1898-1906). Resume is refused (lifecycle.rs:2522-2525) and claim release is refused (store.rs:1055-1062).
- **IDs are random.** Native effect IDs are `uuid::new_v4` (lifecycle.rs:1021, 2365, 2646). Every envelope from `control_event` gets a fresh UUID (lifecycle.rs:880-888). As a result, journal replay deduplication (store.rs:846-858) never sees a logical effect twice.
- **Source subject:** a hash of the index plus untracked files (store.rs:287), not a commit. Delivery must bind it to an exact commit.

## Stage 0 (PR A): effect journal contract, no network

**Change**
- Extend `Effect` with `#[serde(default)]` fields:
  - `fingerprint`
  - `request {kind, remote, target_ref, candidate_commit, candidate_subject, proof_digest, policy_digest, merge_method?}`
  - `outcome ∈ not_sent|accepted|rejected|unknown`
  - `observation`
- Add `EffectKind::Merge`.
- Logical ID = sha256 of the load-bearing inputs. It never includes an attempt, UUID or worker.
- `observe_effect`: same ID with a different fingerprint is an `effect_identity_conflict`.
- `EffectSettled` gains `observation`. Push/Pr/Merge settle only if the observation exactly matches the request. Effects seen only in the owner's shell (`native-effect:`, `delivery:`, `legacy-delivery:`) are still refused at control.rs:883.
- Envelope IDs are derived (`<effect>:intent`, `<effect>:settle:<obs-digest>`) so a restart replays into the same rows. There is no new table: the journal stays `control_events` (store.rs:478).
- **Original-proof preservation:** `proof_digest` freezes the criteria, check refs and bindings at intent time (evidence.rs:77-223). Delivery appends observations and never mutates `checks`/`criteria`.

**Invariants:** replay never sends anything (control.rs:653); unknown outcomes keep the claim (store.rs:1055); legacy rows decode as uncertified.

**Tests (tests/control.rs)**
- `delivery_same_key_changed_payload_conflicts`
- `settlement_requires_matching_observation`
- `owner_observed_push_never_settles`
- `legacy_v2_delivery_effect_decodes_unknown`
- `proof_digest_survives_restart_unchanged`

**Operator gate:** none (pure reducer).

## Stage 1 (PR B): certified push

**Change**
- A run with finish `push`/`pr` that proves its local candidate no longer synthesizes an unknown effect (lifecycle.rs:1868-1882). Instead it becomes Blocked with a typed `delivery_pending` (presentation.rs:241). The claim stays held: no release at lifecycle.rs:2034-2041.
- New `Factory::deliver(id, expected_revision)`, modelled on `steer_at_revision` (lifecycle.rs:2340). It takes the mutation lock, then runs `validate_current_authority` (lifecycle.rs:2256) and `authorize_action("deliver")` (presentation.rs:276).

**Candidate/target/policy checks**
- `converged()` (control.rs:485).
- Settlement is Stopped and there is no `unknown_effect` (control.rs:496).
- `repository_subject == current_subject`, a clean status, and `HEAD == candidate_commit`, which descends from `base_head`.
- `finish_rank(request) ≤ max_finish` is rechecked at effect time.
- The remote matches the admitted repository identity.
- The target is the run's own `luna/<run>` branch, never the default or a protected branch.

**Wire**
- These checks run again inside a guard closure, the same pattern as `dispatch_guard` (lifecycle.rs:2306-2333), immediately before spawning the push.
- The push uses the hooks-disabled `git` helper (store.rs:261-285): `push --atomic --force-with-lease=<ref>:<expected> <commit>:refs/heads/<branch>`.
- The push settles only when `ls-remote` observes `== candidate_commit`.
- A guard refusal is `not_sent`. Any spawn, exit or timeout ambiguity is `unknown`.

**Lost-acknowledgment reconciliation**
- `reconcile` (lifecycle.rs:2699) gains a read-only `ls-remote` branch.
- If the remote matches, the effect is settled. If it is absent or different, it stays unknown.
- Only an explicit `deliver` may re-issue the push, under the same logical ID. The lease prevents clobbering.

**Tests (tests/factory_integration.rs, local bare remote whose hook counts receives)**
- `certified_push_settles_only_on_observed_remote_ref`
- `candidate_moves_between_guard_and_wire_sends_zero_pushes`
- `lost_push_ack_reconciles_read_only_without_second_receive`
- `restart_with_unknown_push_retains_claim_offers_reconcile_not_resume`
- `revoked_push_authority_after_admission_sends_nothing`
- Keep and extend `observed_push_remains_unknown_even_when_owner_requests_an_answer` (:1318) and `unproved_delivery_cannot_offer_retry_or_release_its_claim` (:965).

**Operator gate:** granting `max_finish=push` on a real alias, remote credentials (never stored), and the first live push.

## Stage 2 (PR C): open PR, "Ready for review" (default delivery)

**Change**
- Requires a settled push for the same candidate.
- Writes go to the GitHub REST API through the existing `reqwest` dependency. Reads consume #78's bound observation; there is no second observer.
- Logical ID over (forge repository, head branch, candidate_commit, base). The PR body carries a `luna-effect:<id>` marker.
- Before POST, look up an open PR for head→base. A marker match with a matching head SHA is adopted.
- A 422 "already exists" response is adopted through lookup, never treated as a retry trigger.
- Settles only on an observed `{number, head.sha==candidate_commit, base.ref==target, state=open}`.
- Deliverable `pr_ready`/verified becomes "Ready for review" (presentation.rs:273, schemas.rs:102). Then the run goes CONVERGED and `release_verified` (store.rs:1032).

**Reconciliation:** a missing marker stays unknown ("not found yet" is not proof). Read-only reconcile never POSTs.

**Tests (fake GitHub on axum in tests/support counting POSTs)**
- `pr_settles_only_when_head_sha_and_base_match`
- `lost_pr_ack_adopts_marker_pr_without_second_create`
- `pr_422_exists_is_adopted_not_duplicated`
- `wrong_head_pr_is_conflict_not_ready`
- `read_only_reconcile_never_creates_pr`

**Operator gate:** the installation scope of the #76 App or token, and the first live PR.

## Stage 3 (PR D): merge as a separately granted authority, built but off

**Change**
- Add `integrate` as rank 3 (config.rs:58; enumerations at schemas.rs:41, mcp.rs:41).
- Admission records an immutable `integrate{target_branch, merge_method}` in the request fingerprint (store.rs:615-634), capped by `max_finish`. It moves to the campaign reference when #77 lands.
- The grant is a separate, one-use, local-operator journal event, `IntegrationGranted{pr, head_sha, target, method, revision}`, consistent with `"approval":"local_operator"` (lifecycle.rs:908). It must never be settable through `update_factory_settings` (mcp.rs:154).

**Two separate projections**
- **Ready for review:** the PR effect is settled.
- **Ready for authorized integration:** additionally, #78 observes green required checks bound to `head_sha` and to the required-policy hash, `mergeable` is clean, protection has been read, and a matching grant exists.

**Pre-wire guard and merge**
- The guard repeats all of those checks.
- The merge call is `PUT /merge` with `sha=candidate_commit` and the granted method. There is no admin bypass; GitHub enforces protection.
- Settles only on an observed `merged=true` with `merge_commit_sha`.
- **Proof preservation:** the merge commit is a new subject recorded as an appended observation. Candidate receipts are not rewritten or relabelled as proof of `main`.

**Reconciliation:** GET the PR. If it is merged with the same head, settle. If it is still open, it stays unknown, and only an explicit re-issue may retry (the `sha` precondition makes this idempotent).

**Tests**
- `no_merge_without_grant`
- `grant_for_other_head_target_or_method_refused`
- `required_check_missing_or_on_other_sha_blocks`
- `protection_or_policy_change_after_grant_invalidates`
- `lost_merge_ack_reconciles_without_second_put`
- `read_only_reconcile_never_merges`
- `merge_does_not_rewrite_candidate_receipts`
- `max_finish_pr_rejects_integrate`

**Operator gate:** implementing merge is not permission to use it. Each grant is per run and comes from the local operator. The first real merge needs separate explicit user authorization naming the repository, target and method.

## Ownership

There are four sequential PRs (A→D). One Rust integration owner serializes the control.rs, store.rs and lifecycle.rs edits. Claude owns the UI adoption of the two readiness states; Rust exposes fields only. Proposed issue correction, not applied: drop #79 from #81's "Requires".
