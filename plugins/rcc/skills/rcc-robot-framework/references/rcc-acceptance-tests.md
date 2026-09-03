# RCC Acceptance Tests

This guidance is pinned to released [`joshyorko/rcc` v18.19.3 at `4148c2b71705c9d2baf0e88b48d08a79cb7bda0f`](https://github.com/joshyorko/rcc/tree/4148c2b71705c9d2baf0e88b48d08a79cb7bda0f/robot_tests). Inside an RCC checkout, its repository-local `AGENTS.md` and `docs/skills/rcc-development/SKILL.md` are authoritative.

Inspect the root suite initializer/resources/support library and the nearest suite before editing. Environment Artifact work additionally requires:

- `robot_tests/environment_artifacts.robot`: source-level lifecycle acceptance;
- `robot_tests/environment_artifacts/library.py`: provider, archive, compatibility, and receipt helpers; and
- `robot_tests/environment_artifact_binary.robot`: exact released/built binary A-to-B acceptance.

Run from the RCC checkout after building `build/rcc`:

```bash
python3 -m robot -L DEBUG -d tmp/output robot_tests/environment_artifacts.robot
python3 -m robot -L DEBUG -d tmp/output robot_tests/environment_artifact_binary.robot
python3 -m robot -L DEBUG -d tmp/output robot_tests/robot_bundle.robot
python3 -m robot -L DEBUG -d tmp/output robot_tests
```

Prefer the checked-in RCC toolkit when its dependencies are not already present:

```bash
rcc run -r developer/toolkit.yaml --dev -t unitTests
rcc run -r developer/toolkit.yaml -t robot
```

`robot_tests/__init__.robot` owns root setup/teardown; `resources.robot` supplies the shared command DSL. `Step` preserves stdout/stderr and asserts the expected exit code. Select the intended stream, parse JSON structurally, and never accept a machine contract through substring matching. `Fire And Forget` is unasserted and belongs only in deliberate best-effort setup/cleanup.

The root suite uses shared mutable `tmp/` and RCC state, so run serially unless CI creates truly isolated copies and homes. Keep adversarial files/archives in Python helpers, normalize platform line endings deliberately, and review golden changes.

## Environment Artifact Acceptance

A release-quality runtime case proves, with the exact candidate binary:

1. producer A builds and publishes a real representative environment;
2. consumer B uses a different empty private `ROBOCORP_HOME`;
3. B acquires, verifies, materializes, leases, and executes without package-network installation;
4. provider-dead warm reuse succeeds;
5. representative Python/native imports such as SQLite succeed;
6. incompatibility fails before provider object fetch;
7. execution receipts are complete and the lease is released; and
8. Artifact identity remains independent of provider and local paths.

Keep Linux amd64, Windows amd64, macOS amd64, and macOS arm64 results separate. Platform-specific skips are expected and must be named. A green compile, Go unit suite, source-level Robot suite, or one platform is not the native four-platform result.

The v18.19.3 tag run `33187409580` passed Build, Release Candidate Verification, all four native runtime jobs, and hosted release creation at exact SHA `4148c2b...`. This is upstream release evidence, not a substitute for local verification of a new change. The artifact suite proves lifecycle behavior; it does not prove the complete `env coordinate` CLI or cross-platform prewarm.
