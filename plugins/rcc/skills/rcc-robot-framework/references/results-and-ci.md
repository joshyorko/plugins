# Results and CI

Use the official guides for [parsing results](https://docs.robotframework.org/docs/parsing_results), [flaky tests](https://docs.robotframework.org/docs/flaky_tests), [parallel execution](https://docs.robotframework.org/docs/parallel), and [GitHub Actions](https://docs.robotframework.org/docs/using_rf_in_ci_systems/ci/github-actions).

Robot’s `output.xml` is the result source; `log.html` and `report.html` are human-facing artifacts. Generate xUnit for CI when needed. Process `output.xml` with `ExecutionResult` and a `ResultVisitor`, rather than scraping the HTML. Use `rebot` to post-process results; rerun failures with `--rerunfailed output.xml`, then combine the initial and rerun outputs with `rebot --merge initial/output.xml rerun/output.xml`.

For focused feedback, run one suite/file/test/tag with its own `--outputdir`, retain that `output.xml`, and upload output XML, log, report, and xUnit as CI artifacts. A rerun is a second pass after the initial output is complete, never a concurrent rewrite of its result directory.

Pabot can split execution by suite or test, but the current RCC acceptance suite must run serially: its root setup uses shared `tmp/` paths, sets `ROBOCORP_HOME=tmp/robocorp`, and mutates holotree. Pabot becomes safe only after the suite derives `ROBOCORP_HOME`, temporary roots, fixtures, and cleanup targets from a real worker identity, or CI executes truly isolated repository copies provisioned by concrete commands. PabotLib locks can serialize genuinely shared setup but do not make a shared RCC home, holotree, or `tmp/` safe.

When those prerequisites do not exist, provide runnable serial initial, `--rerunfailed`, and `rebot --merge` commands. Describe Pabot as blocked future work with the required suite or CI changes. Do not claim an undefined listener, hook, wrapper, or executable helper makes a command runnable.

Every RCC CI/Pabot answer must include an **Acceptance contracts** statement: `Step` still asserts the exact exit code, stdout and stderr remain separate, and JSON stdout is parsed structurally with `Must Be Json Response`/`Parse JSON` rather than substring matching.

Treat the official GitHub Actions sample as conceptual: its package/action pins and `continue-on-error` behavior may be stale or mask failures. Pin and validate dependencies in the target repository, propagate the first-run failure status after artifacts are collected, and merge rerun results only after a successful rerun/merge policy is explicit.

For RCC Environment Artifacts, retain the exact binary SHA, source commit, Artifact digest, platform, cold/warm/provider-dead outcomes, compatibility rejection, native import, and lease-release result in machine receipts. Report source/unit, built-binary, native runtime, push, merge, tag, hosted release, and installed-asset gates separately. A release-candidate aggregate cannot hide platform skips, and lifecycle Robot evidence does not imply coordination/prewarm CLI acceptance.
