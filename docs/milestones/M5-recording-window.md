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
