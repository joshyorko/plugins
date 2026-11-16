# Source/Version Pin Matrix & Scoped Capability Qualification Matrix

This document records the authoritative version pins, upstream commit/release provenance, SHA-256 digests, and current qualification gates for the `rcc` plugin's bundled Actions & Robot Bootstrapper (`plugins/rcc/bootstrapper/`).

> [!IMPORTANT]
> **Reporting Integrity Rule**: Upstream release qualification (in `joshyorko/actions` and `joshyorko/rcc`) is strictly separated from `joshyorko/plugins` verification evidence. Any capability or platform that has not yet been executed and backed by a retained receipt in this branch is marked **`PENDING`** or **`NOT QUALIFIED`**.

---

## 1. Source & Version Pin Matrix

### 1.1 Upstream Repositories & Commits

| Component | Upstream Repository | Tag / Release | Exact Commit SHA | Upstream CI / Release Run ID | Role in `plugins/rcc` |
| --- | --- | --- | --- | --- | --- |
| **Bootstrapper Legacy Provenance** | `joshyorko/actions-robot-boostrapper` | `main` (retired upstream) | `2769fb7a939af7db6111428dd27153a1a15e4c53` | N/A | Audited 36-action legacy baseline migrated into `plugins/rcc/bootstrapper/` (Apache-2.0). |
| **Actions Runtime (Target Pin)** | `joshyorko/actions` | `actions-runtime-1.1.1` | `9bfc6ca5337dac14851e5ef3b2ea3a41485c81b2` | `43331625122` | Default runtime hosting `/mcp`, OpenAPI `/api/actions`, native Action Canvas, and `/api/managed-work-items/v1`. |
| **Actions Runtime (Baseline Comparison)** | `joshyorko/actions` | `actions-runtime-1.1.0` | `ced70e0879f93054f8a56a067b584c4c31fc3cb3` | `43292401637` | Baseline release for `1.1.0` → `1.1.1` comparison (`#107` / `#108` / `#109`). |
| **Actions Core Library** | `joshyorko/actions` (`actions-core` PyPI) | `1.1.0` | `ced70e0879f93054f8a56a067b584c4c31fc3cb3` | `43292401637` | Decorators (`@action`, `@tool`, `@resource`, `@prompt`), `Response`, `ActionError`, `Secret`. |
| **Actions Work Items Library** | `joshyorko/actions` (`actions-work-items` PyPI) | `0.5.0` | `ced70e0879f93054f8a56a067b584c4c31fc3cb3` | `43292401637` | Producer/consumer work-item queues (`inputs`, `outputs`, `seed_input`, SQLite/File adapters). |
| **RCC Embedded in Actions Runtime 1.1.x** | `joshyorko/rcc` | `v18.19.8` | `ee7c5234b65ab3015aa75c749d2a167502985d87` | Official release `v18.19.8` | Pinned inside `actions/server/_download_rcc.py` (`RCC_VERSION = "18.19.8"`) for Action package holotree builds (`ACTIONS_HOME` / `ROBOTS_HOME`). |
| **RCC Standalone CLI (Host Installed)** | `joshyorko/rcc` | `v18.19.11` | `f8ef6ff4f73a90ca303a4bb113bc406d3c74a448` | Official release `v18.19.11` | Standalone `rcc` CLI used for direct robot holotree & task execution (`ROBOCORP_HOME`). |

### 1.2 Standalone Binary & Release Asset SHA-256 Digests (Fail-Closed Allowlist)

Before executing `action-server` or `rcc`, `plugins/rcc` verifies the binary's SHA-256 against this allowlist and fails closed on mismatch:

