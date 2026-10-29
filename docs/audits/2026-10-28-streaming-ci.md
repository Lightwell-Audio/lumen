# Disk streaming CI investigation

Branch: `m9-s6-disk-streaming`, fixing the soak on PR #74. Scope: cache
priority under pressure, the cue handoff, and the environment of the
wall-clock streaming test. No change to audio buffering, the 96 MB pool,
the 50 ms readiness check or the 150 ms transport hold limit.

## Evidence and plan

[CI run 40802529823](https://github.com/gummycookie/lumen/actions/runs/40802529823)
passed its workspace tests but failed the isolated disk soak on two runners:

| Runner | Image | Build | Result |
|---|---|---|---|
| Linux x86_64 | Ubuntu 24.04.5, image 20261025.353.1 | Rust 1.99.0, debug | 37 late reads; bursts 49 and 69 ms after seeks |
| macOS ARM64 | macOS 26.6.2, image 20261013.0424.1 | Rust 1.99.0, debug | 2 late reads, 281 ms after a marker seek |
| Windows x86_64 | Hosted Windows runner | Rust 1.99.0, debug | Passed |

The soak has real 128-frame / 48 kHz deadlines (2.67 ms). It injects
50–250 microsecond page reads and a 40 ms stall every thousand reads. Hosted
runners provide neither an audio device nor real-time scheduling guarantees.
Debug loop overhead also consumes the reader's budget, so the timing test
must exercise the optimized application. Deterministic logic tests remain in
the debug workspace suite.

The investigation found production issues as well:

1. The worker marked **all** resident pages in an oversized plan wanted,
   even its low-priority tail. Those pages could prevent missing pages at a
   new cue from acquiring space. Restrict protection and loading to the
   ordered prefix that fits the pool. A direct worker regression must fail
   without this change, replace the old resident point pages, and confirm
   that subsequent planning does not reread the new prefix.
2. Loading each page unnecessarily scanned every pool slot multiple times
   for dead streams. Reclaim once per planning round and whenever free
   pages actually run out; preserve the grace-period rules.
3. Clearing a cue before the engine consumes the queued seek leaves a
   window in which the worker can plan from the old position and evict
   the target. Keep the cue until the engine acknowledges the exact
   queued command, with cancellation still immediate. Cover both states
   with direct, deterministic tests.

These defects are established by code and regression tests; the original
logs alone cannot apportion each late read between them and debug overhead.

## Validation contract

`streaming-soak.yml` runs independently of `ci.yml` on Windows, macOS and
Linux. It builds first, then runs the optimized 64-track soak for 120 s,
with the existing varying reads, disk stalls, loops, seeks and strict
zero-late / zero-unreadable assertions. It records compiler and runner
details. It can also be started manually. The workspace CI continues to
test the worker, transport and pool logic in debug builds.

Before declaring this fixed, require three fully green CI rounds for the
final functional change, including all three streaming runner jobs. Record
the actual results here; a failed round is not a successful validation.

## Results

- The oversized-plan regression fails without the capacity fix and passes
  with it. The original debug soak still fails in isolation in the Linux
  development environment; changing cache priority alone is insufficient.
- Formatting and workspace clippy with warnings denied passed. The focused
  command-serial transport test passed. The direct oversized-plan regression
  passed; it was also shown to fail without the priority-prefix fix.
- The 120 s optimized local soak **failed** while the workspace E2E suite
  was running: 384 late reads, 29,148 missed blocks, longest hold 9.80 s.
  This does not establish isolated optimized soak behavior. The changes
  are a work-in-progress, not a verified fix.
- The local workspace run failed four test targets: app lib (MIDI sender
  worst lateness 38.22 ms), app E2E (749 passed, three failed: effect
  reordering, automation clock alignment, held overdub seam), hosting
  (hung-host/reload cleanup) and plugin-host orphans (killed-parent cleanup).
  Compilation and the other tests overlapped during portions of that run.
  Keep the raw failures as evidence and rerun in the intended environment;
  do not assume they are all environmental.
- No changed-code hosted CI runs have been started. The required three
  fully green rounds are outstanding. The owner requested transfer to the
  Lumen Codex environment before further investigation here.
