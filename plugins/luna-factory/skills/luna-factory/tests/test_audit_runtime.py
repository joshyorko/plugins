import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "audit_runtime.py"


def load_module():
    spec = importlib.util.spec_from_file_location("audit_runtime", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class AuditRuntimeTests(unittest.TestCase):
    def test_catalog_summary_reports_current_and_future_models(self):
        audit = load_module()
        catalog = {
            "models": [
                {
                    "slug": "gpt-6-luna",
                    "default_reasoning_level": "medium",
                    "supported_reasoning_levels": [
                        {"effort": "low"},
                        {"effort": "medium"},
                        {"effort": "high"},
                        {"effort": "xhigh"},
                        {"effort": "max"},
                    ],
                    "multi_agent_version": "v2",
                },
                {
                    "slug": "future-luna",
                    "default_reasoning_level": "low",
                    "supported_reasoning_levels": [{"effort": "low"}],
                },
            ]
        }

        summary = audit.summarize_catalog(catalog)
        self.assertEqual(
            summary["gpt-6-luna"],
            {
                "default_reasoning_effort": "medium",
                "supported_reasoning_efforts": [
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                ],
                "multi_agent_version": "v2",
            },
        )
        self.assertIn("future-luna", summary)
        self.assertFalse(hasattr(audit, "MODEL_SLUGS"))

    def test_explicit_non_luna_metadata_overrides_slug_inference(self):
        audit = load_module()
        route = audit.assess_luna_route(
            {"models": [{"slug": "future-luna", "family": "terra"}]},
            requested_model="future-luna",
            root_model=None,
        )
        self.assertEqual(route["status"], "requested_model_not_luna")

    def test_missing_effective_effort_is_never_reported_as_verified(self):
        audit = load_module()

        status = audit.compare_requested_to_observed(
            requested_model="gpt-6-luna",
            requested_effort="high",
            observed_model="gpt-6-luna",
            observed_effort=None,
        )

        self.assertEqual(status["model"], "verified_match")
        self.assertEqual(status["reasoning_effort"], "unverified")
        self.assertEqual(status["overall"], "partially_verified")

    def test_exact_model_and_effort_are_verified_separately(self):
        audit = load_module()
        status = audit.compare_requested_to_observed(
            requested_model="gpt-6-luna",
            requested_effort="medium",
            observed_model="gpt-6-luna",
            observed_effort="medium",
        )
        self.assertEqual(status["overall"], "verified_match")

    def test_observed_different_model_is_a_verified_mismatch(self):
        audit = load_module()
        status = audit.compare_requested_to_observed(
            requested_model="gpt-6-luna",
            requested_effort="medium",
            observed_model="other-luna",
            observed_effort="medium",
        )
        self.assertEqual(status["model"], "verified_mismatch")
        self.assertEqual(status["overall"], "verified_mismatch")

    def test_missing_requested_luna_reports_limitation_without_fallback(self):
        audit = load_module()
        route = audit.assess_luna_route(
            {"models": [{"slug": "legacy-luna"}]},
            requested_model="gpt-6-luna",
            root_model=None,
        )
        self.assertEqual(route["status"], "requested_model_unavailable")
        self.assertFalse(route["fallback_proposed"])

    def test_feature_summary_preserves_unknown_runtime_entries(self):
        audit = load_module()
        features = audit.parse_features(
            "multi_agent stable enabled\nfuture_collaboration preview experimental"
        )
        self.assertEqual(features["future_collaboration"]["stage"], "preview")
        self.assertEqual(features["future_collaboration"]["enabled"], "experimental")

    def test_absent_observation_does_not_imply_requested_routing(self):
        audit = load_module()

        status = audit.compare_requested_to_observed(
            requested_model="gpt-6-luna",
            requested_effort="low",
            observed_model=None,
            observed_effort=None,
        )

        self.assertEqual(status["overall"], "unverified")
        self.assertNotIn("verified_match", status.values())

    def test_selected_root_requests_inheritance_without_child_proof(self):
        audit = load_module()
        route = audit.assess_luna_route(
            {"models": [{"slug": "gpt-6-luna"}]},
            requested_model=None,
            root_model="gpt-6-luna",
        )
        self.assertEqual(route["status"], "inherit_selected_root")
        self.assertIn("unverified", route["reason"])

    def test_multiple_luna_generations_require_explicit_selection(self):
        audit = load_module()
        route = audit.assess_luna_route(
            {"models": [{"slug": "gpt-6-luna"}, {"slug": "gpt-7-luna"}]},
            requested_model=None,
            root_model=None,
        )
        self.assertEqual(route["status"], "explicit_selection_required")
        self.assertEqual(route["available_luna_models"], ["gpt-6-luna", "gpt-7-luna"])
        self.assertFalse(route["fallback_proposed"])


    def test_session_observation_reads_live_effort_field(self):
        audit = load_module()
        session_id = "test-session"
        with tempfile.TemporaryDirectory() as directory:
            rollout = Path(directory) / f"rollout-{session_id}.jsonl"
            events = [
                {
                    "type": "session_meta",
                    "payload": {
                        "source": {
                            "subagent": {"thread_spawn": {"depth": 1}}
                        }
                    },
                },
                {
                    "type": "turn_context",
                    "payload": {"model": "gpt-6-luna", "effort": "max"},
                },
            ]
            rollout.write_text(
                "\n".join(json.dumps(event) for event in events),
                encoding="utf-8",
            )

            observed = audit.observe_session(session_id, Path(directory))

        self.assertEqual(observed["observed_model"], "gpt-6-luna")
        self.assertEqual(observed["observed_reasoning_effort"], "max")
        self.assertEqual(observed["depth"], 1)


if __name__ == "__main__":
    unittest.main()