| Artifact | Version | Platform | Size (Bytes) | SHA-256 Digest | Upstream Status vs Plugin Evidence |
| --- | --- | --- | ---: | --- | --- |
| `actions-runtime-1.1.1-linux64` | `1.1.1` | Linux x86-64 | `104,464,218` | `8c27fbed198aef16c98cdc7a45230fbc4708b06dff1c3523aeddfcdf7840efd0` | Verified on local Linux host (`/home/linuxbrew/.linuxbrew/bin/action-server`) |
| `actions-runtime-1.1.1-windows64.exe` | `1.1.1` | Windows AMD64 | `71,853,568` | `0388d7f2e4080ba5c3a800bb556f8f01a65aa4834f3698eceb240888288fd8d2` | Published upstream; Windows plugin CI execution `PENDING` |
| `actions-runtime-1.1.1-macos-arm64` | `1.1.1` | macOS arm64 | `77,582,306` | `6710985b3abf1b87b8719af8dad66d5b63f434a41de0f3eb355cd576fc570e91` | Published upstream; **`NOT QUALIFIED`** in `plugins/rcc` |
| `actions-runtime-1.1.1-sha256.txt` | `1.1.1` | Manifest | `289` | `3683f654217b4a9868c1581d13025af2a93093ccbcc87ca6b97c35a55dfef48b` | Upstream manifest digest verified |
| `actions-runtime-1.1.0-linux64` | `1.1.0` | Linux x86-64 | `104,209,586` | `e163684dc296f632a177e95d4f355ac6d4f2299c62aab53789f43a642f41c0f1` | Upstream digest recorded; baseline comparison `PENDING` |
| `actions-runtime-1.1.0-windows64.exe` | `1.1.0` | Windows AMD64 | `71,695,872` | `44507e8dcd82fb367c616c699957611096bc05ff2ae5e0f41a11b98a98332098` | Upstream digest recorded; **`NOT QUALIFIED`** in `plugins/rcc` |
| `rcc-linux64` (`v18.19.8`) | `18.19.8` | Linux x86-64 | `26,337,280` | `ea8dc96e66d7a81536b03a344b4eaf44bbd94122585954e70290bd466f5de4b6` | Embedded by `action-server` `1.1.x` |
| `rcc-windows64.exe` (`v18.19.8`) | `18.19.8` | Windows AMD64 | `27,481,088` | `f5e868af05382731f6675d98ed6ead7c7e95b250466bc5fee054b41817280a89` | Embedded by `action-server` `1.1.x`; Windows CI `PENDING` |
| `rcc-macosarm64` (`v18.19.8`) | `18.19.8` | macOS arm64 | `26,331,890` | `bf11dcee5a1e009a3f5b456bc8a24eb8f3f5d064eb7962c5e90956f13d4dfcba` | Published upstream; **`NOT QUALIFIED`** in `plugins/rcc` |
| `rcc-linux64` (`v18.19.11`) | `18.19.11` | Linux x86-64 | `26,349,568` | `4d68609044d3a39a1728d571d54c77a6174215adb40eb9acd69ce5ad34a6db1f` | Verified on local Linux host (`/home/linuxbrew/.linuxbrew/bin/rcc`) |
| `rcc-windows64.exe` (`v18.19.11`) | `18.19.11` | Windows AMD64 | `27,492,352` | `58f43a3b3c54032740f224cb4a8bbf68c47c865cf2652084fb139b8ecc01cfd0` | Published upstream; Windows plugin CI execution `PENDING` |

### 1.3 RCC Bootstrapper Release Package

| Release Asset | Version | Source | SHA-256 Digest | Status |
| --- | --- | --- | --- | --- |
| `rcc-actions-bootstrapper-0.6.0.zip` | `0.6.0` | Generated from `plugins/rcc/bootstrapper/` with Actions Runtime `1.1.1` | `6b1eaa525e5a1c57c6648f498c09d5fae04d1dd02b14db66cba36b4d15ce0622` | Two independent isolated builds at different output paths matched; release asset awaits publication of tag `v0.6.0`. |

### 1.4 PyPI Package & Wheel SHA-256 Digests

