# Run Luna Factory in rootless Podman

Use this guide to build and run a planning-only Luna Factory 0.2.1 OCI image on Linux. The image does not include Codex CLI, Codex credentials, a host app-server socket, a repository mount, or a CAS target.

For the role of Codex CLI, Codex app-server, CAS, and Luna Factory, read [Container architecture](container-architecture.md).

## What the image contains

The image uses the published 0.2.1 workbench and skill. The Rust server is a local derivative of source commit `ebe2753ed032347456ef9b1c193646469bf17c96`. The derivative adds an opt-in loopback publish authority. It does not change the published tag or archive.

The released server accepts only loopback bind addresses. A rootless Podman port publish forwards traffic to the container interface. The optional `published_origin` setting lets the server bind `0.0.0.0` inside the isolated container while accepting only the exact loopback host and origin. The Compose network is private, and the host port binds to `127.0.0.1`.

The image uses Rust 1.99.0 and Python 3.13 Trixie images pinned by digest. Both provide glibc 2.41. The released binary requires glibc 2.39. Dakota provides glibc 2.44.

## Build the image

Run these commands from a clean `joshyorko/plugins` checkout at the reviewed OCI branch head, which must descend from the pinned release commit. The builder rejects dirty checkouts and unreviewed changes outside this OCI patch. Its build fingerprint covers every Cargo manifest, compiled Rust source file, logo, Containerfile, and health probe consumed by the build. Rootless Podman and its Compose provider must be installed.

Set `RELEASE_DIR` to the private directory containing the released archive, its provenance JSON, and the published `SHA256SUMS` file. The build helper checks the public checksum asset, archive, provenance, internal package checksums, binary version, binary hash, UI hash, skill hash, and source commit before it invokes Podman.

```sh
RELEASE_DIR=/absolute/path/to/luna-factory-v0.2.1-release
DEPLOY_ROOT="${XDG_STATE_HOME:-$HOME/.local/state}/luna-factory-oci/v021-canary"
umask 077
BUILD_RECORD=$(mktemp "${TMPDIR:-/tmp}/luna-factory-oci-build.XXXXXX")
podman pull docker.io/library/rust:1.99.0-slim-trixie@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581
podman pull docker.io/library/python:3.13-slim-trixie@sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f
python3 plugins/luna-factory/container/build_image.py \
  --release-dir "$RELEASE_DIR" > "$BUILD_RECORD"
IMAGE_REF=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["image_ref"])' "$BUILD_RECORD")
python3 plugins/luna-factory/container/init_private.py \
  --root "$DEPLOY_ROOT" \
  --host-port 18788 \
  --image "$IMAGE_REF"
install -m 0600 "$BUILD_RECORD" "$DEPLOY_ROOT/image-build.json"
IMAGE_REF=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["image_ref"])' "$DEPLOY_ROOT/image-build.json")
```

The helper rejects a different release commit or artifact. It extracts only regular files beneath the release bundle directory. The build uses the exact Rust and Python image digests in `container/Containerfile` and runs `cargo build --locked` against the pinned source.

The OCI binary is a derivative build, so its hash differs from the official release binary. `image-build.json` records the release archive, release binary, release UI, complete build-input hash, clean build HEAD, and immutable image reference. The private Compose `.env` is initialized with that digest reference, not a mutable tag. Keep the record outside the repository.

Check the image ID and the private paths before starting the service.

```sh
podman image inspect --format '{{.Id}}' "$IMAGE_REF"
python3 -m json.tool "$DEPLOY_ROOT/config/operator.json" >/dev/null
stat -c '%a %n' "$DEPLOY_ROOT" "$DEPLOY_ROOT/config" "$DEPLOY_ROOT/config/operator.json" "$DEPLOY_ROOT/state"
```

The setup helper refuses an existing deployment directory or a port that is already bound on loopback. It creates separate mode-0700 config and state directories, a mode-0600 operator file, and a private Compose `.env` file. The operator config has no repository roots, no CAS targets, and a nonexistent Codex binary path. Do not add credentials to this canary config.

## Start the isolated canary

Verify that the selected port is free before starting the container.

```sh
ss -ltn 'sport = :18788'
podman compose \
  --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml \
  config --quiet
podman compose \
  --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml \
  up --detach --wait --wait-timeout 60
```

An empty `ss` result means no process is listening on port 18788. If the port is in use, choose another free port and create a new private deployment directory.

The service runs as UID and GID 65532. Rootless `keep-id` maps that identity to the invoking user for the two bind mounts. Podman drops all capabilities, enables `no-new-privileges`, makes the image filesystem read-only, limits memory, CPU, and PIDs, and retries process failures at most three times. The MCP health check reports initialization health; Podman's Compose provider does not restart a still-running container solely because that health check fails.

Compose controls restart after process failure while the container engine is running. It does not create a boot-time user service; start this deployment explicitly after reboot or add a separately reviewed user Quadlet/systemd unit. The host's independent 18787 service remains outside this Compose project.

The host publishes only `127.0.0.1:18788`. The config mount is read-only. The state mount holds `runs.sqlite`, `runs.sqlite-wal`, and `runs.sqlite-shm` across container recreation. The private network has no external route.

## Run and repeat the planning-only check

The `prepare` phase checks MCP initialize, tools/list, resources/list, and the bundled UI resource. It checks that the ledger starts empty and that read-only capability inspection reports zero inference. It then changes only the local `profile` UI setting to `oci-canary`.

```sh
python3 plugins/luna-factory/container/acceptance_canary.py \
  --url http://127.0.0.1:18788/mcp --mode prepare
```

Recreate only this Compose service. Do not remove volumes or delete the state directory.

