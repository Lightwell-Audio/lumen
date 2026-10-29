# Continue the disk streaming CI fix in the Lumen Codex environment

## Latest status: scheduling investigation, not fixed (2026-10-29)

Owner clarification: E2E and every platform soak must pass three times in a
row on CI. Any failure breaks the consecutive streak. Current streak: zero.
The QoS revision `725679a1` passed all soaks but failed Windows input-menu
and macOS instantaneous meter E2E assertions; all links are in the audit.
The next test revision synchronizes the input click with its fully exposed
hit area and the meter assertion with a freshly rendered callback, retaining
all exact assertions. Added worker/stall/held diagnostics will explain the
remaining forced releases. Native QoS setters succeeded but wakeup delays
did not measurably improve; do not claim the scheduling cause is proved.


The prefill correction produced one fully green CI/soak pair, followed by
two failed rounds. The audit links all six runs and describes the macOS
failures: replacement-host timeout, live instrument lateness, and 44 late
soak reads after production's hold deadline. The macOS soaks also recorded
large wakeup overshoot and missed callbacks even when disk reads passed.
A provisional revision adds observable macOS pthread QoS requests at disk,
plugin-host and dedicated soak render-thread entry; fixed before/after
sleep probes run outside the soak timer. This needs native evidence and
three new complete green pairs. It is not a proven root cause yet.
The worker also now obeys Stop when draining queued messages, covered by
a deterministic Scene-then-Stop regression. The failed-device undo wait
adds timeout diagnostics without changing its patience or assertions.
No retries, timing limits or zero-late assertions have been weakened.

## Earlier continuation status (2026-10-29)

The first hosted repair attempt at `10f5c4ec` passed all seven CI jobs and
the Windows/Linux streaming soaks, but failed the macOS ARM64 soak with 28
late reads. It is not a green round. The audit retains run links and the
exact missing-page trace. A further correction now requires the worker to
fill the existing cue-priority prefix before acknowledging preparation;
the old 50 ms check could pass before following pages were loaded. A
staged-read regression reproduces the exact cue boundary and fails with
the barrier removed. The correction passed 3,331 full workspace tests,
the isolated plugin walkthrough, docs, six smoke tests, 30 disk regressions
and the pinned-container performance gate with its baseline unchanged.
Its production-deadline 120 s soak passed all 60 steps with zero late reads,
zero missed callbacks, longest hold 119.997 ms and no forced deadline release.
Three fresh complete green hosted rounds are still required.
The soak also uses production's 150 ms release rule and records render
duration and sleep overshoot, rather than relying on an unbounded test hold.

The original checkpoint below is historical. The continuation uses an isolated
worktree on `fix/streaming-soak-ci`, based on `a8de11c6`, preserving the
environment's original checkout. The audit records the additional first-use
page ordering, cue/scene adoption handshake and loop-lookahead repairs.
All 29 focused disk regressions and the integrated optimized 120 s soak passed
(zero late reads and zero missed blocks). The full workspace passed 3,330
tests, including 752 E2E; the isolated plugin walkthrough, doc tests and six
real-binary smoke tests passed. Smoke required access to its isolated Xvfb
display; the sandboxed attempt failed before opening a window.

The existing performance baseline and +2% gate passed in the pinned Ubuntu
26.04 image. The new soak workflow's checkpoint YAML syntax error is fixed
and both changed workflows pass actionlint. Direct worker regressions now
use a complete logical round independent of OS preemption; the live worker
keeps its five-millisecond read budget. The final checks for that test seam
also passed: full workspace, isolated plugin walkthrough, docs, six smoke
tests, 29 focused disk regressions, and another isolated optimized 120 s
soak (zero late and missed, longest hold 63.977 ms).

Hosted validation is still outstanding: zero of three required complete green
rounds are credited here. No PR has been created and nothing has been merged.
See `docs/audits/2026-10-28-streaming-ci.md` for evidence and the eventual run
ledger. Do not claim completion until that ledger contains three full green
CI and platform-soak rounds for the final functional change.

## Owner request

Investigate and permanently fix the failing CI soak tests linked at
https://github.com/gummycookie/lumen/actions/runs/40802529823/job/123213731910
in `gummycookie/lumen`. Understand the actual runner environments, avoid
hacky workarounds, and run CI at least three times with every check green
before declaring it fixed. The owner permits an independent soak pipeline.
The owner then requested moving the ongoing work to Codex in the **lumen**
environment. Continue the original task; it is not finished.

## Starting point

