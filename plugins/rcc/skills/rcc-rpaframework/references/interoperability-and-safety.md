# Interoperability and safety

| Concern | Legacy RPA Framework | Modern Robocorp Python | Safe boundary |
| --- | --- | --- | --- |
| Work item configuration | `RPA_WORKITEMS_*` with `RPA.Robocorp.WorkItems` | `RC_WORKITEM_*` with `robocorp.workitems` | One family per process; do not share reservation/release or output state. |
| Vault configuration | `RPA_SECRET_*` with `RPA.Robocorp.Vault` | `RC_VAULT_*` with `robocorp.vault` | One secret-manager contract per process; do not make both sources implicit. |
| Hosted adapters/auth | Legacy adapter and Control Room-compatible configuration | Modern adapter/auth configuration | Inspect the provider’s supported adapter/auth setup; local files are test fixtures, not a hosted-auth substitute. |
| Redaction | `RPA.RobotLogListener` protects sensitive Robot keywords | modern logging APIs/redactors | Register redaction before retrieval; never log secret payloads, auth headers, or screenshots containing them. |

## Intentional bridges and migration

An intentional bridge is exceptional and must be explicit: isolate the libraries in separate processes or lifecycle owners, define which side reserves/releases items, pass only serialized/plain data, and test both environments. Do not let both stacks acquire the same work item or independently buffer outputs.

For maintained legacy Robot Framework robots, preserve the RPA Framework keyword stack and its existing `RPA_*` configuration. Migrate one complete robot/process at a time to `robocorp.workitems` and `robocorp.vault`; update its project config, adapters, tests, and operations runbook together. Avoid a broad rewrite merely because modern packages exist.

Local mock secrets prove lookup shape only. They do not prove hosted adapter selection, service identity, authorization scopes, or production rotation behavior. Keep those checks at the hosted adapter/auth boundary, with secret values redacted.

## Archives, browsers, and native automation

`RPA.Archive` handles archive operations, but untrusted extraction has traversal risk. Enumerate archive members first, reject absolute paths and `..` components, resolve each output under the intended artifact directory, and verify current release notes/API behavior before relying on any library protection.

Treat Browser Playwright snippets as version-sensitive. Confirm the current documentation and project dependency configuration. Run `rfbrowser init` only for a Robot Framework Browser project that needs its browser engines; it is not needed for unrelated RCC robots, Selenium projects, or generic Playwright installations.

Desktop and OCR libraries require more than Python dependencies: validate supported OS, an interactive display/session, native automation packages, OCR engines/models, permissions, and the CI runner’s capabilities before making them part of an RCC robot.
