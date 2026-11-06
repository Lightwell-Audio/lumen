#!/usr/bin/env bash
# Decides whether a failed run of CI or Docs failed only for reasons outside
# the code, and if so reruns its failed jobs, once (ADR-0122; rerun.yml).
#
#   rerun-infra.sh                 the run $RUN_ID of $GITHUB_REPOSITORY
#   rerun-infra.sh --classify F    prints `infrastructure` or `code` for the
#                                  jobs JSON in F (the API's jobs list), to
#                                  test the rule by hand
#
# A failed job is the infrastructure's when:
#   - it has no steps (it never got a runner), or
#   - it failed with no failed step (the runner was lost), or
#   - every step that failed is a setup step: getting the runner ready, the
#     checkout, a cache, an install.
# A job cancelled with steps run is a hang that reached the job's timeout,
# which is the code's. Any failed test, check or build is the code's.
set -euo pipefail

# The setup steps, by the names GitHub and ci.yml give them.
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
# One page holds every job of these workflows (CI has 15); per_page is the
# API's largest.
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
