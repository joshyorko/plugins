# LF-07 bounded continuation

LF-07 extends the existing owner-turn lifecycle under issue #61. It does not create a scheduler, queue, daemon, provider or separate workflow database. Planning-only graphs remain non-executable. All fixtures run isolated fake native processes; no production worker or preserved held execution is used.

## Explicit owner semantics, structural authority

An owner report may request routine continuation using `continuation: null | {kind: "next" | "repair", task_id, diagnosis: null | {summary, check_refs}}`. The generated native schema remains closed with every property required; missing continuation in historical reports means no automatic work. A directive is data to validate against the admitted run, never new authority.

Only a live completed owner event with a valid current-subject QUIESCENT report and null blocker can trigger automatic work. NEEDS_INPUT preserves its idempotent operator decision; BLOCKED, CONVERGED and legacy reports stop. New tasks must be owner-declared necessary, selected, READY, unattempted and currently admissible under dependencies, subject, assumptions, effect scope and the held repository claim. They consume no repair allowance. Repair must refer to the just-returned task and exact current, independently observed failed checks; model text cannot claim operator-semantic diagnosis. The server retains an observed-failure diagnosis without changing the objective or creating a new replan allowance.

## Existing ownership boundary

Before the next dispatch, validate original repository/profile/finish authority, the same owner, source identity, observed cessation of owner/descendants/terminal commands, absence of unknown effects, deadline and finite history/repair limits. Persist terminal state and the next attempt's durable intent before contacting native turn/start. Keep original intent lineage and increment dispatch generation exactly once.

Live continuation reuses the connected owner and current observer. It does not call the public resume method recursively or replace its own monitor. Direct recovery/startup/reconcile paths remain read-only with respect to inference. After correlation reattaches an observer to an active dispatch, a genuinely subsequent live completion may request new work under all guards; uncertain dispatches are correlated, never automatically replayed. Crashing between settlement and dispatch leaves a stopped run for explicit bounded continuation; crashing after intent leaves uncertainty for reconciliation.

## Verification boundary

Use deterministic subprocess/SQLite integration to prove two tasks with no external resume, scoped acceptance, bounded repair, independent work despite an unrelated blocked dependency, one real decision with replay-safe answer, unchanged deadline/lineage and fail-closed drift/unknown effects/liveness/budgets. Existing cancellation and crash/recovery suites must remain green. Fixture success is not live native or ChatGPT qualification.
