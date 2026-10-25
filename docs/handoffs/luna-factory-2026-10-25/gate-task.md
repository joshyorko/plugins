Implement the first #71 slice: persisted exact-profile execution qualification enforced at every native effect path, plus a qualification-only canary harness.
Worktree: /home/kdlocpanda/second_brain/Areas/plugins-worktrees/luna-factory-qualification-gate (branch luna-factory/qualification-gate, at origin/main e194b01, clean; #87 merged there).
RESUME NOTE: two earlier attempts were interrupted before writing anything to disk, so start writing code early. CHECKPOINT RULE: after each numbered item below, make a conventional local commit (for example after the module, after enforcement, after the CLI, after tests) so another interruption loses nothing. Push and open the draft PR as soon as the full validation passes.

Verified context on main (line numbers were taken at 57dbdfb; #87 shifted lifecycle.rs slightly, so locate by name):
- Factory::start (server/src/lifecycle.rs near line 937) admits and then calls launch(). launch() calls connect(), then native thread/start and turn/start. There is no qualification check anywhere on that path.
- capabilities() hard-codes execution {eligible:false,...} (near line 907), and schemas.rs makes eligible const false.
- start_factory is model-visible, so a direct MCP call bypasses the UI's disabled Start.
- Native write sends:
  - start_thread_guarded and start_skill_turn_with_id_guarded, in launch near 1011/1061 and in dispatch_existing_owner_locked near 1640, which continuation, repair and resume dispatch use
  - steer_turn, in steer_at_revision near 2348
  - resume_thread, near 2628
- The dispatch_guard closure (near 2280) runs immediately before the first byte.
- Containment (interrupt_turn, terminal stop), reconcile, startup passive recovery, cancel, planning (create_factory_graph/propose/apply) and reads must stay available when execution is denied.

Required:

1. New module server/src/qualification.rs.
   - ExecutionProfile, built from Config plus the profile alias, with canonical JSON and a sha256 fingerprint. Fields:
     - factory_version: CARGO_PKG_VERSION
     - transport: from config.native_transport
     - codex_binary: absolute path string
     - codex_binary_identity: file size plus modification time, or "unavailable"
     - native_socket: optional
     - model: crate::native::LUNA_MODEL
     - profile alias
     - effort
   - A persisted table, execution_qualifications, in the existing SQLite DB.
     - Use CREATE TABLE IF NOT EXISTS. Do not bump user_version; older binaries must still open the DB.
     - Columns: fingerprint (PK), profile_alias, and a JSON payload: {profile, scope: ordinary|qualification_only, max_capacity (default 1), evidence_sha256, evidence_ref, recorded_at, expires_at?, revoked_at?}.
   - A status function returning Qualified or Denied(reason), with these stable codes:
     - execution_not_qualified
     - execution_profile_mismatch
     - execution_qualification_revoked
     - execution_qualification_expired
     - execution_qualification_scope
     - execution_capacity_not_qualified
   - A synchronous check that opens a fresh read-only connection to config.database, for use inside the pre-wire guard closures, so that a revocation by another process takes effect before the next byte. A missing table means not qualified.

2. Fail-closed enforcement.
   - (a) Factory::start: check before store.admit. Return the reason code with no run row, no claim, no receipt, and zero native process spawn or connect.
   - (b) launch(): check at the top, and re-check inside the dispatch_guard closure using the read-only DB check.
   - (c) dispatch_existing_owner_locked: check at the top, before any Dispatch or EffectIntent event or receipt is written. Leave a clear, honest blocker ("execution_not_qualified: <reason>; nothing was dispatched"). Never mark an effect unknown when nothing was sent.
   - (d) steer_at_revision and the resume_thread path: check before EffectIntent and before the send.
   - (e) Do NOT gate containment, reconcile, startup recovery, cancel, planning or reads.

3. capabilities().
   - execution.eligible is true only when some configured profile has a current ordinary qualification.
   - Add execution.reason, plus execution.profiles: [{alias, status, reason, max_capacity}]. No paths.
   - Update schemas.rs (eligible becomes boolean) and the output-schema tests. Keep the UI green; the UI already treats eligible as boolean.

4. Operator-only CLI in server/src/main.rs. There is no MCP write path.
   - qualification-profile --config C --profile A: prints the profile and its fingerprint.
   - record-qualification --config C --profile A --evidence FILE --scope ordinary|qualification_only [--max-capacity 1] [--expires-in-days N]:
     - The evidence JSON must carry profile_fingerprint equal to the current fingerprint.
     - Scope ordinary also requires qualification_run.outcome == "passed" produced by the harness below.
     - Store the evidence sha256 and path. Refuse otherwise.
   - revoke-qualification --config C --fingerprint FP.
   - qualifications --config C: lists records, with no secrets.

5. qualification-canary --config C --profile A --workspace DIR [--output FILE].
   - Requires a current qualification_only grant for that exact profile.
   - Refuses a DIR that isn't a disposable git repository distinct from every configured repository root.
   - Runs one bounded owner turn through the real native client path, with capacity 1 and a tiny wall budget, on a fixed canary objective such as writing a known file.
   - Verifies the result independently: Luna computes the file hash itself, rather than trusting the worker's report. Requires verified cessation.
   - Emits evidence JSON: {profile_fingerprint, qualification_run: {outcome, started_at, finished_at, checks, cessation}}.
   - It must never admit or modify ordinary runs, use repository aliases, or be reachable via MCP. A qualification_only grant must never let start_factory, continuation, steer or resume run.
   - If the full harness is too large, implement grant and scope enforcement plus a precondition-validating command that refuses honestly with "not yet implemented". Never fake success.

6. Tests: new server/tests/qualification.rs, plus updates, using the existing fake native fixtures and tests/support. Use a fake codex binary or app server that records every invocation and request to a file.
   - Public MCP start_factory (through the existing HTTP/MCP test harness) and direct Factory::start, each with a missing, revoked, expired, mismatched (changed effort or binary), qualification_only, and over-capacity grant: each returns the exact code, creates no run, and makes zero native spawns or requests.
   - Continuation, repair, resume and steer of a run admitted while qualified, after revocation: refused, with zero additional sends.
   - Revocation while a dispatch awaits readiness: the fake delays before thread/start or turn/start, and the test revokes through a separate connection. The pre-wire guard refuses and zero start bytes go out.
   - Planning works with no qualification.
   - Cancel and interrupt still work after revocation.
   - A qualification_only grant cannot run an ordinary start.
   - The harness refuses a missing grant and a non-disposable workspace.
   - capabilities reports eligible true and false correctly.
   - CLI refusals for fingerprint mismatch and missing evidence.
   - Existing execution tests must keep passing. Add a support helper (for example support::qualified(config) -> Config) that records an ordinary qualification for the test profile, and apply it only where tests legitimately execute. Do not qualify planning tests.

7. Docs.
   - New docs/execution-qualification.md covering:
     - the record model, reason codes, CLI runbook and harness contract
     - what remains for full #71: live subscription inference, real callback ownership, crash windows, independent native checks, and cessation on the operator runtime
   - Update control-wire.md, mission-control.md and the README pointer. Add the new paths to ALLOWED_RELEASE_DELTA.

8. Validation: one flock-locked cargo invocation at a time with the caps in the v2 brief, then the UI and repo checks.
9. Draft PR against main.
   - Title: "feat(luna-factory): enforce persisted execution qualification at every effect path (#71)".
   - Body: "Part of #71. Part of #61." with an honest residual list.
   - Never enable execution, never record a qualification in any real database, and never run live inference.