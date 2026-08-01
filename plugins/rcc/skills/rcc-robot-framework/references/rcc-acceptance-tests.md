# RCC acceptance tests

This guidance is pinned to [`joshyorko/rcc` commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`](https://github.com/joshyorko/rcc/tree/d5942d90994d7bd9034aeed6b88cc60fd7a3e330). Inspect the pinned [root suite initializer](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/__init__.robot), [root suite resources](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/resources.robot), [Python support library](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/supporting.py), [exit-code suite](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/exitcodes.robot), [hash suite](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/ht_hash.robot), [uv-native suite](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/uv_native.robot), [bundle suite](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/robot_bundle.robot), and [tasks.py](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/tasks.py).

Run locally from the RCC checkout after building `build/rcc`:

```bash
python3 -m robot -L DEBUG -d tmp/output robot_tests
python3 -m robot -L DEBUG -d tmp/output robot_tests/robot_bundle.robot
```

`robot_tests/__init__.robot` declares the root `Prepare Local` suite setup and `Clean Local` suite teardown; `resources.robot` implements those keywords and supplies the shared DSL. `Step` runs a command, preserves `${robot_stdout}` and `${robot_stderr}`, checks the expected exit code, and selects a stream with `Use Stdout` or `Use Stderr`; assertion keywords then target that selected stream. `Must Be Json Response` calls structural Python `Parse JSON`. `Fire And Forget` only logs its code/streams, so use it solely for deliberate cleanup/setup where an unchecked result is acceptable.

For a nonzero command, use `Step    command    expected_code`, then select stderr for the diagnostic and stdout for structural JSON parsing. Do not collapse streams or assert JSON with text fragments.

The root preparation places `ROBOCORP_HOME=tmp/robocorp`, removes mutable temporary paths, and initializes/revokes holotree state. The support library scrubs activation-environment variables for RCC subprocesses and normalizes `build/rcc` to the Windows executable spelling. Preserve both behaviors when adding helpers. Use suite-local fixtures for files and directories; make adversarial archives/input through Python helpers such as `create_traversal_bundle`, not fragile shell construction.

Golden outputs are created only when missing and otherwise compared after newline normalization. Review every golden diff before retaining it. Keep command matrix fixtures deterministic and place cleanup beside their suite. `uv_native.robot` shows platform tags and runtime `Skip`; `robot_bundle.robot` shows fixtures, intentional nonzero exits, and cleanup.
