# Robot Framework authoring and execution

Consult the [Robot Framework User Guide](https://robotframework.org/robotframework/latest/RobotFrameworkUserGuide.html) first; it is the source of truth for language syntax and command-line behavior. Use the [project-structure example](https://docs.robotframework.org/docs/examples/project_structure) and [style guide](https://docs.robotframework.org/docs/style_guide) for layout and naming.

## Source hierarchy and structure

Read configuration and existing resources before a suite: `__init__.robot` supplies directory-suite settings and setup; suite `.robot` files contain cohesive tests/tasks; resource files contain shared variables and user keywords; Python libraries provide implementation-level helpers. Keep the suite’s public language readable and move repeatable process/control logic into a resource keyword or a small custom [Python library](https://docs.robotframework.org/docs/extending_robot_framework/custom-libraries/python_library).

Use `*** Settings ***` for Library, Resource, Suite/Test Setup and Teardown; `*** Variables ***` for values; `*** Test Cases ***` or `*** Tasks ***` for executable cases; and `*** Keywords ***` for reusable behavior. Define suite/test setup and teardown explicitly when state must be initialized or cleaned.

Variables include scalar `${value}`, list `@{items}`, dictionary `&{options}`, and environment `%{NAME}` variables. Prefer a passed scalar/dictionary to implicit global state; read environment values only where the environment boundary is part of the behavior.

Use native `IF` / `ELSE IF` / `ELSE` / `END`, `FOR` / `END`, and `TRY` / `EXCEPT` / `FINALLY` / `END` to keep expected control flow visible. A test template turns a command matrix into one keyword plus data rows; tags identify selections, and runtime `Skip` documents an intentionally inapplicable platform/capability case.

## Process example

The [Process library](https://robotframework.org/robotframework/latest/libraries/Process.html) captures return code and streams, while [OperatingSystem](https://robotframework.org/robotframework/latest/libraries/OperatingSystem.html) manages fixtures and environment variables.

```robot
*** Settings ***
Library         Process
Resource        resources.robot

*** Test Cases ***
Invalid hash keeps JSON stdout separate
    ${result}=    Run Process    build/rcc    ht    hash    --json    missing.yaml    stdout=PIPE    stderr=PIPE
    Should Be Equal As Integers    ${result.rc}    2
    Should Not Be Empty    ${result.stderr}
    ${payload}=    Parse JSON    ${result.stdout}
    Dictionary Should Contain Key    ${payload}    error
    Assert Hash Error    ${payload}
```

`Assert Hash Error` is a resource keyword: keep domain assertions there, but preserve the explicit status/stdout/stderr contract in the test. For an RCC-contained project, execute through its declared RCC environment rather than installing libraries into the host interpreter.

## Running and artifacts

Use `robot --dryrun suite.robot` before execution when checking parsing, imports, and control flow. Select scope with a file/directory suite, `--suite`, `--test`, `--include`, or `--exclude`; use focused scope first. Put outputs in an explicit `--outputdir tmp/output` (or an isolated CI directory).

Robot emits `output.xml` as the machine-readable execution result and produces `log.html` and `report.html`; request xUnit with `--xunit xunit.xml` when CI consumes it. Preserve these files as CI artifacts rather than parsing HTML for assertions.
