#!/usr/bin/env bash
# Builds the documentation into <out>/docs beside the landing page
# (docs/USER_DOCS.md §8). Needs cargo, zola 0.22.1 and npx (for Pagefind
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
(cd "$repo" && DOCS_DATA=site/docs/data cargo test -q -p lumen-app --test docs_data -- --ignored)

rm -rf "$out/docs"
(cd "$here" && "$zola" build --base-url "$base" --output-dir "$out/docs")
npx -y pagefind@1.6.0 --site "$out/docs"
