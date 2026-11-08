#!/usr/bin/env bash
# The docs' media (docs/USER_DOCS.md §13, the owner's choice A), for the
# Site workflow (pages.yml) and the pull-request check (docs.yml):
#
#   docs-media.sh plan <published> <plan.json> [full] [jobs]
#       Fetches <published>/manifest.json (the published media folder's
#       address) and writes which scenes to render and which to reuse
#       (plan_the_render in crates/lumen-app/tests/docs_coverage.rs).
#   docs-media.sh render <scenes-folder> <piece>...
#       Renders each piece into <scenes-folder>/<test>/, with how long it
#       took in <scenes-folder>/<test>.<theme>.seconds. A piece is a scene
#       (`<test>`: it runs once) or one theme of a scene (`<test>:<theme>`:
#       the themes are rendered apart, in jobs of their own, and their
#       folders together hold what both themes write).
#       With DOCS_SCENE_CACHE and DOCS_PLAN_JSON set, a scene already in the
#       cache for its fingerprint is copied from it and not rendered.
#   docs-media.sh keep <cache> <plan.json> <scenes-folder>
#       Fills <cache> with the scenes of the plan's render list, named by
#       fingerprint, from what the render jobs left in <scenes-folder>.
#   docs-media.sh plans <folder> <plan.json>
#       Checks that the plans the render jobs made (<folder>/*/plan.json)
#       are the same, and leaves that plan at <plan.json>.
#   docs-media.sh gather <published> <plan.json> <scenes-folder> <media>
#       Copies the rendered scenes into <media>, fetches the other scenes'
#       files from the published site, and writes <media>/manifest.json.
#
# Needs cargo and jq; render also needs ffmpeg and a Vulkan device.
set -euo pipefail

command=${1:?usage: docs-media.sh plan|render|keep|plans|gather ...}
shift

# How long a scene took to render: its pieces' seconds, added up
# (`<test>.seconds` of a scene taken from the cache, or one
# `<test>.<theme>.seconds` for each theme rendered).
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
  # The cache's generation: a full render starts a new one (the pictures
  # were drawn again from the app as it is), so scenes kept before it are
  # never taken for ones drawn after it. Carried over from the manifest;
  # "0" before any deploy has written one.
  generation=0
  if [ "$full" = true ]; then
    # Unique to this run, even for a second full render of the same commit
    # (a short hash, as the workflow keys on its first 12 characters).
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
  # A scene rendered before, for the same fingerprint, is taken from
  # DOCS_SCENE_CACHE (a folder the workflow restores) instead of
  # rendered again; DOCS_PLAN_JSON is the plan whose fingerprints name it.
  # DOCS_SCENE_CACHE_READ=0 (a full render) renders anyway. The cache is
  # filled by `keep`, once, from every job's scenes.
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
      # The whole scene, both themes: the other theme's piece, in this
      # job or another, finds it in the cache as well.
      rm -rf "$out/$test"
      cp -R "$entry" "$out/$test"
      cp "$entry.seconds" "$out/$test.seconds"
      continue
    fi
    if ! $built; then
      cargo test --locked -p lumen-app --test docs_media --no-run
      built=true
    fi
    # The workflow prunes the encoded clips only after a run that drew
    # frames, as a run that reused every scene marks none of them as used.
    if [ -n "${DOCS_RENDERED_MARK:-}" ]; then touch "$DOCS_RENDERED_MARK"; fi
    started=$(date +%s)
    # A piece writes into its scene's folder, beside the other piece's.
    if [ -n "$theme" ]; then export DOCS_THEME=$theme; else unset DOCS_THEME; fi
    DOCS_MEDIA="$out/$test" cargo test --locked -p lumen-app --test docs_media \
      -- --ignored --exact --test-threads=1 --nocapture "$test"
    unset DOCS_THEME
    echo $(( $(date +%s) - started )) > "$out/$test${theme:+.$theme}.seconds"
  done
  ;;
keep)
  # keep <cache> <plan.json> <scenes-folder>: the whole set at once, as the
  # jobs' scenes are only together once they are gathered, so a scene is
  # found whichever job drew it last time. Holds one set of clips, never
  # more than the plan's.
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
  # plans <folder> <plan.json>: every render job made the plan itself, from
  # the published manifest it fetched. They must be the same plan, or a
  # scene could be rendered twice, or by nobody (a deploy landed between
  # two jobs' fetches): then the run is to be started again. Leaves the
  # plan at <plan.json>.
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
