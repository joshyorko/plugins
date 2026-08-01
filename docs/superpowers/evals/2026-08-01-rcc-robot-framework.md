# RCC Robot Framework evaluations

## Baseline 1: ht hash regression

Prompt:

```text
In joshyorko/rcc, add a Robot Framework regression test for `rcc ht hash` returning exit code 2. The diagnostic is on stderr while stdout must remain valid JSON. Explain the files and assertions you would use and the focused command you would run.
```

Response:

```text
Add the regression in the existing `rcc ht hash` Robot Framework test suite, with a fixture that causes hashing to fail. Assert exit code `2`, verify the diagnostic appears on stderr, and parse stdout as JSON to confirm it remains valid machine-readable output (and contains the expected error result).

Run the focused test with:

```sh
robot <path-to-ht-hash-robot-test>
```
```

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | FAIL | "existing `rcc ht hash` Robot Framework test suite" does not identify `robot_tests/ht_hash.robot`, `resources.robot`, or the RCC ownership boundary. |
| exit-code contract | PASS | "Assert exit code `2`" |
| stdout/stderr separation | PASS | "diagnostic appears on stderr" and "stdout ... valid machine-readable output" |
| JSON structural assertion | PASS | "parse stdout as JSON" |
| RCC state isolation | FAIL | No isolation rule is provided. |
| safe parallelism | FAIL | No Pabot/parallel-safety rule is provided. |
| focused command | FAIL | "robot <path-to-ht-hash-robot-test>" is not the repository’s focused command. |
| source grounding | FAIL | No pinned source, files, or source links are named. |

## Baseline 2: CI parallel reruns

Prompt:

```text
Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Response:

```text
```yaml
- name: Robot tests (parallel, rerun failures)
  shell: bash
  run: |
    set -euo pipefail
    export ROBOCORP_HOME="${ROBOCORP_HOME:?must be set by the existing workflow}"

    pabot --processes 4 --outputdir robot-results robot_tests
    first_status=$?

    if [ -f robot-results/output.xml ]; then
      pabot --processes 1 --rerunfailed robot-results/output.xml \
        --outputdir robot-rerun robot_tests
      rebot --merge robot-results/output.xml robot-rerun/output.xml \
        --output robot-results/output.xml
    fi

    exit "$first_status"
```

Isolation rules:

- Do not override, delete, or create a per-worker `ROBOCORP_HOME`; preserve the workflow’s existing value so RCC uses its established holotree cache behavior.
- Let Pabot isolate only Robot execution and output directories. Each Pabot invocation gets a distinct `--outputdir`.
- Do not run the rerun pass concurrently with the initial Pabot pass; rerun failed tests serially (`--processes 1`) against the original `output.xml`.
- Merge only after both runs complete, and retain the original `ROBOCORP_HOME` for every command.
```

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | FAIL | No RCC Robot Framework/RCC acceptance ownership is named. |
| exit-code contract | PASS | "exit \"$first_status\"" |
| stdout/stderr separation | FAIL | No command-stream preservation rule is supplied. |
| JSON structural assertion | FAIL | No structural JSON rule is supplied. |
| RCC state isolation | FAIL | "Do not ... create a per-worker `ROBOCORP_HOME`" conflicts with isolated mutable RCC state. |
| safe parallelism | FAIL | "Let Pabot isolate only Robot execution and output directories" leaves holotree and `tmp/` shared. |
| focused command | PASS | "pabot --processes 4 --outputdir robot-results robot_tests" |
| source grounding | FAIL | No RCC snapshot/source links are named. |

## Forward 1: ht hash regression (first run)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

In joshyorko/rcc, add a Robot Framework regression test for `rcc ht hash` returning exit code 2. The diagnostic is on stderr while stdout must remain valid JSON. Explain the files and assertions you would use and the focused command you would run.
```

Response:

