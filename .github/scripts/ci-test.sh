#!/usr/bin/env bash
set -euo pipefail

part=${1:?usage: ci-test.sh rest | e2e-<shard> | timing}
shards=${E2E_SHARDS:-2}

export LUMEN_TEST_PATIENCE=${LUMEN_TEST_PATIENCE:-180}
export LUMEN_TEST_PLUGINS_BUILT=1

nextest=(cargo nextest run --locked --color never)

timing_file=.github/timing-tests.txt
timing_entries() {
  sed -e 's/#.*//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$timing_file"
}
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
  targets=()
  while IFS= read -r line; do targets+=("$line"); done < <(app_targets)
  [ "${#targets[@]}" -gt 0 ] || { echo "no lumen-app test targets found" >&2; exit 1; }
  status=0
  "${nextest[@]}" --profile ci --workspace --exclude lumen-app \
    --lib --bins --tests --examples -E "not ($timing)" --no-tests=pass || status=$?
  "${nextest[@]}" --profile ci -p lumen-app --lib --bins "${targets[@]}" \
    -E "not ($timing)" --no-tests=pass || status=$?
  cargo test --locked --workspace --exclude lumen-app --benches || status=$?
  cargo test --locked --workspace --exclude lumen-app --doc || status=$?
  exit $status
  ;;
timing)
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
  "${nextest[@]}" --profile ci -p lumen-app --test e2e --partition "count:$shard/$shards"
  ;;
*)
  echo "unknown part: $part"
  exit 1
  ;;
esac
