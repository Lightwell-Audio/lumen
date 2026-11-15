#!/usr/bin/env bash
set -uo pipefail

status=0
for attempt in 1 2 3; do
  "$@" && exit 0
  status=$?
  if [ "$attempt" -lt 3 ]; then
    echo "::warning title=Retried a setup step::$* failed (exit $status) on attempt $attempt of 3"
    sleep 20
  fi
done
exit "$status"