```text
Add the regression to `robot_tests/ht_hash.robot`, using shared keywords from `robot_tests/resources.robot` and JSON parsing from `robot_tests/supporting.py`.

Use `Step    rcc ht hash ...    2`, then:

- `Use Stderr` and assert the expected diagnostic.
- `Use Stdout`, run `Must Be Json Response`, and assert the parsed JSON’s expected shape/fields.

This preserves the three contracts: exit code `2`, diagnostic only on stderr, and parseable JSON only on stdout.

Run:

```bash
python3 -m robot -L DEBUG -d tmp/output robot_tests/ht_hash.robot
```
```

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "`robot_tests/ht_hash.robot`" with shared suite files. |
| exit-code contract | PASS | "`Step    rcc ht hash ...    2`" |
| stdout/stderr separation | PASS | "`Use Stderr`" and "`Use Stdout`" |
| JSON structural assertion | PASS | "`Must Be Json Response`" and "expected shape/fields" |
| RCC state isolation | FAIL | No state-isolation decision is stated. |
| safe parallelism | FAIL | No parallel-safety decision is stated. |
| focused command | PASS | "python3 -m robot -L DEBUG -d tmp/output robot_tests/ht_hash.robot" |
| source grounding | PASS | "`robot_tests/resources.robot`" and "`robot_tests/supporting.py`" |

Small guidance change: require implementation/review answers to state source revision, focused command, contract boundaries, and isolation/parallel-safety decisions.

## Forward 1: ht hash regression (rerun)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

In joshyorko/rcc, add a Robot Framework regression test for `rcc ht hash` returning exit code 2. The diagnostic is on stderr while stdout must remain valid JSON. Explain the files and assertions you would use and the focused command you would run.
```

Response:

````text
At `joshyorko/rcc` commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`, add the regression to `robot_tests/ht_hash.robot`, with a deterministic invalid `package.yaml` fixture under `robot_tests/`.

Use:

```robot
Step    build/rcc ht hash --json --controller citests robot_tests/<invalid-fixture>.yaml    2
Use Stderr
Must Have    Blueprint calculation failed:
Use Stdout
Must Be Json Response
```

This asserts all three contracts independently: exact exit code `2`, the human diagnostic on stderr, and structurally valid JSON on stdout. Keep the JSON assertion structural via `Must Be Json Response`/`supporting.py:parse_json`, not a substring or golden-file comparison.

`robot_tests/resources.robot` already provides `Step`, stream selection, and JSON parsing; `robot_tests/__init__.robot` isolates `ROBOCORP_HOME` and serial shared holotree setup. No helper changes are needed unless the invalid input must be generated dynamically.

After building `build/rcc`, run serially with isolated artifacts:

```bash
python3 -m robot -L DEBUG -d tmp/ht-hash-output robot_tests/ht_hash.robot
```
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "`robot_tests/ht_hash.robot`" and shared suite files. |
| exit-code contract | PASS | "exact exit code `2`" |
| stdout/stderr separation | PASS | "diagnostic on stderr" and "JSON on stdout" |
| JSON structural assertion | PASS | "`Must Be Json Response`/`supporting.py:parse_json`" |
| RCC state isolation | PASS | "isolates `ROBOCORP_HOME`" and "isolated artifacts" |
| safe parallelism | PASS | "serial shared holotree setup" and "run serially" |
| focused command | PASS | "python3 -m robot -L DEBUG -d tmp/ht-hash-output robot_tests/ht_hash.robot" |
| source grounding | PASS | "commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`" and named source files. |

## Forward 2: CI parallel reruns (first run)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Response:

````text
`robot_tests` cannot run safely under Pabot unchanged: `Prepare Local` hard-codes `tmp/robocorp`, `tmp/fluffy`, and `tmp/nodogs`. Give each Pabot process a private checkout and run the existing setup unchanged inside it; this preserves its `ROBOCORP_HOME=tmp/robocorp` and holotree initialization/revocation behavior.

```yaml
robot-tests:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4

    - uses: actions/setup-python@v5
      with:
        python-version: "3.11"

    - name: Install Robot runners
      run: python -m pip install --disable-pip-version-check "robotframework==7.2.2" "robotframework-pabot==5.1.0"

    - name: Build RCC
      run: go build -o build/rcc ./cmd/rcc

    - name: Run parallel tests, rerun failures, and merge results
      shell: bash
      run: |
        set -o pipefail
        mkdir -p artifacts/initial artifacts/rerun artifacts/merged ci/workers

        set +e
        pabot \
          --processes 4 \
          --command bash ci/run-robot-worker.sh --end-command \
          --outputdir "$GITHUB_WORKSPACE/artifacts/initial" \
          --xunit xunit.xml \
          robot_tests
        initial_status=$?
        set -e

        if [ "$initial_status" -ne 0 ]; then
          set +e
          pabot \
            --processes 4 \
            --command bash ci/run-robot-worker.sh --end-command \
            --rerunfailed "$GITHUB_WORKSPACE/artifacts/initial/output.xml" \
            --outputdir "$GITHUB_WORKSPACE/artifacts/rerun" \
            robot_tests
          rerun_status=$?
          set -e

          rebot \
            --merge \
            --outputdir "$GITHUB_WORKSPACE/artifacts/merged" \
            --xunit xunit.xml \
            "$GITHUB_WORKSPACE/artifacts/initial/output.xml" \
            "$GITHUB_WORKSPACE/artifacts/rerun/output.xml"
          merge_status=$?

          [ "$rerun_status" -eq 0 ] || exit "$rerun_status"
          exit "$merge_status"
        fi

    - name: Upload Robot results
      if: always()
      uses: actions/upload-artifact@v4
      with:
        name: robot-results
        path: artifacts/
```

