# RCC distribution and skill discovery

A user receives consistent RCC manifests and discoverable canonical skills across portable, Codex, Claude, Hermes, and repository discovery views.

## Sub-features

- `distribution-generated`: generated marketplaces, skill links, and Hermes shims are current.
- `distribution-contracts`: portable/client manifests agree and do not register the removed Dagger runtime.
- `distribution-discovery`: canonical RCC skills and this verifier resolve through generated links and Hermes registration.
- `distribution-recipes`: specialist recipe links and command safety contracts pass.

## How to get to it (user POV)

- Browse the RCC Automation marketplace entry and its packaged skill descriptions.
- Discover skills through `skills/`, `.agents/skills/`, or Hermes registration.
- As a maintainer, run `bin/check` and the RCC plugin test suite before distributing a change.

## Driving it with verify-rcc

Preconditions: Launch and Doctor passed; checked-out generated views are already built. Fix stale generated files with the three documented build scripts in a development checkout, then start a fresh verification run.

- **Drive:** run `"$verification_python" "$verify_rcc" drive distribution --run-dir "$verification_run"`. It runs `bash bin/check` and `python -m unittest discover -s plugins/rcc/tests -v` using the recorded verification Python.
- **Inspect discovery:** command evidence includes the real manifest/schema/skill validators, symlink assertions, Hermes `register` invocation, and recipe checks with successful exit codes.
- **Classify skips:** onboarding runtime tests are intentionally opt-in here; Tealdeer cases skip if unavailable. Run their mapped journey before claiming those behaviors. A green distribution check alone qualifies packaging contracts, not those runtime features.

## Gotchas

- Native marketplace installation, remote bootstrap downloads, graphical clients, Windows PowerShell installation, and actual native session hook execution are outside this journey. Do not describe validator/test evidence as installed-client acceptance.
- Existing unit tests isolate Codex command selection at the production CLI boundary with a stub. That proves command selection, not the real client's behavior.
- `bin/check` checks generated files without rebuilding them; stale views are a real failure.
