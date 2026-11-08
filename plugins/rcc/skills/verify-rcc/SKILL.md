---
name: verify-rcc
description: "Verify the RCC plugin from its source checkout on Linux: drive real RCC CLI health, onboarding and local SDK fixtures, offline Tealdeer, and packaged skill discovery with retained evidence. Use after RCC guidance, helper, or distribution changes."
---

# Verify RCC

RCC's primary surface is a short-lived CLI, not a server. Use the existing source-checkout harnesses and the executable `scripts/verify-rcc` helper. Read [the feature map](features/README.md), choose the affected journey, and keep unrun journeys explicitly unverified. This verifier adds to the eight existing RCC skills; it does not replace their routing.

The canonical skill lives at `plugins/rcc/skills/verify-rcc/`. `.cursor/skills/verify-rcc`, `skills/verify-rcc`, and `.agents/skills/verify-rcc` are discovery symlinks. An installed bundle alone lacks repository tests: pass `--repo` pointing at the plugins source checkout rather than improvising replacement tests.

## Launch

Run on Linux (Bluefin host or the project's Linux container), from the plugins repository root, with Python 3.11+ and Git. RCC must already be installed; Tealdeer is needed only for `quickref`. Use `$rcc-core` if RCC is missing; do not install or upgrade host tools implicitly.

Check `df -h "$HOME"` and, on btrfs, `btrfs filesystem usage /var` before dependency provisioning or onboarding. Stop below 20 GiB free, 5 GiB unallocated, or above 90% metadata usage. Onboarding provisions its template's real contained environment and can consume gigabytes.

Create one new scratch directory. Reuse the repository's validation Python if available; otherwise prepare the declared dependencies in this disposable venv. These commands use only the venv, not host site-packages:

```sh
verification_run=$(mktemp -d "${TMPDIR:-/tmp}/codex-verify-rcc-XXXXXXXX")
python3 -m venv "$verification_run/venv"
verification_python="$verification_run/venv/bin/python"
"$verification_python" -m pip install -r requirements-dev.txt
verify_rcc="$PWD/plugins/rcc/skills/verify-rcc/scripts/verify-rcc"
verification_evidence="$PWD/docs/verification/rcc/$(date -u +%Y%m%dT%H%M%SZ)-${verification_run##*-}"
"$verification_python" "$verify_rcc" launch --run-dir "$verification_run" --evidence-dir "$verification_evidence"
```

`Ready:` means dependencies are importable and an owned, isolated run with an external evidence directory exists. Run Doctor before driving. Each run has its own RCC home, XDG directories, temporary files, and explicit test Codex home. There are no migrations, accounts, ports, or persistent server to launch. Separate runs can coexist; an exclusive run lock refuses concurrent driving of the same scratch directory.

If provisioning fails before `launch` creates its ownership marker, remove only the scratch directory created by this sequence. After launch, use Cleanup even when Doctor or a journey fails. Do not reuse a partially completed run as a fresh proof.

## Doctor

```sh
"$verification_python" "$verify_rcc" doctor --run-dir "$verification_run"
```

Require exit zero and `Doctor passed`. It reads the recorded source inputs, imports validation dependencies, and runs `rcc version`; it does not resolve project dependencies or authenticate. RCC's version transcript identifies the actual binary. Missing tools or changed inputs require a fresh run, not a silent host-Python fallback. This helper uses POSIX process groups and file locks; native Windows execution is not qualified.

## Drive

Choose a feature from the map; run it once per fresh run:

```sh
"$verification_python" "$verify_rcc" drive health --run-dir "$verification_run"
```

Other literal feature names are `onboarding`, `quickref`, and `distribution`; their recipes and prerequisites are in the map. `onboarding` runs the real generated Python task plus FileAdapter producer/consumer and dummy FileSecrets fixtures. Python socket networking is denied within compatibility tasks after RCC environment provisioning. `distribution` runs repository checks and plugin contracts; that is not native marketplace installation or client-upgrade acceptance.

## Evidence

Keep the directory named by `verification_evidence` outside scratch. It contains `receipt.json` with the base revision, source-input hashes, Python identity, selected journey status, and cleanup result. `command-*.json` captures each command, working directory, stdout, stderr, and exit code. `health-state.json` or `quickref-state.json` records inspected side effects; onboarding retains its nested command transcripts and actual task/queue artifacts.

Proof requires the real user command and resulting state: final task PASS plus exit zero and files, queue payloads after each stage, or page lookup plus ownership-safe removal. A test count alone does not prove a native client install. A skipped prerequisite is unverified, never PASS through another entry point. Use only dummy fixture secrets; do not add live credentials or user robot output to evidence. No dry-run mode is claimed here: provisioning can download dependencies, while offline TLDR lookup is checked against unchanged configuration and an empty download cache.

## Cleanup

The helper waits for each command, kills only the process group it started on interruption/timeout, and releases its run lock. Remove the owned run, including its disposable venv and RCC cache, with:

```sh
"$verification_python" "$verify_rcc" cleanup --run-dir "$verification_run"
test ! -e "$verification_run"
test -s "$verification_evidence/receipt.json"
```

Cleanup validates the ownership marker and separate evidence path before deleting scratch. It never deletes the evidence directory. Record the retained evidence path and size in the handoff. Use `/maintain-verification-skill` to keep this map and its journeys aligned with future changes.

## Helpers

`scripts/verify-rcc` is executable; all four invocations (`launch`, `doctor`, `drive`, `cleanup`) appear above. Run `python3 "$verify_rcc" --help` for its arguments. It requires the source checkout and declared validation dependencies, preserves command evidence on failure, and never starts Dagger or changes the caller's shell environment.
