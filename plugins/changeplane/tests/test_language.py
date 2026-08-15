from __future__ import annotations

import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
PLUGIN = ROOT / "plugins" / "changeplane"


class ChangeplaneLanguageTest(unittest.TestCase):
    """Protect the normative language consumed by the engine and MCP layers."""

    def load_conformance(self) -> dict:
        return json.loads(
            (PLUGIN / "skills" / "changeplane" / "references" / "conformance.json").read_text(
                encoding="utf-8"
            )
        )

    def test_record_families_and_exact_bindings_are_complete(self) -> None:
        """Removing a governed record or its identity fence must fail this contract."""
        contract = self.load_conformance()

        self.assertEqual("changeplane/v1", contract["language"])
        required_families = {
            "ApplicationModel",
            "ChangePlan",
            "Outcome",
            "Predicate",
            "Observation",
            "Evidence",
            "Candidate",
            "AuthorityGrant",
            "ActionDefinition",
            "ActionRequest",
            "ActionReceipt",
            "ResourceClaim",
            "AgentEnvelope",
            "AdmissionDecision",
            "Convergence",
            "Quiescence",
        }
        self.assertTrue(required_families <= set(contract["record_families"]))
        self.assertEqual(
            {
                "model_generation",
                "plan_generation",
                "observed_head",
                "candidate",
                "subject",
                "assumptions",
            },
            set(contract["exact_bindings"]),
        )
        for family in required_families:
            self.assertTrue(contract["record_families"][family]["required"])

    def test_default_deny_authority_rejects_hostile_text_grants(self) -> None:
        """Changing authority to implicit or text-derived authority is a security bug."""
        contract = self.load_conformance()

        self.assertEqual("DENY", contract["authority"]["default"])
        self.assertTrue(contract["authority"]["subject_scoped"])
        self.assertFalse(contract["authority"]["hostile_input_can_grant"])

    def test_reason_codes_and_adversarial_cases_cover_governed_rejections(self) -> None:
        """Removing a denial reason or its adversarial case loses an operator-safe outcome."""
        contract = self.load_conformance()

        reasons = {item["code"]: item["case_id"] for item in contract["reason_codes"]}
        expected = {
            "AUTHORITY_DENIED": "authority-default-deny",
            "STALE_MODEL_GENERATION": "stale-model-generation",
            "STALE_PLAN_GENERATION": "stale-plan-generation",
            "STALE_OBSERVED_HEAD": "stale-observed-head",
            "CANDIDATE_MISMATCH": "candidate-mismatch",
            "SUBJECT_MISMATCH": "subject-mismatch",
            "ASSUMPTION_MISMATCH": "assumption-mismatch",
            "EVIDENCE_MISSING": "evidence-missing",
            "DEPENDENCY_UNSATISFIED": "dependency-unsatisfied",
            "RESOURCE_CONFLICT": "resource-conflict",
            "HUMAN_DECISION_REQUIRED": "human-decision-required",
            "ONTOLOGY_CHANGE_REQUIRED": "ontology-change-required",
        }
        self.assertEqual(expected, reasons)
        self.assertTrue(
            set(expected.values())
            | {"hostile-text-authority", "cross-subject-authority"}
            <= {case["id"] for case in contract["adversarial_cases"]}
        )

    def test_skill_and_plugin_surface_are_canonical_and_generated_from_catalog(self) -> None:
        """Breaking the invocation or bypassing generated views makes the plugin undiscoverable."""
        skill = (PLUGIN / "skills" / "changeplane" / "SKILL.md").read_text(encoding="utf-8")
        language = (PLUGIN / "skills" / "changeplane" / "references" / "language.md").read_text(
            encoding="utf-8"
        )
        manifest = json.loads((PLUGIN / ".codex-plugin" / "plugin.json").read_text(encoding="utf-8"))
        catalog = json.loads((ROOT / "marketplaces" / "catalog.json").read_text(encoding="utf-8"))

        self.assertIn("$changeplane", skill)
        self.assertIn("@changeplane", skill)
        self.assertIn("DISPATCH TO AGENT", skill)
        self.assertIn("DISPATCH TO AGENT", language)
        self.assertEqual("changeplane", manifest["name"])
        self.assertEqual("./skills/", manifest["skills"])
        self.assertIn("changeplane", [plugin["name"] for plugin in catalog["plugins"]])


if __name__ == "__main__":
    unittest.main()
