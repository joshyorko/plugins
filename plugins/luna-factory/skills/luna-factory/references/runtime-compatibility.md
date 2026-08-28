# Runtime compatibility

Read this reference when starting on a new Codex version, routing differs from expectations, nested delegation matters, or effective model/effort must be proven.

## Audit procedure

Reuse the verified snapshot when the runtime signature matches and observed behavior remains consistent. The signature covers Codex version, multi-agent mode, relevant `[agents]` configuration, model catalog routing fields, and verified nesting/metadata behavior.

Re-audit when no verified snapshot exists, the signature changes, or a spawn/result contradicts the snapshot:

1. Run `python scripts/audit_runtime.py`.
2. Inspect the current live collaboration tool schemas. Confirm spawn model/effort fields, fork constraints, follow-up semantics, wait bounds, and available agent roles.
3. Read the configured concurrency cap; treat it as capacity, not a target.
4. Perform a harmless clean-context spawn if the task requires delegation.
5. Verify effective routing only from runtime/session metadata that names the executed model or effort. Preserve the source. A requested field or worker self-identification is not equivalent evidence.
6. Test nested delegation only if the planned graph needs it. Otherwise use a flat owner-to-workers graph.
7. Test a Sol override before relying on ESCALATE. On failure or unverifiable execution, return a decision packet for manual review.

## Current observed snapshot (2026-08-26)

This snapshot is evidence from Codex CLI 0.149.1 on one installation, not a portable contract:

- `codex debug models` listed `gpt-5.6-luna` with `low`, `medium`, `high`, `xhigh`, and `max`; default `medium`; catalog multi-agent version `v1`.
- It listed `gpt-5.6-terra` and `gpt-5.6-sol` with `low` through `ultra`; catalog multi-agent version `v2`.
- `features.multi_agent` was stable/enabled; `features.multi_agent_v2` was stable/disabled.
- Local config capped spawned threads at 12, excluding the primary. The live session advertised 13 total slots.
- The root live `spawn_agent` schema exposed optional model and reasoning overrides, with fork/history constraints; the wait primitive was event-oriented.
- Direct Luna spawns created depth-1 sessions. Their rollout `turn_context` identified both `model: gpt-5.6-luna` and the effective `effort`, so direct model/effort overrides were verifiable after execution.
- A new Luna Medium root successfully spawned one clean-context child with model/effort omitted and completed one native event wait. This proved the spawn/wait path and that inheritance was requested; child effective model/effort remained unverified because the result exposed no routing metadata.
- Depth-1 Luna children spawned from the existing root had no collaboration primitives. Use a flat root-to-worker graph here; do not assume deeper nesting merely because a Luna root can spawn.
- A direct Sol Low override completed and its rollout `turn_context` verified both `gpt-5.6-sol` and `effort: low`.
- Agent-list status did not expose effective model or effort.

Re-audit when the signature changes or evidence contradicts these conclusions; do not rediscover an unchanged verified runtime on every factory invocation.

## Safe degradation

| Missing capability | Behavior |
|---|---|
| No subagent tools | SOLO; report that SPLIT is unavailable |
| No nested spawn | Flat owner-to-worker graph |
| No effort override | Use inherited/default behavior and mark effort unverified |
| No effective metadata | Record request separately; make no savings claim |
| No Sol override | Produce the bounded decision packet for manual/external Sol |
| Spawn rejects a model | Surface the error; never substitute a family silently |
| Wait primitive absent | Do not busy-poll; keep useful local work or report the lifecycle limitation |
