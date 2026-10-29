# Execution source follow-up (#71)

Goal: prepare the Rust Factory to execute a newly admitted, qualified campaign run
from one explicitly approved planning revision. Cloud tests remain synthetic.

Mandatory invariants: immutable planning runs; exact revision/source/repository and
operator authority; atomic admission/claim/link and idempotency; no worker adoption;
trusted daemon identity independent of initialize user-agent; explicit shared paths;
unchanged qualification/effect/cessation guards and nullable task-edit semantics.

Authority: source, fixture tests, checkpoint pushes and one draft follow-up PR.
No live service access, real grants, credentials, inference, deployment or merge.

| Task | Gap | Owner | State | Proof required |
| --- | --- | --- | --- | --- |
| Trusted identity | Codex 0.165 initialize lacks serverInfo | integration writer; Luna read-only design review | source verified | Linux peer binding; same-namespace/mismatch fixtures; host visibility unproved |
| Promotion | static refusal | integration writer | source verified | atomic snapshot, capacity, concurrent replay, rollback and framing fixtures |
| Shared paths | host and OCI namespaces differ | integration writer | source verified | mapping fingerprint, local/daemon framing, private comparison probe |
| Operator canary | no live grant or execution | integration writer | operator boundary | prepared bounded runbook; no live grant or inference |
| Acceptance | changed source | fresh Luna reviewer | source accepted; final CI pending | independent review; 344 Rust, 282 UI, 84 package and 79 repo tests; compiled integration |

Use pinned repository container images; serialize Rust builds and captures; Vitest
one worker if needed. Preserve existing UX evidence. Keep source, CI, deployment and
actual native execution receipts separate. Parent integration writer owns all edits.
