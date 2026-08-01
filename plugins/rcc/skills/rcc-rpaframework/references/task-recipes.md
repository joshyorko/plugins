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

## Safe archive creation and extraction

Create only reviewed files. For untrusted ZIP input, validate every member with standard Python path and ZIP metadata before allowing `RPA.Archive` to extract it:

```robot
*** Settings ***
Library    RPA.Archive

*** Tasks ***
Create reviewed ZIP
    Archive Folder With Zip    %{ROBOT_ARTIFACTS}${/}reviewed    %{ROBOT_ARTIFACTS}${/}submission.zip

Extract reviewed ZIP
    Extract Validated ZIP    ${CURDIR}${/}received.zip    %{ROBOT_ARTIFACTS}${/}extracted

*** Keywords ***
Extract Validated ZIP
    [Arguments]    ${archive}    ${destination}
    ${root}=    Evaluate    pathlib.Path($destination).resolve()    modules=pathlib
    ${zip}=    Evaluate    zipfile.ZipFile($archive)    modules=zipfile
    @{members}=    Evaluate    list($zip.infolist())
    Evaluate    $zip.close()
    FOR    ${member}    IN    @{members}
        ${name}=    Evaluate    $member.filename.replace(chr(92), "/")
        ${safe_name}=    Evaluate    not $name.startswith("/") and not pathlib.PureWindowsPath($member.filename).drive and ".." not in pathlib.PurePosixPath($name).parts    modules=pathlib
        Should Be True    ${safe_name}    Unsafe ZIP member path: ${member.filename}
        ${mode}=    Evaluate    $member.external_attr >> 16
        ${safe_type}=    Evaluate    stat.S_IFMT($mode) in (0, stat.S_IFREG, stat.S_IFDIR)    modules=stat
        Should Be True    ${safe_type}    ZIP member is not a regular file or directory: ${member.filename}
        ${candidate}=    Evaluate    ($root / $name).resolve()
        ${contained}=    Evaluate    $candidate == $root or $root in $candidate.parents
        Should Be True    ${contained}    ZIP member escapes destination: ${member.filename}
    END
    Extract Archive    ${archive}    ${destination}
```

Backslashes are normalized before checking POSIX absolute paths, Windows drive/UNC/root-relative paths, and `..` components. Unix-mode symlinks, devices, and other non-file entries are rejected; each resolved output candidate must remain under the resolved destination. Any check or ZIP parse error stops before extraction. This is a fail-closed compatibility gate, not a claim that released 32.0.1 fixes Zip Slip. The traversal fix appears in upcoming 32.0.2; once live metadata confirms that fixed release and the project freeze uses it, direct extraction may rely on its documented `ValueError` boundary. Until then, retain member verification.

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
    ${download_promise}=    Promise To Wait For Download
    Click    text=Download receipt
    ${download}=    Wait For    ${download_promise}
    Save Download    ${download}    %{ROBOT_ARTIFACTS}${/}receipt.pdf
```

Declare both `rpaframework` and `robotframework-browser` in the RCC project environment, including project-managed Node.js where the selected Browser release requires it. Run `rfbrowser init` inside that RCC environment only for this Robot Framework Browser project to install its browser engines; it is not generic RCC or Selenium setup. Verify the current Browser Playwright API: stale examples commonly use obsolete initialization and keyword forms.

## Excel, PDF, email, and database selection

```robot
*** Settings ***
Library    RPA.Excel.Files

*** Tasks ***
Read workbook
    Open Workbook    ${CURDIR}${/}source.xlsx
```

```robot
*** Settings ***
Library    RPA.PDF

*** Tasks ***
Read PDF
    Get Text From PDF    ${CURDIR}${/}source.pdf
```

```robot
*** Settings ***
Library    RPA.Email.ImapSmtp

*** Tasks ***
Send receipt
    Send Message    sender=${SENDER}    recipients=${RECIPIENT}    subject=Receipt    body=Complete
```

```robot
*** Settings ***
Library    RPA.Database

*** Tasks ***
Query database
    Connect To Database Using Custom Params    ${DATABASE_CONNECT_STRING}
```

Configure mail TLS/OAuth and database drivers through the RCC environment and keep exports under artifacts. Import only the library for the selected task.

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
