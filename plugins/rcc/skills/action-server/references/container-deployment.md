# Build and run an ACTIONS container

Use `joshyorko/actions`, branch `community`, as the owning source. Install the PyPI packages `actions-runtime` and `actions-core`. The runtime uses MCP v2 at `/mcp`; this guide does not use a vendor binary or the older standalone release line.

## Select the runtime

The bundled [Dockerfile](../assets/container/Dockerfile) pins the latest PyPI versions inspected on 2026-10-03:

| Component | Pin |
| --- | --- |
| `actions-runtime` | `1.0.2` |
| `actions-core` | `1.0.1` |
| Python | `3.12.11`, Debian Bookworm, base image digest pinned |
| Container architecture | `linux/amd64` |

The runtime 1.0.2 wheels support CPython 3.12 and 3.13. Its Linux wheels are x86-64. On ARM, this starter requires amd64 emulation; do not claim native ARM support. Alpine/musl is not this recipe's runtime.

Check PyPI when refreshing these pins, then rebuild and run the smoke check. Do not select the runtime from the numerically highest historical GitHub tag. To freeze all transitive dependencies, additionally lock the Python installation and export a package environment freeze. The starter pins the two ACTIONS packages and the base image, not every transitive Python or Debian dependency.

Import action APIs from `actions` and MCP decorators from `actions.mcp`. Use `my_actions.py`, not a top-level `actions.py` or `actions/` directory, which shadows `actions-core`. Existing packages that use a different action-library namespace need an explicit package migration before adopting this runtime.

## Copy the starter

Run the Docker commands on the Bluefin/Linux host, or in a devcontainer that already has access to a Docker daemon and Compose. Python dependencies install inside the image. No host package layering is needed.

From the plugins source checkout:

```sh
action_skill_dir="$(realpath plugins/rcc/skills/action-server)"
cp -a "$action_skill_dir/assets/container" ./my-action-container
cd my-action-container
```

For an installed plugin, resolve its `action-server` skill directory and copy that directory's `assets/container` instead. Keep all starter files, including `.dockerignore` and `.gitignore`.

For an existing action package, copy `Dockerfile`, `compose.yaml`, and `entrypoint.sh` into the package root. Merge the ignore rules into its existing ignore files. Keep its actual `package.yaml`, action code, `pythonpath`, and required data files; do not replace them with the demonstration package. Align its `actions-core` dependency with the selected PyPI core pin, currently `actions-core=1.0.1`, while preserving unrelated dependency pins. The image's Python and the RCC-managed package environment are separate installations; changing a Docker build argument does not update `package.yaml`. Add package-specific OS libraries and CLIs to the Dockerfile before the non-root `USER` instruction.

## Create the runtime secret and start

Create a local development API key without displaying it:

```sh
python3 - <<'PY'
from pathlib import Path
import secrets

directory = Path('.secrets')
directory.mkdir(mode=0o700, exist_ok=True)
directory.chmod(0o700)
key = directory / 'action-server-api-key'
if key.exists():
    raise SystemExit('API-key file already exists; keep it or rotate it deliberately.')
key.write_text(secrets.token_hex(32) + '\n')
key.chmod(0o444)
PY
docker compose build
docker compose up -d --wait --wait-timeout 180
docker compose ps
```

Local Compose secrets are file bind mounts. The private host directory prevents other host users from reading the key; the file's read permission lets container UID 10001 read the mounted secret. Do not rely on Compose secret `uid`, `gid`, or `mode` to remap a local file. For servers and Kubernetes, provision the platform's secret with permissions readable by the runtime user.

The [entrypoint](../assets/container/entrypoint.sh) rejects missing, empty, or `None` keys. It passes the key through the runtime's supported `--api-key` flag. The key is not a build argument, image layer, or printed shell command. Operators with access to container processes can inspect its arguments; do not run with shell tracing or publish process/config dumps.

