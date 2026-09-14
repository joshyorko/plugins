#!/usr/bin/env python3
"""Executable acceptance checks for the canonical Changeplane Language skill."""

from pathlib import Path
from copy import deepcopy
import json
import sys
import unittest


SKILL_ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(SKILL_ROOT / "references"))
from language_contract_schema import validate_case  # noqa: E402
SKILL = (SKILL_ROOT / "SKILL.md").read_text()
REFERENCE = (SKILL_ROOT / "references" / "language-contract.md").read_text()
FIXTURES = json.loads(
    (SKILL_ROOT / "tests" / "fixtures" / "semantic_cases.json").read_text()
)


class ChangeplaneLanguageContractTests(unittest.TestCase):
    def admitted_case(self):
        return deepcopy(FIXTURES["valid"][0])

    def test_direct_probe_rejects_cross_bound_subject_and_movement(self):
        case = self.admitted_case()
        case["subject"] = {
            "repository": "repo-1", "base": "base-1", "candidate": "candidate-1",
            "model": "model-1", "plan": "plan-1", "materialization": "materialization-1",
        }
        case["assumption"]["scope"]["base"] = "other-base"
        case["movement"] = {"old": "candidate-1", "new": "candidate-2", "predecessor": "candidate-1"}
        self.assertEqual(
            ["exact subject binding", "movement effects"], validate_case(case)
        )

    def test_direct_probe_rejects_unadmitted_dependencies_and_claims(self):
        case = self.admitted_case()
        case["plan"]["dependencies"] = ["unaccepted-outcome"]
        case["envelope"]["claims"] = ["foreign:path"]
        case["claims"] = [{"claim": "paths:x", "owner": "worker"}, {"claim": "paths:x", "owner": "other"}]
        self.assertEqual(
            ["admission dependencies", "claim confinement", "claim ownership"],
            validate_case(case),
        )

    def test_direct_probe_rejects_unproven_predicates_and_incomplete_checkpoint(self):
        case = self.admitted_case()
        case["receipt"]["predicates"] = ["missing-predicate"]
        case["checkpoint"]["generation"] = "sha256:plan"
        self.assertEqual(
            ["checkpoint identity", "checkpoint binding", "receipt predicates", "quiescence binding", "drain receipts", "replay binding"], validate_case(case)
        )

    def test_direct_probe_rejects_active_drain_and_unbound_quiescence(self):
        case = self.admitted_case()
        case["drain"]["activeWriters"] = 1
        case["drain"]["activeWork"] = 1
        case["quiescence"]["checkpoint"] = "other-checkpoint"
        self.assertEqual(
            ["quiescence binding", "drain completion"], validate_case(case)
        )

    def test_direct_probe_rejects_dependency_without_bound_passing_evidence(self):
        case = self.admitted_case()
        case["plan"]["dependencies"] = ["outcome-2"]
        case["acceptedDependencies"] = ["outcome-2"]
        self.assertEqual(["admission dependencies"], validate_case(case))

    def test_direct_probe_rejects_request_grant_and_envelope_claim_outside_plan(self):
        case = self.admitted_case()
        case["envelope"]["claims"] = ["paths:y"]
        case["request"]["claims"] = ["paths:y"]
        case["grant"]["claims"] = ["paths:y"]
        self.assertEqual(["claim confinement"], validate_case(case))

    def test_direct_probe_rejects_foreign_claim_owner(self):
        case = self.admitted_case()
        case["claims"] = [{"claim": "paths:x", "owner": "other"}]
        self.assertEqual(["claim ownership"], validate_case(case))

    def test_direct_probe_rejects_omitted_predicate_receipt_binding(self):
        case = self.admitted_case()
        case["receipt"]["predicates"] = []
        self.assertEqual(["receipt predicates"], validate_case(case))

    def test_direct_probe_rejects_predicate_without_evidence(self):
        case = self.admitted_case()
        case["predicate"]["evidence"] = []
        self.assertEqual(["receipt predicates", "predicate evidence"], validate_case(case))

    def test_direct_probe_rejects_stale_predicate_evidence(self):
        case = self.admitted_case()
        case["evidence"]["fresh"] = False
        self.assertEqual(["receipt predicates", "predicate evidence"], validate_case(case))

    def test_direct_probe_rejects_failed_predicate_receipt(self):
        case = self.admitted_case()
        case["predicate"]["result"] = False
        self.assertEqual(["receipt predicates"], validate_case(case))

    def test_direct_probe_rejects_checkpoint_generation_mismatch(self):
        case = self.admitted_case()
        case["checkpoint"]["generation"] = "sha256:" + "b" * 64
        case["replay"]["checkpoint"] = case["checkpoint"]["generation"]
        case["drain"]["checkpoint"] = case["checkpoint"]["generation"]
        case["quiescence"]["checkpoint"] = case["checkpoint"]["generation"]
        self.assertEqual(["checkpoint binding"], validate_case(case))

    def test_direct_probe_rejects_replay_checkpoint_position_mismatch(self):
        case = self.admitted_case()
        case["replay"]["position"] = "event-2"
        self.assertEqual(["replay binding"], validate_case(case))

    def test_direct_probe_rejects_drain_without_recorded_receipt(self):
        case = self.admitted_case()
        case["drain"]["receipts"] = []
        self.assertEqual(["drain completion", "drain receipts"], validate_case(case))

    def test_direct_probe_rejects_unbound_quiescence(self):
        case = self.admitted_case()
        case["quiescence"]["checkpoint"] = "other-checkpoint"
        self.assertEqual(["quiescence binding"], validate_case(case))

    def test_direct_probe_rejects_foreign_dependency_proof_owner(self):
        case = self.admitted_case()
        case["plan"]["dependencies"] = ["outcome-2"]
        case["acceptedDependencies"] = [{
            "outcome": "outcome-2", "owner": "other-worker", "candidate": "candidate-1",
            "plan": "plan-1", "passed": True, "evidence": ["evidence-1"],
        }]
        self.assertEqual(["dependency proof owner"], validate_case(case))

    def test_direct_probe_rejects_duplicate_dependency_proofs(self):
        case = self.admitted_case()
        case["plan"]["dependencies"] = ["outcome-2"]
        proof = {"outcome": "outcome-2", "owner": "worker", "candidate": "candidate-1",
                 "plan": "plan-1", "passed": True, "evidence": ["evidence-1"]}
        case["acceptedDependencies"] = [proof, deepcopy(proof)]
        self.assertEqual(["dependency proof uniqueness"], validate_case(case))

    def test_direct_probe_rejects_foreign_predicate_subject_and_evidence(self):
        case = self.admitted_case()
        case["predicate"].update({"subject": "other-outcome", "candidate": "other-candidate", "generation": "other-generation"})
        case["evidence"].update({"subject": "other-outcome", "candidate": "other-candidate", "generation": "other-generation"})
        self.assertEqual(["evidence subject binding", "predicate subject binding"], validate_case(case))

    def test_direct_probe_rejects_checkpoint_without_subject(self):
        case = self.admitted_case()
        del case["checkpoint"]["subject"]
        self.assertEqual(["checkpoint shape"], validate_case(case))

    def test_direct_probe_rejects_fabricated_quiescence_drain(self):
        case = self.admitted_case()
        case["quiescence"]["drain"] = "fabricated-drain"
        self.assertEqual(["quiescence binding"], validate_case(case))

    def test_direct_probe_requires_complete_normative_record_families(self):
        case = self.admitted_case()
        for field in ("candidate", "generation"):
            case["predicate"].pop(field, None)
        case["drain"].pop("identity", None)
        self.assertEqual(["predicate shape", "quiescence binding", "drain shape"], validate_case(case))

    def test_direct_probe_requires_non_weakening_engine_handoff(self):
        self.assertIn("must deterministically enforce every Language invariant", REFERENCE)
        self.assertIn("without redefining, omitting, weakening, skipping, or marking any semantic optional", REFERENCE)

    def test_skill_has_discoverable_frontmatter_and_contract_sections(self):
        self.assertTrue(SKILL.startswith("---\nname: changeplane\n"))
        self.assertIn("description: Use when", SKILL)
        for heading in (
            "## Language boundary",
            "## Authority and actions",
            "## Plans and execution",
            "## Harness boundary",
            "## Lifecycle",
            "## Safety rules",
        ):
            self.assertIn(heading, SKILL)

    def test_reference_carries_required_nouns_links_and_evidence(self):
        for noun in (
            "ChangeplaneConstitution",
            "ApplicationModel",
            "ChangeIntent",
            "Outcome",
            "Predicate",
            "ChangePlan",
            "Candidate",
            "AgentEnvelope",
            "ActionRequest",
            "ActionReceipt",
            "AdmissionDecision",
            "Quiescence",
        ):
            self.assertIn(noun, REFERENCE)
        for link in (
            "ApplicationModel contains Component",
            "ChangePlan contains Outcome",
            "Outcome dependsOn Outcome",
            "Evidence proves Predicate",
            "Candidate implements Outcome",
            "ActionReceipt records ActionRequest",
        ):
            self.assertIn(link, REFERENCE)
        for term in ("provenance", "disposition", "CREATE_NEW", "REUSE", "REJECT"):
            self.assertIn(term, REFERENCE)

    def test_authority_is_default_deny_and_text_is_not_authority(self):
        for term in (
            "default deny",
            "explicit grant",
            "untrusted text",
            "hostile",
            "UNKNOWN",
            "BLOCK",
            "self-certification",
        ):
            self.assertIn(term, REFERENCE)
        self.assertIn("ActionRequest", REFERENCE)
        self.assertIn("AuthorityGrant", REFERENCE)
        self.assertIn("ActionReceipt", REFERENCE)

    def test_lifecycle_defines_replay_drain_checkpoint_and_terminal_states(self):
        for term in (
            "replay",
            "drain",
            "checkpoint",
            "quiescence",
            "READY",
            "CONVERGED",
            "REJECTED",
            "BLOCKED",
        ):
            self.assertIn(term, REFERENCE)

    def test_non_goals_are_explicit_and_no_engine_api_is_invented(self):
        for term in (
            "enterprise RBAC",
            "event bus",
            "hosted multitenancy",
            "Kubernetes",
            "Hive",
            "hosted Streamable HTTP v1",
            "autonomous MCP mutation",
        ):
            self.assertIn(term, REFERENCE)
        self.assertNotIn("def execute_", REFERENCE)
        self.assertNotIn("class Engine", REFERENCE)

    def test_semantic_fixtures_validate_typed_links_and_shapes(self):
        for case in FIXTURES["valid"]:
            self.assertEqual([], validate_case(case), case["name"])

    def test_semantic_fixtures_reject_unsafe_or_ambiguous_cases(self):
        for case in FIXTURES["invalid"]:
            self.assertEqual(case["errors"], validate_case(case), case["name"])

    def test_valid_fixture_exercises_every_record_family(self):
        families = FIXTURES["valid"][0]["families"]
        self.assertEqual(
            {"links", "envelope", "request", "grant", "receipt", "evidence",
             "predicate", "plan", "assumption", "admission", "checkpoint",
             "replay", "drain", "quiescence", "lifecycle"},
            set(families),
        )

    def test_invalid_fixtures_report_specific_machine_reasons(self):
        for case in FIXTURES["invalid"]:
            self.assertTrue(case["errors"], case["name"])
            self.assertEqual(case["errors"], validate_case(case), case["name"])

    def test_adversarial_fixtures_execute_each_approved_predicate_family(self):
        for case in FIXTURES["adversarial"]:
            self.assertEqual(case["errors"], validate_case(case), case["name"])

    def test_assumption_invalidation_and_exact_subject_movement_are_explicit(self):
        self.assertIn("satisfaction", REFERENCE)
        self.assertIn("invalidat", REFERENCE)
        self.assertIn("base movement", REFERENCE)
        self.assertIn("read-only repair loop", REFERENCE)

    def test_transition_matrix_constrains_unknown_block_and_terminal_states(self):
        self.assertIn("Transition matrix", REFERENCE)
        self.assertIn("UNKNOWN -> READY", REFERENCE)
        self.assertIn("BLOCK -> READY", REFERENCE)
        self.assertIn("CONVERGED -> READY", REFERENCE)
        self.assertIn("DRAINING -> QUIESCENT", REFERENCE)


if __name__ == "__main__":
    unittest.main()
