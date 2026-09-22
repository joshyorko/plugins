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

## Current observed snapshot (2026-09-22)

Codex CLI 0.155.1 is authenticated in the executing environment. This is current runtime evidence, not a portable contract:

- `codex --version` → `codex-cli 0.155.1`.
- `codex login status` → `Logged in`.
- Authenticated `codex debug models` includes both `gpt-6-luna` and `gpt-5.6-luna`; GPT-6 Luna is the current intended request. No fallback is proposed.
- Generated app-server v1/v2 schemas expose `collaborationMode`, collaboration/list methods, and model plus reasoning-effort request fields.
- A native spawn canary completed with a parent requesting `gpt-6-luna` at `max`; rollout metadata identified the parent as `gpt-6-luna`/`max` and an inherited child as the selected route. The audit observed `gpt-6-luna`/`max` and reported `model: verified_match`, `reasoning_effort: verified_match`, and `overall: verified_match`.
- A direct audit with `--requested-model gpt-6-luna --requested-effort max` reported `routing_capability.status: explicit_request` and `fallback_proposed: false`.

Re-audit when the Codex version, model catalog, collaboration configuration, or observed routing changes. Catalog/request acceptance is not execution proof; retain session metadata as the effective-routing evidence.

## Historical snapshot (2026-08-26; not a current contract)

The following evidence came from Codex CLI 0.149.1 on one installation. It remains historical context only and must not determine current model selection:

- `codex debug models` listed `gpt-5.6-luna` with `low`, `medium`, `high`, `xhigh`, and `max`; default `medium`; catalog multi-agent version `v1`.
- It listed `gpt-5.6-terra` and `gpt-5.6-sol` with `low` through `ultra`; catalog multi-agent version `v2`.
- `features.multi_agent` was stable/enabled; `features.multi_agent_v2` was stable/disabled.
- Local config capped spawned threads at 12, excluding the primary. The live session advertised 13 total slots.
- The root live `spawn_agent` schema exposed optional model and reasoning overrides, with fork/history constraints; the wait primitive was event-oriented.
- Direct Luna spawns created depth-1 sessions whose rollout `turn_context` identified the requested model and effective effort, so direct overrides were verifiable after execution.
- A Luna root spawned a clean-context child with model/effort omitted, proving inheritance was requested; child effective model/effort remained unverified because the result exposed no routing metadata.
- Depth-1 Luna children had no collaboration primitives; use a flat root-to-worker graph unless current schemas prove otherwise.
- A direct Sol override verified model and effort in rollout metadata; agent-list status did not expose effective model or effort.

Re-audit when the signature changes or observed behavior contradicts these historical conclusions; do not rediscover an unchanged verified runtime on every invocation.

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
