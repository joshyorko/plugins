# RPA Framework library selection

Inspect the project first. The sources below describe the library surface, not a dependency pin. RPA Framework **32.0.1** was the latest released version observed on 2026-08-01; [32.0.2 was listed as upcoming](https://rpaframework.org/releasenotes.html). Check [live package metadata](https://pypi.org/pypi/rpaframework/json) and the project’s own freeze before making any current-version claim.

| Task | Primary library | Package/platform needs | Prefer instead when | Source |
| --- | --- | --- | --- | --- |
| Files and directories | `RPA.FileSystem` | `rpaframework`; writable artifact directory | Python `pathlib` for small project-local code | [Filesystem](https://rpaframework.org/libraries/filesystem/index.html) |
| JSON documents/schema | `RPA.JSON` | `rpaframework` | Python validation code for non-Robot services | [JSON](https://rpaframework.org/libraries/json/index.html) |
| CSV/tabular transformation | `RPA.Tables` | `rpaframework` | `RPA.Excel.Files` for workbook semantics | [Tables](https://rpaframework.org/libraries/tables/index.html) |
| ZIP/TAR creation or extraction | `RPA.Archive` | `rpaframework`; validate untrusted members | system tooling only when project policy requires it | [Archive](https://rpaframework.org/libraries/archive/index.html) |
| HTTP APIs/downloads | `RPA.HTTP` | network policy, credentials, artifact target | a declared project HTTP client for Python-only code | [HTTP](https://rpaframework.org/libraries/http/index.html) |
| Excel workbooks | `RPA.Excel.Files` | workbook engine/dependencies in project environment | `RPA.Tables` for CSV/table-only work | [Excel.Files](https://rpaframework.org/libraries/excel_files/index.html) |
| PDF content/forms | `RPA.PDF` | PDF dependencies; verify OS/fonts if rendering | a project-specific PDF tool when supported feature differs | [PDF](https://rpaframework.org/libraries/pdf/index.html) |
| Mail protocols/messages | `RPA.Email.ImapSmtp` | mail server/auth/TLS configuration | provider API client when OAuth/provider policy requires it | [Email](https://rpaframework.org/libraries/email_imapsmtp/index.html) |
| Relational databases | `RPA.Database` | driver plus database network/auth access | project ORM/data layer | [Database](https://rpaframework.org/libraries/database/index.html) |
| New web UI automation | `RPA.Browser.Playwright` | Browser project and its initialized engines | Selenium only for maintained Selenium suites or required capability | [Browser Playwright](https://rpaframework.org/libraries/browser_playwright/index.html) |
| Existing Selenium browser suite | `RPA.Browser.Selenium` | matching browser/driver strategy in project | Playwright for new compatible work | [Browser Selenium](https://rpaframework.org/libraries/browser_selenium/index.html) |
| Desktop UI/OCR | `RPA.Desktop`, `RPA.Windows`, `RPA.Recognition` | OS session/display, native packages, OCR engine/model | browser/API integration if available | [Desktop](https://rpaframework.org/libraries/desktop/index.html) |
| Secret-safe Robot logs | `RPA.RobotLogListener` | import before sensitive calls | modern logging APIs for modern Python robots | [RobotLogListener](https://rpaframework.org/libraries/robotloglistener/index.html) |
| Legacy secrets | `RPA.Robocorp.Vault` | legacy `RPA_SECRET_*` manager contract | `robocorp.vault` in a modern Python boundary | [Vault](https://rpaframework.org/libraries/robocorp_vault/index.html) |
| Legacy work items | `RPA.Robocorp.WorkItems` | legacy `RPA_WORKITEMS_*` adapter contract | `robocorp.workitems` in a modern Python boundary | [WorkItems](https://rpaframework.org/libraries/robocorp_workitems/index.html) |
| Cloud/file storage | `RPA.Cloud.*` | provider package, credentials, network policy | provider SDK already declared by the project | [Library index](https://rpaframework.org/) |
| Human-in-the-loop dialog | `RPA.Assistant` | supported browser/UI runtime and artifact policy | a project-owned UI when process must outlive the run | [Assistant](https://rpaframework.org/libraries/assistant/index.html) |

Use the [library index](https://rpaframework.org/) to confirm a module’s current ownership and dependencies. Link a browser choice to its actual installation guide, not an old copied snippet.
