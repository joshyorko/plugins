from __future__ import annotations

import unittest

from plugins.changeplane.core.changeplane.engine import evaluate_action, evaluate_outcome, schedule
from plugins.changeplane.core.changeplane.identity import canonical_hash
from plugins.changeplane.core.changeplane.envelope import compile_envelope


def state(**overrides):
    value = {
        "model_generation": "model-1", "plan_generation": "plan-1", "observed_head": "head-1",
        "outcomes": [{"id": "build", "predicate": "built", "subject": "repo", "dependencies": [], "claims": []}],
        "evidence": [], "candidates": [{"id": "candidate-1", "outcome": "build", "subject": "repo", "base_head": "head-1", "candidate": "commit-1"}],
        "authorities": [{"id": "grant-1", "actor": "bot", "subject": "repo", "actions": ["deploy"], "model_generation": "model-1", "plan_generation": "plan-1"}],
        "capacity": 1, "receipts": [], "assumptions": ["clean"],
    }
    value.update(overrides)
    return value


class EngineTest(unittest.TestCase):
    def test_canonical_hash_normalizes_mapping_order(self):
        self.assertEqual(canonical_hash({"b": [2, 1], "a": "x"}), canonical_hash({"a": "x", "b": [2, 1]}))

    def test_default_deny_and_stale_fences(self):
        request = {"id": "r", "action": "deploy", "actor": "other", "subject": "repo", "model_generation": "model-1", "plan_generation": "plan-1", "observed_head": "head-1", "assumptions": ["clean"]}
        self.assertEqual("AUTHORITY_DENIED", evaluate_action(state(), request)["reason_code"])
        request["actor"] = "bot"; request["observed_head"] = "old"
        self.assertEqual("STALE_OBSERVED_HEAD", evaluate_action(state(), request)["reason_code"])

    def test_idempotency_returns_prior_receipt(self):
        request = {"id": "r", "action": "deploy", "actor": "bot", "subject": "repo", "model_generation": "model-1", "plan_generation": "plan-1", "observed_head": "head-1", "assumptions": ["clean"], "idempotency_key": "same"}
        prior = {"id": "receipt", "idempotency_key": "same", "result": "ok"}
        self.assertEqual(prior, evaluate_action(state(receipts=[prior]), request)["receipt"])

    def test_outcome_requires_exact_evidence_and_dependencies(self):
        blocked = state(outcomes=[{"id": "deploy", "predicate": "deployed", "subject": "repo", "dependencies": ["build"], "claims": []}])
        self.assertEqual("DEPENDENCY_UNSATISFIED", evaluate_outcome(blocked, "deploy")["reason_code"])
        self.assertEqual("EVIDENCE_MISSING", evaluate_outcome(state(), "build")["reason_code"])
        evidence = {"id": "e", "predicate": "built", "subject": "repo", "model_generation": "model-1", "plan_generation": "plan-1", "observed_head": "head-1"}
        self.assertEqual("SATISFIED", evaluate_outcome(state(evidence=[evidence]), "build")["decision"])

    def test_candidate_repair_invalidates_predecessor_and_envelope_is_exact(self):
        broken = state(candidates=[{"id": "old", "outcome": "build", "subject": "repo", "base_head": "head-1", "candidate": "commit-old", "superseded": True}])
        self.assertEqual("CANDIDATE_MISMATCH", evaluate_outcome(broken, "build")["reason_code"])
        envelope = compile_envelope(state(), "build", "any-harness")
        self.assertEqual("DISPATCH TO AGENT", envelope["instruction"])
        self.assertEqual("build", envelope["candidate"]["outcome"])
        self.assertEqual("repo", envelope["authority"]["subject"])

    def test_schedule_respects_claims_and_capacity_but_not_waiting(self):
        outcomes = [
            {"id": "a", "predicate": "a", "subject": "repo", "dependencies": [], "claims": [{"resource": "file", "mode": "write"}]},
            {"id": "b", "predicate": "b", "subject": "repo", "dependencies": [], "claims": [{"resource": "file", "mode": "write"}]},
            {"id": "wait", "predicate": "w", "subject": "repo", "dependencies": ["missing"], "claims": []},
        ]
        decisions = {x["outcome"]: x for x in schedule(state(outcomes=outcomes, capacity=2))}
        self.assertEqual("ADMIT", decisions["a"]["decision"])
        self.assertEqual("WAIT", decisions["wait"]["decision"])
        self.assertEqual("WAIT", decisions["b"]["decision"])
        self.assertEqual("RESOURCE_CONFLICT", decisions["b"]["reason_code"])

    def test_unknown_lane_does_not_consume_a_writer_slot(self):
        outcomes = [
            {"id": "a-unknown", "predicate": "u", "subject": "repo", "dependencies": [], "claims": [], "status": "UNKNOWN"},
            {"id": "z-ready", "predicate": "r", "subject": "repo", "dependencies": [], "claims": []},
        ]
        decisions = {x["outcome"]: x for x in schedule(state(outcomes=outcomes, capacity=1))}
        self.assertEqual("WAIT", decisions["a-unknown"]["decision"])
        self.assertEqual("ADMIT", decisions["z-ready"]["decision"])


if __name__ == "__main__":
    unittest.main()
