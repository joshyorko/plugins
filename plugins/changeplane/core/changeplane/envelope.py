from __future__ import annotations

from typing import Any, Mapping

from .engine import _candidate, _outcome
from .identity import canonical_hash


def compile_envelope(state: Mapping[str, Any], outcome_id: str, executor: str) -> dict[str, Any]:
    """Compile a portable preview; this function never dispatches or mutates."""
    outcome = _outcome(state, outcome_id)
    if outcome is None:
        raise ValueError("unknown outcome")
    candidate = _candidate(state, outcome)
    if candidate is None:
        raise ValueError("candidate required")
    authority = next((grant for grant in state.get("authorities", []) if grant.get("subject") == outcome.get("subject")), None)
    if authority is None:
        raise ValueError("authority required")
    assumptions = state.get("assumptions", [])
    envelope: dict[str, Any] = {"outcome": outcome_id, "objective": outcome.get("predicate"), "subject": outcome.get("subject"), "candidate": dict(candidate), "assumptions": list(assumptions), "model_generation": state.get("model_generation"), "plan_generation": state.get("plan_generation"), "observed_head": state.get("observed_head"), "claims": list(outcome.get("claims", [])), "authority": dict(authority), "acceptance": {"predicate": outcome.get("predicate"), "assumptions": list(assumptions)}, "budget": state.get("budget", {"max_actions": 1}), "stopping_conditions": state.get("stopping_conditions", ["binding changes", "authority denied"]), "receipt_schema": {"required": ["action", "actor", "subject", "model_generation", "plan_generation", "observed_head", "candidate", "authority", "idempotency_key", "result", "evidence"]}, "executor": executor, "instruction": "DISPATCH TO AGENT"}
    envelope["id"] = canonical_hash({key: value for key, value in envelope.items() if key != "id"})
    return envelope
