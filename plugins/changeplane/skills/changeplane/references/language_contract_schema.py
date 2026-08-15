"""Small, declarative CP-LANGUAGE validator used by the semantic fixtures."""

KNOWN = {"ApplicationModel", "Component", "Outcome", "Candidate", "Predicate", "Evidence", "AgentExecutor", "Dispatch", "ResourceClaim", "ActionRequest", "Repository", "Invariant", "ChangePlan", "Assumption", "AuthorityGrant", "ActionReceipt"}
LINKS = {"contains", "declares", "dependsOn", "affects", "implements", "proves", "targets", "executedBy", "holds", "records"}
RELATION_TYPES = {"contains": {("ApplicationModel", "Component"), ("ChangePlan", "Outcome")}, "declares": {("ApplicationModel", "Invariant")}, "dependsOn": {("Outcome", "Outcome")}, "affects": {( "Outcome", target) for target in KNOWN}, "implements": {("Candidate", "Outcome")}, "proves": {("Evidence", "Predicate")}, "targets": {("Dispatch", "Outcome")}, "executedBy": {("Dispatch", "AgentExecutor")}, "holds": {("Dispatch", "ResourceClaim")}, "records": {("ActionReceipt", "ActionRequest")}}
TERMINAL = {"CONVERGED", "REJECTED", "CANCELLED", "QUIESCENT"}
ALLOWED = {"WAITING": {"READY", "UNKNOWN", "BLOCK"}, "UNKNOWN": {"WAITING", "BLOCK"}, "BLOCK": {"WAITING", "UNKNOWN"}, "READY": {"PROGRESSING", "WAITING", "BLOCK", "UNKNOWN"}, "PROGRESSING": {"CONVERGED", "WAITING", "BLOCK", "UNKNOWN", "CANCELLED"}, "DRAINING": {"QUIESCENT", "CANCELLED"}}
MOVEMENTS = {"REPOSITORY", "BASE", "CANDIDATE", "MODEL", "PLAN", "NONE"}

def _missing(value, fields):
    return not isinstance(value, dict) or any(field not in value for field in fields)

def _add(errors, reason):
    if reason not in errors:
        errors.append(reason)

def _identity(value):
    if not isinstance(value, str) or not value.strip():
        return False
    if value.startswith("sha256:"):
        digest = value[7:]
        return len(digest) == 64 and all(char in "0123456789abcdef" for char in digest)
    return True

def _check_identity(errors, value):
    if not _identity(value):
        _add(errors, "record identity")

def _check_exact_identity(errors, value):
    if not isinstance(value, str) or not value.startswith("sha256:") or not _identity(value):
        _add(errors, "exact identity")

def _bound_evidence(case, evidence_ids):
    evidence = case.get("evidence")
    return isinstance(evidence, dict) and evidence.get("identity") in evidence_ids and evidence.get("fresh") is True and evidence.get("disposition") != "REJECT"

