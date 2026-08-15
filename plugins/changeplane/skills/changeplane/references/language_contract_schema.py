"""Small semantic vocabulary for CP-LANGUAGE acceptance fixtures."""

KNOWN = {
    "ApplicationModel", "Component", "Outcome", "Candidate", "Predicate",
    "Evidence", "AgentExecutor", "Dispatch", "ResourceClaim", "ActionRequest",
}
LINKS = {"contains", "declares", "dependsOn", "affects", "implements", "proves", "targets", "executedBy", "holds", "records"}
TERMINAL = {"CONVERGED", "REJECTED", "CANCELLED", "QUIESCENT"}
ALLOWED = {
    "WAITING": {"READY", "UNKNOWN", "BLOCK"},
    "UNKNOWN": {"WAITING", "BLOCK"},
    "BLOCK": {"WAITING", "UNKNOWN"},
    "READY": {"PROGRESSING", "WAITING", "BLOCK", "UNKNOWN"},
    "PROGRESSING": {"CONVERGED", "WAITING", "BLOCK", "UNKNOWN", "CANCELLED"},
    "DRAINING": {"QUIESCENT", "CANCELLED"},
}


def validate_case(case):
    errors = []
    links = case.get("links", [])
    for link in links:
        if (set(link) != {"source", "target", "relation", "generation", "provenance"}
                or link["source"] not in KNOWN or link["target"] not in KNOWN
                or link["relation"] not in LINKS or not link["generation"]
                or not link["provenance"].get("digest")):
            errors.append("typed link")

    evidence = case.get("evidence", {})
    if set(evidence) != {"provenance", "disposition"}:
        errors.append("provenance/disposition separation")
    if evidence.get("disposition") not in {"CREATE_NEW", "REUSE", "REJECT", "SUPERSEDE"}:
        errors.append("disposition")
    if not evidence.get("provenance", {}).get("observedHead"):
        errors.append("provenance")

    request, grant = case.get("request", {}), case.get("grant")
    if request.get("text", "").lower().find("ignore policy") >= 0 or request.get("hostile"):
        if grant is not None:
            errors.append("hostile text grant")
    if grant is None:
        if case.get("admission") == "READY":
            errors.append("default deny")
    elif any(grant.get(field) != request.get(field) for field in ("action", "subject", "actor", "claims")):
        errors.append("grant scope")

    assumption = case.get("assumption", {})
    if assumption.get("state") not in {"SATISFIED", "UNSATISFIED", "UNKNOWN", "INVALIDATED"}:
        errors.append("assumption state")
    if assumption.get("state") != "SATISFIED" and case.get("scheduled"):
        errors.append("unsatisfied assumption scheduled")
    if assumption.get("state") == "INVALIDATED" and case.get("admission") == "READY":
        errors.append("invalidated assumption admitted")
    if assumption.get("movement") == "BASE" and assumption.get("acceptanceValid"):
        errors.append("base movement")

    transition = case.get("transition")
    if transition and transition[1] not in ALLOWED.get(transition[0], set()):
        errors.append("transition")
    if transition and transition[0] in TERMINAL:
        errors.append("terminal reopened")
    return errors
