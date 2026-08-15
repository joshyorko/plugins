# Task 2 report — Deterministic Engine and Governed Scheduling

## TDD evidence

- RED: `python3 -m unittest plugins/changeplane/tests/test_engine.py plugins/changeplane/tests/test_adversarial.py -v`
  - Expected failure: `ModuleNotFoundError: No module named 'plugins.changeplane.core'` for both focused modules.
- RED (unknown lane): `python3 -m unittest plugins/changeplane/tests/test_engine.py -v`
  - Expected failure: `a-unknown` was admitted, demonstrating it incorrectly consumed a writer slot.
- GREEN: `python3 -m unittest plugins/changeplane/tests/test_engine.py plugins/changeplane/tests/test_adversarial.py plugins/changeplane/tests/test_language.py -v`
  - Result: 16 tests passed.
- Verification: `python3 -m compileall -q plugins/changeplane/core`, `git diff --check`, and `bin/check`
  - Result: passed; `bin/check` ran 30 tests and validated repo structure.

## Changed files

- `plugins/changeplane/core/changeplane/{__init__,model,identity,engine,reconcile,envelope}.py`
- `plugins/changeplane/tests/test_engine.py`
- `plugins/changeplane/tests/test_adversarial.py`

## Design decisions

- All APIs are pure and JSON-compatible; envelope compilation is a preview marked `DISPATCH TO AGENT` and cannot dispatch or mutate.
- Canonical identity uses normalized sorted JSON with SHA-256.
- Authority is explicit, actor/action/subject scoped, and default-deny; stale bindings and changed assumptions deny action requests.
- Evidence, candidates, scheduling, receipts, checkpoint/drain, and quiescence have deterministic binding checks. Unknown and dependent lanes wait without consuming capacity; overlapping writes wait separately.

## Commit and push

- Implementation commit: `a38ccbcd19f6e0a078e6e5d09af887e5a61b6a53` (`feat(changeplane): add deterministic engine`)
- Push target: `origin experiment/changeplane-fast-reference`

## Self-review and concerns

- Reviewed for side effects: no filesystem, network, subprocess, dispatch, or state mutation is performed by the core.
- Concern: the core intentionally accepts plain mappings as the portable record boundary; callers should supply records conforming to the Task 1 corpus.