`ci/run-robot-worker.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

outputdir=
for ((i=1; i <= $#; i++)); do
  if [ "${!i}" = "--outputdir" ]; then
    next=$((i + 1))
    outputdir="${!next}"
    break
  fi
done

[ -n "$outputdir" ] || { echo "missing Pabot worker output directory" >&2; exit 2; }

worker_root="$(dirname "$outputdir")/worker-${outputdir##*/}"
mkdir -p "$worker_root"
rsync -a --delete \
  --exclude .git \
  --exclude artifacts \
  --exclude ci/workers \
  "$GITHUB_WORKSPACE/" "$worker_root/"
cd "$worker_root"

exec robot "$@"
```

Isolation rules:

- Never share a checkout, `tmp/`, `ROBOCORP_HOME`, holotree, Robot output directory, or cleanup target between Pabot workers.
- Let each copied checkout retain `Prepare Local`; it creates `tmp/robocorp`, runs `ht init --revoke`, and primes holotree only within that worker.
- Keep initial, rerun, and merged output directories distinct. Reruns begin only after the initial `output.xml` is complete.
- Serialize any setup that cannot be confined to the worker checkout, or protect it with PabotLib; locks do not make shared RCC state safe.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "`robot_tests` cannot run safely under Pabot unchanged" |
| exit-code contract | PASS | "`[ \"$rerun_status\" -eq 0 ] || exit \"$rerun_status\"`" |
| stdout/stderr separation | FAIL | No explicit command-stream contract. |
| JSON structural assertion | FAIL | No explicit structural JSON contract. |
| RCC state isolation | PASS | "Never share a checkout, `tmp/`, `ROBOCORP_HOME`, holotree" |
| safe parallelism | PASS | "Serialize any setup ... or protect it with PabotLib" |
| focused command | PASS | "`pabot --processes 4`" with initial/rerun outputs. |
| source grounding | FAIL | No pinned revision/source files are stated. |

Small guidance change: require implementation/review answers to state source revision and all contract boundaries, including cross-cutting stream/JSON rules.

## Forward 2: CI parallel reruns (rerun)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Response:

