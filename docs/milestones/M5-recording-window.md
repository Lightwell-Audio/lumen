# MIDI recording window correction

Owner request, 2026-10-14: keep recording MIDI continuously until Stop or
Record-off. Loop and enabled punch boundaries mask the clip; they do not
discard the performance outside that window.

Plan:

1. Keep the capture independent of the visible range, including notes that
   straddle a boundary and controllers before and after it.
2. Place the whole performance in the existing hidden MIDI clip content;
   apply the brackets only to the visible clip range and replacement area.
3. Remove automatic Record-off at punch-out. Preserve looping, count-in,
   overdub, note releases, preview, undo and save/load behavior.
4. Add pure and end-to-end regressions, run workspace formatting, clippy,
   tests and real-binary smoke checks, and record hardware verification steps.

No new widgets or project schema are planned. Physical controller timing
and subjective playback checks remain manual verification.

Implemented: continuous capture, hidden notes/events in both recording
modes, boundary-safe timing on first loop entry, full held-note tails when
Loop is turned off, current-pass held-note previews, and bounded recording
indicators. Replace suppression preserves the surrounding arrangement;
looping clips use at most three cached engine fragments instead of
expanding all their repeats. Existing same-pitch layering and seam onset
rules remain in effect. Captures wholly outside the active window use a
one-tick hidden-content anchor (ADR-0064).

Verification on Windows, 2026-10-14:

- `cargo test --workspace`: 1,472 passed, including 336 E2E scenarios,
  13 engine recording tests and seven engine allocation-guard tests.
- `cargo test -p lumen-app --test smoke -- --ignored`: six passed.
- Workspace clippy with `--all-targets -- -D warnings`, formatting and
  `git diff --check` pass.
- Regression coverage includes hidden pre/post-window notes and controls,
  both recording modes, trim recovery, save/load, one-step undo/redo,
  real loop wraps, Loop-off held tails, and bounded cache storage for a
  `1 << 52` tick clip with a one-tick loop period (at most three fragments).

Not run: physical MIDI/controller listening checks, Linux/WSL instruction
performance gate, macOS cross-target checks or a fresh visual scale/theme
sweep. No audio throughput or frame-time measurements are claimed. The
updated MT-MIDI-7 step 7 is the owner walkthrough. This correction does
not close M5 or begin another milestone.

Recording-start follow-up, 2026-10-14: reproduce the owner's first-note
finding at a nonzero play marker, separate Record-start flooring from the
visible mask, and verify early compensation, pre-punch preservation and
later passes. The new E2E failed before the fix with the held note at
local tick -33,869; it now begins exactly at tick zero. Three focused unit
regressions cover compensated notes/controllers, later passes and a silent
initial pass followed by delayed messages (including counter rollover).
The fix adds no schema or callback changes. MT-MIDI-7 step 17 is the owner
hardware recheck.

Follow-up verification: `cargo test --workspace` passes 1,476 tests,
including 337 E2E scenarios. Workspace clippy with warnings denied,
formatting and diff checks pass; the six real-binary smoke tests pass.
The hardware recheck remains pending.

Owner recheck, 2026-10-14: the owner reports that everything works after
`409212c` and authorizes committing and pushing the feature branch. This
confirms the reported recording-start regression is resolved; it does not
close the remaining M5 milestone checks.

Pre-push WSL performance check on `409212c`: all nine instruction-count
scenarios are within the +2% gate. Largest increases: transport +1.50%,
64-track synth +1.36%; project parse +0.34% and serialization +0.17%.
No baseline update was needed.

PR 16 review follow-up: the engine selects recording bounds from its own
pass counter and uses the same compensated, floored, loop-normalized
positions as capture, including already-held keys. Saved loop ends bound
placement before the first wrap when Loop is disabled. Format v16 marks
hidden captured notes as ineligible to continue a note across a seam;
ordinary notes and genuine split copies keep their carry behavior. Old
four- and five-element notes migrate with their existing behavior, and a
v16 golden fixture covers the new encoding. A legacy v15 capture's exact
seam provenance cannot be reconstructed (KNOWN_ISSUES).

Performance comparison on the managed Debian 13 environment, against
the PR's prior head `d1060d7`: all nine instruction-count scenarios stay
within +2%; largest increase +0.052% (serialization), parsing -0.113%.
The published baseline check reports parsing +3.61% here, versus +3.72%
at the prior head on this same host. This is the existing environment
baseline mismatch, not an increase caused by this correction. The shared
baseline is unchanged; no WSL or macOS check is claimed.

Final automated verification for this follow-up: workspace all-target
tests pass 1,493 tests, including 339 E2E scenarios and the engine's
allocation-guard coverage. Documentation tests, workspace clippy with
warnings denied, formatting, diff checks and all six real-binary smoke
tests pass. The smoke checks use an isolated virtual display, software
GPU and silent test device. Physical MIDI timing remains a manual check.
