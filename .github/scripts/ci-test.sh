#!/usr/bin/env bash
# The CI test runner (ADR-0122), one part per step of the `test` job in ci.yml.
#
#   ci-test.sh rest      every test but the E2E suite and the real-time
#                        tests; the doc tests; one pass of every benchmark
#   ci-test.sh e2e-<i>   shard <i> (1-based) of E2E_SHARDS of the E2E suite
#   ci-test.sh timing    the real-time tests (.github/timing-tests.txt), one
#                        at a time; ci.yml does not fail the job on them
#
# The first two gate a merge. Their tests assert what the code decides, not
# how fast the runner is: the test audio devices wait for plugin hosts as an
# offline render does, and every wait is bounded by lumen_testkit's patience,
# a hang guard (LUMEN_TEST_PATIENCE). They run under cargo-nextest, each test
# in a process of its own (so no test shares a process's locks, statics or
# CPU budget with another), with the `ci` profile's retries: a pass on a
# retry is printed as FLAKY and ci.yml turns it into a warning
# (.config/nextest.toml).
#
# The real-time tests claim something about time (a block answered within
# its deadline on a shared three-core runner), which no runner promises. They
# run last, alone, and report rather than gate.
set -euo pipefail

part=${1:?usage: ci-test.sh rest | e2e-<shard> | timing}
shards=${E2E_SHARDS:-2}

# The hang guard of every wait in the tests, in seconds: far past what any
# wait needs on a loaded runner, and free while the tests pass.
export LUMEN_TEST_PATIENCE=${LUMEN_TEST_PATIENCE:-180}
# The test plugin and its host are built once, below, not by each test's
# process (the tests' `built()` helpers).
export LUMEN_TEST_PLUGINS_BUILT=1

nextest=(cargo nextest run --locked --color never)

# The real-time tests, one entry per line (comments and blank lines dropped).
timing_file=.github/timing-tests.txt
timing_entries() {
  sed -e 's/#.*//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$timing_file"
}
# A nextest filter for the timing list: whole test targets
# (`package::target`) and lumen-app's unit tests by path (`lumen-app name`).
timing_filter() {
  local filters=() package name
  while read -r package name; do
    if [ -z "$name" ]; then
      filters+=("binary_id(=$package)")
    else
      [ "$package" = lumen-app ] || { echo "$timing_file: unit tests of $package are not supported" >&2; exit 1; }
      filters+=("(binary_id(=lumen-app) & test(~$name))")
    fi
  done < <(timing_entries)
  local IFS='|'
  echo "${filters[*]}"
}
timing=$(timing_filter)
timing=${timing:-none()}

# lumen-app's test targets but the E2E suite: the files in tests/ and the
# directories with a main.rs, as cargo finds them. Cargo has no "every test
# target but one", and building the E2E suite here would cost minutes.
app_targets() {
  local entry name
  for entry in crates/lumen-app/tests/*.rs crates/lumen-app/tests/*/main.rs; do
    [ -e "$entry" ] || continue
    case "$entry" in
    */main.rs) name=$(basename "$(dirname "$entry")") ;;
    *) name=$(basename "$entry" .rs) ;;
    esac
    [ "$name" = e2e ] || printf -- '--test\n%s\n' "$name"
  done
}

build_plugins() {
  cargo build --locked -p lumen-test-plugin -p lumen-plugin-host
}

case "$part" in
rest)
  build_plugins
  # A read loop, not `mapfile`: macOS runs bash 3.2, which has no mapfile.
  targets=()
  while IFS= read -r line; do targets+=("$line"); done < <(app_targets)
  [ "${#targets[@]}" -gt 0 ] || { echo "no lumen-app test targets found" >&2; exit 1; }
  status=0
  # Two runs: the workspace but lumen-app, then lumen-app's targets but the
  # E2E suite, which the e2e parts run.
  "${nextest[@]}" --profile ci --workspace --exclude lumen-app \
    --lib --bins --tests --examples -E "not ($timing)" --no-tests=pass || status=$?
  "${nextest[@]}" --profile ci -p lumen-app --lib --bins "${targets[@]}" \
    -E "not ($timing)" --no-tests=pass || status=$?
  # Every divan benchmark once, as a smoke test, and the doc tests: the same
  # packages as the first run, since cargo unifies features over the
  # packages it is given and another set rebuilds what is built.
  cargo test --locked --workspace --exclude lumen-app --benches || status=$?
  cargo test --locked --workspace --exclude lumen-app --doc || status=$?
  exit $status
  ;;
timing)
  # After `rest`, which built the same targets: nothing is rebuilt.
  build_plugins
  status=0
  "${nextest[@]}" --profile timing --workspace --exclude lumen-app \
    --lib --bins --tests --examples -E "$timing" --no-tests=pass || status=$?
  "${nextest[@]}" --profile timing -p lumen-app --lib -E "$timing" --no-tests=pass || status=$?
  exit $status
  ;;
e2e-*)
  shard=${part#e2e-}
  if [ "$shard" -lt 1 ] || [ "$shard" -gt "$shards" ]; then
    echo "E2E shard $shard is outside 1..$shards (E2E_SHARDS)"
    exit 1
  fi
  build_plugins
  # nextest deals the scenarios out by count; every shard reads the same
  # list, so each scenario runs in exactly one shard, and a new one is
  # picked up with no edit here.
  "${nextest[@]}" --profile ci -p lumen-app --test e2e --partition "count:$shard/$shards"
  ;;
*)
  echo "unknown part: $part"
  exit 1
  ;;
esac