````text
At `joshyorko/rcc` commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`, a direct Pabot invocation is unsafe: [`robot_tests/__init__.robot`](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/__init__.robot) calls `Prepare Local`, and [`resources.robot`](https://github.com/joshyorko/rcc/blob/d5942d90994d7bd9034aeed6b88cc60fd7a3e330/robot_tests/resources.robot) deletes shared `tmp/*`, sets `ROBOCORP_HOME=tmp/robocorp`, and revokes/deletes holotrees. Do not parallelize until that state is isolated.

Preserve that behavior by giving every Pabot worker a separate checkout/working directory, such as `ci/workers/$PABOTQUEUEINDEX`, each with its own `tmp/robocorp`; run the existing setup unchanged inside that directory. Build `build/rcc` in every worker checkout. Pabot’s worker results must be separate (`ci/initial/pabot_results/<worker>` and `ci/rerun/pabot_results/<worker>`), while its merged `output.xml` belongs only to the corresponding pass. Shared setup that cannot be moved into the worker checkout must run serially or use a PabotLib lock; locks do not make shared `ROBOCORP_HOME` or holotrees safe.

With a Pabot worker-start hook that changes to `ci/workers/$PABOTQUEUEINDEX` before suite setup, the CI commands are:

```bash
python -m robot --dryrun --outputdir ci/dryrun robot_tests

set +e
pabot --processes 4 \
  --outputdir ci/initial \
  --xunit xunit.xml \
  --listener ci/pabot_worker_cwd.py \
  robot_tests
first_status=$?
set -e

if [ "$first_status" -ne 0 ]; then
  pabot --processes 4 \
    --rerunfailed ci/initial/output.xml \
    --outputdir ci/rerun \
    --xunit xunit.xml \
    --listener ci/pabot_worker_cwd.py \
    robot_tests

  rebot --merge \
    --outputdir ci/merged \
    --xunit xunit.xml \
    ci/initial/output.xml ci/rerun/output.xml
fi

exit "$first_status"
```

Upload `ci/initial`, `ci/rerun`, and `ci/merged` artifacts (`output.xml`, `log.html`, `report.html`, and xUnit) with an always-run artifact step. The rerun starts only after the initial output is complete; it never rewrites the initial directory. Keep the first-pass failure status as the job result so a passing retry does not silently mask flakes.

No test assertion contract changes: `Step` must continue checking the exact exit code and retaining separate stdout/stderr; JSON remains structurally checked through `Must Be Json Response`/`Parse JSON`, never substring-matched.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "a direct Pabot invocation is unsafe" for RCC acceptance suites. |
| exit-code contract | PASS | "`Step` must continue checking the exact exit code" and `exit "$first_status"`. |
| stdout/stderr separation | PASS | "retaining separate stdout/stderr" |
| JSON structural assertion | PASS | "structurally checked through `Must Be Json Response`/`Parse JSON`" |
| RCC state isolation | FAIL | The proposed isolation depends on an undefined "Pabot worker-start hook" and does not define how worker copies are provisioned. |
| safe parallelism | FAIL | `--listener ci/pabot_worker_cwd.py` references a helper that does not exist or appear in the response. |
| focused command | FAIL | The Pabot commands are not runnable without the undefined `ci/pabot_worker_cwd.py` prerequisite. |
| source grounding | PASS | pinned commit and links to `robot_tests/__init__.robot` and `resources.robot`. |

Correction after review: this rerun did not pass all rows. The unchanged shared-state suite must run serially. Pabot is permitted only after suite code derives all mutable paths from a worker identity or CI provisions truly isolated copies with defined, runnable commands.

## Fix Round 1 forward CI (first run)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Response:

````text
No safe Pabot job exists for `joshyorko/rcc` at commit `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`. `robot_tests/resources.robot` hard-codes shared `tmp/` paths and `ROBOCORP_HOME=tmp/robocorp`, while setup revokes and deletes holotree state. Concurrent Pabot workers would race and corrupt one another. PabotLib locks do not fix this.

Use this serial rerun job now:

```yaml
robot:
  name: Robot Tests (${{ matrix.os }})
  runs-on: ${{ matrix.os }}
  strategy:
    fail-fast: false
    matrix:
      os: [ubuntu-latest, macos-latest, windows-latest]

  steps:
    - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

    - uses: actions/setup-go@4a3601121dd01d1626a1e23e37211e3254c1c06c
      with:
        go-version: "1.26.5"

    - uses: actions/setup-python@a309ff8b426b58ec0e2a45f0f869d46889d02405
      with:
        python-version: "3.10"

    - name: Install and build
      shell: bash
      run: |
        python -m pip install invoke==2.2.0
        inv robotsetup
        inv assets
        inv local

    - name: Initial Robot run
      id: initial
      continue-on-error: true
      shell: bash
      run: |
        python -m robot \
          -L DEBUG \
          --outputdir tmp/robot-initial \
          --xunit xunit.xml \
          robot_tests

    - name: Rerun failed tests
      id: rerun
      if: steps.initial.outcome == 'failure'
      continue-on-error: true
      shell: bash
      run: |
        python -m robot \
          -L DEBUG \
          --rerunfailed tmp/robot-initial/output.xml \
          --outputdir tmp/robot-rerun \
          --xunit xunit.xml \
          robot_tests

    - name: Merge initial and rerun results
      if: steps.initial.outcome == 'failure'
      shell: bash
      run: |
        python -m robot.rebot \
          --merge \
          --output tmp/robot-final/output.xml \
          --log tmp/robot-final/log.html \
          --report tmp/robot-final/report.html \
          --xunit tmp/robot-final/xunit.xml \
          tmp/robot-initial/output.xml \
          tmp/robot-rerun/output.xml

    - name: Preserve successful initial results
      if: steps.initial.outcome == 'success'
      shell: bash
      run: |
        mkdir -p tmp/robot-final
        cp tmp/robot-initial/output.xml tmp/robot-final/output.xml
        cp tmp/robot-initial/log.html tmp/robot-final/log.html
        cp tmp/robot-initial/report.html tmp/robot-final/report.html
        cp tmp/robot-initial/xunit.xml tmp/robot-final/xunit.xml

    - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
      if: always()
      with:
        name: ${{ matrix.os }}-test-reports
        path: |
          tmp/robot-initial/
          tmp/robot-rerun/
          tmp/robot-final/
```

The merge command is the final test gate: it exits nonzero if failures remain, while a clean rerun makes the job pass.

Pabot can be enabled only after all mutable state is worker-scoped. The required isolation rules are:

- Derive a stable worker root from Pabot’s real worker identity, such as `${PABOTEXECUTIONPOOLID}`.
- Set each worker’s `ROBOCORP_HOME` to a unique path such as `tmp/pabot/${PABOTEXECUTIONPOOLID}/robocorp`.
- Move every mutable fixture and cleanup target—including `tmp/fluffy`, `tmp/nodogs`, archives, traversal bundles, and suite-generated files—under that worker root.
- Keep holotree initialization, revoke, delete, caches, and spaces inside the worker-specific `ROBOCORP_HOME`.
- Never delete another worker’s root.
- Keep `build/rcc` read-only during execution.
- Give the initial run, rerun, and merged results separate output directories.
- Preserve `supporting.py`’s RCC activation-variable scrubbing and Windows executable normalization.

Only after those repository changes exist is this Pabot sequence valid:

```bash
python -m pabot -L DEBUG --processes 4 --outputdir tmp/robot-initial robot_tests

python -m pabot -L DEBUG --processes 4 \
  --rerunfailed tmp/robot-initial/output.xml \
  --outputdir tmp/robot-rerun \
  robot_tests

python -m robot.rebot --merge \
  --output tmp/robot-final/output.xml \
  --log tmp/robot-final/log.html \
  --report tmp/robot-final/report.html \
  --xunit tmp/robot-final/xunit.xml \
  tmp/robot-initial/output.xml \
  tmp/robot-rerun/output.xml
```

Until that isolation refactor lands, replacing the serial commands with Pabot would not preserve the repository’s existing `ROBOCORP_HOME` and holotree behavior.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "No safe Pabot job exists for `joshyorko/rcc`" |
| exit-code contract | PASS | "The merge command is the final test gate: it exits nonzero if failures remain" |
| stdout/stderr separation | FAIL | No explicit stdout/stderr assertion contract is stated. |
| JSON structural assertion | FAIL | No explicit structural JSON assertion contract is stated. |
| RCC state isolation | PASS | "Pabot can be enabled only after all mutable state is worker-scoped" plus concrete path rules. |
| safe parallelism | PASS | "Only after those repository changes exist is this Pabot sequence valid" |
| focused command | PASS | The response gives runnable serial initial/rerun/merge commands and labels Pabot commands conditional. |
| source grounding | PASS | pinned commit plus `robot_tests/resources.robot` and `supporting.py`. |

Small guidance change: require an explicit **Acceptance contracts** slot in every RCC CI/Pabot answer.

## Fix Round 1 forward CI (rerun)

Prompt:

```text
Use $rcc-robot-framework at plugins/rcc/skills/rcc-robot-framework/SKILL.md.

Design a GitHub Actions job that reruns failed tests and uses Pabot to parallelize joshyorko/rcc/robot_tests. Preserve the repository's existing ROBOCORP_HOME and holotree behavior. Give the exact commands and isolation rules.
```

Response:

````text
At pinned source revision `d5942d90994d7bd9034aeed6b88cc60fd7a3e330`, direct Pabot worker parallelism is unsafe: `robot_tests/__init__.robot` runs `Prepare Local`, which shares `tmp/robocorp`, other `tmp/` fixtures, and holotree state. Use Pabot sharding across isolated GitHub-hosted matrix jobs, with one Pabot process per checkout:

```yaml
robot:
  name: Robot Tests (${{ matrix.os }}, shard ${{ matrix.shard }}/${{ matrix.shards }})
  runs-on: ${{ matrix.os }}
  if: github.event_name == 'push' || github.event_name == 'pull_request'
  strategy:
    fail-fast: false
    matrix:
      os: [ubuntu-latest, macos-latest, windows-latest]
      shard: [1, 2, 3]
      shards: [3]

  steps:
    - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd

    - uses: actions/setup-go@4a3601121dd01d1626a1e23e37211e3254c1c06c
      with:
        go-version: "1.26.5"

    - uses: actions/setup-python@a309ff8b426b58ec0e2a45f0f869d46889d02405
      with:
        python-version: "3.10"

    - name: Install test tooling
      run: |
        python -m pip install invoke==2.2.0
        inv robotsetup
        python -m pip install robotframework-pabot==2.18.0

    - name: Build local RCC
      run: |
        inv assets
        inv local

    - name: Show commit info
      run: inv what

    - name: Run shard and rerun failures
      shell: bash
      env:
        SHARD: ${{ matrix.shard }}
        SHARDS: ${{ matrix.shards }}
      run: |
        initial_rc=0
        python -m pabot.pabot \
          --processes 1 \
          --shard "${SHARD}/${SHARDS}" \
          --loglevel DEBUG \
          --outputdir tmp/output/initial \
          robot_tests || initial_rc=$?

        if [ "$initial_rc" -eq 0 ]; then
          python -m robot.rebot \
            --outputdir tmp/output/final \
            --output output.xml \
            --log log.html \
            --report report.html \
            --xunit xunit.xml \
            tmp/output/initial/output.xml
          exit $?
        fi

        rerun_rc=0
        python -m robot \
          --rerunfailed tmp/output/initial/output.xml \
          --loglevel DEBUG \
          --outputdir tmp/output/rerun \
          robot_tests || rerun_rc=$?

        merge_rc=0
        python -m robot.rebot \
          --merge \
          --outputdir tmp/output/final \
          --output output.xml \
          --log log.html \
          --report report.html \
          --xunit xunit.xml \
          tmp/output/initial/output.xml \
          tmp/output/rerun/output.xml || merge_rc=$?

        exit "$merge_rc"

    - name: Upload Robot reports
      if: always()
      uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
      with:
        name: ${{ matrix.os }}-robot-shard-${{ matrix.shard }}
        path: tmp/output/
```

Isolation rules:

- Each shard must run on its own fresh GitHub-hosted runner and checkout.
- Keep `--processes 1`; do not run multiple Pabot workers inside one checkout.
- Do not set a job-level `ROBOCORP_HOME`, `RCC_HOLOTREE_SPACE_ROOT`, or shared cache path.
- Keep `robot_tests/__init__.robot`, `resources.robot`, and `supporting.py` unchanged so each isolated checkout still creates `ROBOCORP_HOME=tmp/robocorp`, scrubs activation variables, and performs `ht init --revoke`.
- Never share `tmp/`, `tmp/robocorp`, holotree state, fixtures, or cleanup targets between shards.
- Do not use PabotLib locks as a substitute for isolation.
- The rerun gets its own output directory; only `rebot --merge` combines completed initial and rerun results.
- The merged result determines the job status, while `if: always()` preserves all artifacts.

Acceptance contracts: `Step` still asserts the exact exit code, stdout and stderr remain separate, and JSON stdout is parsed structurally with `Must Be Json Response`/`Parse JSON`, never substring matching.
````

| Rubric | Verdict | Response fragment |
| --- | --- | --- |
| correct owner/skill | PASS | "direct Pabot worker parallelism is unsafe" for `robot_tests/__init__.robot`. |
| exit-code contract | PASS | "`Step` still asserts the exact exit code" and "merged result determines the job status". |
| stdout/stderr separation | PASS | "stdout and stderr remain separate" |
| JSON structural assertion | PASS | "parsed structurally with `Must Be Json Response`/`Parse JSON`" |
| RCC state isolation | PASS | "own fresh GitHub-hosted runner and checkout" and no shared RCC/temp state. |
| safe parallelism | PASS | "Keep `--processes 1`; do not run multiple Pabot workers inside one checkout." |
| focused command | PASS | Exact conditional Pabot shard, serial rerun, merge, and artifact commands are provided without undefined helpers. |
| source grounding | PASS | pinned revision plus `robot_tests/__init__.robot`, `resources.robot`, and `supporting.py`. |

Fix Round 1 conclusion: all rows pass with current-suite safety stated truthfully. Parallelism uses isolated matrix checkouts with one Pabot process per checkout; the unchanged suite is never concurrently executed inside shared mutable state.