The server listens on `0.0.0.0:8080` inside the container. Compose publishes only `127.0.0.1:8080` on the host. Set `ACTION_SERVER_HOST_PORT` when that host port is occupied. `--expose` starts a public tunnel and is not needed for container networking. For remote access, add an authenticated TLS reverse proxy or ingress that supports MCP streaming, preserves authorization headers, and disables response buffering. Set `--server-url` to that public URL when needed, and configure explicit browser origins for a separate browser frontend.

The HTTP healthcheck verifies that the server serves OpenAPI. It does not prove action execution, authorization, or MCP operation.

## Initialize and update persistent data

The image builds the RCC-managed action environment and validates imports as UID 10001. Resolved environments remain at `/home/action-user/.actions` in the image. A disposable build datadir is removed after validation.

At every container start, the entrypoint runs `action-server start --dir /opt/actions --datadir /var/lib/action-server --actions-sync=true`. The runtime imports and synchronizes the package before serving, keeps run history, and disables removed actions. Import failure prevents startup. This starter owns one action package per datadir.

Do not substitute a separate `action-server import` followed by `--actions-sync=false`. The import command leaves absent actions enabled, so that sequence can advertise removed tools after an image update. For intentionally pre-imported immutable catalogs, sync-disabled startup is a separate operational choice, not this update recipe.

Docker initializes a new named volume from the image directory, including ownership. An existing named volume or a bind mount overrides image directory contents and permissions. For an existing datadir, stop its writer, back it up, and correct ownership for UID/GID 10001 before attaching it. On an SELinux host, give bind mounts the appropriate `:Z` or `:z` label. Do not recursively change ownership of unrelated host directories.

Keep the RCC cache at its image path. Mounting an empty volume over `/home/action-user/.actions` hides the prebuilt environments. Do not seed a live datadir by copying a build-time SQLite database over existing history. Do not run multiple replicas against this SQLite datadir; distributed deployment needs the ACTIONS shared-backend contract and its own validation.

For an application update, back up the datadir with the server stopped, then:

```sh
docker compose build
docker compose up -d --force-recreate --wait --wait-timeout 180
```

Startup synchronization refreshes the catalog in the existing volume. Check both changed and removed actions, and read a previously retained run ID. Runtime upgrades can migrate database schemas; an older image is not automatically a safe database rollback.

`docker compose down` stops the application and retains the named volume. Do not add `--volumes` to an application shutdown unless you explicitly intend to delete its run history and persistent state.

## Verify action execution and MCP v2

The demonstration action accepts `{"message":"hello"}` and returns the JSON string `"container:hello"`. Read its actual POST path from `/openapi.json`; package/action names are normalized in URLs. Send `Authorization: Bearer <key>` from a secret file or client environment, never a literal key pasted into shell history.

For MCP, initialize a session at `/mcp`, retain any returned `Mcp-Session-Id`, send the initialized notification, list tools, and call the advertised `container_echo` tool. Use the negotiated protocol version and accept both JSON and event-stream responses. A bare `curl /mcp` is not an MCP acceptance test. The runtime's MCP SDK major version is 2; negotiate the protocol date separately.

Run the bundled check from the skill directory or through the resolved skill path:

```sh
python3 "$action_skill_dir/scripts/smoke-container.py"
```

The check creates an isolated temporary project, random host port, and API key. It builds the starter, checks non-root execution and health, rejects unauthorized action calls, invokes the action over HTTP and MCP v2, recreates the container, and reads its retained run ID. It then rebuilds changed action code and verifies that the existing volume gets the updated catalog. It saves diagnostics and removes only its own temporary containers, network, and volume, never an existing application's volume.

For a real package, repeat those checks with a harmless package-specific action and expected result. The starter's smoke check does not validate your external services, browser session, OAuth provider, or production deployment.

## Add browser or infrastructure dependencies

