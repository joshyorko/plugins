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
            ["checkpoint identity", "receipt predicates"], validate_case(case)
        )

    def test_direct_probe_rejects_active_drain_and_unbound_quiescence(self):
        case = self.admitted_case()
        case["drain"]["activeWriters"] = 1
        case["drain"]["activeWork"] = 1
        case["quiescence"]["checkpoint"] = "other-checkpoint"
        self.assertEqual(
            ["quiescence binding", "drain completion"], validate_case(case)
        )

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
