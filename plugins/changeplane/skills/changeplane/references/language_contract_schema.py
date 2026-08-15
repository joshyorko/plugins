"""Small, declarative CP-LANGUAGE validator used by the semantic fixtures."""

KNOWN = {"ApplicationModel", "Component", "Outcome", "Candidate", "Predicate", "Evidence", "AgentExecutor", "Dispatch", "ResourceClaim", "ActionRequest"}
LINKS = {"contains", "declares", "dependsOn", "affects", "implements", "proves", "targets", "executedBy", "holds", "records"}
TERMINAL = {"CONVERGED", "REJECTED", "CANCELLED", "QUIESCENT"}
ALLOWED = {"WAITING": {"READY", "UNKNOWN", "BLOCK"}, "UNKNOWN": {"WAITING", "BLOCK"}, "BLOCK": {"WAITING", "UNKNOWN"}, "READY": {"PROGRESSING", "WAITING", "BLOCK", "UNKNOWN"}, "PROGRESSING": {"CONVERGED", "WAITING", "BLOCK", "UNKNOWN", "CANCELLED"}, "DRAINING": {"QUIESCENT", "CANCELLED"}}
MOVEMENTS = {"REPOSITORY", "BASE", "CANDIDATE", "MODEL", "PLAN", "NONE"}

def _missing(value, fields):
    return not isinstance(value, dict) or any(field not in value for field in fields)

def _add(errors, reason):
    if reason not in errors:
        errors.append(reason)

def validate_case(case):
    errors = []
    for link in case.get("links", []):
        source = link.get("source", "").split(":", 1); target = link.get("target", "").split(":", 1); provenance = link.get("provenance")
        if (set(link) != {"source", "target", "relation", "generation", "provenance"} or len(source) != 2 or len(target) != 2 or source[0] not in KNOWN or target[0] not in KNOWN or link.get("relation") not in LINKS or not link.get("generation") or _missing(provenance, {"source", "observedHead", "digest"}) or not provenance.get("digest", "").startswith("sha256:")):
            _add(errors, "typed link endpoint" if len(source) == 2 and len(target) == 2 and (source[0] not in KNOWN or target[0] not in KNOWN) else "typed link")
    evidence = case.get("evidence")
    if evidence is not None:
        provenance = evidence.get("provenance") if isinstance(evidence, dict) else None
        if (_missing(evidence, {"identity", "provenance", "disposition", "fresh"}) or _missing(provenance, {"source", "observedHead", "digest"}) or not provenance.get("digest", "").startswith("sha256:") or not isinstance(evidence.get("fresh"), bool)): _add(errors, "evidence shape")
        if evidence.get("disposition") not in {"CREATE_NEW", "REUSE", "REJECT", "SUPERSEDE"}: _add(errors, "disposition")
    envelope = case.get("envelope")
    if envelope is not None and _missing(envelope, {"identity", "outcome", "plan", "role", "executor", "objective", "inputs", "allowed", "forbidden", "claims", "evidence", "receipt"}): _add(errors, "envelope shape")
    request, grant = case.get("request"), case.get("grant")
    if request is not None:
        if _missing(request, {"identity", "action", "subject", "actor", "outcome", "claims", "inputs", "preconditions", "idempotency"}): _add(errors, "request shape")
        if "text" in request and "ignore policy" in request["text"].lower() and grant is not None: _add(errors, "hostile text grant")
    if grant is not None:
        if _missing(grant, {"identity", "request", "action", "subject", "actor", "claims", "issuer", "policy", "validity", "revocation"}): _add(errors, "grant shape")
        elif request is not None and any(grant.get(field) != request.get(field) for field in ("request", "action", "subject", "actor", "claims")): _add(errors, "grant binding")
        if isinstance(grant, dict) and (not grant.get("validity", {}).get("expires") or grant.get("revocation") not in (None, "")): _add(errors, "grant validity")
    elif case.get("admission") == "READY": _add(errors, "default deny")
    receipt = case.get("receipt")
    if receipt is not None and (_missing(receipt, {"identity", "request", "subject", "grant", "started", "ended", "result", "evidence", "predicates", "sideEffects", "next"}) or (request is not None and receipt.get("request") != request.get("identity")) or (grant is not None and receipt.get("grant") != grant.get("identity")) or (receipt.get("result") == "SUCCEEDED" and not receipt.get("evidence"))): _add(errors, "receipt binding/completeness")
    predicate = case.get("predicate")
    if predicate is not None and (_missing(predicate, {"identity", "subject", "evidence", "result"}) or not isinstance(predicate.get("result"), bool)): _add(errors, "predicate shape")
    plan = case.get("plan")
    if plan is not None and (_missing(plan, {"identity", "generation", "outcomes", "predicates", "dependencies", "assumptions", "claims", "admission"}) or not plan.get("generation", "").startswith("sha256:")): _add(errors, "plan shape")
    assumption = case.get("assumption")
    if assumption is not None:
        scope = assumption.get("scope") if isinstance(assumption, dict) else None
        if (_missing(assumption, {"identity", "subject", "scope", "evidence", "state", "fresh", "movement", "acceptanceValid", "reconcile"}) or _missing(scope, {"repository", "base", "candidate", "model", "plan"}) or not assumption.get("evidence") or not isinstance(assumption.get("fresh"), bool) or assumption.get("movement") not in MOVEMENTS): _add(errors, "assumption identity/evidence/freshness")
        if assumption.get("movement") != "NONE" and (assumption.get("state") != "INVALIDATED" or assumption.get("acceptanceValid") or case.get("admission") == "READY" or case.get("scheduled") or case.get("claims")): _add(errors, "assumption movement effects")
        if case.get("admission") == "READY" and (assumption.get("state") != "SATISFIED" or not assumption.get("fresh") or assumption.get("reconcile") != "fresh"): _add(errors, "assumption admission")
    transition = case.get("transition")
    if transition:
        if transition[1] not in ALLOWED.get(transition[0], set()): _add(errors, "transition")
        if transition[0] in TERMINAL: _add(errors, "terminal reopened")
        if transition[1] == "READY" and transition[0] in {"UNKNOWN", "BLOCK"}: _add(errors, "fresh reconciliation")
    if case.get("readOnly") and case.get("repairLoop"): _add(errors, "read-only repair loop")
    if case.get("admission") in {"UNKNOWN", "BLOCK"} and case.get("scheduled"): _add(errors, "admission consistency")
    checkpoint = case.get("checkpoint")
    if checkpoint is not None and (_missing(checkpoint, {"generation", "claims", "receipts", "position", "state", "evidence", "pending"}) or not checkpoint.get("generation", "").startswith("sha256:")): _add(errors, "checkpoint shape")
    quiescence = case.get("quiescence")
    if quiescence is not None and (any(quiescence.get(field, 1) != 0 for field in ("ready", "running", "blockers", "claims")) or not quiescence.get("completeReceipts") or (checkpoint and checkpoint.get("pending"))): _add(errors, "quiescence")
    drain = case.get("drain")
    if drain is not None and (not drain.get("stopped") or drain.get("pending") or not drain.get("receipts")): _add(errors, "drain completion")
    replay = case.get("replay")
    if replay is not None and (not replay.get("position") or replay.get("sideEffects") is not False): _add(errors, "replay safety")
    return errors
