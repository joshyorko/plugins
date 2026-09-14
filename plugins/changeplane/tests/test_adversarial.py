from __future__ import annotations

import unittest

from plugins.changeplane.core.changeplane.engine import evaluate_outcome
from plugins.changeplane.core.changeplane.reconcile import reconcile


def state(**overrides):
    value = {"model_generation": "m", "plan_generation": "p", "observed_head": "h", "outcomes": [{"id": "o", "predicate": "ok", "subject": "s", "dependencies": [], "claims": []}], "evidence": [], "candidates": [{"id": "c", "outcome": "o", "subject": "s", "base_head": "h", "candidate": "c1"}], "assumptions": []}
    value.update(overrides); return value


class AdversarialTest(unittest.TestCase):
    def test_foreign_duplicate_and_wrong_binding_proof_cannot_satisfy(self):
        proof = {"id": "proof", "predicate": "ok", "subject": "other", "model_generation": "m", "plan_generation": "p", "observed_head": "h"}
        self.assertEqual("SUBJECT_MISMATCH", evaluate_outcome(state(evidence=[proof]), "o")["reason_code"])
        proof["subject"] = "s"; proof["predicate"] = "wrong"
        self.assertEqual("EVIDENCE_MISSING", evaluate_outcome(state(evidence=[proof, proof]), "o")["reason_code"])

    def test_checkpoint_drain_and_quiescence_are_not_fabricated(self):
        current = state(checkpoint={"model_generation": "old", "plan_generation": "p", "observed_head": "h"}, drain={"satisfied": True}, quiescence={"safe_authorized_transition": True})
        result = reconcile(current)
        self.assertFalse(result["checkpoint_valid"])
        self.assertFalse(result["drain_valid"])
        self.assertFalse(result["quiescence"]["satisfied"])

    def test_stale_reconciliation_returns_no_mutation_and_incomplete_receipt_fails(self):
        stale = state(observation={"model_generation": "old", "plan_generation": "p", "observed_head": "h"})
        self.assertEqual([], reconcile(stale)["mutations"])
        receipt = {"id": "r", "action": "x"}
        self.assertFalse(reconcile(state(receipts=[receipt]))["receipts_valid"])


if __name__ == "__main__":
    unittest.main()
