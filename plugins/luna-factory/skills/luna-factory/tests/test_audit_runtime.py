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
    def test_catalog_summary_preserves_supported_luna_efforts(self):
        audit = load_module()
        catalog = {
            "models": [
                {
                    "slug": "gpt-5.6-luna",
                    "default_reasoning_level": "medium",
                    "supported_reasoning_levels": [
                        {"effort": "low"},
                        {"effort": "medium"},
                        {"effort": "high"},
                        {"effort": "xhigh"},
                        {"effort": "max"},
                    ],
                    "multi_agent_version": "v1",
                }
            ]
        }

        self.assertEqual(
            audit.summarize_catalog(catalog)["gpt-5.6-luna"],
            {
                "default_reasoning_effort": "medium",
                "supported_reasoning_efforts": [
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                ],
                "multi_agent_version": "v1",
            },
        )

    def test_missing_effective_effort_is_never_reported_as_verified(self):
        audit = load_module()

        status = audit.compare_requested_to_observed(
            requested_model="gpt-5.6-luna",
            requested_effort="high",
            observed_model="gpt-5.6-luna",
            observed_effort=None,
        )

        self.assertEqual(status["model"], "verified_match")
        self.assertEqual(status["reasoning_effort"], "unverified")
        self.assertEqual(status["overall"], "partially_verified")

    def test_absent_observation_does_not_imply_requested_routing(self):
        audit = load_module()

        status = audit.compare_requested_to_observed(
            requested_model="gpt-5.6-sol",
            requested_effort="low",
            observed_model=None,
            observed_effort=None,
        )

        self.assertEqual(status["overall"], "unverified")
        self.assertNotIn("verified_match", status.values())

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
                    "payload": {"model": "gpt-5.6-luna", "effort": "max"},
                },
            ]
            rollout.write_text(
                "\n".join(json.dumps(event) for event in events),
                encoding="utf-8",
            )

            observed = audit.observe_session(session_id, Path(directory))

        self.assertEqual(observed["observed_model"], "gpt-5.6-luna")
        self.assertEqual(observed["observed_reasoning_effort"], "max")
        self.assertEqual(observed["depth"], 1)


if __name__ == "__main__":
    unittest.main()
