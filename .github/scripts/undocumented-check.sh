#!/usr/bin/env bash
set -euo pipefail

base="$1"
head="$2"

entries() { grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$1" || true; }
id_of() { sed -E 's/ @[^ ]*$//' <<<"$1"; }
tag_of() { if [[ "$1" == *" @"* ]]; then echo "${1##* @}"; fi; }

base_ids=""
base_untagged=""
while IFS= read -r line; do
  [ -z "$line" ] && continue
  id=$(id_of "$line")
  base_ids+="$id"$'\n'
  [ -z "$(tag_of "$line")" ] && base_untagged+="$id"$'\n'
done < <(entries "$base")

status=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  id=$(id_of "$line")
  tag=$(tag_of "$line")
  if ! grep -qxF -- "$id" <<<"$base_ids"; then
    if [ -z "$tag" ]; then
      echo "::error file=$head::$id is new in undocumented.txt with no tag. Give it a page, or defer it to its milestone's docs pass with: $id @m<milestone>"
      status=1
    fi
  elif [ -n "$tag" ] && grep -qxF -- "$id" <<<"$base_untagged"; then
    echo "::error file=$head::$id is part of the backlog and cannot be deferred with @$tag. Give it a page."
    status=1
  fi
done < <(entries "$head")
exit $status
