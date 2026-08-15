#!/usr/bin/env python3
"""Executable acceptance checks for the canonical Changeplane Language skill."""

from pathlib import Path
import unittest


SKILL_ROOT = Path(__file__).parent.parent
SKILL = (SKILL_ROOT / "SKILL.md").read_text()
REFERENCE = (SKILL_ROOT / "references" / "language-contract.md").read_text()


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


if __name__ == "__main__":
    unittest.main()