- Original open PR: #74, `m9-s6-disk-streaming`.
- Base for this checkpoint: `cc4266a6e0128b731c9e2b2ffbd84ad0a2ce4b98`.
- Checkpoint branch: `fix/streaming-soak-ci`, dependent on the unmerged PR.
- Do not merge or create a new PR unless the owner requests it. Follow the
  repository's AGENTS.md and applicable architecture/documentation skills.
- No changed-code hosted CI runs have been started. Zero of the required
  three green validation rounds have been achieved.

## Investigation and provisional changes

Read `docs/audits/2026-10-28-streaming-ci.md` for the failure evidence,
runner versions, proposed fixes and the validation contract.

1. `disk.rs`: a plan can exceed the 3,072-page pool. Marking every
   resident page in its low-priority tail wanted can prevent a missing
   high-priority cue from acquiring space. Restrict wanted/loading pages
   to the ordered prefix that fits. `Worker::new` supports a direct
   regression, with no thread timing assumptions.
2. `PoolWriter::load` and the worker loop avoid full pool reclaim scans
   when safe free pages already exist. Explicit reclaim/grace-period
   semantics are unchanged.
3. The transport used to clear the disk cue before the queued seek was
   consumed. Commands now carry successful-send serials internally;
   `EngineHandle::sent_command()` and the seqlock-published
   `TransportSnapshot.command_serial` provide an exact acknowledgement.
   `Cuer::follow_after(serial)` keeps cue priority until that command is
   applied. Cue cancellation and replacement clear pending follow state.
   Transport start, held release, stopped locate and the soak are wired.
4. New `.github/workflows/streaming-soak.yml`: independent Windows,
   macOS ARM64 and Linux jobs, build first, optimized 120 s soak, strict
   existing zero-late/zero-failed assertions. The debug workspace still
   skips the timing soak and covers deterministic logic tests. No retry
   loops or relaxed assertions were added.

An independent read-only review found no actionable defects in these
changes, but that is not verification. The fixes do not yet satisfy the
owner's acceptance criteria.

## Checks actually run

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Engine pool tests: 11 passed before the command-serial changes.
- Focused new command-serial integration test: passed.
- Oversized-plan direct regression: failed with the capacity fix removed,
  passed with it; later extended to cover acknowledgement and cancellation.
- Debug soak alone after the capacity/reclaim changes: failed, 25 late.
- Final optimized 120 s soak while the workspace E2E suite was running:
  **failed**, 384 late, 29,148 missed blocks, longest hold 9.80 s.
  No isolated final optimized run was completed.
- Workspace all-targets run (skipping `walkthrough_plugins::` and
  `disk::soak::`): **failed**, four targets. App lib had one MIDI
  sender deadline failure (worst lateness 38.22 ms). App E2E: 749 passed,
  three failed (`hosting::reordering_effects_while_playing_keeps_every_host_and_the_sound_continuous`,
  `lane_record::the_playhead_that_stamps_the_points_follows_the_clock_to_within_a_frame`,
  `overdub_seams::a_note_held_where_overdub_begins_sustains_across_the_new_clip`).
  Hosting: hung-host and reload cleanup failed. Plugin-host orphans:
  killed-parent cleanup failed. Compiler/test workloads overlapped; do not
  assume these are all infrastructure failures without investigating.
- Smoke, final isolated plugin walkthrough, RTSan, performance gate and
  changed-code hosted CI: not yet run. No hardware claims.

The previous scratch host's Rust and Linux development packages had to
be installed. Two initial release compilation attempts left zero-length
object files in dependencies; a subsequent build completed. Its CLI Git
push had no GitHub credentials, so the connected GitHub tools were used
for the checkpoint. Do not transfer credentials or depend on that host.

## Continue here

1. Read and review this checkpoint rather than assuming its proposed fixes
   suffice. Run the direct regressions and an **isolated**, fully built
   optimized soak in the Lumen environment. Add diagnostics for any late
   page: target/source frame, cache residency, cue/ack, worker round and
   elapsed read time. Explain failures rather than discarding them.
2. Investigate remaining failures. In particular, the planner's PAGE-sized
   slices can enqueue a later page for one clip before a current page for
   another when source offsets are unaligned. This observation has not
   been fixed or established as the original soak's cause.
3. Make the intended CI runs against the final functional code. Both the
   full CI and every platform soak job must be green in three validation
   rounds. Preserve run links, job results and runner environment details.
   Do not count retrying only failed jobs as three clean full rounds.
4. Update the report truthfully, keep hardware limitations clear, and
   return the result to the owner without merging.
