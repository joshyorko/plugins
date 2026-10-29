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
| Trusted identity | Codex 0.165 initialize lacks serverInfo | integration writer; Luna read-only design review | investigating | peer identity and mismatch regressions |
| Promotion | static refusal | integration writer | ready | atomic new-run lineage, retry/fence/claim regressions |
| Shared paths | host and OCI namespaces differ | integration writer | ready | explicit mapping validation and fixture checks |
| Operator canary | no live grant or execution | integration writer | prepare only | bounded runbook; live evidence remains operator-only |
| Acceptance | changed source | fresh Luna reviewer | pending implementation | changed-path checks and exact-head CI |

Use pinned repository container images; serialize Rust builds and captures; Vitest
one worker if needed. Preserve existing UX evidence. Keep source, CI, deployment and
actual native execution receipts separate. Parent integration writer owns all edits.
