# Milestone 7b — Advanced Editing & Render: plan

**Status:** Approved to start by the owner, 2026-10-22. E0 (PR #51), E1
(PR #52) and E2 (PR #54, MT-EDIT-3 passed 2026-10-24) are merged. E3 is
planned (§6, "E3 implementation plan") and under way on
`m7b-s3-render-consolidate`; E4 to E6 have not started.
The behavior choices below remain the owner's accepted choices. This is the second half of the roadmap's
milestone 7, split off at the owner's decision of 2026-10-19 (S1 A). The
automation half is [M7](M7.md). M8 keeps its number.

**Why a separate milestone.** CLAUDE.md §9 lists "Automation and envelopes" and
"Editing power tools" as separate steps. They share almost nothing: M7
changes the engine, the model and the lane interface, and M7b changes clip
functions, the command table, the clip player, and adds an offline renderer.
The tool model and the clip commands (E0, E1, E2) need no automation, so they
may run alongside M7's A1 to A4 if the owner wants editing sooner. Only E3
onward uses M7: a render must include clip envelopes (A7) once they exist.

**How this plan was made.** The same three read-only explorations as M7
(engine, project model, app interface). Nothing was built or run; each slice
re-reads its own area before it starts.

**Goal (ROADMAP §7, CLAUDE.md §5 and §6.6).** The editing tools of a
professional workstation work on clips and time ranges: a tool model, split
(with the drag-to-cut gesture), join, crop, reverse, normalize, crossfades,
consolidate, bounce, and freeze.

**Exit criterion.** A user can split and rejoin clips and hear no change,
crossfade two clips at a join, consolidate and bounce a track with effects,
freeze and unfreeze a track, save and reopen, and hear the same result.

Requirements covered: CLAUDE.md §5 (tool model), §6.2 (clip properties,
automatic crossfades), §6.6 (editing actions), and the matching rows of §7.

## 1. What the code gives us

| Area | Fact (pointer) | Consequence |
|---|---|---|
| Existing clip edits | Pure functions in `lumen-project/src/edit/mod.rs`: `slice`, `clear`, `clear_except`, `overwrite`, `trim`, `slip`, `delete_time` (ripple), `insert_time`; copy and paste in `edit/clipboard.rs`; Document methods in `document/clips.rs`; duplicate in `app/editing.rs`. Notes-only (MIDI) versions of split, glue, nudge and reverse exist in `edit/notes.rs`. | Clip split is a thin use of `slice`; the rest are new pure functions plus commands. |
| Missing for clips | Split at the playhead, join/glue, crop to selection, consolidate, unloop into copies, reverse, normalize, nudge, polarity, select invert and its variants. | The editing list. |
| Overlap rule | Owner rule (M3 6, ADR-0032): a clip put down on others cuts away what it covers (`edit::overwrite`). Clips never overlap in the model. | A crossfade cannot be an overlap without a model change (§3, X1). |
| Edge fades | The engine declicks moves (5 ms) and gives every clip edge a 2 ms fade (`clips.rs`, `declick.rs`). | After a split of contiguous material, playback must still equal the unsplit clip (§3, E0 pass condition). |
| Fades | `Clip.fade_in/out: Fade{length, curve}` (`lumen-core/src/fade.rs`) edited by handles in `arrangement/handles.rs`. | A crossfade can build on the fade-in. |
| Audio clip data | `ClipContent::Audio{media, offset, gain_db}`, `Clip.loop_length`; no pan or polarity on a clip. Time ranges are `TimeRange{start, end, tracks}` (`arrangement/select.rs`). | Polarity is a clip flag the clip player multiplies by. |
| Media | `Media{id, path (relative, `/` separators), frames, channels, sample_rate}`; the save worker copies imported audio into the project and `Document.media_files` maps ids to paths. Unreferenced media can sit in `Project.media`; no purge exists. | Rendered files register like imports and Save copies them. |
| No audio writer | `lumen-media` decodes (symphonia) and resamples (rubato) only. A test WAV encoder exists in `lumen-media/tests/files.rs`; `lumen_project::write_atomic` gives atomic writes. | A hand-written WAV writer settles M7b; FLAC waits for M9. |
| Offline rendering | No production device-free renderer. The pattern exists: `Engine::process` driven directly (`lumen-engine/tests/mixer.rs` `Rig::run`), `Lanes::update` builds an engine from a document (`lumen-app/src/lanes.rs`), `app/audio.rs` assembles the set. | A worker can build and drive an engine without a device. |
| Plugin hosts offline | The host's process mode is real time (`instance/mod.rs`, `setup.rs`) and has no protocol field. `Engine::process` computes a wall-clock deadline (`HOST_SHARE` 3/4 of the period) and `Guard` turns a late block into silence. Each host is one process per plugin and cannot be shared with the live engine. | Plugin tracks in a render need an offline host mode and a deadline override. Unproven (§3, R2). |
| Tools and keys | No global tool. The piano roll has `note_editor::Tool{Pointer, Draw}`. Plain keys in use include B, L, Q, M, F, X, W, H, Z, 0. A, E, J, S, V are free. Ctrl+E (split notes) and Ctrl+J (glue notes) are note commands today, available only with notes picked. | The tool model is new state; widening the two chords is an availability change in `app/commands.rs`. |
| Gestures | `Gesture::{Move, Marquee, Range, Trim, Slip, Fade, Bend, Gain}` in `select.rs`, started in `start_gesture` (`app/editing.rs`). | Drag-to-cut is a new `Gesture`; a tool state is read there. |

## 2. Scope and shape

Seven slices (§5). Order: E0 and E1 first; E2 (crossfades) next; E3 to E5 are
the render chain and follow in order; E6 closes.

## 3. Decisions the evidence settles, and one design to choose

Each becomes an ADR when its slice starts. The owner may overrule any of them.

**R1. One render core.** Consolidate, bounce and render selection use one
device-free render driver in a worker thread (the pattern `Lanes::update` and
the mixer tests already use) and a hand-written WAV writer (16, 24 and 32-bit
float, no new dependency, no licence question). Output is written beside the
project's media and registered like an imported file, so Save copies it. FLAC
waits for M9's export, where its dependency is decided.

**R2. Plugin tracks in bounce and freeze are decided by a spike**, not by
opinion: render the test plugins offline through a separate host process per
plugin and compare against a real-time render. If hosts can render offline
with a repeatable result, they do. If not, plugin tracks bounce by capturing
the live engine (real time, with progress and cancel), and the plan records
that deviation from the quality bar (F1).

**E0 seamless joins.** Clips cut from one source that touch and are
contiguous in the source play as one clip: no edge fade, no dip. The engine
test is a null test of a split clip against the original.

**X1. Crossfades.** Automatic short crossfades on overlaps are an [M] item
(§6.2), so they are in scope. The model keeps clips from overlapping (the
overwrite rule), so a crossfade needs a design:

| Option | What it is | Cost |
|---|---|---|
| **A (recommended): a crossfade at a join.** The later clip's fade-in can be marked as crossing. While it plays, the clip before it keeps playing past its end, from its own source, fading out with the mirrored curve. It needs source material beyond the earlier clip's end; where there is none, the crossfade shortens or is skipped. No clips overlap in the model; the overwrite rule is untouched. A project setting turns on an automatic short crossfade (default 5 ms) at every join of clips that were not cut from contiguous material, and a handle at the join lengthens it | One flag on `Clip` (format v28, after E0 took v26 and E1's reverse and polarity flags v27), engine reads the earlier clip's tail, one handle in the arrangement |
| B | Allow a bounded overlap only inside a crossfade pair, stored as an overlap | Clips overlap in the model in one case; hit testing, selection and the overwrite rule need exceptions |
| C | Full overlap with the later clip on top and an automatic crossfade over the overlap | Reverses the owner's overwrite rule (M3) and touches every edit function |

**Decided 2026-10-19: A.** It keeps the rule the owner already chose and gives
the same sound. B and C stay on the specimen board as the options not taken.

## 4. Choices for the owner

| # | Choice | State |
|---|---|---|
| S1 | Scope | **Decided 2026-10-19: A.** This is M7b, split from M7 |
| X1 | Crossfade model | **Decided 2026-10-19: A.** A crossfade at a join, with a project setting for an automatic short one (default 5 ms) and a handle to lengthen it. The specimen of the handle and of the setting was accepted |
| K1 | The tool model and the split gesture | **Decided 2026-10-19.** Pointer, draw, erase, split, mute and zoom, picked from a switch in the corner above the track headers. **Keys follow the common workstation convention listed in CLAUDE.md §5, with no new single-letter tool keys**: B toggles Pointer and Draw, Esc returns to Pointer, Ctrl+E cuts, 0 mutes, Z and X zoom, Delete erases the selection. **Cut gestures**: press, aim and release with the Split tool (a click without moving cuts at once), and the edit cursor with Ctrl+E for the keyboard. The ghost-line variant was not taken. Every tool has a command and a palette entry |
| Y1 | The static clip pitch property | **Decided 2026-10-19: deferred.** CLAUDE.md §6.2 lists clip "pitch" as [M]. The owner deferred the audio pitch envelope because it needs a time-stretch and pitch engine; a static pitch shift needs the same engine (or a resampling change that alters the clip's length). The owner approved deferring it with the envelope, to be built with warp and time-stretch ([S]) |
| F1 | Real-time bounce for plugin tracks | **Conditional.** Asked only if the R2 spike fails: A real-time capture for plugin tracks with the deviation recorded, or B hold bounce and freeze for plugin tracks until the host gains an offline mode |

## 5. Slices

Branches follow `lumen-branching` (`m7b-s<n>-<topic>`), cut from `origin/main`.
Parts are commits. Each slice ends with a manual test and the gates in §6.
Sizes are rough (S/M/L).

| # | Slice | Delivers | Pass condition |
|---|---|---|---|
| E0 (M) | Tools and split | K1 as chosen: the global tool state and its switch, the keys above, split at playhead, edit cursor, grid and range edges (click-free), the press-aim-release cut gesture, Ctrl+E widened to clips, seamless joins | Split then join returns the original; playback across a split equals the unsplit clip (null test); the keymap has no conflict; every tool and command is in the palette |
| E1 (M) | Clip commands | Join (glue), crop to selection, unloop into copies, nudge by grid and finely, reverse (engine clip flag and reversed peaks), normalize (peak scan off the UI thread), polarity (clip flag), select invert, in track, to start, to end | Each is one undo step; reverse and normalize are golden renders; undo restores exactly |
| E2 (M) | Crossfades | X1 as chosen: format v28 (E0 took v26, E1 v27), the engine tail read, the handle, the project setting, equal-power default curve | A crossfade is click-free and null-tests against an equal-power reference; an edit that breaks the join clears or ignores it; golden render |
| E3 (L) | Render core and consolidate | R1: the worker render driver, WAV writer with atomic writes, registration as media, Ctrl+J widened to clips, consolidate for audio (gain, fades, envelopes, gaps as silence) and MIDI (merge and bake loops), originals kept in the media pool; the R2 spike moved to the start of E4 (owner, 2026-10-24) | Consolidating unprocessed audio is identical to the original (null test); render equals real-time playback in a golden render; progress and cancel |
| E4 (M) | Bounce | Bounce in place and render selection to a new track, with or without effects and tails | Bounce equals playback; a tail setting is honoured; the new track is one undo step |
| E5 (L) | Freeze family | Freeze, unfreeze and flatten, including hosted plugins by the R2 route | A frozen track runs no plugin processes; a plugin crash during freeze is a result, not an exit; F1 only if the spike failed |
| E6 (S) | Close M7b | Walkthrough (split, crossfade, consolidate, bounce, freeze, save, reopen), `lumen-review`, report, manual tests | The owner's MT-EDIT series passes; review findings fixed or tracked |

## 6. Validation and records

### E3 implementation plan (2026-10-24)

Branch: `m7b-s3-render-consolidate`, cut from `origin/main` after E2 merged
(PR #54). Owner choices (2026-10-24, all recommendations): **C1** Consolidate
bakes clip-level processing only (gain, polarity, reverse, fades, crossfades,
clip envelopes), not the track's fader, pan, sends or devices; those are
bounce's work (E4). **C2** the file is a 32-bit float WAV at the project's
sample rate, mono when every clip is mono and stereo otherwise, so
consolidating plain audio is bit-identical to the original. **C3** Ctrl+J on
audio clips that are contiguous in one source still joins them with no new
file (E1); on any other audio clips it consolidates, gaps as silence. There is
also a Consolidate command and palette entry. **C4** the R2 spike moves to the
start of E4 (consolidate never runs a plugin); the plan's pass condition for E3
no longer includes it. Assumptions (logged, not asked): a render works on a
snapshot of the clips and its result lands only if those clips are unchanged
when it finishes; otherwise it says "Changed while rendering" and the file
stays unused in the media pool.

Parts (commits on this branch):

1. **3a, WAV writer, no format change.** `lumen_media::wav`: 16, 24 and 32-bit
   float, mono or stereo (more is refused; consolidate and bounce write no more), written block by block so a long render
   never holds the whole file twice; written through `write_atomic`'s
   temp-and-rename pattern. Round trip against the decoder; header checks.
2. **3b, the render driver.** A device-free `lumen_engine` driver that plays a
   clip list through the clip player and a unity routing in blocks, with
   progress, cancel and an exact length. Golden test: render equals the
   real-time path block for block, at several block sizes.
3. **3c, consolidate for audio.** A pure planner in `lumen_project::edit`
   (which clips, the span, the channel count, the result clip with the first
   clip's name), a worker in the app that bakes via the driver and writes the
   WAV into the project's media folder, and `Document::consolidate_clips`
   registering the media like an import and replacing the sources in one undo
   step. Originals stay in `Project.media`.
4. **3d, consolidate for MIDI.** Unloop then merge (E1's pieces) as one undo
   step; no file.
5. **3e, interface and close.** Ctrl+J widened, the Consolidate command and
   clip-menu row, progress and cancel in the info line, E2E scenarios,
   MT-EDIT-4, PROGRESS, ADR, handoff.

### E2 implementation plan (2026-10-23)

Branch: `m7b-s2-join-crossfades`, cut from `origin/main` after E1 merged.
Implement the accepted X1 join design using existing fade handles, menu
controls, and the tokens in `docs/design-system/lumen-tokens.json` and
`theme.rs`; no new visual direction or assets.

1. Persist incoming crossfade requests and the automatic-crossfade project
   setting in format v28, with migration, frozen fixtures, and undoable
   Document methods. New projects enable automatic 5 ms joins; older projects
   retain their playback by migrating with the setting off.
2. Resolve touching audio joins and available source tails off the audio
   thread. Preserve seamless split joins. Render the outgoing continuation
   and incoming clip with equal-power gains; shorten or skip unavailable
   tails and ignore joins broken by edits.
3. Expose the setting and clip command through existing menus and the
   palette. Reuse the fade-in handle for length, with crossing curves and
   matching accessible feedback. One drag is one undo step.
4. Add reference-render, real-time safety, migration, undo, and user-driven
   E2E coverage. Inspect both themes at 100%, 150%, and 200%; run the slice
   gates and record MT-EDIT-3 and the handoff before reporting.

### E0 implementation plan (2026-10-22)

1. Add pure clip splitting over one or several cut positions, wrapped in a
   named Document method. Preserve source windows, hidden MIDI, envelopes,
   locked clips, and exact undo. A range cuts its two edges in one step.
2. Remove artificial edge dips only at contiguous audio seams, retaining
   the outside edge fades and the transport's locate and loop declicking.
   Compare rendered split and unsplit material sample for sample and exercise
   the allocation guard.
3. Add the approved six-tool switch above the track headers, palette entries,
   contextual B and Escape behavior, and the press-aim-release split gesture.
   Widen Ctrl+E to clips and ranges while retaining note-editor behavior.
4. Cover the workflows through the real app's accessible controls and keys;
   inspect both themes at 100%, 150% and 200%; run the slice gates and record
   the owner's MT-EDIT-1 steps. General join/glue remains E1; E0 verifies
   preserved source continuity and undo, with the join-command round trip
   verified when E1 supplies that command.

### E1 implementation plan (2026-10-23)

Owner choices (2026-10-23, all recommendations): **J1** Ctrl+J joins selected
clips, seamless only now (MIDI clips merge; audio clips merge only when
contiguous in the same source, which undoes a split exactly; any other audio
says "Use Consolidate", and E3 gives the same key that behaviour). **N1** Alt+Left/Right
nudges selected clips by a grid step, Alt+Shift+Left/Right by a 128th note;
the clip editor keeps Alt+arrows for notes (focus decides). **G1** Normalize sets the
clip gain so the loudest sample of the clip's source window reaches 0 dBFS;
non-destructive, per clip, scanned off the UI thread. **K2** Reverse,
polarity, crop, unloop and the select variants have commands, clip-menu rows
and palette entries but no default keys.

Parts (commits on this branch):

1. **1a, pure edits and commands, no format change.** `lumen_project::edit::
   clip_ops`: `join`, `crop` (to a time range, clips cut to it and the rest of
   each affected clip removed), `unloop` (a looping clip becomes real clips,
   one per repeat, content baked for MIDI, source windows for audio), `nudge`.
   `Document::{join_clips, crop_clips, unloop_clips, nudge_clips}`, one undo
   step each, refused with a reason on locked clips. Select invert, all in
   track, to start, to end as commands. Ctrl+J widens `GlueNotes` to clips as
   Ctrl+E did for split.
2. **1b, polarity and reverse, format v27.** `Clip.inverted` and
   `Clip.reversed` (written only when set; identity migration; frozen golden).
   A reversed clip's `offset` is a position in the reversed file, so every
   existing edit (trim, split, slip, loops) works unchanged; the toggle
   converts offset, `audio_phase_seconds` and `loop_phase` once, and a second
   toggle returns them. The engine clip carries `reversed` and reads frame
   `frames - 1 - (offset + inner)`; polarity is the sign of the engine gain.
   The waveform shader mirrors the pyramid lookup. Golden renders cover both.
3. **1c, normalize.** A peak scan of the clip's source window on a worker
   (cancelable, progress), then `Document::set_clip_gain` for all scanned
   clips in one step.
4. **1d, interface and close.** Clip menu rows, palette entries, E2E scenarios
   for each command (by names and keys), shots in both themes at 100, 150 and
   200%, MT-EDIT-2 in `docs/manual-tests/M7b.md`, PROGRESS, ADR, handoff.

Progress (2026-10-23): 1a to 1c are built and 1d is in the docs (ADR-0093, MT-EDIT-2). The plan changed in two ways: reverse uses reversed copies of files made on a worker (no engine change), and Join does not close audio gaps until E3 (the owner asked for it on 2026-10-23; KNOWN_ISSUES).

Known cost: at a device rate other than the file's, a reversed clip can sit
one frame off the forward clip (the file length and the offset are rounded
separately); recorded in KNOWN_ISSUES if it shows in the golden render.

Bounded implementation work is delegated under the repository's delegation
policy; the parent integrates and runs the final checks. E0 now uses format v26: null checks exposed the need to persist loop phase
and partial fade windows. E2 consequently uses v27 (now v28, see the E1 plan). See ADR-0092.

- Gates before each commit: `cargo fmt --all`, workspace clippy with
  `-D warnings`, `cargo test --workspace`. Before pushing: the macOS
  cross-clippy; the WSL mirror and the performance gate for engine and project
  changes (+2% fails). Real-time paths run under the allocation guard and
  RealtimeSanitizer in CI.
- End of every slice (ADR-0029): the E2E suite and the six smoke tests pass, and
  new E2E scenarios cover each delivered workflow. E2E waits for what the engine
  answers, never a fixed time.
- Format: each new persisted field bumps the version with a migration and a
  frozen golden.
- Specimens come first for visual and gesture choices (K1, X1), rendered for
  the owner to choose from.
- Manual tests (owner, Windows): MT-EDIT-1 to 6 for E0 to E6, written in
  `docs/manual-tests/M7b.md` when each slice lands.
- Records per slice: PROGRESS (§6.2, §6.6 and §7 rows), DECISIONS, ARCHITECTURE,
  KNOWN_ISSUES, the handoff in `CLAUDE.md`.
- Documentation follows the owner's naming rule.

## 7. Risks

1. **Offline plugin rendering is unproven** (R2). If it fails, bounce and
   freeze of plugin tracks deviate from "offline equals real time" and the
   owner decides (F1).
2. **Seamless joins.** The 2 ms edge fades are there to avoid clicks; skipping
   them for contiguous material must not reintroduce clicks at a locate or a
   loop. The null test and a locate test cover it.
3. **Crossfade tail needs material.** A clip trimmed to the end of its file has
   no tail to play; the crossfade shortens or is skipped, and the interface
   says so.
4. **Reverse changes the clip player** and the peak display; both are tested
   with golden renders.
5. **Static clip pitch** (Y1) is deferred with the owner's approval (2026-10-19)
   and is built with warp and time-stretch.
6. **Cannot be verified in the cloud container:** installed plugins in a render,
   and anything on macOS. Those are the owner's manual runs.

## 8. Alternatives rejected

- **A new encoder dependency now.** A hand-written WAV writer settles M7b; the
  FLAC decision belongs to M9 with the export design.
- **Bounce through the live engine for every track.** Takes the song's length
  and blocks the engine for other work; kept only as the plugin fallback (F1).
- **Consolidate by writing beside the source file.** Breaks relative paths and
  collect-and-save; rendered files live in the project's media.
