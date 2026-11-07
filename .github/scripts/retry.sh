#!/usr/bin/env bash
# Runs a setup command up to three times, 20 s apart (ADR-0122): for the
# steps that reach the network (package lists, toolchains), whose failures
# are the network's, not the code's. Tests are never run through it.
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
