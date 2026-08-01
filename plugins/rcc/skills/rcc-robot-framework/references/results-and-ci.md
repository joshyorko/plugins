# Results and CI

Use the official guides for [parsing results](https://docs.robotframework.org/docs/parsing_results), [flaky tests](https://docs.robotframework.org/docs/flaky_tests), [parallel execution](https://docs.robotframework.org/docs/parallel), and [GitHub Actions](https://docs.robotframework.org/docs/using_rf_in_ci_systems/ci/github-actions).

Robot’s `output.xml` is the result source; `log.html` and `report.html` are human-facing artifacts. Generate xUnit for CI when needed. Process `output.xml` with `ExecutionResult` and a `ResultVisitor`, rather than scraping the HTML. Use `rebot` to post-process results; rerun failures with `--rerunfailed output.xml`, then combine the initial and rerun outputs with `rebot --merge initial/output.xml rerun/output.xml`.

For focused feedback, run one suite/file/test/tag with its own `--outputdir`, retain that `output.xml`, and upload output XML, log, report, and xUnit as CI artifacts. A rerun is a second pass after the initial output is complete, never a concurrent rewrite of its result directory.

Pabot can split execution by suite or test. Use PabotLib locks around genuinely shared resources, but locks do not fix shared RCC process state. RCC acceptance suites may mutate `ROBOCORP_HOME`, holotree, and `tmp/`; Pabot is unsafe until every worker receives an isolated `ROBOCORP_HOME` and temporary/output root. Preserve the project’s intended holotree behavior within each worker, never by allowing workers to share a home. Run cleanup/setup serially or under a lock when it cannot be isolated.

Treat the official GitHub Actions sample as conceptual: its package/action pins and `continue-on-error` behavior may be stale or mask failures. Pin and validate dependencies in the target repository, propagate the first-run failure status after artifacts are collected, and merge rerun results only after a successful rerun/merge policy is explicit.
