# First ChatGPT dogfood recovery

The first preserved `sandbox-test` run failed at the provider request, before an
owner acceptance report was produced. The provider rejected the native response
schema because candidate assumption objects lacked explicit properties. Strict
structured output now uses closed objects with explicit required keys, including
empty assumption maps and the known current assumption keys in actual dispatch.

The locally served catalog contained reconciliation, but its app-only visibility
removed it from ChatGPT's callable model catalog. Reconciliation is now typed,
read-only and visible to both contexts. The production-path regression filters
the same catalog by model visibility before checking the advertised primary action.

Pasted acceptance/non-goal headings and bullet markers are normalized in the New
Run form. Existing stored criteria are not rewritten: removing a mandatory item
from an admitted run would silently change its original goal and evidence binding.

## Ownership and recovery limits

Native loaded-thread lists are process-local. Executor's list does not describe a
Luna-owned stdio app-server. Read-only probes in the service cwd and exact trusted
repository cwd both read the preserved thread and failed turn; neither fresh probe
had it loaded, and background-terminal reads returned thread-not-found. Cwd alone
does not supply original-session ownership or cessation proof.

The original live run remains evidence. Reconciliation never repeats generation 1,
changes its deadline/repair budget, substitutes a new owner, or treats an unloaded
thread, failed turn, empty queue or missing terminal observation as stopped proof.
Same-session unknown ownership can be reconciled once fresh native observations
prove all owned execution idle. After a stdio session is lost/restarted, the runtime
fails closed with `stdio_process_lifetime_unknown_after_restart`; persisted history
does not prove that its old writers stopped. Existing-daemon recovery remains a
separate explicitly configured transport, not an automatic stdio fallback.

Do not cancel, delete, reset or redispatch the preserved dogfood run to pass a check.
Keep its claim and snapshots when original-session cessation remains unproved.
Do not replace the live service around unknown owned execution or retained claims.
The source repair is not live/native recovery proof until those conditions hold.
