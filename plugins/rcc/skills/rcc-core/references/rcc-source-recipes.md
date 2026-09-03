# RCC Source And Command Recipes

Use this guide when the work is about RCC itself: the CLI, source tree, holotree, cache behavior, templates, endpoint configuration, or Josh's fork.

## RCC As Gateway

RCC is the environment and execution gateway: it resolves `robot.yaml`, `conda.yaml`, holotree environments, dependency freeze/export, templates, bundles, profiles/endpoints, and command execution. Control Room-adjacent runtime provisioning is a separate lifecycle layer that can ask a worker to link back to Control Room, but RCC remains the local environment boundary inside that worker.

## Public On-Demand Runtime Contract

Public `robocorp/k8s-on-demand-runtimes` and `robocorp/azure-on-demand-runtimes` examples expose a provider-neutral webhook shape with `type` values `start`, `stop`, and `status`, plus `workspaceId`, `runtimeId`, `runtimeLinkToken`, and optional `maxLifetimeSeconds`. These examples support public contract guidance and architecture inference only; they are not a released Control Room backend implementation.

One-time `runtimeLinkToken` values affect retry semantics. Retrying infrastructure after a token is consumed may require a fresh Control Room request rather than rerunning the same worker container.

Do not copy public provisioner examples as production hardening guides. The reviewed Azure example logs request/secret material during validation, so production provisioners need stricter secret handling and logging policy.

## Command Map

Start with read-only commands:

```bash
rcc version
rcc diagnostics --quick
rcc diagnostics --quick --json
rcc configuration diagnostics --quick --json
rcc docs recipes
rcc docs changelog
rcc docs troubleshooting
```

Robot-facing environment checks:

```bash
rcc diagnostics --robot robot.yaml --json
rcc robot diagnostics -r robot.yaml --json
rcc holotree check --retries 5
rcc ht vars -r robot.yaml
rcc ht vars -r robot.yaml --json
rcc task script -r robot.yaml --silent -- python --version
rcc task script -r robot.yaml --silent -- python -m pip list
rcc task shell -r robot.yaml
```

Template and package commands:

```bash
rcc pull github.com/robocorp/template-python-browser
rcc pull github.com/joshyorko/template-python-browser
rcc robot init --json
rcc robot init -t python -d my-robot
rcc run -r robot.yaml -t Main
rcc run -r robot.yaml --dev -t Test
```

Environment Artifact and provider orientation:

```bash
rcc env --help
rcc provider list --json
rcc provider inspect local --json
rcc env lifecycle inspect --artifact sha256:<64-hex> --json
rcc cache serve --help
rcc env coordinate --help
```

Read `environment-artifacts.md` before publish/acquire/export/exec or lifecycle mutation, `providers-and-trust.md` before provider/trust/cache-server work, and `build-coordination.md` before using coordination or prewarm. Confirm the exact subcommand help because several machine-contract details differ from broad design documents.

Use `--debug`, `--trace`, and `--timeline` only after a normal command has shown the failing layer.

## Troubleshooting Ladder

Use this order before changing code:

1. `rcc diagnostics --quick --json`: proves binary, profile, endpoint, and host basics.
2. `rcc diagnostics --robot robot.yaml --json`: adds project-specific context.
3. `rcc robot diagnostics -r robot.yaml --json`: checks robot package structure.
4. `rcc holotree check --retries 5`: checks cache health.
5. `rcc ht vars -r robot.yaml`: proves environment resolution and prints runtime vars.
6. `rcc task script -r robot.yaml --silent -- python -m pip list`: proves Python and packages inside the RCC environment.
7. `rcc run -r robot.yaml -t <Task> --silent`: only now debug task code.

If step 1-5 fails, stay in `$rcc-core`. If step 6-7 fails, switch to `$rcc-robots`.

## Source Tree Orientation

Use Josh's fork for current source behavior. The current `robocorp/rcc` repository is documentation-focused; `github.com/joshyorko/rcc` contains the active command/source tree.

Useful Josh fork entry points are:

- `cmd/rcc/`: CLI entrypoint and command wiring.
- `cmd/rccremote/` and `remotree/`: remote/cache client and server behavior.
- `robot/`: `robot.yaml` parsing, task definitions, and testdata.
- `conda/`: conda/uv environment resolution testdata.
- `htfs/`, `blobs/`, and `journal/`: holotree filesystem and cache internals.
- `environmentartifact/`: canonical Manifest, Specification, Object/Platform Index, digest, archive, catalog, and compatibility contracts.
- `environmentlifecycle/`: publish/acquire/materialize/execute, archives, leases, reconciliation, repair, GC, and native supervision.
- `artifactprovider/` and `artifactpolicy/`: filesystem/journal CAS, HTTP/admin protocol, quotas, recovery, and transport policy.
- `artifacttrust/`: provenance, SBOM, signatures, revocation, carriers, policies, and receipts.
- `buildcoord/`: build keys, claims/epochs, fencing, recovery, nondeterminism, staging, and prewarm.
- `cmd/environment*.go`, `cmd/provider*.go`, and `cmd/cacheServe.go`: released CLI wiring and output behavior.
- `settings/` and `assets/robocorp_settings.yaml`: endpoint/profile defaults and overrides.
- `templates/` and `assets/templates.yaml`: template catalog behavior.
- `developer/toolkit.yaml`: RCC's own RCC-driven development tasks.
- `docs/recipes.md`, `docs/holotree.md`, `docs/troubleshooting.md`, `docs/environment-caching.md`: command-level explanations worth checking before inventing new guidance.