```sh
podman compose \
  --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml \
  up --detach --force-recreate --wait --wait-timeout 60
python3 plugins/luna-factory/container/acceptance_canary.py \
  --url http://127.0.0.1:18788/mcp --mode verify
```

The `verify` phase requires the same `oci-canary` setting after recreation. It repeats MCP discovery and the UI hash check. The settings canary rejects execution and CAS tool calls.

## Optional disposable planning-graph canary

The default deployment has no repository aliases or repository mounts. To exercise graph creation and revision-fenced planning, create a private throwaway Git repository and use the separate read-only Compose override. Do not point it at another project or worktree.

```sh
CANARY_REPO_DIR="$DEPLOY_ROOT/repository"
mkdir -m 0700 "$CANARY_REPO_DIR"
git -C "$CANARY_REPO_DIR" init --initial-branch main
printf '%s\n' 'Disposable Luna OCI planning canary.' > "$CANARY_REPO_DIR/README.md"
git -C "$CANARY_REPO_DIR" add README.md
git -C "$CANARY_REPO_DIR" -c user.name='Luna OCI Canary' -c user.email='luna-oci-canary@localhost' commit -m 'Initialize disposable planning canary'
export LUNA_CANARY_REPO_DIR="$CANARY_REPO_DIR"
python3 - "$DEPLOY_ROOT/config/operator.json" "$DEPLOY_ROOT/config/operator.planning-canary.json" <<'PY'
import json, sys
source, destination = sys.argv[1:]
config = json.load(open(source, encoding='utf-8'))
config['repositories'] = {'canary': {'root': '/opt/luna-canary/repo', 'max_finish': 'local_candidate'}}
with open(destination, 'x', encoding='utf-8') as output:
    json.dump(config, output, indent=2)
    output.write('\n')
PY
chmod 0600 "$DEPLOY_ROOT/config/operator.planning-canary.json"
```

The override exposes only that repository as a read-only bind mount. The graph canary allowlist permits graph create/read, proposal/apply, list, and capability reads. It checks identical request/proposal replay, an old-revision rejection, dependency context, native-local target preference, zero claims/inference/workers/CAS, and the bundled UI resource. The target is planning metadata only; the canary never starts it.

```sh
podman compose --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml \
  --file plugins/luna-factory/container/compose.planning-canary.yaml \
  config --quiet
podman compose --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml \
  --file plugins/luna-factory/container/compose.planning-canary.yaml \
  up --detach --force-recreate --wait --wait-timeout 60
python3 plugins/luna-factory/container/planning_graph_canary.py \
  --mode prepare --receipt "$DEPLOY_ROOT/planning-graph-receipt.json"
```

After `prepare`, the disposable ledger contains one `planning_only` graph record and no claims. Recreate only this service with the same two Compose files, then run the canary with `--mode verify` and the same receipt path. It requires the same graph ID, revision, node dependency, and target preference after recreation.

Check SQLite through a read-only connection after the canary.

```sh
python3 - "$DEPLOY_ROOT/state/runs.sqlite" <<'PY'
import sqlite3
import sys

uri = "file:" + sys.argv[1] + "?mode=ro"
with sqlite3.connect(uri, uri=True) as db:
	print("integrity:", db.execute("PRAGMA integrity_check").fetchone()[0])
	print("journal mode:", db.execute("PRAGMA journal_mode").fetchone()[0])
	print("runs:", db.execute("SELECT COUNT(*) FROM runs").fetchone()[0])
PY
```

Expect `integrity: ok`, WAL mode, and zero runs. The setting lives in SQLite, so it can persist without an execution graph or a worker. This proves local service persistence. It does not prove ChatGPT tool discovery or UI rendering.

Check both Luna listeners after the canary.

```sh
ss -ltnp | rg '127\.0\.0\.1:(18787|18788)\b'
XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  systemctl --user show luna-factory-v021-isolated-ebe275-20261009.service \
  -p ActiveState -p SubState -p MainPID
```

The existing host listener on 18787 and its Codex MCP entry must remain unchanged. The OCI canary uses 18788 and has no tunnel configuration.

## Back up the private state

Use SQLite's online backup API while the service is running. It includes committed WAL data in the destination database.

```sh
python3 - "$DEPLOY_ROOT/state/runs.sqlite" "$DEPLOY_ROOT/state-backup.sqlite" <<'PY'
import os
import sqlite3
import sys

source_uri = "file:" + sys.argv[1] + "?mode=ro"
with sqlite3.connect(source_uri, uri=True) as source:
	with sqlite3.connect(sys.argv[2]) as backup:
		source.backup(backup)
os.chmod(sys.argv[2], 0o600)
PY
```

Keep the backup outside the mounted state directory. Do not copy only `runs.sqlite` while SQLite is active. Keep the matching operator config and image-build record with the backup.

## Stop or roll back

Stop or remove the container without removing bind-mounted data.

```sh
podman compose \
  --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml stop
podman compose \
  --env-file "$DEPLOY_ROOT/.env" \
  --file plugins/luna-factory/container/compose.yaml down
```

Do not add `--volumes` or delete the config or state directories. The current host service on port 18787 remains available as the rollback path. The OCI canary uses an empty separate ledger. Never copy the original BLOCKED-run ledger into this image or volume.

Do not run an older runtime against a database after a schema change. Restore only a consistent SQLite backup with a compatible binary and config. Keep any newer state separately.

## Acceptance limits

This canary proves an OCI image build, rootless loopback service, MCP discovery, bundled-resource integrity, and local setting persistence across a container recreate. It does not register a Secure MCP Tunnel profile, alter the protected Executor tunnel, connect a ChatGPT account, render the workbench in ChatGPT, or run Codex inference. Keep those gates unverified until the exact private connection passes them.
