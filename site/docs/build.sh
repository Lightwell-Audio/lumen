#!/usr/bin/env bash
# Builds the documentation into <out>/docs beside the landing page
# (docs/USER_DOCS.md §8). Needs cargo (unless DOCS_DATA_READY), zola 0.22.1 and npx (for Pagefind
# 1.6.0).
#   site/docs/build.sh [out-folder] [base-url]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$here/../.."
out="${1:-$here/public}"
base="${2:-https://lightwell-audio.github.io/lumen/docs}"
zola="${ZOLA:-zola}"

# Zola reads data only inside its folder: copy in what the app owns, and
# export the app's tables (commands, preferences, controls; §5.1).
cp "$repo/docs/design-system/lumen-tokens.json" "$here/data/tokens.json"
# DOCS_DATA_READY: the export was made already, by the job that had the app
# built (docs-render.yml), and is in place; this one has no Rust.
if [ -z "${DOCS_DATA_READY:-}" ]; then
  (cd "$repo" && DOCS_DATA=site/docs/data cargo test -q --locked -p lumen-app --test docs_data -- --ignored)
fi

rm -rf "$out/docs"
(cd "$here" && "$zola" build --base-url "$base" --output-dir "$out/docs")
npx -y pagefind@1.6.0 --site "$out/docs"
