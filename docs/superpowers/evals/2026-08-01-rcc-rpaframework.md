# RCC RPA Framework evaluations

## Baseline 1

Prompt:

```text
Build an RCC-managed Robot Framework task that reads a CSV into a table, validates JSON, creates a ZIP artifact, and uploads through a browser. Choose the RPA Framework libraries and show robot.yaml/conda.yaml implications without copying stale dependency pins.
```

Response:

````text
```yaml
# robot.yaml
tasks:
  Run:
    shell: robot --outputdir output tasks.robot
condaConfigFile: conda.yaml
artifactsDir: output
```

```yaml
# conda.yaml
channels:
  - conda-forge
dependencies:
  - python
  - pip
  - pip:
      - rpaframework
```

```robot
*** Settings ***
Library    RPA.Tables
Library    RPA.JSON
Library    RPA.Archive
Library    RPA.Browser.Selenium

*** Variables ***
${CSV}          ${CURDIR}${/}input.csv
${JSON}         ${CURDIR}${/}payload.json
${SCHEMA}       ${CURDIR}${/}schema.json
${ARTIFACTS}    ${CURDIR}${/}output
${ZIP}          ${ARTIFACTS}${/}submission.zip
${UPLOAD_URL}   %{UPLOAD_URL}

*** Tasks ***
Create and upload submission
    ${table}=     Read table from CSV    ${CSV}    header=${TRUE}
    Should Not Be Empty    ${table}
    ${document}=  Load JSON from file    ${JSON}
    ${schema}=    Load JSON from file    ${SCHEMA}
    Validate JSON by schema    ${document}    ${schema}
    Create Directory    ${ARTIFACTS}
    Copy File    ${CSV}    ${ARTIFACTS}
    Copy File    ${JSON}   ${ARTIFACTS}
    Archive Folder With Zip    ${ARTIFACTS}    ${ZIP}
    Open Available Browser    ${UPLOAD_URL}
    Choose File    css:input[type="file"]    ${ZIP}
    Click Element    css:button[type="submit"]
    Close All Browsers
```

Use `RPA.Tables` for CSV parsing, `RPA.JSON` for schema validation, `RPA.Archive` for ZIP creation, and `RPA.Browser.Selenium` for browser upload. Keep Python and `rpaframework` unpinned unless the project has tested compatibility requirements; RCC resolves them from the configured channels.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| Correct library selection | PASS | Tables, JSON, Archive, and a browser library are named. |
| RCC environment boundary | PASS | `robot.yaml`/`conda.yaml` and no host installation. |
| Current-version verification | FAIL | No live metadata or release-note check. |
| Archive safety | FAIL | Archives its own output folder without review or traversal policy. |
| Browser initialization | FAIL | Does not establish project/browser-engine initialization. |
| `RPA_*`/`RC_*` separation | PASS | Neither family is mixed. |
| Hosted-adapter framing | PASS | No adapter is involved. |
| Migration restraint | PASS | No migration is proposed. |

## Baseline 2

Prompt:

```text
Review an RCC robot that combines RPA.Robocorp.WorkItems, robocorp.workitems, RPA.Robocorp.Vault, and robocorp.vault in one run. Explain which environment contracts conflict, what should remain for maintenance, and what a safe migration boundary looks like.
```

Response:

```text
These are two separate runtime stacks, not interchangeable APIs.

`RPA.Robocorp.WorkItems` is the Robot Framework/RPA Framework keyword-library stack; `robocorp.workitems` is the newer Python API stack. They differ in adapter configuration, input reservation/completion lifecycle, output-item buffering, and serialization expectations. Using both against one work-item run can cause each library to manage the same input independently or publish incompatible/duplicate outputs.

Likewise, `RPA.Robocorp.Vault` and `robocorp.vault` use separate secret-manager configuration contracts (including distinct environment-variable namespaces and manager implementations). Configuring both makes the effective secret source ambiguous and can yield different behavior locally versus in Control Room.

For maintenance, retain the RPA Framework libraries only for existing Robot Framework robots that already depend on their keyword APIs, and keep their configuration isolated. Do not mix their work-item or vault calls with the `robocorp.*` Python packages in the same robot execution.

A safe migration boundary is a whole robot/process boundary: migrate one robot at a time to `robocorp.workitems` and `robocorp.vault`, update its RCC/environment configuration and tests together, then remove its `RPA.Robocorp.*` usage. If shared business logic is needed, pass plain data across the boundary rather than sharing active work-item handles, output-item state, or vault clients.
```

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| Correct library selection | PASS | Correctly distinguishes stacks. |
| RCC environment boundary | PASS | Requires isolated configuration. |
| Current-version verification | PASS | No version claim. |
| Archive safety | PASS | No archive operation. |
| Browser initialization | PASS | No browser operation. |
| `RPA_*`/`RC_*` separation | FAIL | Namespaces are not identified. |
| Hosted-adapter framing | FAIL | Local versus hosted adapter/auth boundary is omitted. |
| Migration restraint | PASS | Whole robot/process migration is recommended. |

