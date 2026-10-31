#!/usr/bin/env bash
# The CI test runner, one part per job (see the `test` job in ci.yml).
#
#   ci-test.sh rest      every test target but the E2E suite, plus doc tests
#   ci-test.sh e2e-<i>   shard <i> (1-based) of the E2E suite's modules
#
# The E2E suite is one test binary of about 760 scenarios that takes most of
# the time. Its scenarios share in-process locks (`timing_sensitive`) that
# keep real-time scenarios from starving one another's plugin hosts, so it is
# split by module across jobs and never into one process per test.
#
# Every module goes to exactly one shard: the shards read the same list and
# deal the modules out the same way (largest first, to the lightest shard),
# so a new module is picked up with no edit here.
set -euo pipefail

part=${1:?usage: ci-test.sh rest | e2e-<shard>}
shards=${E2E_SHARDS:-2}

case "$part" in
rest)
  # Cargo has no "all test targets but one", so name lumen-app's others: the
  # files in tests/ and the directories with a main.rs, as cargo finds them.
  targets=()
  for entry in crates/lumen-app/tests/*.rs crates/lumen-app/tests/*/main.rs; do
    [ -e "$entry" ] || continue
    case "$entry" in
    */main.rs) name=$(basename "$(dirname "$entry")") ;;
    *) name=$(basename "$entry" .rs) ;;
    esac
    [ "$name" = e2e ] || targets+=(--test "$name")
  done
  status=0
  # --all-targets also runs every divan benchmark once as a smoke test.
  # --no-fail-fast runs every test binary, so one run shows all failures.
  cargo test --workspace --exclude lumen-app --all-targets --locked --no-fail-fast || status=$?
  cargo test -p lumen-app --lib --bins --benches --examples "${targets[@]}" --locked --no-fail-fast || status=$?
  # The same packages as the first run: cargo unifies features over the
  # packages it is given, so a different set rebuilds crates that are already
  # built (23 s for lumen-app alone on Windows). lumen-app has no doc tests.
  cargo test --workspace --exclude lumen-app --doc --locked || status=$?
  exit $status
  ;;
e2e-*)
  shard=${part#e2e-}
  if [ "$shard" -lt 1 ] || [ "$shard" -gt "$shards" ]; then
    echo "E2E shard $shard is outside 1..$shards (E2E_SHARDS)"
    exit 1
  fi
  # The scenarios' names are module::scenario; the plugin routing walkthrough
  # runs on its own, below, so it is not dealt out.
  scenarios=$(cargo test -p lumen-app --test e2e --locked -- --list --format terse 2>/dev/null |
    sed -n 's/: test$//p' | grep -v '^walkthrough_plugins::' || true)
  # A filter is a substring, so `record::` also matches `audio_record::`. A
  # module and the modules whose names end with it are one unit, named by
  # the shortest of them, and its filter matches exactly the unit.
  modules=$(printf '%s\n' "$scenarios" | awk -F'::' '
      NF { n[$1]++ }
      END {
        for (m in n) {
          root = m
          for (r in n)
            if (length(r) < length(root) && substr(m, length(m) - length(r) + 1) == r) root = r
          size[root] += n[m]
        }
        for (r in size) print size[r], r
      }' |
    LC_ALL=C sort -k1,1nr -k2,2 |
    awk -v shards="$shards" -v shard="$shard" '
      { lightest = 0
        for (s = 1; s < shards; s++) if (load[s] < load[lightest]) lightest = s
        load[lightest] += $1
        if (lightest == shard - 1) print $2 }')
  if [ -z "$modules" ]; then
    echo "E2E shard $shard of $shards has no modules: the suite could not be listed"
    exit 1
  fi
  echo "E2E shard $shard of $shards: $(echo "$modules" | wc -l | tr -d ' ') modules"
  filters=()
  for m in $modules; do filters+=("$m::"); done
  status=0
  # `plugins::` also matches `walkthrough_plugins::`, which is not dealt out
  # (its unit is `plugins`): skip it here.
  cargo test -p lumen-app --test e2e --locked -- --skip walkthrough_plugins:: "${filters[@]}" || status=$?
  if [ "$shard" -eq 1 ]; then
    # Five live plugin hosts share the callback deadline in this flow.
    # Run it without competing E2E scenarios: peer tests can starve a
    # host and trigger the intended dry fallback, resetting peak meters.
    # Keep its audio deadlines and sound assertions unchanged.
    cargo test -p lumen-app --test e2e --locked walkthrough_plugins:: -- --nocapture || status=$?
  fi
  exit $status
  ;;
*)
  echo "unknown part: $part"
  exit 1
  ;;
esac
