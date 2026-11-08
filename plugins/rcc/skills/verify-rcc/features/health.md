# RCC health and failure boundary

A user can inspect RCC health and diagnose an invalid robot environment without executing its task or touching an existing project.

## Sub-features

- `health-diagnostics`: quick JSON diagnostics return successfully from the installed CLI.
- `health-reject`: environment resolution rejects a missing config file.
- `health-no-task`: the failing resolution produces no task marker.

## How to get to it (user POV)

- Run `rcc diagnostics --quick --json` for machine health.
- Run `rcc ht vars -r robot.yaml --json` to diagnose a robot before executing it.

## Driving it with verify-rcc

Preconditions: Launch and Doctor passed; `rcc` is installed. No task dependencies, credentials, or remote backend are required.

- **Drive both entry points:** run `"$verification_python" "$verify_rcc" drive health --run-dir "$verification_run"`. It runs real quick diagnostics, creates a disposable robot pointing to `missing-environment.yaml`, and requests its environment.
- **Inspect diagnosis:** command evidence shows diagnostics exit zero, resolution exit nonzero, and the missing filename in the error.
- **Inspect side effects:** `health-state.json` reports `missing_environment_rejected: true` and `task_marker_exists: false`. Receipt status is `health: passed` only when both checks hold.

## Gotchas

- A diagnostics PASS does not prove a robot's packages or task behavior.
- The missing-environment failure is intentional. An unexpected successful resolution fails this journey.
- All writable RCC state and the robot fixture belong to this run; do not substitute a user's failing robot into the helper.
