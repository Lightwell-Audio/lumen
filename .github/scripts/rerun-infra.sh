#!/usr/bin/env bash
set -euo pipefail

setup='^(Set up job|Initialize containers|Run actions/checkout|Run Swatinem/rust-cache|Run taiki-e/install-action|Install |Post |Complete job)'

classify() {
  jq -r --arg setup "$setup" '
    def failed: .conclusion == "failure" or .conclusion == "cancelled" or .conclusion == "timed_out";
    def infrastructure:
      (.steps // []) as $steps
      | ($steps | map(select(.conclusion == "failure"))) as $failed
      | ($steps | length) == 0
        or (.conclusion == "failure" and ($failed | length) == 0)
        or (($failed | length) > 0 and ($failed | all(.name | test($setup))));
    [.jobs[] | select(failed)] as $jobs
    | if ($jobs | length) > 0 and ($jobs | all(infrastructure))
      then "infrastructure" else "code" end
  ' "$1"
}

if [ "${1:-}" = --classify ]; then
  classify "${2:?usage: rerun-infra.sh --classify jobs.json}"
  exit 0
fi

: "${RUN_ID:?the run to look at}" "${GITHUB_REPOSITORY:?owner/name}"
jobs=$(mktemp)
gh api "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/attempts/1/jobs?per_page=100" > "$jobs"

jq -r '.jobs[] | select(.conclusion != "success" and .conclusion != "skipped")
  | "\(.name): \(.conclusion); failed steps: \([.steps[]? | select(.conclusion == "failure") | .name] | join(", "))"' "$jobs"

verdict=$(classify "$jobs")
echo "verdict: $verdict"
if [ "$verdict" = infrastructure ]; then
  echo "::notice title=Rerun::run $RUN_ID failed only in setup or for want of a runner; its failed jobs run again"
  gh run rerun "$RUN_ID" --failed --repo "$GITHUB_REPOSITORY"
else
  echo "run $RUN_ID failed in the code (a test, check or build): not rerun"
fi
