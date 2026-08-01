# RCC task recipes

Set `${ARTIFACTS}` from `ROBOT_ARTIFACTS` (or the project’s configured `artifactsDir`). Resolve all packages and browser requirements through the project’s `conda.yaml` or declared freeze; do not use host installation.

## CSV to table

```robot
*** Settings ***
Library    RPA.Tables

*** Tasks ***
Transform CSV
    ${table}=    Read table from CSV    ${CURDIR}${/}input.csv    header=${TRUE}
    Sort Table By Column    ${table}    customer_id
    Write table to CSV    ${table}    %{ROBOT_ARTIFACTS}${/}customers.csv
```

## JSON validation

```robot
*** Settings ***
Library    RPA.JSON

*** Tasks ***
Validate payload
    ${payload}=    Load JSON from file    ${CURDIR}${/}payload.json
    ${schema}=     Load JSON from file    ${CURDIR}${/}schema.json
    Validate JSON by schema    ${payload}    ${schema}
```

## Safe archives

Before extraction, enumerate members and reject absolute paths, `..` segments, and any resolved destination outside `${ARTIFACTS}`. Then create/extract only reviewed inputs:

```robot
*** Settings ***
Library    RPA.Archive

*** Tasks ***
Create reviewed ZIP
    Archive Folder With Zip    %{ROBOT_ARTIFACTS}${/}reviewed    %{ROBOT_ARTIFACTS}${/}submission.zip
```

Do not treat `RPA.Archive` extraction as a traversal defense; verify current release behavior and enforce the member policy in a project helper before extracting untrusted ZIP/TAR files.

## HTTP download artifact

```robot
*** Settings ***
Library    RPA.HTTP

*** Tasks ***
Download report
    Download    ${REPORT_URL}    target_file=%{ROBOT_ARTIFACTS}${/}report.csv    overwrite=${TRUE}
```

## Browser upload with evidence

```robot
*** Settings ***
Library    RPA.Browser.Playwright

*** Tasks ***
Upload archive
    New Browser    chromium    headless=${TRUE}
    New Page    ${UPLOAD_URL}
    Upload File By Selector    input[type=file]    %{ROBOT_ARTIFACTS}${/}submission.zip
    Take Screenshot    path=%{ROBOT_ARTIFACTS}${/}upload.png
    ${download}=    Promise To Wait For Download
    Click    text=Download receipt
    Save Download    ${download}    %{ROBOT_ARTIFACTS}${/}receipt.pdf
```

Initialize with `rfbrowser init` only when this is a Robot Framework Browser project and the project documentation requires its browser engines. Verify the current Browser Playwright API: stale examples commonly use obsolete initialization and keyword forms.

## Choose a document/data integration

```robot
*** Settings ***
Library    RPA.Excel.Files
Library    RPA.PDF
Library    RPA.Email.ImapSmtp
Library    RPA.Database

*** Tasks ***
Select the owning library
    Open Workbook    ${CURDIR}${/}source.xlsx
    Get Text From PDF    ${CURDIR}${/}source.pdf
    Connect To Database Using Custom Params    ${DATABASE_CONNECT_STRING}
```

Use only the library needed by the task; the grouped imports demonstrate selection, not a default bundle. Configure mail TLS/OAuth and database drivers through the RCC environment and keep exports under artifacts.

## Secret-safe logging

```robot
*** Settings ***
Library    RPA.RobotLogListener
Library    RPA.Robocorp.Vault

*** Tasks ***
Use a secret without exposing it
    Register Protected Keywords    Get Secret
    ${secret}=    Get Secret    service-account
    # Pass fields directly to the client; never Log ${secret} or its values.
```

Register protection before sensitive calls. For a modern Python robot, use its logging/redaction APIs and retain no secret value in screenshots, artifacts, exceptions, or HTTP diagnostics.
