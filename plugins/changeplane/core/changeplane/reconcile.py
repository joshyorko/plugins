from __future__ import annotations

from typing import Any, Mapping

from .model import exact


RECEIPT_FIELDS = {"id", "action", "actor", "subject", "model_generation", "plan_generation", "observed_head", "candidate", "authority", "idempotency_key", "result", "evidence"}


def reconcile(state: Mapping[str, Any]) -> dict[str, Any]:
    observation = state.get("observation")
    stale = bool(observation and exact(state, observation))
    checkpoint = state.get("checkpoint", {})
    checkpoint_valid = bool(checkpoint) and not exact(state, checkpoint)
    receipts_valid = all(RECEIPT_FIELDS <= set(receipt) and not exact(state, receipt) for receipt in state.get("receipts", []))
    drain_valid = checkpoint_valid and receipts_valid and bool(state.get("drain", {}).get("observed_empty"))
    quiescence = state.get("quiescence", {})
    safe = quiescence.get("safe_authorized_transition")
    return {"mutations": [] if stale else [], "checkpoint_valid": checkpoint_valid, "drain_valid": drain_valid, "receipts_valid": receipts_valid, "quiescence": {"model_generation": state.get("model_generation"), "plan_generation": state.get("plan_generation"), "safe_authorized_transition": safe, "satisfied": safe in (False, None) and not state.get("known_safe_transition", False)}}