At least one baseline fails two or more rubric rows: baseline 1 fails three and baseline 2 fails two.

## Forward 1 (first run)

Prompt:

```text
Use $rcc-rpaframework at plugins/rcc/skills/rcc-rpaframework/SKILL.md.

Build an RCC-managed Robot Framework task that reads a CSV into a table, validates JSON, creates a ZIP artifact, and uploads through a browser. Choose the RPA Framework libraries and show robot.yaml/conda.yaml implications without copying stale dependency pins.
```

Response:

````text
```yaml
# robot.yaml
artifactsDir: output

tasks:
  Upload validated submission:
    shell: robot --outputdir output tasks.robot
```

```yaml
# conda.yaml
channels:
  - conda-forge

dependencies:
  - python=3.11
  - pip
  - pip:
      - rpaframework
```

```robot
# tasks.robot
*** Settings ***
Library    RPA.Tables
Library    RPA.JSON
Library    RPA.FileSystem
Library    RPA.Archive
Library    RPA.Browser.Playwright

*** Variables ***
${ARTIFACTS}      %{ROBOT_ARTIFACTS}
${INPUT_CSV}      ${CURDIR}${/}input${/}customers.csv
${INPUT_JSON}     ${CURDIR}${/}input${/}payload.json
${SCHEMA_JSON}    ${CURDIR}${/}input${/}payload.schema.json
${REVIEWED_DIR}   ${ARTIFACTS}${/}reviewed
${ZIP_FILE}       ${ARTIFACTS}${/}submission.zip
${UPLOAD_URL}     %{UPLOAD_URL}

*** Tasks ***
Upload validated submission
    Create Directory    ${ARTIFACTS}
    Create Directory    ${REVIEWED_DIR}

    ${customers}=    Read Table From Csv    ${INPUT_CSV}    header=${TRUE}
    Sort Table By Column    ${customers}    customer_id
    Write Table To Csv    ${customers}    ${REVIEWED_DIR}${/}customers.csv

    ${payload}=    Load JSON From File    ${INPUT_JSON}
    ${schema}=    Load JSON From File    ${SCHEMA_JSON}
    Validate JSON By Schema    ${payload}    ${schema}
    Copy File    ${INPUT_JSON}    ${REVIEWED_DIR}${/}payload.json

    # Archive only the two known, generated/reviewed files; do not extract uploads.
    Archive Folder With Zip    ${REVIEWED_DIR}    ${ZIP_FILE}

    New Browser    chromium    headless=${TRUE}
    New Page    ${UPLOAD_URL}
    Upload File By Selector    input[type="file"]    ${ZIP_FILE}
    Take Screenshot    path=${ARTIFACTS}${/}upload.png
    Close All Browsers
```