The starter is an API/action baseline. Browser packages need browser binaries and their OS libraries. Install OS libraries as root during the image build, then install the browser in the package's resolved environment as UID 10001. For `robocorp-browser`, use the package's `post-install` command `python -m robocorp.browser install chrome --isolated`. For direct Playwright, install the matching browser through that package's Playwright CLI.

Keep the same runtime-user browser/cache paths during build and execution. Use headless mode, allocate shared memory such as Compose `shm_size: 1gb` when Chromium requires it, and check container sandbox support. Do not copy a host browser profile or default to `--privileged`, `ipc: host`, or disabling the browser sandbox. Verify a real browser launch after adding dependencies.

Home-lab actions need their actual pinned tools, such as `kubectl` or the Rancher CLI, and scoped runtime configuration. Budget actions need the finance service and database. Add those only to packages that require them. Nginx and Supervisor from older examples are optional topology choices, not ACTIONS runtime prerequisites; Compose already supervises the single server process.

## Troubleshoot the failing boundary

| Symptom | Check |
| --- | --- |
| `cannot import name 'action'` from a partially initialized module | Rename top-level `actions.py` or `actions/`; inspect package `pythonpath`. |
| Runtime wheel unavailable | Verify Python 3.12/3.13, amd64, glibc, and the selected PyPI pin. |
| RCC/package import fails during build | Read the complete build log, package pins, trusted CA configuration, and network access. Keep TLS verification enabled. |
| Worker logs `No such file or directory: 'ps'` | Install `procps` in the image; the runtime uses `ps` to check parent-process liveness. |
| Startup reports an unreadable key or datadir | Check runtime UID 10001, local secret file permissions, volume ownership, and SELinux labels. |
| Healthy server but missing/stale tools | Check startup import output and `/openapi.json` plus MCP `tools/list`; a healthcheck alone is insufficient. |
| Browser fails while action import passes | Launch the browser in the action environment; check binaries, OS libraries, writable cache, and shared memory. |
| Proxy works for HTTP but MCP fails | Check `/mcp` routing, authorization, streaming, negotiated protocol, and session headers. |

Release 1.0.2 still consumes the literal environment variable `SEMA4AI_OPTIMIZE_FOR_CONTAINER=1`. The starter retains that implementation compatibility switch to select RCC's container mode and suppress the startup API-key banner. It does not select a different runtime or package ecosystem. Do not replace it with an invented `ACTIONS_...` variable unsupported by the selected release.

## Sources

- [ACTIONS community source](https://github.com/joshyorko/actions/tree/community), including `developer/toolkit.yaml`, `action_server/tasks.py`, `_actions_import.py`, `_rcc.py`, and `mcp/setup_mcp_server_v2.py`.
- [PyPI actions-runtime 1.0.2](https://pypi.org/project/actions-runtime/1.0.2/) and [release JSON](https://pypi.org/pypi/actions-runtime/1.0.2/json), including wheel platforms, Python constraints, and MCP v2 dependency metadata.
- [PyPI actions-core 1.0.1](https://pypi.org/project/actions-core/1.0.1/) and [release JSON](https://pypi.org/pypi/actions-core/1.0.1/json).
- Local examples inspected under `Projects/automation-control-plane/RPA/action_servers/`: [home-lab-actions Dockerfile](https://github.com/joshyorko/home-lab-actions/blob/88d0786deaf670852c7114a2a5770808485f722d/Dockerfile) and `docker-compose.yml`, [yo-dawg Dockerfile](https://github.com/joshyorko/yo-dawg/blob/fdaf7422d4dc333186ff88d8440c2e69008be9a1/Dockerfile) and `docker-compose.yaml`, and [actions-actual-budget-server Dockerfile](https://github.com/joshyorko/actions-actual-budget-server/blob/09ff8d78347955c92fd61e78ea79276f296e51bc/Dockerfile) and `docker-compose.yml`. These supplied non-root, volume, browser, and infrastructure patterns; their older runtime downloads and proxy configurations are not the current baseline. The inspected Docker/Compose files match those commits; unrelated local edits were left untouched.
