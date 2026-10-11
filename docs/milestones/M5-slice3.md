# M5 slice 3 — MIDI clips and the piano roll

**Status (2026-10-11):** Built (ADR-0047); MT-MIDI-4 waits. The owner chose **Look A (studio
keys), Draw A (click adds, drag sets length), Live A (preview, then
commit on release)** from the live specimens (artifact "Lumen Piano Roll
Specimens"). Implementation by Sonnet agents at medium effort in
worktrees (the owner's request), merged and checked by the integrator.

## Scope and choices

The approved editor location remains the detail panel's Clip tab, with a
real secondary OS window available through Pop out (M5 C2). Both use one
document and editor state. Closing the window docks the editor again.

| Choice | Recommendation | Alternative |
|---|---|---|
| Visual treatment | **Studio keys:** familiar piano key shapes, subtle black-key lanes, track-coloured notes, explicit selection outline and handles on hover | **Pitch lanes:** a uniform labelled pitch rail, stronger octave divisions and a flatter note treatment |
| Drawing notes | **Click to add, drag to set length:** clicking adds one current-grid step; dragging sets the new note's end | Drag paints repeated notes on successive grid steps |
| Editing during playback | **Preview, then commit:** notes follow the pointer immediately and pitch changes can be auditioned; the playing clip changes once on release | Change the playing clip continuously during the drag; requires a more extensive incremental playback-update path |

The specimens compare the visual treatments. Their local interactions
are design previews, not an implemented Rust editor or audio test.

## Proposed defaults

- Double-clicking an empty MIDI lane creates a snapped one-bar clip and
  opens it. Create MIDI clip uses a selected time range when present,
  otherwise one bar at the edit position. Locked material stays intact.
- Double-clicking a MIDI clip opens it. Notes appear in arrangement clip
  bodies and retain the existing clip selection, move, trim, loop,
  split, duplicate, mute and colour behaviour.
- In the piano roll, clicking a note selects it without moving the
  transport. The ruler locates playback. Body drag moves, edge drag
  resizes, empty drag selects a marquee, Ctrl-drag copies, Shift adds to
  selection and Alt bypasses snapping. B toggles Draw when computer
  keyboard mode and text entry do not own the key.
- Grid settings stay shared with the arrangement. Horizontal zoom and
  scroll belong to the editor (amended 2026-10-11, owner decision, ADR-0048):
  the roll has its own view, fitted to a clip when it opens. Pitch scroll
  and row height are the editor's too. Position conversions account for
  the clip start, trim offset and loop source.
- Note operations include add, delete, move, resize, split, glue, mute,
  duplicate, nudge, transpose and legato. Glue combines selected touching
  or overlapping notes of the same pitch; legato extends each selected
  note toward the next onset of a different time, bounded by clip content.
  Existing overlaps are preserved. Group edits preserve relative offsets
  and clamp the whole gesture at time/pitch limits.
- Audition can be disabled. Audition releases on gesture end, cancellation,
  editor/clip change, focus loss and Panic, without releasing a held live
  or playback note of the same pitch. The source-ownership path must be
  proved before this is considered complete.
- Escape cancels a gesture. A completed gesture is one undo step. The
  document is not mutated each frame for a visual preview.
- Controller/velocity lanes remain slice 4 and quantize remains slice 5.
  Core legato ships here and will be reused by slice 5. Optional scale,
  fold, ghost-note and multi-clip editing features are not added here.

## Implementation sequence and agent boundaries

1. **3a — Model and editing contract.** A project/editor agent owns pure
   note transforms and named `Document` methods in new `document/notes.rs`
   and `document/midi_clips.rs` modules. Define selection remapping and
   gesture inputs first. The current four-element `Note` has no mute
   state: add explicit note mute in format v14, migrate v13, preserve old
   fixtures, add the new golden fixture, validate and round-trip. Index
   selection is scoped to an immutable gesture base; stale selections
   are cleared or remapped after undo and document changes.
2. **3b — GPU editor and arrangement previews.** A rendering agent owns
   the note layer, visible time/pitch culling, arrangement mini-notes and
   piano-roll benchmark support. Reuse instanced boxes with a distinct
   layer identity; pan/zoom updates transforms without rebuilding note
   data. Cache by content and selection generation. Preview a gesture as
   an overlay, not 100,000 model edits per frame.
3. **3c — Interaction and integration.** An app agent owns piano-roll
   input, the Clip tab, native pop-out, command registration, context
   menus and accessible controls. Extract detail-panel code from
   `lib.rs` instead of growing it. Use existing tokens, widgets, snapping
   semantics, focus handling and command dispatch.
4. **3d — Audition and playback.** Integrate preview note ownership,
   muted-note playback and committed edits. Investigate the recorded
   live/playback same-pitch release issue: preview must not amplify it.
   Keep queues bounded and every new audio path under the RT guard.
5. **3e — Verification and close.** The integrating agent reconciles
   shared files, reviews the changes, runs the gates, records measurements
   and adds MT-MIDI-4. Update the ADR, architecture module map, progress,
   known issues and handoff. Do not claim the slice complete on design
   approval or model-only delivery.

Agents may implement 3b and 3c in parallel once 3a's API contract is fixed.
One integrator owns overlapping module declarations, `lib.rs`, command
dispatch and final records. Implementation subagents are Sonnet at medium effort (owner, 2026-10-11).

## Verification and performance

- Pure editing tests cover exact undo/redo, overlap, source offsets and
  loop repeats, pitch/time boundaries, locked clips, mute migration and
  save/reopen. Controller events survive note-only edits unchanged.
- E2E drives clip creation and opening, every note edit and undo through
  names/keys/pointer gestures, context menus/palette, keyboard-only
  operation, focus routing, and audition release. Native pop-out needs a
  real-window smoke check as well as headless state tests.
- Listen through the fake audio device; pair every preview note-on with
  its own note-off, including same-pitch live/playback overlap. Exercise
  changes while playing and verify RT allocation safety.
- Add a 100,000-note editor stress scene alongside the existing 200-track,
  20,000-clip arrangement. Measure scroll, zoom, marquee and active drag
  at 1600 x 900 and 3440 x 1380. Report p50/p95/p99/worst, commit latency,
  playback translation cost and idle repaints. Target p99 <= 8.33 ms;
  report any miss honestly, even if the existing 16.67 ms gate passes.
- Inspect dark/light at 100%, 150% and 200%, including the 960 x 540 pt
  minimum and the popped-out editor. Use isolated test settings/devices.
- Run fmt, workspace clippy and tests, the full E2E suite and ignored
  real-binary smoke suite. Before pushing, run the WSL instruction-count
  gate for engine/project changes and platform checks for window code.
- Hardware timing, actual keyboard feel and multi-monitor/DPI behaviour
  remain explicit manual checks; mocked inputs cannot establish them.

No new performance results have been measured during this planning step.