Use `RPA.Tables` for CSV, `RPA.JSON` for schema validation, `RPA.Archive` for the bounded ZIP, and `RPA.Browser.Playwright` for the new upload flow. Keep `rpaframework` unpinned unless the project already maintains a tested lock/freeze; resolve it through RCC with `rcc run`, not a host `pip install`. If this project’s Browser Playwright setup requires engines, run `rfbrowser init` within that RCC-managed Browser project only.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| Correct library selection | PASS | Uses Tables, JSON, Archive, and Browser Playwright. |
| RCC environment boundary | PASS | Uses `robot.yaml`/`conda.yaml` and `rcc run`, not host pip. |
| Current-version verification | FAIL | Avoids a pin but does not state a live metadata/release-note check. |
| Archive safety | PASS | Archives only reviewed generated inputs and does not extract uploads. |
| Browser initialization | PASS | Limits `rfbrowser init` to a qualifying Browser project. |
| `RPA_*`/`RC_*` separation | PASS | No incompatible environment stacks are mixed. |
| Hosted-adapter framing | PASS | No adapter is involved. |
| Migration restraint | PASS | No migration is proposed. |

Revision: require every implementation/review answer to name the live check used for a version claim, or explicitly state that it makes no current-version claim.

## Forward 1 (rerun)

Prompt:

```text
Use $rcc-rpaframework at plugins/rcc/skills/rcc-rpaframework/SKILL.md.

Build an RCC-managed Robot Framework task that reads a CSV into a table, validates JSON, creates a ZIP artifact, and uploads through a browser. Choose the RPA Framework libraries and show robot.yaml/conda.yaml implications without copying stale dependency pins.
```

Response:

````text
`tasks.robot`

```robot
*** Settings ***
Library    RPA.Tables
Library    RPA.JSON
Library    RPA.Archive
Library    RPA.FileSystem
Library    RPA.Browser.Playwright

*** Variables ***
${ARTIFACTS}       %{ROBOT_ARTIFACTS}
${INPUT_CSV}       ${CURDIR}${/}input.csv
${PAYLOAD_JSON}    ${CURDIR}${/}payload.json
${SCHEMA_JSON}     ${CURDIR}${/}schema.json
${STAGING_DIR}     ${ARTIFACTS}${/}submission
${ARCHIVE}         ${ARTIFACTS}${/}submission.zip

*** Tasks ***
Prepare and upload submission
    ${table}=      Read table from CSV    ${INPUT_CSV}    header=${TRUE}
    Write table to CSV    ${table}    ${ARTIFACTS}${/}records.csv

    ${payload}=    Load JSON from file    ${PAYLOAD_JSON}
    ${schema}=     Load JSON from file    ${SCHEMA_JSON}
    Validate JSON by schema    ${payload}    ${schema}

    Remove Directory    ${STAGING_DIR}    recursive=${TRUE}
    Create Directory    ${STAGING_DIR}
    Copy File    ${ARTIFACTS}${/}records.csv    ${STAGING_DIR}${/}records.csv
    Copy File    ${PAYLOAD_JSON}    ${STAGING_DIR}${/}payload.json
    Archive Folder With Zip    ${STAGING_DIR}    ${ARCHIVE}

    New Browser    chromium    headless=${TRUE}
    New Page    %{UPLOAD_URL}
    Upload File By Selector    input[type=file]    ${ARCHIVE}
    Take Screenshot    path=${ARTIFACTS}${/}upload.png
```

`robot.yaml`

```yaml
tasks:
  Prepare and upload submission:
    shell: robot --outputdir output tasks.robot

condaConfigFile: conda.yaml
artifactsDir: output
```

`conda.yaml`

```yaml
channels:
  - conda-forge

dependencies:
  - python
  - pip
  - pip:
      - rpaframework
```

Use `RPA.Tables`, `RPA.JSON`, `RPA.Archive`, and `RPA.Browser.Playwright`; `RPA.FileSystem` keeps ZIP inputs explicitly bounded to the artifact directory. Let the existing RCC environment resolve Python and `rpaframework`; no package-version claim or stale pin is being made. Run `rfbrowser init` only if this Robot Framework Browser project’s documented setup requires browser-engine installation.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| Correct library selection | PASS | Uses Tables, JSON, Archive, FileSystem, and Browser Playwright appropriately. |
| RCC environment boundary | FAIL | The RCC config omits the project-managed `robotframework-browser` and Node.js dependencies required by the imported Browser library. |
| Current-version verification | PASS | Explicitly makes no version claim or stale pin. |
| Archive safety | PASS | Copies known files into a bounded staging directory and does not extract untrusted input. |
| Browser initialization | FAIL | It scopes `rfbrowser init` correctly but does not provide a runnable RCC-managed initialization path or declare its engine-install prerequisites. |
| `RPA_*`/`RC_*` separation | PASS | No incompatible environment stacks are mixed. |
| Hosted-adapter framing | PASS | No adapter is involved. |
| Migration restraint | PASS | No migration is proposed. |