def validate_case(case):
    errors = []
    for link in case.get("links", []):
        source = link.get("source", "").split(":", 1); target = link.get("target", "").split(":", 1); provenance = link.get("provenance")
        relation = link.get("relation")
        if (set(link) != {"source", "target", "relation", "generation", "provenance"} or len(source) != 2 or len(target) != 2 or source[0] not in KNOWN or target[0] not in KNOWN or relation not in LINKS or not link.get("generation") or _missing(provenance, {"source", "observedHead", "digest"}) or not _identity(provenance.get("digest"))):
            _add(errors, "typed link endpoint" if len(source) == 2 and len(target) == 2 and (source[0] not in KNOWN or target[0] not in KNOWN) else "typed link")
        elif (source[0], target[0]) not in RELATION_TYPES[relation]:
            _add(errors, "typed link relation")
    evidence = case.get("evidence")
    if evidence is not None:
        provenance = evidence.get("provenance") if isinstance(evidence, dict) else None
        if (_missing(evidence, {"identity", "provenance", "disposition", "fresh"}) or not _identity(evidence.get("identity")) or _missing(provenance, {"source", "observedHead", "digest"}) or not _identity(provenance.get("digest")) or not isinstance(evidence.get("fresh"), bool)): _add(errors, "evidence shape")
        if evidence.get("disposition") not in {"CREATE_NEW", "REUSE", "REJECT", "SUPERSEDE"}: _add(errors, "disposition")
    envelope = case.get("envelope")
    if envelope is not None and _missing(envelope, {"identity", "outcome", "plan", "role", "executor", "objective", "inputs", "allowed", "forbidden", "claims", "evidence", "receipt"}): _add(errors, "envelope shape")
    request, grant = case.get("request"), case.get("grant")
    if request is not None:
        if _missing(request, {"identity", "action", "subject", "actor", "outcome", "claims", "inputs", "preconditions", "idempotency"}): _add(errors, "request shape")
        _check_identity(errors, request.get("identity"))
        if "text" in request and "ignore policy" in request["text"].lower() and grant is not None: _add(errors, "hostile text grant")
    if grant is not None:
        if _missing(grant, {"identity", "request", "action", "subject", "actor", "claims", "issuer", "policy", "validity", "revocation"}): _add(errors, "grant shape")
        elif request is not None and any(grant.get(field) != request.get(field) for field in ("request", "action", "subject", "actor", "claims")): _add(errors, "grant binding")
        _check_identity(errors, grant.get("identity"))
        if isinstance(grant, dict) and (not grant.get("validity", {}).get("expires") or grant.get("revocation") not in (None, "")): _add(errors, "grant validity")
    elif case.get("admission") == "READY": _add(errors, "default deny")
    receipt = case.get("receipt")
    if receipt is not None and (_missing(receipt, {"identity", "request", "subject", "grant", "started", "ended", "result", "evidence", "predicates", "sideEffects", "next"}) or not _identity(receipt.get("identity")) or (request is not None and (receipt.get("request") != request.get("identity") or receipt.get("subject") != request.get("subject"))) or (grant is not None and receipt.get("grant") != grant.get("identity")) or (receipt.get("result") == "SUCCEEDED" and not receipt.get("evidence"))): _add(errors, "receipt binding/completeness")
    predicate = case.get("predicate")
    if predicate is not None and (_missing(predicate, {"identity", "subject", "evidence", "result"}) or not isinstance(predicate.get("result"), bool)): _add(errors, "predicate shape")
    plan = case.get("plan")
    if plan is not None and (_missing(plan, {"identity", "generation", "outcomes", "predicates", "dependencies", "assumptions", "claims", "admission"}) or not _identity(plan.get("identity")) or not plan.get("generation", "").startswith("sha256:")): _add(errors, "plan shape")
    if plan is not None and "generation" in plan: _check_exact_identity(errors, plan.get("generation"))
    subject = case.get("subject")
    if subject is not None:
        fields = {"repository", "base", "candidate", "model", "plan", "materialization"}
        if _missing(subject, fields) or not all(_identity(subject.get(field)) for field in fields):
            _add(errors, "exact subject binding")
        scope = (case["assumption"].get("scope") if isinstance(case.get("assumption"), dict) else {})
        if any(scope.get(field) != subject.get(field) for field in ("repository", "base", "candidate", "model", "plan")):
            _add(errors, "exact subject binding")
        if isinstance(case.get("evidence"), dict) and case["evidence"].get("subject") not in (None, subject):
            _add(errors, "exact subject binding")
    movement = case.get("movement")
    if movement is not None and (
        _missing(movement, {"old", "new", "predecessor"})
        or not all(_identity(movement.get(field)) for field in ("old", "new", "predecessor"))
        or movement.get("old") != movement.get("predecessor")
        or case.get("acceptanceValid")
        or case.get("admission") == "READY"
        or case.get("scheduled")
        or case.get("claims")
    ):
        _add(errors, "movement effects")
    assumption = case.get("assumption")
    if assumption is not None:
        scope = assumption.get("scope") if isinstance(assumption, dict) else None
        if (_missing(assumption, {"identity", "subject", "scope", "evidence", "state", "fresh", "movement", "acceptanceValid", "reconcile"}) or not _identity(assumption.get("identity")) or _missing(scope, {"repository", "base", "candidate", "model", "plan"}) or not all(_identity(scope.get(field)) for field in ("repository", "base", "candidate", "model", "plan")) or not assumption.get("evidence") or not isinstance(assumption.get("fresh"), bool) or assumption.get("movement") not in MOVEMENTS): _add(errors, "assumption identity/evidence/freshness")
        if assumption.get("movement") != "NONE" and (assumption.get("state") != "INVALIDATED" or assumption.get("acceptanceValid") or case.get("admission") == "READY" or case.get("scheduled") or case.get("claims") or not assumption.get("admissionCancelled") or not assumption.get("claimsReleased") or assumption.get("reconcile") != "fresh"): _add(errors, "assumption movement effects")
    if case.get("admission") == "READY" and (assumption.get("state") != "SATISFIED" or not assumption.get("fresh") or assumption.get("reconcile") != "fresh"): _add(errors, "assumption admission")
    if case.get("admission") == "READY" and plan is not None:
        if case.get("subject") and (not isinstance(case.get("capacity"), dict) or case["capacity"].get("available") is not True or case["capacity"].get("reserved") is not True):
            _add(errors, "admission capacity")
        if case.get("subject") and (not isinstance(grant, dict) or not case.get("claims") or any(not isinstance(claim, dict) for claim in case.get("claims", []))):
            _add(errors, "admission grant/claim")
        accepted = case.get("acceptedDependencies", [])
        accepted_by_outcome = {item.get("outcome"): item for item in accepted if isinstance(item, dict)}
        for dependency in plan.get("dependencies", []):
            proof = accepted_by_outcome.get(dependency)
            if not isinstance(proof, dict) or proof.get("plan") != plan.get("identity") or proof.get("candidate") != case.get("subject", {}).get("candidate") or proof.get("passed") is not True or not _bound_evidence(case, proof.get("evidence", [])):
                _add(errors, "admission dependencies")
        if plan.get("dependencies") and not accepted_by_outcome:
            _add(errors, "admission dependencies")
        claim_sets = [
            case.get("envelope", {}).get("claims", []) if isinstance(case.get("envelope"), dict) else [],
            case.get("request", {}).get("claims", []) if isinstance(case.get("request"), dict) else [],
            case.get("grant", {}).get("claims", []) if isinstance(case.get("grant"), dict) else [],
        ]
        if any(not set(claims).issubset(set(plan.get("claims", []))) for claims in claim_sets):
            _add(errors, "claim confinement")
    claims = case.get("claims")
    if claims is not None and any(isinstance(claim, dict) for claim in claims):
        claim_names = [claim.get("claim") for claim in claims if isinstance(claim, dict)]
        owner = case.get("request", {}).get("actor") if isinstance(case.get("request"), dict) else None
        if len(claim_names) != len(set(claim_names)) or any(not claim.get("owner") or claim.get("owner") != owner for claim in claims if isinstance(claim, dict)):
            _add(errors, "claim ownership")
        if len(claim_names) != len(claims):
            _add(errors, "claim ownership")
    if case.get("admission") == "READY" and case.get("subject") and claims is not None:
        owner = case.get("request", {}).get("actor") if isinstance(case.get("request"), dict) else None
        if not claims or any(not isinstance(claim, dict) or claim.get("owner") != owner for claim in claims):
            _add(errors, "claim ownership")
    transition = case.get("transition")
    if transition:
        if transition[1] not in ALLOWED.get(transition[0], set()): _add(errors, "transition")
        if transition[0] in TERMINAL: _add(errors, "terminal reopened")
        if transition[1] == "READY" and transition[0] in {"UNKNOWN", "BLOCK"}: _add(errors, "fresh reconciliation")
    if case.get("readOnly") and case.get("repairLoop"): _add(errors, "read-only repair loop")
    if case.get("admission") in {"UNKNOWN", "BLOCK"} and case.get("scheduled"): _add(errors, "admission consistency")
    if case.get("admission") == "READY" and case.get("scheduled") is False: _add(errors, "admission consistency")
    checkpoint = case.get("checkpoint")
    if checkpoint is not None and (_missing(checkpoint, {"generation", "claims", "receipts", "position", "state", "evidence", "pending"}) or not checkpoint.get("generation", "").startswith("sha256:")): _add(errors, "checkpoint shape")
    if checkpoint is not None and (not isinstance(checkpoint.get("generation"), str) or len(checkpoint["generation"]) != 71 or not _identity(checkpoint["generation"])): _add(errors, "checkpoint identity")
    if checkpoint is not None and plan is not None and (checkpoint.get("generation") != plan.get("generation") or checkpoint.get("subject") != case.get("subject", {}).get("candidate") or checkpoint.get("receipts") != ([receipt.get("identity")] if isinstance(receipt, dict) else []) or checkpoint.get("evidence") != ([evidence.get("identity")] if isinstance(evidence, dict) else [])): _add(errors, "checkpoint binding")
    if receipt is not None and (not receipt.get("predicates") or predicate is None or (isinstance(plan, dict) and set(receipt.get("predicates", [])) != set(plan.get("predicates", []))) or not set(receipt.get("predicates", [])).issubset({predicate.get("identity")}) or not _bound_evidence(case, predicate.get("evidence", [])) or predicate.get("result") is not True): _add(errors, "receipt predicates")
    if predicate is not None and (not predicate.get("evidence") or not _bound_evidence(case, predicate.get("evidence", []))): _add(errors, "predicate evidence")
    quiescence = case.get("quiescence")
    if quiescence is not None and (any(quiescence.get(field, 1) != 0 for field in ("ready", "running", "blockers", "claims")) or not quiescence.get("completeReceipts") or (checkpoint and checkpoint.get("pending"))): _add(errors, "quiescence")
    if quiescence is not None and (checkpoint is None or quiescence.get("checkpoint") != checkpoint.get("generation") or not quiescence.get("drain")): _add(errors, "quiescence binding")
    drain = case.get("drain")
    if drain is not None and (not drain.get("stopped") or drain.get("pending") or not drain.get("receipts")): _add(errors, "drain completion")
    if drain is not None and any(drain.get(field, 0) for field in ("activeWriters", "activeWork", "pendingEvents", "unrecordedReceipts")): _add(errors, "drain completion")
    if drain is not None and checkpoint is not None and (drain.get("checkpoint") != checkpoint.get("generation") or drain.get("receipts") != checkpoint.get("receipts")): _add(errors, "drain receipts")
    replay = case.get("replay")
    if replay is not None and (not replay.get("position") or replay.get("sideEffects") is not False): _add(errors, "replay safety")
    if replay is not None and checkpoint is not None and (replay.get("checkpoint") != checkpoint.get("generation") or replay.get("position") != checkpoint.get("position")): _add(errors, "replay binding")
    return errors
