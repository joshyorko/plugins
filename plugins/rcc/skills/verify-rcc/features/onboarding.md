# First PASS and local SDK fixtures

A user scaffolds a real listed Python starter, sees its first PASS and artifacts, and runs a file-backed producer/consumer chain with test-only upstream Vault compatibility.

## Sub-features

- `onboarding-catalog`: select `01-python` only if the installed catalog lists it.
- `onboarding-preserve`: refuse destinations with visible data, hidden files, or Git history.
- `onboarding-pass`: run the exact generated task key and inspect its greeting and log.
- `local-chain`: feed producer output into a separate consumer output file.
- `local-vault`: read a dummy secret through explicit FileSecrets with Python networking denied.

## How to get to it (user POV)

- Run `rcc robot initialize --list` and `rcc robot initialize -t 01-python -d new-project`.
- Read the task key from the generated `robot.yaml`, then use `rcc task run --robot robot.yaml --task` with that exact key.
- Supply explicit FileAdapter/FileSecrets env files with `rcc task run ... -e devdata/env-producer.json`, then the consumer env file.

## Driving it with verify-rcc

Preconditions: Launch and Doctor passed; sufficient disk space and package-download access for the installed starter's unmodified environment pins. The real template must list `01-python` and include `robocorp` SDK dependencies.

- **Drive all entry points:** run `"$verification_python" "$verify_rcc" drive onboarding --run-dir "$verification_run"`. It runs `plugins/rcc/tests/test_onboarding.py` with disposable private RCC state and a separately retained evidence directory.
- **Prove the starter:** nested command records show the actual catalog, initialization, exact generated task key, exit zero, and final PASS. Inspect retained `onboarding/greeting.txt` and `onboarding/log.html`.
- **Prove preservation:** three initializer calls must fail while the existing visible/hidden/Git sentinel is unchanged; `onboarding/preservation.json` records the checked cases.
- **Prove the chain:** inspect `onboarding/producer-output.json` for `fixture-1` with `status: new`, then `onboarding/consumer-output.json` for the same ID with `processed: true`. Producer and consumer command transcripts must each show PASS and exit zero. The producer also asserts the dummy FileSecrets value; compatibility tasks block Python socket connects and DNS resolution.

## Gotchas

- Missing templates, dependency downloads, or incompatible pins are failures/preconditions to report, not permission to generate a replacement starter or use host Python.
- The safety fixture uses a dummy `.git/HEAD` sentinel; it does not modify a real repository's history.
- An item context can record a failed item while the runner reports PASS. Inspect exact output payloads as well as runner status.
- The fixture proves upstream SDK compatibility only, not production secrets, distributed queue semantics, hosted Control Room access, or all `actions-work-items` adapters.