### Forward 1 final-review correction

This is an honest rescore, not a fresh-context rerun. The stored rerun remains useful for its library choices and use of `RPA.FileSystem` keywords, but its environment and browser-initialization rows were over-scored. A corrected project contract adds the missing dependencies and an explicit, project-scoped engine setup task:

```yaml
# conda.yaml
channels:
  - conda-forge
dependencies:
  - python
  - nodejs
  - pip
  - pip:
      - rpaframework
      - robotframework-browser
```

```yaml
# robot.yaml (additional one-time setup task for this Browser project)
tasks:
  Initialize Browser Engines:
    shell: rfbrowser init
  Prepare and upload submission:
    shell: robot --outputdir output tasks.robot

condaConfigFile: conda.yaml
artifactsDir: output
```

Run `rcc run -t "Initialize Browser Engines"` when provisioning this Robot Framework Browser project. It installs browser engines inside the RCC-managed environment and is not a setup step for unrelated RCC, Selenium, or generic Playwright projects. The project freeze should retain the tested Python, Node.js, `rpaframework`, `robotframework-browser`, and browser-engine resolution; this correction makes no current-version claim.

| Rubric | Verdict | Evidence-backed correction |
| --- | --- | --- |
| Correct library selection | PASS | The response uses the task-owning RPA libraries, and `RPA.FileSystem` keywords are actually used for staging. |
| RCC environment boundary | PASS | Both Python packages and Node.js are declared in `conda.yaml`, with no host installation. |
| Current-version verification | PASS | The correction explicitly makes no current-version claim or copied pin. |
| Archive safety | PASS | Only known generated files are archived; no untrusted archive is extracted. |
| Browser initialization | PASS | The setup task runs `rfbrowser init` through this RCC-managed Robot Framework Browser project only. |
| `RPA_*`/`RC_*` separation | PASS | No incompatible environment stacks are mixed. |
| Hosted-adapter framing | PASS | No adapter is involved. |
| Migration restraint | PASS | No migration is proposed. |

## Forward 2

Prompt:

```text
Use $rcc-rpaframework at plugins/rcc/skills/rcc-rpaframework/SKILL.md.

Review an RCC robot that combines RPA.Robocorp.WorkItems, robocorp.workitems, RPA.Robocorp.Vault, and robocorp.vault in one run. Explain which environment contracts conflict, what should remain for maintenance, and what a safe migration boundary looks like.
```

Response:

```text
`RPA.Robocorp.WorkItems` expects `RPA_WORKITEMS_*`; `robocorp.workitems` expects `RC_WORKITEM_*`. They must not share work-item reservation, release, or output lifecycles. `RPA.Robocorp.Vault` likewise expects `RPA_SECRET_*`, while `robocorp.vault` uses `RC_VAULT_*`; running both makes secret-manager selection and hosted adapter/auth contracts ambiguous.

For a maintained Robot Framework robot, retain the legacy RPA Framework work-item and vault stack with its established `RPA_*` configuration. Keep `RPA.RobotLogListener` registered before secret retrieval.

Migrate at a complete robot/process boundary: one side owns item acquisition, completion, and outputs; each process uses exactly one environment family and secret-manager contract. Pass only serialized/plain data across the boundary—never live work-item or vault objects—and validate both local fixtures and hosted adapter/auth behavior with secrets redacted.
```

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| Correct library selection | PASS | Correctly maps all four libraries. |
| RCC environment boundary | PASS | Requires one environment family per process. |
| Current-version verification | PASS | Makes no current-version claim. |
| Archive safety | PASS | No archive operation. |
| Browser initialization | PASS | No browser operation. |
| `RPA_*`/`RC_*` separation | PASS | Names all four exact namespace families. |
| Hosted-adapter framing | PASS | Requires hosted adapter/auth validation beyond local fixtures. |
| Migration restraint | PASS | Preserves maintained legacy robots and migrates by process. |
