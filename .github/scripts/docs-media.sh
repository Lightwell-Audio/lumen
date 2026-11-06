#!/usr/bin/env bash
# The docs' media (docs/USER_DOCS.md §13, the owner's choice A), for the
# Site workflow (pages.yml) and the pull-request check (docs.yml):
#
#   docs-media.sh plan <published> <plan.json> [full] [jobs]
#       Fetches <published>/manifest.json (the published media folder's
#       address) and writes which scenes to render and which to reuse
#       (plan_the_render in crates/lumen-app/tests/docs_coverage.rs).
#   docs-media.sh render <scenes-folder> <test>...
#       Renders each scene into <scenes-folder>/<test>/, with how long it
#       took in <scenes-folder>/<test>.seconds.
#   docs-media.sh gather <published> <plan.json> <scenes-folder> <media>
#       Copies the rendered scenes into <media>, fetches the other scenes'
#       files from the published site, and writes <media>/manifest.json.
#
# Needs cargo and jq; render also needs ffmpeg and a Vulkan device.
set -euo pipefail

command=${1:?usage: docs-media.sh plan|render|gather ...}
shift

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
  cargo test --locked -p lumen-app --test docs_media --no-run
  for test in "$@"; do
    started=$(date +%s)
    DOCS_MEDIA="$out/$test" cargo test --locked -p lumen-app --test docs_media \
      -- --ignored --exact --test-threads=1 --nocapture "$test"
    echo $(( $(date +%s) - started )) > "$out/$test.seconds"
  done
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
      --argjson seconds "$(cat "$scenes/$test.seconds")" \
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
    '{commit: $commit, scenes: (.reuse + $rendered)}' "$plan" > "$media/manifest.json"
  du -sh "$media"
  ;;
*)
  echo "usage: docs-media.sh plan|render|gather ..." >&2
  exit 2
  ;;
esac
