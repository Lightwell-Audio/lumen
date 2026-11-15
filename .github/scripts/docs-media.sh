#!/usr/bin/env bash
set -euo pipefail

command=${1:?usage: docs-media.sh plan|render|keep|plans|gather ...}
shift

scene_seconds() {
  local total=0 file
  for file in "$1/$2.seconds" "$1/$2".*.seconds; do
    if [ -f "$file" ]; then total=$((total + $(cat "$file"))); fi
  done
  echo "$total"
}

case $command in
plan)
  published=$1 plan=$2 full=${3:-false} jobs=${4:-4}
  manifest=$(mktemp)
  if ! curl -fsSL --retry 3 "$published/manifest.json" -o "$manifest"; then
    echo "No published media yet: every scene renders."
    rm -f "$manifest"
  fi
  mkdir -p "$(dirname "$plan")"
  DOCS_PLAN="$plan" DOCS_PLAN_PUBLISHED="$manifest" DOCS_PLAN_FULL="$full" \
    DOCS_PLAN_JOBS="$jobs" cargo test --locked -p lumen-app --test docs_coverage \
    plan_the_render -- --ignored --exact
  generation=0
  if [ "$full" = true ]; then
    generation=$(printf '%s-%s-%s' "${GITHUB_SHA:-local}" "${GITHUB_RUN_ID:-0}" \
      "${GITHUB_RUN_ATTEMPT:-0}" | sha256sum | cut -c1-16)
  elif [ -s "$manifest" ]; then
    generation=$(jq -r '.generation // "0"' "$manifest" 2> /dev/null || echo 0)
  fi
  jq --arg g "$generation" '.generation = $g' "$plan" > "$plan.tmp"
  mv "$plan.tmp" "$plan"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "### Docs media"
      echo
      echo "| Scene | This run | Rendered at |"
      echo "|---|---|---|"
      jq -r '(.why | to_entries[] | "| \(.key) | renders (\(.value)) | this commit |"),
             (.reuse | to_entries[] | "| \(.key) | reused | \(.value.commit[0:8]) |")' "$plan"
    } >> "$GITHUB_STEP_SUMMARY"
  fi
  ;;
render)
  out=$1
  shift
  mkdir -p "$out"
  cache=${DOCS_SCENE_CACHE:-}
  plan=${DOCS_PLAN_JSON:-}
  built=false
  for piece in "$@"; do
    test=${piece%%:*}
    theme=
    if [ "$piece" != "$test" ]; then theme=${piece#*:}; fi
    entry=
    if [ -n "$cache" ] && [ -n "$plan" ]; then
      entry="$cache/$test.$(jq -r --arg t "$test" '.fingerprints[$t]' "$plan")"
    fi
    if [ -n "$entry" ] && [ "${DOCS_SCENE_CACHE_READ:-1}" != 0 ] \
      && [ -d "$entry" ] && [ -f "$entry.seconds" ] \
      && [ -n "$(ls -A "$entry")" ]; then
      echo "$test: rendered before from the same sources; reusing it."
      rm -rf "$out/$test"
      cp -R "$entry" "$out/$test"
      cp "$entry.seconds" "$out/$test.seconds"
      continue
    fi
    if ! $built; then
      cargo test --locked -p lumen-app --test docs_media --no-run
      built=true
    fi
    if [ -n "${DOCS_RENDERED_MARK:-}" ]; then touch "$DOCS_RENDERED_MARK"; fi
    started=$(date +%s)
    if [ -n "$theme" ]; then export DOCS_THEME=$theme; else unset DOCS_THEME; fi
    DOCS_MEDIA="$out/$test" cargo test --locked -p lumen-app --test docs_media \
      -- --ignored --exact --test-threads=1 --nocapture "$test"
    unset DOCS_THEME
    echo $(( $(date +%s) - started )) > "$out/$test${theme:+.$theme}.seconds"
  done
  ;;
keep)
  cache=$1 plan=$2 scenes=$3
  rm -rf "$cache"
  mkdir -p "$cache"
  for test in $(jq -r '.render[]' "$plan"); do
    entry="$cache/$test.$(jq -r --arg t "$test" '.fingerprints[$t]' "$plan")"
    cp -R "$scenes/$test" "$entry"
    scene_seconds "$scenes" "$test" > "$entry.seconds"
  done
  du -sh "$cache"
  ;;
gather)
  published=$1 plan=$2 scenes=$3 media=$4
  mkdir -p "$media"
  rendered='{}'
  for test in $(jq -r '.render[]' "$plan"); do
    cp -R "$scenes/$test/." "$media/"
    rendered=$(jq -c --arg t "$test" \
      --arg fingerprint "$(jq -r --arg t "$test" '.fingerprints[$t]' "$plan")" \
      --arg commit "${GITHUB_SHA:-local}" \
      --argjson seconds "$(scene_seconds "$scenes" "$test")" \
      --argjson files "$(ls "$scenes/$test" | jq -R . | jq -sc .)" \
      '.[$t] = {fingerprint: $fingerprint, commit: $commit, seconds: $seconds, files: $files}' \
      <<< "$rendered")
  done
  for test in $(jq -r '.reuse | keys[]' "$plan"); do
    for file in $(jq -r --arg t "$test" '.reuse[$t].files[]' "$plan"); do
      if ! curl -fsSL --retry 3 "$published/$file" -o "$media/$file"; then
        echo "::error::$file of $test is not on the published site; run the Site workflow by hand with a full render"
        exit 1
      fi
    done
  done
  jq --arg commit "${GITHUB_SHA:-local}" --argjson rendered "$rendered" \
    '{commit: $commit, generation: .generation, scenes: (.reuse + $rendered)}' \
    "$plan" > "$media/manifest.json"
  du -sh "$media"
  ;;
plans)
  folder=$1 plan=$2
  first=
  for each in "$folder"/*/plan.json; do
    if [ ! -f "$each" ]; then
      echo "::error::no render job left a plan in $folder"
      exit 1
    fi
    if [ -z "$first" ]; then
      first=$each
    elif ! cmp -s "$first" "$each"; then
      echo "::error::the render jobs made different plans (the published site changed under them); run the workflow again"
      exit 1
    fi
  done
  mkdir -p "$(dirname "$plan")"
  cp "$first" "$plan"
  ;;
*)
  echo "usage: docs-media.sh plan|render|keep|plans|gather ..." >&2
  exit 2
  ;;
esac
