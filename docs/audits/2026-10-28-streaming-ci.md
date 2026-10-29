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

### Continuation in the Lumen cloud environment

The checkpoint at `a8de11c6ac29107d414cf501e4d7ad388da44e03` was fetched
into an isolated worktree, preserving the existing checkout. Its unchanged
optimized 120 s soak passed in isolation: 60 steps, 142,735 pages read,
23.998 ms longest hold, zero late reads, zero missed blocks. The local
environment uses Rust 1.99.0 on Linux x86_64, with a two-CPU quota and
8 GiB memory limit. This result establishes isolated checkpoint behavior;
it does not explain away the earlier failures under overlapping workloads.

Review identified additional correctness gaps:

- A PAGE-sized timeline slice could list a future source page of one clip
  before another clip's current page. With a capacity-limited prefix, this
  can exclude audio needed immediately. Pages are now ordered by their
  first timeline use within each priority span, with deterministic tie
  order. Repeated sources retain their phase and wrap behavior.
- Checking residency immediately after sending an asynchronous cue could
  race a worker round still using the previous plan. The worker now
  acknowledges adoption of a planning generation after marking that plan's
  resident prefix wanted; both cues and scene changes advance this generation.
  Transport readiness requires this acknowledgement, including when a scene
  changes while a start is held.
  The separate engine-command acknowledgement still controls cue release.
- Readiness and cue planning must follow playback through a loop boundary,
  including playback that begins before the loop start. The clip player's
  page lookahead must continue from the loop start after reaching its end;
  it must not request post-loop frames that will never be played.

Deterministic regressions cover these cases, including cue replacement and
cancellation. Timing-soak failures additionally report the observed transport,
worker round and cue state, latest read duration, and missing source pages.
These diagnostic observations are snapshots from separate threads and do not
claim an atomic history of the failure.

The original Windows job used Windows Server 2025 (10.0.26100), image
`windows-2025-vs2026`, version `20261021.278.1`; its 16 s debug soak had
zero late reads and zero missed blocks. The original macOS image was
`macos-26-arm64`, version `20261013.0424.1`. Raw original job logs and local
verification logs are retained in the worktree's `work/` directory.

The pinned Ubuntu 26.04 performance gate passed with the existing baseline
and +2% threshold unchanged. `stream_plan_64` measured 162,302,574
instructions against 168,325,510 (-3.58%); `engine_streamed_64` changed
+0.07%, and every other scenario stayed within the gate. This measurement
includes the wrapped-head lookahead correction.

The first focused disk run passed 22 tests and exposed an incorrect new
fixture: its one-frame cue window did not exercise the intended loop wrap,
while its stopped playhead legitimately requested a post-loop page. The
fixture now isolates a cue window spanning the boundary from an unrelated
stopped playhead. All 23 disk tests and the initial three cue adoption tests
then passed. Scene adoption and wrapped-head regressions were added after
that run; final integrated tests remain pending.

Final local validation also passed after making direct worker regressions
independent of scheduler preemption: clippy, 3,330 full workspace tests,
the isolated plugin walkthrough, doc tests, six real-binary smoke tests,
29 focused disk regressions and an isolated optimized 120 s soak. That soak
read 142,960 pages across 60 steps, with longest hold 63.977 ms, zero late
reads, zero failed reads and zero missed blocks. The live worker's five-ms
budget and all timing assertions remain unchanged.

Hosted validation is pending. No green hosted rounds are credited by local
results.

The checkpoint also had a workflow-validation failure before any runner
started: [run 40809053091](https://github.com/gummycookie/lumen/actions/runs/40809053091)
reports invalid YAML at line 49 of `streaming-soak.yml`, with no jobs.
The plain `run:` values containing `disk::soak::` ended a YAML token with
a colon followed by a space. Both commands now use block strings. All
repository workflow YAML files parse successfully. This failed validation
is not a hosted soak execution or a green round.

The final integrated local optimized soak passed for 120 s: 60 steps,
142,960 page reads, longest hold 63.989 ms, zero late reads, zero failed
reads and zero missed blocks. All 29 final focused disk tests passed.

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