| Package | Version | Distribution Filename | SHA-256 Digest |
| --- | --- | --- | --- |
| `actions-runtime` | `1.1.1` | `actions_runtime-1.1.1-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | `9e6dbe3508437de8eb6c0996523af3fc3e86b832833b55735c1eb0601d2bec16` |
| `actions-runtime` | `1.1.1` | `actions_runtime-1.1.1-cp313-cp313-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | `b1561427be68b1bade7646c0a0aa952807b21061b4d831a0819e3e8671a3db6b` |
| `actions-runtime` | `1.1.1` | `actions_runtime-1.1.1-cp312-cp312-win_amd64.whl` | `42f0c3392241d3beb7e1b2db680d11fe3a998624d2d493f3ee6f38c67305ea12` |
| `actions-runtime` | `1.1.1` | `actions_runtime-1.1.1-cp313-cp313-win_amd64.whl` | `f1bf66b6174461d853ede307367b91c182484d88de9cb92e44f028cba7c955b9` |
| `actions-runtime` | `1.1.1` | `actions_runtime-1.1.1.tar.gz` | `b73e3bebba1d446754acc41e6c1e716fdcaff6246a12412eaced661f579dc7ec` |
| `actions-core` | `1.1.0` | `actions_core-1.1.0-py3-none-any.whl` | `38bd11c756cc60d0c028094d6e7f5f3859d724114e2abd48e54f0951e2a3dac6` |
| `actions-core` | `1.1.0` | `actions_core-1.1.0.tar.gz` | `135bef2b94a45e25b1adff9358dc2082364afc12617e35efc33e78f32d461554` |
| `actions-work-items` | `0.5.0` | `actions_work_items-0.5.0-py3-none-any.whl` | `0aaea32c8e3b9f25a1de40083909d684418e8b4288fd504383233f53b95dcbb4` |
| `actions-work-items` | `0.5.0` | `actions_work_items-0.5.0.tar.gz` | `957366932bfc9adef1808b95a84d3cf0c3a1ba68e1db3ba5c77f9d34ceb983f9` |

---

## 2. Scoped Capability & Qualification Matrix (Updated Only After Actual Receipts)

We distinguish four evidence levels and never conflate them:

1. **`ACTUAL-NATIVE-RUNTIME`**: Driven through the real `action-server` (`1.1.1`) binary over `/mcp` (`initialize` → `notifications/initialized` → `tools/list` → `resources/list` → `prompts/list` → `tools/call`) with retained output receipt.
2. **`ACTUAL-SOURCE-UNIT`**: Executed in local pytest against `plugins/rcc/bootstrapper` source with real filesystem/process isolation.
3. **`ACTUAL-INSTALLED-CLIENT`**: Executed through the installed Codex CLI MCP client using an environment-backed Bearer token.
4. **`ACTUAL-INSTALLED-PACKAGE`**: Installed through `install_bootstrapper.py` from checksum-verified package and native artifacts, then launched from the extracted package without site packages or the source tree.
5. **`PENDING`**: Implementation in progress or awaiting CI / independent reviewer run; **not yet qualified**.
6. **`NOT QUALIFIED`**: Unsupported or unexecuted in this environment (e.g., zero-touch plugin auth, Windows native Canvas storage, macOS plugin runtime, ChatGPT Mode E1 public tunnel, `celld`).

