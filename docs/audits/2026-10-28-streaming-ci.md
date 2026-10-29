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

Before declaring this fixed, require three **consecutive** fully green CI
rounds for the final functional change, including the E2E suite and all
three streaming runner jobs in every round. Any failure breaks the streak.
Record the actual results here; failed-job-only reruns do not count.

## Results

### Subsequent prefill validation: incomplete

The functional tree `831d42709d14bde8ffc94350f85c90e7ff941162` received three
complete workflow launches, without failed-job reruns:

| Commit | Full CI | All platform soaks | Accepted outcome |
|---|---|---|---|
| `30a39f1d` | [40884477656](https://github.com/gummycookie/lumen/actions/runs/40884477656), 7/7 passed | [40884477611](https://github.com/gummycookie/lumen/actions/runs/40884477611), 3/3 passed | One historical green pair |
| `bea14169` | [40886141785](https://github.com/gummycookie/lumen/actions/runs/40886141785), macOS failed | [40886141482](https://github.com/gummycookie/lumen/actions/runs/40886141482), 3/3 passed | Failed round |
| `faa1f710` | [40888073864](https://github.com/gummycookie/lumen/actions/runs/40888073864), macOS failed | [40888073837](https://github.com/gummycookie/lumen/actions/runs/40888073837), macOS failed | Failed round |

The second CI run timed out after the existing 30 seconds waiting for the
failed-device remove/undo scenario's replacement host. The third CI run
passed all 752 active E2E scenarios, including that one, but failed the
offline instrument comparison: none of its five existing live runs kept
up. These logs do not establish a shared cause. The undo wait now reports
device state and host count on failure, with its timeout, polling cadence
and ready/settings assertions unchanged. No new retry was added.

The macOS soaks measured substantial wakeup delays despite little time in
rendering: the first successful soak spent 2.705 s rendering and 101.067 s
in sleep overshoot (maximum 72.152 ms); the second spent 5.527 s rendering
and 91.948 s in overshoot (maximum 244.172 ms). They recorded 23,218 and
15,630 missed callbacks respectively. The third failed with 44 late reads
and 22,479 missed callbacks. At the late event, the worker had consumed the
new cue but had loaded only streams 1–20 of the next required page when
the production hold deadline forced playback. A pending engine serial is
expected while that locate remains held. This is distinct from the earlier
50 ms readiness gap; the prefill barrier cannot override the existing
150 ms escape. That limit and all zero-late assertions remain unchanged.

### QoS revision: soaks passed, full CI failed

At `725679a1`, [soak 40901230905](https://github.com/gummycookie/lumen/actions/runs/40901230905)
passed on all three platforms, but [full CI 40901230795](https://github.com/gummycookie/lumen/actions/runs/40901230795)
failed Windows and macOS E2E targets. The consecutive green streak is zero.
Windows failed the input-chip menu assertion after unfolding; macOS failed
an instantaneous meter-peak assertion (the held peak matched the expected
tone). All other jobs passed. The owner clarified the acceptance criterion:
E2E and all soaks must pass three times **in a row** on CI.

The input-test helper returned when a row appeared in the accessibility
tree, while expansion could still clip the input chip or leave it under
the resize hit area. It now waits after one unfold click until the chip's
entire rectangle clears that area, then performs the original single input
click and exact menu/model/undo assertions. At full expansion the chip ends
at row top +80 pt and the resize band begins at +82 pt. This synchronizes
on the actual hit geometry, with no repeated click.

The meter test used wall-clock sleeps before reading current peak. The fake
device advances independently on supervisor polls; a UI frame can decay the
peak without a fresh callback. It now settles by observed audio frames,
then waits for a post-drain callback tap before drawing the final UI frame.
The callback measures the engine before writing the tap, so that observation
precedes taking fresh meter levels. The wait does not examine the peak and
all peak/RMS/hold/rest assertions remain unchanged.

Native macOS logs confirmed Default → UserInteractive for the soak render
thread and Default → UserInitiated for the disk worker, both with setter
return code zero. They **did not establish a wakeup improvement**: fixed
32-sleep overshoot totaled 250.885 ms before and 282.328 ms after the render
thread request. The timed soak reported 92.522 s cumulative sleep overshoot,
119.408 ms maximum, 16,005 missed callbacks, six production-deadline releases,
and zero late pages. Linux and Windows had zero late pages and zero missed
callbacks. These are measured results, not proof that QoS caused the older
failure or that real-time scheduling is guaranteed.

Further test-only diagnostics record actual injected-stall durations and
held-release readiness, required-page residency, worker phase and per-cue
planning/preparation/read counters. They print after the worker stops.
Readiness plans are computed before starting the worker, so diagnostics add
no warmup and do not replan or allocate before a deadline release. The
small residency scan's duration and any stall-buffer truncation are explicit.
These observations will distinguish slow reads, planning overhead, pool
retirement and an unfinished cue prefix without changing timing limits.

The synchronization/diagnostic revision passed 3,332 local workspace tests
across 108 targets, including all 752 active E2E scenarios, plus the isolated
walkthrough, doc tests, six smoke tests and 31 optimized disk regressions.
The instrumented 120 s soak passed 60 steps and 142,960 reads, with zero
late/failed reads, zero missed callback deadlines, longest hold 119.990 ms
and no forced releases. It rendered 45,000 callbacks (4.285 s total,
maximum 1.905 ms); sleep overshoot totaled 3.908 s (maximum 3.735 ms).
Linux and macOS cross-target clippy passed with warnings denied. Production
code is unchanged from the QoS revision; this further revision changes test
synchronization and diagnostics. Native consecutive validation is pending.

### macOS scheduling investigation and worker shutdown

The next functional revision requests documented, non-privileged pthread
QoS at thread entry: UserInitiated for the disk worker and UserInteractive
for the plugin-host audio worker and the soak's dedicated render thread.
The device-owned audio callback is unchanged. A shared helper in
`lumen-rt::thread_qos` logs the reported class before the request, setter
return code and class afterward. Windows retains MMCSS; other targets
leave their scheduler setup unchanged. This is a scheduling hypothesis,
not established attribution of the preceding failures. Native macOS logs
must confirm what classes were actually configured and retain wakeup,
render, missed-callback and late-read measurements. Fixed 32-sleep probes
before and after the soak thread's request run outside its 120 s timer;
they neither retry nor impose a favorable-timing threshold.

A separate deterministic worker defect was found during this review:
`Stop` received while draining queued scene/cue messages was ignored by
`take`, so shutdown could be lost while another sender remained alive.
Both receive paths now terminate on Stop. A regression queues Scene then
Stop before starting the worker and forbids any page read afterward.

These functional changes reset the required accepted-round count to zero.
Three fresh fully green CI and three-platform soak pairs are still required.

Local checks for this revision: macOS ARM64 cross-target clippy and Linux
workspace clippy passed with warnings denied. All 3,332 workspace tests
across 108 targets passed, including 752 active E2E scenarios; the isolated
plugin walkthrough, doc tests, six real-binary smoke tests, and 31 optimized
disk regressions passed. The isolated optimized 120 s soak passed all 60
steps with 142,960 reads, zero late/failed reads, longest hold 119.991 ms,
no forced deadline releases, and two missed callback deadlines. It rendered
44,998 callbacks in 4.346 s total (maximum 8.501 ms); sleep overshoot totaled
3.484 s (maximum 4.035 ms). These retain observed timing limitations.
The full pinned Ubuntu 26.04 performance gate also passed with its baseline
unchanged: streamed playback +0.07%, planning -3.58%, and every other
scenario within the existing +2% limit.


### Hosted investigation attempt at `10f5c4ec`

[Full CI 40875049859](https://github.com/gummycookie/lumen/actions/runs/40875049859)
passed all seven jobs, including tests on all three platforms, RTSan and
the unchanged performance gate. However,
[streaming soak 40875049767](https://github.com/gummycookie/lumen/actions/runs/40875049767)
failed on macOS ARM64. This attempt counts as **zero accepted green rounds**.

| Platform | Logical CPUs | Duration / steps | Late reads | Missed callbacks | Longest hold |
|---|---|---|---|---|---|
| Linux x86_64 | 4 | 120 s / 60 | 0 | 0 | 63.997 ms |
| Windows x86_64 | 4 | 120 s / 60 | 0 | 45 | 64.182 ms |
| macOS ARM64 | 3 | 120 s / 60 | 28 | 26,418 | 46.290 ms |

The macOS log exposes a startup-prefill gap.
These runners used Rust 1.99.0. Linux used image `ubuntu-24.04`
`20261025.353.1`; Windows used `windows-2025-vs2026`
`20261021.278.1`; macOS used `macos-26-arm64` `20261021.0445.8`,
newer than the original failing job's image.

The cue was sample 1,488,000.
The 50 ms residency check ended at 1,490,400, before page 364 starts at
1,490,944 (+61.33 ms). At the failure, playback had reached cue +24 ms;
its 40 ms lookahead already included page 364. The worker had read that
page only for streams 1–36; streams 37–64 were still missing. Publishing
adoption after merely protecting current pages allowed this early release.
The test's injected 40 ms stalls can exhaust that initial margin.

The correction makes adoption require the existing high-priority cue prefix
to be resident, limited to pool capacity. The planner returns that prefix
length without planning twice. This fills the existing 500 ms near horizon
before normal readiness can release a cold start; it does not enlarge the
pool, change the 50 ms check or raise the 150 ms hold cap. Cold starts may
wait longer inside that existing cap. A staged-read regression uses the
exact failing cue and proves that cached page 363 cannot release while
following pages are blocked. Removing only the prefill barrier makes the
regression fail at the intended assertion; restoring it passes.

The soak now uses `Held::due`, the same 150 ms release rule as production.
Its previous unbounded test hold could otherwise conceal a cold-start
problem after the new prefill requirement. Zero-late and zero-failed-read
assertions remain unchanged. It also records render elapsed time and sleep
overshoot separately, so missed callbacks are measured without assuming
they were caused by CPU load. The new code still requires three complete
green hosted rounds; the failed macOS attempt is not credited.

The prefill correction passed local validation: clippy, all 3,331 workspace
tests, the isolated plugin walkthrough, docs, six smoke tests and 30 focused
disk regressions. Its 120 s soak, now using the production hold rule, read
142,809 pages across all 60 steps with zero late reads, zero failed reads,
zero missed callbacks, a 119.997 ms longest hold and no forced deadline
release. All 45,000 callbacks rendered; total render elapsed time was 4.282 s,
with a 3.238 ms maximum, and total sleep overshoot was 3.983 s with a
2.402 ms maximum. The remeasured unchanged +2% performance gate passed:
`stream_plan_64` used 162,302,689 instructions (-3.58%); all other scenarios
also remained within the existing gate. Hosted verification remains pending.

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
