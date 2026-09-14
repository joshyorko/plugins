from __future__ import annotations

from typing import Any, Mapping

from .model import exact


def _decision(outcome: str, decision: str, reason: str | None = None, **extra: Any) -> dict[str, Any]:
    return {"outcome": outcome, "decision": decision, "reason_code": reason, **extra}


def _outcome(state: Mapping[str, Any], outcome_id: str) -> Mapping[str, Any] | None:
    return next((item for item in state.get("outcomes", []) if item.get("id") == outcome_id), None)


def _candidate(state: Mapping[str, Any], outcome: Mapping[str, Any]) -> Mapping[str, Any] | None:
    return next((item for item in state.get("candidates", []) if item.get("outcome") == outcome.get("id") and not item.get("superseded")), None)


def _evidence_decision(state: Mapping[str, Any], outcome: Mapping[str, Any]) -> str | None:
    matches = [item for item in state.get("evidence", []) if item.get("predicate") == outcome.get("predicate")]
    if not matches:
        return "EVIDENCE_MISSING"
    if any(item.get("subject") != outcome.get("subject") for item in matches):
        return "SUBJECT_MISMATCH"
    for item in matches:
        stale = exact(state, item)
        if stale:
            return stale
    return None


def evaluate_outcome(state: Mapping[str, Any], outcome_id: str) -> dict[str, Any]:
    outcome = _outcome(state, outcome_id)
    if outcome is None:
        return _decision(outcome_id, "DENY", "ONTOLOGY_CHANGE_REQUIRED")
    for dependency in outcome.get("dependencies", []):
        if evaluate_outcome(state, dependency).get("decision") != "SATISFIED":
            return _decision(outcome_id, "WAIT", "DEPENDENCY_UNSATISFIED")
    candidate = _candidate(state, outcome)
    if candidate is None or candidate.get("subject") != outcome.get("subject") or candidate.get("base_head") != state.get("observed_head"):
        return _decision(outcome_id, "DENY", "CANDIDATE_MISMATCH")
    reason = _evidence_decision(state, outcome)
    if reason:
        return _decision(outcome_id, "DENY", reason)
    return _decision(outcome_id, "SATISFIED")


def evaluate_action(state: Mapping[str, Any], request: Mapping[str, Any]) -> dict[str, Any]:
    stale = exact(state, request)
    if stale:
        return _decision(request.get("action", ""), "DENY", stale)
    if request.get("assumptions") != state.get("assumptions", []):
        return _decision(request.get("action", ""), "DENY", "ASSUMPTION_MISMATCH")
    grants = [grant for grant in state.get("authorities", []) if grant.get("actor") == request.get("actor") and grant.get("subject") == request.get("subject") and request.get("action") in grant.get("actions", []) and not exact(state, grant, ("model_generation", "plan_generation"))]
    if not grants:
        return _decision(request.get("action", ""), "DENY", "AUTHORITY_DENIED")
    key = request.get("idempotency_key")
    if key:
        prior = next((item for item in state.get("receipts", []) if item.get("idempotency_key") == key), None)
        if prior is not None:
            return _decision(request.get("action", ""), "ADMIT", receipt=prior)
    return _decision(request.get("action", ""), "ADMIT")


def _conflicts(left: list[Mapping[str, Any]], right: list[Mapping[str, Any]]) -> bool:
    return any(a.get("resource") == b.get("resource") and "write" in {a.get("mode"), b.get("mode")} for a in left for b in right)


def schedule(state: Mapping[str, Any]) -> list[dict[str, Any]]:
    admitted: list[Mapping[str, Any]] = []
    result: list[dict[str, Any]] = []
    capacity = state.get("capacity", 1)
    for outcome in sorted(state.get("outcomes", []), key=lambda item: item.get("id", "")):
        if outcome.get("status") == "UNKNOWN":
            result.append(_decision(outcome.get("id", ""), "WAIT", "EVIDENCE_MISSING")); continue
        base = evaluate_outcome(state, outcome.get("id", ""))
        if base["reason_code"] == "DEPENDENCY_UNSATISFIED":
            result.append(base); continue
        claims = outcome.get("claims", [])
        if _conflicts(claims, [claim for item in admitted for claim in item.get("claims", [])]):
            result.append(_decision(outcome.get("id", ""), "WAIT", "RESOURCE_CONFLICT")); continue
        if len(admitted) >= capacity:
            result.append(_decision(outcome.get("id", ""), "WAIT", "RESOURCE_CONFLICT")); continue
        if base["reason_code"] in {"EVIDENCE_MISSING", "CANDIDATE_MISMATCH"}:
            admitted.append(outcome); result.append(_decision(outcome.get("id", ""), "ADMIT"))
        else:
            result.append(base)
    return result
