#!/usr/bin/env python3
"""Executable acceptance checks for the canonical Changeplane Language skill."""

from pathlib import Path
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