| Capability / Surface | Linux x86-64 (This Branch) | Windows AMD64 | macOS arm64 | Evidence Level & Notes |
| --- | --- | --- | --- | --- |
| **Plugin Manifests (`mcp.json`, `.mcp.json`, `.codex-plugin/plugin.json`)** | `ACTUAL-SOURCE-UNIT` (`bin/check` + `test_rcc_distribution.py` passed) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Verified locally on Linux via `bin/check` (82 tests) and `tests/test_rcc_distribution.py`. |
| **Bootstrapper Core Unit & Security Regression Suite (`test_bootstrapper_unit.py`)** | `ACTUAL-SOURCE-UNIT` (17/17 passed) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Covers approved/trusted roots, traversal/symlink, overwrite, secret redaction, fail-closed generator/binary checks, timeout, active cancellation, stale run/runtime metadata, occupied ports, and caller HOME preservation. |
| **Packaging/Installer Security Regression Suite (`test_bootstrapper_packaging.py`)** | `ACTUAL-SOURCE-UNIT` (12/12 passed) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Covers stdlib-only setup, checksum/download retry and Windows-style rename semantics, ZIP path/content validation, symlink rejection, idempotency, and pinned binary priority. |
| **Runtime Launcher Lifecycle (`bootstrapper_runtime.py`: fail-closed SHA-256, occupied-port, stale-PID, SQLite backup)** | `ACTUAL-NATIVE-RUNTIME` (pinned startup, owned stop and SQLite backup) + `ACTUAL-SOURCE-UNIT` (stale metadata and occupied-port fixtures) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Binary digests are checked before Runtime spawn; missing auth and unrelated listeners fail closed before launch. |
| **Packaged installer, extracted tree verification, and source-free launcher** | `ACTUAL-INSTALLED-PACKAGE` ([`dogfood-receipt-linux-packaged-install.json`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/dogfood-receipt-linux-packaged-install.json)) | `PENDING` (native Windows CI) | `NOT QUALIFIED` | Linux installed the generated ZIP plus pinned Runtime/RCC assets in a fresh home, verified the extracted tree and `python -S` launcher, then exercised MCP, Action, Robot, and failure/retry paths. The live plugin release URL remains unavailable until `v0.6.0` is published. |
| **Actions Runtime `1.1.1` `/mcp` Discovery & Authorized Tool Invocation (`dogfood_bootstrapper.py`)** | `ACTUAL-NATIVE-RUNTIME` ([`dogfood-receipt-linux-1.1.1.json`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/dogfood-receipt-linux-1.1.1.json)) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Proved unauthenticated HTTP 403 rejection + Bearer-authenticated `initialize` → `notifications/initialized` → `tools/list` (15 tools) → `resources/list` → `prompts/list` → `tools/call`. |
| **Golden Path: Action (`workflow-producer-consumer`) + Robot (`01-python`) Preview → Scaffold → Validate → Run → Failure/Retry** | `ACTUAL-NATIVE-RUNTIME` ([`dogfood-receipt-linux-1.1.1.json`](file:///home/kdlocpanda/plugins/plugins/rcc/docs/dogfood-receipt-linux-1.1.1.json)) | `PENDING` (Windows CI) | `NOT QUALIFIED` | Reuses published `action-server new` and `rcc robot initialize` templates; verified non-empty overwrite refusal, work-item producer/consumer SQLite queue (`seeded=2`, `done=2`), secret redaction, controlled failure (`exit_code=2`), `failure_diagnosis`, and retry to `succeeded`. |
| **Runtime authentication and executable trust boundaries** | `ACTUAL-NATIVE-RUNTIME` (unauthenticated startup refused; keyed `/mcp` rejects missing Bearer; execution requires configured trusted root and explicit run confirmation) | `PENDING` | `NOT QUALIFIED` | Workspace write approval does not imply executable trust; caller-provided `trust_project=true` cannot add server-owned trusted roots. |
| **Installed Codex CLI client with Bearer token environment** | `ACTUAL-INSTALLED-CLIENT` (Codex CLI 0.169.0; temporary config override; `bootstrapper_plan` returned 3 files, no writes) | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | The client performed its MCP handshake and harmless tool call using `bearer_token_env_var`; this does not qualify automatic plugin attachment. |
| **Zero-touch MCP authentication from portable/Codex plugin manifests** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Requires host-specific secret injection; manifests do not embed credentials. |
| **Native Action Canvas resource and tool discovery** | `ACTUAL-NATIVE-RUNTIME` (tool names discovered; `ui://action-canvas/v1/canvas.html` read) | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | `canvas_open`/create/apply user workflow is **NOT QUALIFIED**; resource read alone is not feature acceptance. |
| **Managed Work Items HTTP API** | `ACTUAL-NATIVE-RUNTIME` (unauthenticated schema route 401; authorized schema route 200); functional operations **`NOT QUALIFIED`** (`/capabilities` returned 422, zero operations) | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Receipt records route reachability only, not successful managed Work Item operations. |
| **Runtime `1.1.0` vs `1.1.1` Baseline/Candidate Comparison** | `PENDING` (only `1.1.1` binary installed on host; `1.1.0` digest pinned) | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Harness supports `--baseline-receipt`; live `1.1.0` binary execution not run on this host. |
| **Independent Reviewer Re-Verification** | `PENDING` (awaiting independent reviewer) | `PENDING` (awaiting CI / reviewer) | **`NOT QUALIFIED`** | This branch is published as a **Draft PR checkpoint** prior to independent review. |
| **Live Desktop GUI MCP Client Session (Codex / Claude Desktop UI)** | **`NOT QUALIFIED`** (headless CLI/protocol only) | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Headless `/mcp` streamable HTTP protocol is verified; interactive GUI client attachment is not run in headless automation. |
| **ChatGPT Developer Mode / Public Tunnel (Mode E1)** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Public tunnels are prohibited in repo automation. |
| **`celld` / Cloud Control Plane Integration** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | **`NOT QUALIFIED`** | Out of scope for local `plugins/rcc` bootstrapper. |