## Holotree And Cache

Holotree is RCC's isolated environment/cache layer. Treat failures here as RCC/environment failures, not Python code failures.

Vocabulary worth preserving:

- `catalog`: metadata describing available environment/cache entries.
- `identity`: RCC's view of the configured user/profile.
- `hololib`: local holotree library/cache storage.
- `space`: one concrete environment/cache slot.
- `shared`: reusable cache space intended for reuse.
- `private`: isolated cache space for one use case.
- `unmanaged`: environment outside RCC's normal lifecycle.
- `pristine` / `dirty`: whether a space matches its expected recipe.

```bash
rcc holotree list
rcc holotree catalogs --identity
rcc holotree variables --space dev --robot robot.yaml
rcc holotree delete --space <space>
rcc holotree remove <catalog-substring> --check 5
rcc holotree check --retries 5
rcc holotree shared --enable
```

Use a scoped home for experiments and CI:

```bash
export ROBOCORP_HOME="$PWD/.cache/robocorp"
# local settings file: "$ROBOCORP_HOME/settings.yaml"
rcc ht vars -r robot.yaml
```

Cache debugging checklist:

- Is `ROBOCORP_HOME` writable in the host/container/CI context?
- Did `environmentConfigs` select the expected freeze or `conda.yaml`?
- Did a stale lock or half-built space survive a canceled run?
- Did a dependency or Python version change without invalidating cache keys?
- Is this conda-forge mode or uv-native mode with no `channels`?
- In CI, is RCC pinned to a known version rather than `releases/latest`?

## Josh Fork Notes

Josh's RCC fork is Linux-friendly and intentionally supports Homebrew on Linux:

```bash
brew tap joshyorko/tools
brew install --cask rcc
# or:
brew install --cask joshyorko/tools/rcc
```

When working on Bluefin, prefer this or a repo-native container path for a host-level `rcc`; do not layer host packages casually.

Josh's fork also exposes endpoint overrides for private control planes and mirrors:

```zsh
export RCC_ENDPOINT_CLOUD_API="https://api.example.com/"
export RCC_ENDPOINT_CLOUD_LINKING="https://console.example.com/link/"
export RCC_ENDPOINT_CLOUD_UI="https://console.example.com/"
export RCC_ENDPOINT_DOWNLOADS="https://downloads.example.com/"
export RCC_ENDPOINT_DOCS="https://docs.example.com/"
export RCC_ENDPOINT_PYPI="https://pypi.org/simple/"
export RCC_ENDPOINT_PYPI_TRUSTED="https://pypi.org/"
export RCC_ENDPOINT_CONDA="https://conda.anaconda.org/"
export RCC_ENDPOINT_UV_RELEASES="https://github.com/astral-sh/uv/releases/download/"
export RCC_AUTOUPDATES_TEMPLATES="https://github.com/joshyorko/robot-templates/releases/latest/download/templates.yaml"
export RCC_AUTOUPDATES_RCC_INDEX="https://github.com/joshyorko/rcc/releases/latest/download/index.json"
rcc diagnostics --quick --json
```

Use endpoint overrides only when the task is actually about private control planes, mirrors, or offline operation.

## Developing RCC From Source

The RCC repo bootstraps its own development through RCC:

```bash
rcc run -r developer/toolkit.yaml -t robot
rcc run -r developer/toolkit.yaml --dev -t unitTests
rcc run -r developer/toolkit.yaml --dev -t local
rcc run -r developer/toolkit.yaml --dev -t build
rcc run -r developer/toolkit.yaml --dev -t tools
```

When running Go tests outside the toolkit, verify the repo's developer docs first. Some tests assume `GOARCH=amd64`.

At v18.19.3 the release Go toolchain is 1.26.5, while the contained toolkit may remain on the newest available Conda Forge build. Treat those as distinct evidence. The toolkit's micromamba bootstrap uses bounded 64 MiB downloads, immutable SHA-256-pinned fallbacks, three attempts with 10/20-second delays, and strict archive-member/executable validation. Do not bypass checksum, size, traversal, symlink, hard-link, or exact-member failures to make setup pass.

General RCC downloads now stage to a temporary file and replace the destination only after success. Preserve an existing binary/cache input after a failed replacement. Linux diagnostics prefer `/etc/os-release` or `/usr/lib/os-release` and include Bluefin, bootc/image, variant, and OSTree metadata before falling back to `lsb_release` and `uname`.

## Remote Cache / Remote Client

For legacy `rccremote` work, prove server and client separately:

```bash
export RCC_REMOTE_ORIGIN=https://rccremote.example.com
rcc holotree pull -r robot.yaml

# or without exporting:
rcc holotree pull -r robot.yaml --origin https://rccremote.example.com
```

If remote pull fails, debug the remote deployment and endpoint variables before touching robot dependencies.

This is the legacy v12 `/parts` and `/delta` protocol. Environment Artifact named/direct providers and `rcc cache serve` use the separate Manifest v1 HTTP contract. Do not exchange their endpoints, credentials, storage roots, or health assumptions. See `providers-and-trust.md`.
