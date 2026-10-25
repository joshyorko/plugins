# Luna Factory slice brief v2: addendum to /home/kdlocpanda/.cache/luna-factory-briefs/lf-brief.md (read both)

## Machine safety (mandatory; the owner's machine has 15 GB RAM and the owner's desktop apps use about 10 GB of it)
- **Rust builds and tests run one at a time machine-wide**, behind a host lock, with resource caps:
  ```
  flock /tmp/luna-cargo.lock podman run --rm --cpus 4 --memory 3500m --memory-swap 5g --security-opt label=disable --network=host \
    -v $WT:/work -v $HOME/.cache/luna-cargo-target:/target -w /work/plugins/luna-factory/server -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS=3 -e CARGO_PROFILE_DEV_DEBUG=line-tables-only \
    localhost/luna-factory-dogfood-tooling:rust-1.99.0-20261010 sh -c '<cargo commands>'
  ```
  Batch your cargo commands into as few locked invocations as possible, for example fmt, clippy, test and build in one invocation.
- **Browser captures run one at a time:**
  ```
  flock /tmp/luna-browser.lock npm run test:host
  flock /tmp/luna-browser.lock env LUNA_FACTORY_BINARY=... npm run test:live-host
  ```
- **Plain `npm test`, `tsc` and Python tests** need no lock.
- **Never run more than one heavy command in the background yourself.**
- **Never copy `server/target` into `/tmp`** (it's a tmpfs). If a test copies the tree, set `TMPDIR` to a directory inside your worktree's gitignored `server/target`.

## Integration facts (current as of this brief)
- `/tmp` is a tmpfs and was wiped by a reboot on 2026-10-11. Never keep anything you need there; the lock files are recreated by `flock` automatically. The Python venv is `$HOME/.cache/lf-venv`.
- `origin/main` is `e194b01`. #87 (receipts `thread_id`, run timing, `source.display`, `blocker_kind`) is MERGED; rebase your branch onto `origin/main` (`e194b01`) before your final push and resolve conflicts keeping both sides' behavior.
  - GitHub CI can be starved by another repo's queue, so do not wait on hosted CI; the integrator watches it.
  - Never rebase onto another unmerged branch.
- **Canary tool counts** (`container/acceptance_canary.py`, `planning_graph_canary.py`) hard-code the MCP tool count, which is still 22 on main (#87 added no tools). Update the literals to YOUR branch's count relative to main (main's 22 plus the tools you add), and assert any new tool's visibility. The integrator reconciles the final count at merge time.
- **`ALLOWED_RELEASE_DELTA`** in `container/build_image.py` is per-file. Add every new or changed path outside `container/` and `ui/`, and make `tests/test_container_build.py::test_current_head_release_delta_is_reviewed` pass on your HEAD.

## Shared forge-identity contract (#76 and #77 must agree)
- **Forge identity** for a GitHub item: `{provider: "github", repository: "owner/repo" (lowercase), number: <u64>, node_id?: <string>}`.
  - Canonical key: `github:owner/repo#<number>`.
  - Provider is a lowercase enum: `github | local | fixture`.
- **Physical identity** stays the graph's opaque local repository identity plus the repository alias. Never a filesystem path.
- **Graph node sources** keep the existing contract: `{provider, repository_id: <physical identity>, item_id: "<repo_db_id>:<node_id>", revision, display?{number,url}}`.
- **Provenance** must be explicit:
  - `"supplied"` for caller or ChatGPT-provided metadata
  - `"github_app"` only for data Luna itself fetched with the GitHub App

  Never label supplied data "reported by GitHub".

## Issue hygiene
- **Do not write `Closes #N` unless every acceptance criterion of #N is met.** Use `Part of #N`, and list the residual acceptance explicitly in the PR body.
- **Do not create new issues.**

## Result
Finish with the structured result the integrator asks for. Be exact. Never claim a check you didn't run.

LOW-MEMORY RULE (user: "don't overload my device"; ~5 GB free). Never run more than one cargo/podman build or browser run at a time (always through the flock lines above). Run vitest/npm tests with `nice -n 10` and `--maxWorkers=2` (or `--pool=forks --poolOptions.forks.maxForks=2`). Never start a dev server, watch mode or a second Chrome. If `free -g` shows under 2 GB available, wait 60s and retry instead of starting a heavy step.

SHARED CARGO CACHE (2026-10-14): every worktree builds into the ONE shared target dir ~/.cache/luna-cargo-target (mounted at /target, see the command above). Never create plugins/luna-factory/server/target in a worktree; if one exists, delete it. The flock keeps builds one at a time; dependencies stay compiled across branches, so rebuilds are fast. The compiled binary for graph_e2e is /target/release/luna-factoryd inside the container (host: ~/.cache/luna-cargo-target/release/luna-factoryd), but the last build may be from another branch, so rebuild under the lock right before using it.
SIX WORKERS RUN AT ONCE: use `nice -n 10` for everything, `vitest run --maxWorkers=1`, one browser capture run at a time via flock /tmp/luna-browser.lock, and if `free -g` shows under 2 GB available, wait 60s before any heavy step. Leave the release build, validate jobs and full Python container tests to GitHub CI.
