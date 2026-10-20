# Lumen — Progress

## Status

**M6 slice 7 (compatibility and closeout): the review is done; all its findings (High, Medium M1 to M14, Low L1 to L6) are fixed, 2026-10-19** on `m6-s7-compatibility-closeout` (`docs/audits/2026-10-18-m6.md`, Status table). Built in slice 7: the probe (`lumen-host-probe`), the fault soak (`lumen-hosting/tests/soak.rs`), the callback benchmark (`lumen --bench-callback`) and the combined walkthrough (E2E `walkthrough_plugins`). Not done: the owner's MT-VST-7 runs on real plugins, the M6 report. The earlier summaries follow.

**M6 slice 5 (presets and resilient state) is built, 2026-10-17** on
`m6-s5-presets-state` (cut from `main` after PR #33; ADR-0077; plan, owner
choices R1 to R5 and progress in `docs/milestones/M6.md`). *Shipped:* host
program lists, the edit controller's own state and bounded `.vstpreset` files
(protocol version 6); program change for plugin instruments; loading a program
or a preset file as one undo step that undo gives back exactly
(`Document::load_device_state`); background checkpoints about every 30 s with an
immutable last good state, failures counted per plugin and never restarted by
themselves, Reload from the last good state and Reload without sound; safe mode
(`--safe-mode`, Shift at start on Windows, an offer after a start that did not
finish; no plugin scanned, loaded or hosted, cards keep their state, load one at
a time, leave to load the rest); the card's preset field with arrows, a
searchable menu of factory programs and preset files, Save, Save as… and Show
presets folder, a dot for a changed sound; the failed card saying where Reload
starts from, and "Keeps stopping" after three failures. *Verified (Windows, test
plugin and real host):* workspace clippy, workspace tests, E2E for every part,
mutation checks on the program-change flag, reload from the last good state and
hosts starting in safe mode, screenshots in both themes at 100, 150 and 200%.
*Not run:* any real plugin (MT-VST-5 is the owner's), smoke tests, the WSL
gates, macOS and Linux runs. Format unchanged (v19). Known limits are in
KNOWN_ISSUES (the sound's name is kept for the session; choosing the program a
plugin already has does nothing).

**M6 slice 3 (chain editing) is built, 2026-10-17** on `m6-s3-chain-editing`
(cut from `m6-s2-chain-states`, which is not merged; ADR-0073; plan and
progress in `docs/milestones/M6.md`). *Shipped:* format v18 (the view keeps
which cards are folded); `Document::{move_device, copy_device, paste_device}`
(one undo step each, a reason for every refusal); drag to reorder or onto
another track's header (Alt copies), Alt+Left and Alt+Right, Left and Right,
Ctrl+C, Ctrl+V and Ctrl+D on cards, six new device commands, the card's menu,
folding to a strip; the master's chain; the plugin's own picture on the card.
*Moved out, with the owner's choice:* the preset menu (slice 5), the editor
button and the parameter strip (slice 4). *Verified (Windows, test plugin and
real host):* workspace clippy, workspace tests, E2E for moving, copying and
reordering under playback, drops on headers and the master, folding and its
saving, pictures; mutation checks on the move gap, Alt copying, the strip's
width and an instrument's track. *Found, not fixed:* a plugin instrument that
finishes loading while a note is held does not sound it until the next note
(an engine change, proposed). *Not run:* the WSL performance gate and Linux and
macOS runs (the branch is not pushed), any real plugin, MT-VST-3 (the owner's).
*Still open from slice 2:* the ADR-0069 measurements and the real-plugin reload
result.

**M6 slice 2 (the first complete device workflow) is built, report 2026-10-17**
on `m6-s2-device-chain` (merged, PR #26) and `m6-s2-chain-states` (ADR-0070 to
ADR-0072; plan and progress in `docs/milestones/M6.md`). *Shipped:* format v17;
`Document::{add_device, remove_device, set_device_bypass}` (one undo step each);
the engine's host path for hosted instruments and effects (fake hosts); the
hosting manager (`lumen-hosting`, one `lumen-plugin-host` process per device,
loaded in the background); the Devices tab (cards in four faces, browser
double-click and drag, `devices.add`, `devices.remove`, `devices.bypass`);
failed and missing cards with Reload and Rescan; 14 device E2E scenarios plus
hosting scenarios, all against the real host and the test plugin. *Verified:*
workspace clippy, the macOS cross-clippy, 374 E2E and 6 smoke on Windows; the
WSL performance gate and RealtimeSanitizer at 2b and 2c. *Owner:* the MT-VST-2
walkthrough passed ("everything is good"), reported without step detail or
numbers. *Deferred:* the ADR-0069 measurements (the whole callback with hosts at
64 and 128 frames; recording with monitoring through an effect, with loopback
latency) were **not taken**, so **the callback's bounded wait stays on trial**
and the pipelined path stays the fallback; the sample-rate-change reaction with
plugins loaded is untested in the app; real plugins RP-1 to RP-4 failed in
earlier runs and their reload result is unreported. *Not run:* macOS, any run
of the new scenarios on Linux, the WSL performance gate after 2e (tests only).
*Risks:* the trial's outcome decides the latency of monitoring through
effects; real-plugin compatibility is unknown beyond one passing editor spike.
Slice 3 (chain editing) may start; the measurements are due before the M6
report and before ADR-0069 leaves "on trial".

**M6 slice 1 (plugin discovery) built 2026-10-16** on `m6-s1-plugin-scan`
(ADR-0068; plan in `docs/milestones/M6.md`). Startup scan in the background, a
child process per module with a deadline, a cache keyed by file stamp, a
blocked list with reasons that retries only when a file changes or on Retry,
and the browser's Devices tab in the owner's layout B with a type filter.
Verified on Linux with the test plugin; **nothing has run on Windows or
macOS** (MT-VST-1 ran on Windows with real plugins, owner-reported pass, no measurements). Slice 0's
architecture gate (the audio-path ADR) is ADR-0069 (accepted 2026-10-16, the callback's wait on trial); it does not block this
slice.
**M6 slice 0 activity (2026-10-15):** The corrected Windows MT-VST-0
full-grid rerun had no MMCSS warning. Event and event/spin had zero misses
across the 48 kHz grid; hybrid modes and spin missed in several cases. The two
heap operations the crash runs reported in Lumen's loop are fixed (the loop
no longer words a crash in a `String`), and the 64-host budget is measured
(run 3: event holds the half-period budget up to 100 µs of plugin work per
track and at p99 up to 200 µs). The audio-path ADR is still unwritten. See
[the M6 test record](manual-tests/M6.md) and [the M6 plan](milestones/M6.md).
M5 closeout remains pending.
**Documentation policy, 2026-10-14:** third-party product comparisons and
named plugin examples are replaced with direct behavior descriptions and
anonymous test labels. Technical identifiers and required legal attribution
are retained. The rule is in the repository instructions and
`.agents/skills/lumen-documentation/SKILL.md`; this editorial pass changes
no implementation or feature status.

**Current milestone:** 5 - MIDI and the piano roll (plan: [milestones/M5.md](milestones/M5.md)), approved 2026-10-10. **Slices 0–7 are built**: MIDI devices and format v11; MIDI playback and instruments (ADR-0045); live MIDI (ADR-0046); clips and piano roll (ADR-0047); velocity and controller lanes (ADR-0050); quantize and tools (ADR-0054); MIDI recording (ADR-0055); MIDI files and clock out (ADR-0062). MIDI learn moved to M7 by the owner on 2026-10-13. Owner manual runs on 2026-10-11: MT-MIDI-1 passed; MT-MIDI-2 and MT-MIDI-3 passed functionally, with remaining issues in `docs/KNOWN_ISSUES.md`. MT-MIDI-4, 5 and 6 passed through 2026-10-12. MT-MIDI-7 and MT-MIDI-8 wait for the owner. **Slice 8 is underway:** its combined record, quantize, save/reopen and playback E2E scenario is added, and the full repository review is done with its fixes (`docs/audits/2026-10-13-m5.md`: two High, an audio-thread free and a UI-thread disk check, both fixed; module splits, the synth under the allocation guard, bounds on positions). MT-MIDI-7 to 9, the WSL gates and the milestone report remain.
**2026-10-14 playback fix:** imported notes (and controllers) that round
to a block's first sample are now scheduled reliably, including Play at
their onset. The owner's ocarina file exposed a two-tick search margin
that was smaller than sample-rounding error (ADR-0062 follow-up). Synthetic
source-timing E2E and boundary regressions added; MT-MIDI-8 run 2 awaits
the owner's source-file recheck.

**2026-10-14 PR 14 follow-up:** the native clip editor consumes computer
keyboard MIDI input before editing shortcuts, matching the main window
(ADR-0047 follow-up). Separate-viewport regression added; OS-focus and
sound recheck in MT-MIDI-9 step 5 is pending. Duplicate MIDI port identity
remains logged in KNOWN_ISSUES, as the owner requested.

**Before it:** 4.5 — Polish pass, a mid milestone before M5
(plan: [milestones/M4.5.md](milestones/M4.5.md)), approved 2026-10-10
from the interface audit's mock-ups. **All six slices built**; every audit
finding is built. MT-POL-5 and MT-POL-6 wait for the owner. The report is
below. **Proposed next:** M5, MIDI and the piano roll
([milestones/M5.md](milestones/M5.md)), for the owner's approval.
**Before it:** 4 — Mixer, Routing & Groups (plan:
[milestones/M4.md](milestones/M4.md)). **Complete 2026-10-10**: all eight
slices built, and every manual test passed (MT-APP-17, MT-MIX-1 to
MT-MIX-8, including the Focusrite's dropout count). The end-of-milestone
review (`docs/audits/2026-10-10-m4.md`) found no High issues. Four Medium
and eight Low findings are fixed (the audit's Status table), merged into
`main` 2026-10-10 (PR #3). The report is below.
**Previous:** M3 complete and approved 2026-10-09; M2 complete and approved 2026-10-07 (report below); M1 complete 2026-10-06 (report below; [milestones/M1.md](milestones/M1.md))

Statuses: 🔲 Not started · 🟡 In progress · ✅ Complete · ⚠️ Partial / known limitation · ⏸️ Deferred with approval · ❌ Blocked

## Milestones

| # | Milestone | Status | Notes |
|---|---|---|---|
| 1 | Foundations & Architecture | ✅ Complete | Plan and risk register in `docs/milestones/M1.md` |
| 2 | Audio Engine, Devices & Transport | ✅ Complete, approved 2026-10-07 | Plan: `docs/milestones/M2.md` |
| 3 | Arrangement & Project System | ✅ Complete, approved 2026-10-09 | Plan: `docs/milestones/M3.md` |
| 4 | Mixer, Routing & Groups | ✅ Complete 2026-10-10 (review fixes merged, PR #3) | Plan: `docs/milestones/M4.md` |
| 4.5 | Polish pass (mid milestone: the interface audit's fixes) | 🟡 Built 2026-10-10; MT-POL-5 and 6 wait | Plan: `docs/milestones/M4.5.md`; audit: `docs/audits/2026-10-10-design.md` |
| 5 | MIDI & Piano Roll | 🟡 Approved 2026-10-10; slices 0 to 7 built, slice 8 underway (review done); MT-MIDI-7, MT-MIDI-8 and MT-MIDI-9 wait | Plan: `docs/milestones/M5.md`; report written 2026-10-19 (below), awaiting the owner's approval |
| 6 | VST3 Hosting & Device System | 🟡 Started 2026-10-15 at the owner's instruction; slice 0 complete 2026-10-16 (report below; ADR-0069 accepted, the callback's wait on trial) and slice 1 built (MT-VST-1 passed on Windows, owner-reported); slice 2 built and its walkthrough passed (owner-reported; ADR-0069 measurements outstanding); slices 3 to 6 built; slice 7 (compatibility and closeout) under way: the probe, the fault soak, the callback benchmark and the combined walkthrough are built (branch `m6-s7-compatibility-closeout`), the review is done and its seven High findings fixed (`docs/audits/2026-10-18-m6.md`); the owner's MT-VST-7 runs, the Medium and Low findings and the M6 report remain | Plan: [milestones/M6.md](milestones/M6.md); M5 closeout (MT-MIDI-7 to 9, the report) remains pending and is not marked passed |
| 7 | Automation & Envelopes | 🟡 Under way (the owner told A0 to start, 2026-10-19). **A4a built 2026-10-20** (ADR-0086, branch `m7-sa4a-follow-override`, cut from `main` after A3 merged; no format, engine or host change): a control whose lane plays shows the lane's value at the playhead, stopped or playing (headers, mixer strip, send knobs, M, the plugin card's strip and the All parameters window); moving a control takes its lane over for the session (`Overrides` in the `Document`, outside the history, not saved), a lane taken over plays nothing (the routing, the plugin-lane bake and the hosting bridge all read one rule) and the control's own value is heard; the owner's look A (amber ring, amber A button, amber chip with a Re-enable button, dashed curve and a dotted line at the control's value) and Re-enable by the button or by two commands; the A3 knob and plugin-window put-back is gone; tests: 8 units, routing, MIDI-list and plugin-edit tests, 7 E2E scenarios that read the fader and listen (`tests/e2e/lane_override.rs`, `plugin_lanes.rs`; mutation checked), a shot (`automation_override`); Windows gates: fmt, workspace clippy, the workspace tests (all pass but one demo-file check, which fails only on an unrelated uncommitted local edit and passes against the committed file), 6 smoke, macOS cross-clippy; not run: the WSL gate and performance gate (no engine or project change), RealtimeSanitizer, macOS runs, any real plugin; **MT-AUTO-5 waits for the owner**. **A3 built 2026-10-20** (ADR-0084, branch `m7-sa3-device-targets`, cut from `main` after A2 merged; no format change, no host protocol change): a lane drives a parameter of a hosted plugin effect or instrument on its exact sample (a flat `ParamBlock` rides with each block through `Hosted::submit`, `Link` and the shared region; the mixer and the MIDI player fill it from baked lanes), the control channel no longer sends a lane-driven parameter's resting value, and the lane chip's menu opens a chooser of the plugin's parameters (a pill per plugin, a search, group chips, only the rows in view; two thousand parameters filter at once); tests: units for the block and the generator (steps on their sample, curves within 1e-3, block-size invariance, cut lanes), engine tests at five block sizes for an effect and an instrument and under the allocation guard (mutation checked), real-host tests through the link, 7 E2E scenarios (`tests/e2e/plugin_lanes.rs`, mutation checked), a shot (`automation_plugin_chooser`), a perf scenario (`engine_plugin_lanes_64`); **MT-AUTO-4 passed (owner, 2026-10-20; no step detail)**, and again after two fixes for a parameter that could not be changed once its lane was switched off or removed (a stale value queued in the plugin host; a plugin instrument offered the removed lane's last block with every block: ADR-0084 amendments, engine test `a_lane_that_is_taken_away_stops_reaching_a_plugin_instrument`); gates and what was not run are in ADR-0084. **A2 built 2026-10-19** (ADR-0083, branch `m7-sa2-edit-a-lane`, cut from `main` after A1 merged; no format change): several points picked (Shift+click, Shift+drag box) and moved together, resting places for values, segment shapes (commands and a bend dot), the pencil, five shape tools, a live value chip, and copy, paste, duplicate and select all of points; 22 unit tests for the pure edits (`lumen_project::edit::automation`), 19 new E2E scenarios (`tests/e2e/lane_edit.rs`, mutation checked), a shot (`automation_editing`); gates run in the Linux cloud container: fmt, workspace clippy, the workspace tests (2,375 passed; one timing-sensitive hosting soak failed under load and passes alone), 6 smoke, macOS cross-clippy; not run: Windows runs, the WSL gate, the release frame benchmark with lanes, RealtimeSanitizer; **MT-AUTO-3 passed (owner, 2026-10-20; no step detail)**, with one note to revisit after the core product: copy and paste should work on time ranges, not on points. **A1 built 2026-10-19** (ADR-0082, branch `m7-sa1-see-a-lane`, cut from the A0 branch; no format change): the lane over the clips, the A key and button, the parameter chip, click, drag and delete of points, the keys alone; 13 E2E scenarios, 16 unit tests; Windows gates: fmt, workspace clippy, the whole workspace test run, 6 smoke, macOS cross-clippy; not run: the WSL gate and Linux and macOS runs, the release frame benchmark with lanes (the owner's Lumen held the binary), RealtimeSanitizer; **MT-AUTO-2 passed (owner, 2026-10-19) except step 12 (release frame numbers, blocked by a stress-project issue in KNOWN_ISSUES)**. **A0 built** (ADR-0081, format v22): lanes in the model, the baking and the mixer play trim, mute, fader, pan and send lanes on their sample; no interface yet (A1); MT-AUTO-1 passed (owner, by ear, 2026-10-19; no step detail reported). Gates run 2026-10-19 (Linux container): fmt, workspace clippy, 2,301 workspace tests (500 E2E), 6 smoke, macOS cross-clippy, the Valgrind performance gate (new scenario `engine_automated_64` 2,052,748,374 instructions, added to the baseline by hand; existing engine scenarios +0.03% to +0.63%; `project_parse` reads +4.61% here against +5.70% at the base commit, the known offset of this machine). Not run: RealtimeSanitizer, Windows and macOS runs, the WSL gate. Slices A1 to A8 follow. Decisions 2026-10-19: the milestone is split (editing and render are 7b), tempo ramps, the audio pitch envelope and the static clip pitch are deferred; the lane look is the overlay on the clip lane (L1 B). M5 and M6 are not signed off | Plan: `docs/milestones/M7.md` |
| 7b | Advanced Editing & Render | 🔲 Plan written 2026-10-19 for the owner's review, not started. Slices E0 to E6: tool model and split, clip commands, crossfades, consolidate, bounce, freeze. Chosen 2026-10-19: a crossfade at a join (X1 A), the press-aim-release cut and the Ctrl+E edit cursor, keys per the CLAUDE.md §5 convention (K1). Open: the static clip pitch (Y1) | Plan: `docs/milestones/M7b.md` |
| 8 | Drum Maps, Drum Editor & Built-In Devices | 🔲 | |
| 9 | Recording, Media, Export & Production Workflow | 🔲 | |
| 10 | Hardening, Performance & Release | 🔲 | |
| S | Stretch goals (unscheduled; pulled into a milestone only with owner approval) | 🔲 | VST3 scanning and hosting on macOS and Linux |

## Milestone 5 slices

2026-10-14 recording correction (ADR-0064;
[plan](milestones/M5-recording-window.md)): MIDI capture continues outside
loop/punch brackets until Stop or Record-off. Full notes and controllers
remain as hidden clip content; widening edges recovers them. Replace
playback resumes after punch-out. Automated verification is recorded in
the correction plan; the hardware walkthrough in MT-MIDI-7 step 7 waits.

PR 16 review corrections: playback eligibility and capture share compensated
positions and engine-side loop-pass bounds; disabling Loop before its first
wrap preserves the saved take boundary. Format v16 distinguishes hidden
captured notes from genuine seam continuations, including identical onsets
and shorter notes, while preserving trim recovery (ADR-0064).

Recording-start follow-up: latency-compensated initial-pass messages are
floored at Record start, preserving first-note visibility away from song
start while keeping punch masking and subsequent loop-pass positions.
The regression failed at local tick -33,869 before the fix; it now passes
at tick zero. MT-MIDI-7 step 17 adds the owner recheck.
The owner reports the fix works after `409212c` (2026-10-14) and
authorizes pushing the feature branch. M5 closeout remains pending.

| # | Slice | Status | Commits |
|---|---|---|---|
| 0 | MIDI devices and the model: `lumen-midi` (midir), the MIDI page of the settings, format v11 (controller, bend, pressure and program events in clips); ADR-0044 | ✅ Built 2026-10-10; MT-MIDI-1 passed 2026-10-11 | `32a751a`, `5fa36c3` |
| 1 | Hearing MIDI (ADR-0045): **1a** the event path in the engine (sample-exact events, the note tracker, chase, every release case, hardware queue); **1b** the built-in synth in the new `lumen-devices` crate (five presets), format v12 (instrument device and MIDI To on the track), `set_instrument` and `set_midi_to`, the track menu's Instrument and MIDI To, palette commands; **1c** MIDI To timing and its tests; **1d** Panic, E2E (11 scenarios), the demo project for MT-MIDI-2, perf scenario `engine_synth_64`, records | ✅ Built 2026-10-10; MT-MIDI-2 functionally passed 2026-10-11, with playback pops and preset-response latency recorded | `19babb4`, `d1a0761`, `22ac950`, `25b48a5`, `b856d79` |
| 2 | Playing live (ADR-0046; owner's L A, J A, C B, K A, H B, R A from the "MIDI: Playing Live" canvas): **2a** live input through the engine, placed one block after it was played (`LiveInput`, `Listen`, a live note tracker; `live.rs` maps driver stamps to frames); **2b** format v13 (`midi_from`, `monitor`), who listens (the selected MIDI track, as selection-based auto-arm, plus armed tracks), the track menu's MIDI From and Monitor, the mixer strip's From and To, the hollow ring; **2c** the computer keyboard (Ctrl+Shift+K, lit keycap and readout, info line); **2d** the MIDI chip, its popover and activity counted at the source; **2e** header lights, the driver waking the UI (no polling), E2E that the keyboard is heard, the design sweep (`shots::live_sweep`); then the transport bar fitting 960 × 540 by a set order (`transport/fit.rs`). Built by Sonnet agents in worktrees, reviewed and merged. 9 engine tests, 5 feed tests, E2E `midi_from`, `keyboard`, `midi_chip`, `live_lights`, `barfit` | ✅ Built 2026-10-11; MT-MIDI-3 passed 2026-10-11 with channel filtering unverified on this controller and latency noted | `015c23d`, `456b914`, `040191d`, `b41cb2a`, `d3ddee1`, `d209702` |
| 3 | MIDI clips and the piano roll (ADR-0047; owner's Look A, Draw A, Live A from live specimens; [plan](milestones/M5.md)): **3a** note mute (format v14) and pure note edits with index selections and `Document` methods, `create_midi_clip`; **3b** notes on the GPU per surface, mini notes on arrangement clips, the 100,000-note stress track; **3c** the clip editor in the Clip tab and its pop-out, Draw and pointer tools, Live A previews, commands and keys by context, double-click to make and open clips; **3d** auditioning as its own live source, and two hanging-note fixes (same-frame on/off; hardware ons counted per key). Built by Sonnet agents in worktrees, merged and checked. 25 E2E in `notes`, 6 in `audition`; shots `note_editor`. Perf gate (WSL): all within +2% (`engine_synth_64` +1.35%, `engine_transport` +0.94%). Release frame times on the stress project with the 100,000-note track, p99 per phase (scroll time, scroll tracks, zoom): arrangement only 1.62, 1.77, 1.53 ms at 1600 × 900 and 3.08, 4.47, 3.11 ms at 3440 × 1380; with the dense clip open in the clip editor (`--bench-editor`) 1.26, 1.82, 1.20 ms and 2.22, 2.10, 2.17 ms. All under the 8.33 ms (120 fps) target. Idle 3 repaints in 3 s (one 3440 run read 11 and 19 with a cause inside egui; two re-runs read 3) | 🟡 Built 2026-10-11; MT-MIDI-4 waits | `b789ca7`, `baac990`, `ca78e6d`, `57435f1`, `cfcfae1`, `c32d03f`, `6cbb365`, `78e794b` |
| 4 | Velocity and controller lanes (ADR-0050; owner's V A, L A, T A, R B from live specimens; [plan](milestones/M5.md)): **4a** pure edits in `lumen_project::edit` (`velocity.rs`, `events.rs`); **4b** `Document` methods in `document/lanes.rs` (one undo step per gesture, the document changed on release) and the lane under the roll in `note_editor/lane/` (velocity stems, controller step traces, Pencil, Line, Curve, move, box, Delete, the bend dot, the "+ CC" menu, ten palette commands); **4c** tests: 17 E2E in `lanes`, the engine check of drawn controller and bend timing, the `clip_lane` shot. No format change (v14). 16 + 30 + 16 + 63 unit tests; the workspace tests pass (lumen-app lib 417, E2E 271) and clippy is clean. Frame times with a lane open are not measured (MT-MIDI-5 step 12). Built in parts by delegated agents, integrated and checked by the integrator. Keys 1, 2, 3 and Shift-Pencil from the plan were not built. | 🟡 Built 2026-10-11; MT-MIDI-5 waits | — |
| 5 | Quantize and its tools (ADR-0054; owner's ghost trails and comet sliders from live specimens, Q on every note when none are picked, restore-recorded-timing deferred): **5a** pure `edit::quantize` (grid, Start/End/Both/Length, strength, swing, window) and `edit::note_tools` (humanize, fixed length, scale constrain, reverse); **5b** five undoable `Document` methods; **5c** the Quantize popover (Shift+Q, Q applies the last settings), the ghost-trail preview, `widgets::comet_slider`, `Menu::{staying, open_for, close_for}`, six commands and the tools in the roll's menu; **5d** E2E `quantize` (7), unit tests (17 + 23 pure, Document methods), shot `quantize_popover`, MT-MIDI-6. No format change (v14). Workspace tests, clippy and fmt pass (E2E 296). Perf gate not run (WSL only; no hot path changed). | ✅ Built 2026-10-12; MT-MIDI-6 passed 2026-10-12 (run 2) | — |
| 6 | Recording MIDI (ADR-0055; owner's C4 Replace with an Overdub switch, loop passes layer, settings in the app file, a clip from record start to stop, and 1-A, 2-B, 3-A from the "Lumen Recording Specimens" canvas; [plan](milestones/M5.md)): **6a** the engine (`Command::Record` and `EndRecord`, the count-in in the metronome, `midi/capture.rs` and `Captured`, the snapshot's recording and count-in fields); **6b** the take builder `lumen_project::edit::record` (Replace, Overdub, loops, punch, locked clips) and `Document::begin_recording` and `record_take`, one undo step per recording; **6c** the app recorder (`app/record.rs`, `recorder.rs`), `settings.record` (`record_settings.rs`) and the commands (F9, Overdub, count-in, metronome only while recording); **6d** the Record and Overdub buttons, the count-in on the position field (`transport/record.rs`, `transport/bar.rs`) and the record-red clip (`arrangement/recording_look.rs`); **6e** E2E `record` (9), engine `record` (8), the allocation guard, `shots_recording`, MT-MIDI-7 and these records. No format change (v14). Not verified here: real controller timing, the WSL perf gate, the macOS cross-clippy. | 🟡 Built 2026-10-13; MT-MIDI-7 waits for the owner | 3beff4b (plan), 6f46c65, 3cd6698, b4e96dd, e9ac6e0, 4b74b61, 7c90298, 02e7f01 |
| 7 | MIDI files and clock out (ADR-0062; owner's F1–F4 A; MIDI learn moved to M7): **7a** `lumen_project::smf` on `midly` (type 0 and 1 in, split by channel; type 0 or 1 out at 960 PPQ), hand-made files in `tests/smf/` and a golden file; **7b** import by drop and **Import audio or MIDI…** (on a MIDI track one clip, between tracks a track per part), the tempo offer banner, **Export MIDI clip…** and **Export MIDI file…** writing what plays, off the UI thread; **7c** MIDI clock: a Sync switch and delay per output, the clock made in the engine (`midi/clock.rs`), the delay and stall Stop in the sender. E2E `smf` (4) and `clock` (1); E2E 331, smoke 6, workspace tests and clippy pass. Perf gate run here under Valgrind: slice 7 changes nothing measurable against its base commit (`engine_synth_64` +0.0001%, `engine_transport` −0.02%); against `perf/baseline.json` this container reads `project_parse` +5.83% at the base commit too (KNOWN_ISSUES). Not run: the macOS cross-clippy | 🟡 Built 2026-10-13; MT-MIDI-8 waits for the owner | af76215, a2c69e5, 2ef1d3f |
| 8 | Close: combined E2E `midi_walkthrough` records a part, quantizes it, saves and reopens the same MIDI content, then plays it through a fake output; preliminary review found and fixed Save As directory probing on the UI thread; owner closeout MT-MIDI-9, full review and report | 🟡 In progress 2026-10-13; checks and owner run pending ([preliminary review](audits/2026-10-13-m5.md)) | — |

## Milestone 4.5 slices

| # | Slice | Status | Commits |
|---|---|---|---|
| 1 | The audit's bugs: value fields follow the surface (T1), a `well` token for grooves and meters (T2), the scroll kept on the tracks (U1), the shots harness (a taller window, one thread) | ✅ 2026-10-10 (MT-POL-1 passed) | `37bfea0` |
| 2 | Shared widgets and sizes: the size scale (S4), the handle that opens near its value (S2, the owner's C), scale numbers thinned (S3), the switch, segmented control and choice rows replace egui's checkboxes and radios (T3) | ✅ 2026-10-10 (MT-POL-2 passed in run 2; menus close gently, the owner's C) | `614dbfa`, `173a51a`, `33a44c1` |
| 3 | Chrome: toggles keep a button's fill and the detail tabs are underlined words (T4), the arm button grey at rest (T5), one empty-state widget and the mixer's ghost strip (S5), the clip card's 2 × 6 swatches and middle-shortened path (S6), one time-signature field (S7), the metronome's volume as a value slider | ✅ 2026-10-10 (MT-POL-3 passed) | `70d68d0` |
| 4 | Numbers in Martian Mono (S1), at width 87.5; licence in `assets/fonts` and THIRD_PARTY_NOTICES | ✅ 2026-10-10 (MT-POL-4 passed) | `259f63a` |
| 5 | Motion: clips glide to their place and undo washes what it changed (M1, M4); rows grow in, glide and fold shut (M2, M3); fading scroll bars on the tracks and the mixer (U2); the palette's sliding highlight, lit states cross-fading, windows fading out (M5). Idle: 3 repaints in 3 s, as before | 🟡 Built 2026-10-10, MT-POL-5 waits for the owner; release frame times measured in slice 6 | `ebce840`, `67137da` |
| 6 | Close: E2E `access` and the walkthroughs re-run; the design sweep (`shots::polish_sweep`, both themes at 100/150/200%) found switches, segmented controls and captions off the menus' label line (fixed, `widgets::menu_caption`) and the audit's mixer edge fades unbuilt (built); the frame benchmark lets animations settle before counting idle; the report | 🟡 Built 2026-10-10, MT-POL-6 waits for the owner | this commit |

## Milestone 3 slices

| # | Slice | Status | Commits |
|---|---|---|---|
| 1 | Performance gates by instruction count (ADR-0022) | ✅ 2026-10-07 | `03e2228` |
| 2 | Design system, motion system, M2 retrofit; typefaces (ADR-0023, MT-APP-7) | ✅ 2026-10-07 | `dbe4777`, `392a5f8`, `d320cf0` |
| 3 | Project model and undo, save/open, autosave and recovery (ADR-0024, MT-APP-8) | ✅ 2026-10-07 | `fd7583b`, `e2768fb`, `43c86f1`, `578bc6f`, `7780b11`, `4a7f615` |
| 4 | Tracks and the arrangement view at scale: groundwork (format v3), 4a track headers (ADR-0025, MT-APP-9 passed), architecture audit fixed, 4b clips on the GPU with a frame-time benchmark (ADR-0026, MT-APP-10 passed), 4c waveforms (ADR-0027, MT-APP-11 passed), 4d navigation (ADR-0028, MT-APP-12 passed) | ✅ 2026-10-08 | 4a and audit through `eea085a`; 4b from `397f846` |
| 5 | Grid, snapping, rulers, scrub, markers, loop: groundwork (format v4), 5a grid and snapping, 5b rulers (layout A), 5c loop and punch, 5d locators, 5e scrubbing | ✅ 2026-10-08 (ADR-0030, MT-APP-13 passed) | `1317d07` groundwork; `c7104a5`, `b3fd9d6`, `d9eab12`, `5fdc6e4`, `b3ccb4b` (5a to 5e); `656a10a`, `efc3012`, `9492652` (MT-APP-13 run 1); `ec5541f` closes it |
| 6 | Clip editing and selection: groundwork (format v5), 6a selection and moving, 6b time ranges and edits, 6c trim, loop-extend and slip, 6d fades and clip gain (format v6, ADR-0031; MT-APP-14 run 1 passed its steps), 6e clip properties (mute, lock, rename, colour, the pinned card) and drops on a chosen track (ADR-0032) | ✅ 2026-10-08 (ADR-0031, ADR-0032; MT-APP-14 passed in run 4, after the owner's run 2 changes and run 3 fixes) | `f66c647` to `a6a2164`; 6e `6cc6860`; run-2 changes `3127ec7`; run-3 fixes `4e8c779` |
| 7 | Command palette, shortcuts, context menus: 7a the registry and one dispatcher, 7b the palette (P2 with P3's values, K5 keycaps), 7c remapping (E2 keyboard map with E3 in the palette, saved in the settings), 7d menus and tooltips from the keymap, ruler menus, Z, Ctrl+A, Ctrl+L, Fit loop, F6 panes, Shift+Tab by focus (ADR-0033) | ✅ 2026-10-08 (ADR-0033, MT-APP-15 passed) | `c4c008f` 7a, `9983bf1` 7b, `3d21a26` 7c, `36573a7` 7d |
| 8 | The exit flow (E2E `walkthrough`), an accessibility audit (E2E `access`), the end-of-milestone review (`docs/audits/2026-10-08-m3.md`) and its fixes: the smooth playhead, Import audio…, clips chosen with the arrows (the owner's choice), the time signature as a method, drop order, three modules split (ADR-0028 and ADR-0033 amendments) | ✅ 2026-10-09 (MT-APP-16 passed) | `ebf08a8`, `0e6b8f9` review; `273af92`, `5d1bab6`, `29c5870`, `b376107` fixes |

## Milestone 2 slices

All 8 slices are done and approved (2026-10-07). Details and manual test
results are in [milestones/M2.md](milestones/M2.md); the report is below.

## Milestone 1 slices

| # | Slice | Status | Commit |
|---|---|---|---|
| 1 | Repository, workspace, CI, license policy, RT allocation guard, benchmark harness, plan and ADRs | ✅ | `8d8f0a2` |
| 2 | P-ENGINE: real-time core prototype. All criteria met: 0 heap operations, bit-identical across block sizes, 64 tracks × 128 frames = 0.85% of budget | ✅ | `5084725` |
| 3 | P-ASIO: native ASIO probe. 7 drivers queried safely; 30 s Focusrite streams with 0 discontinuities and 0 heap operations; 64-frame ASIO4ALL stable | ✅ | `7a85a6f` |
| 4 | P-ISO: isolation transport. 1 host p99 19.5 µs, 8 hosts p99 35.5 µs at 128 frames; crash and hang detected and replaced in about 11–13 ms with audio uninterrupted | ✅ | `0fbaf7d` |
| 5 | P-VST3: out-of-process scanning. 39 of 40 real modules enumerated in 10.4 s; moduleinfo matches 4 of 4; crash, hang and no-factory test modules contained (CI self-test) | ✅ | `7e8f47c` |
| 6 | P-FORMAT: project format and time base (`crates/lumen-project`, `lumen_core::time`). 29 new tests; quality-bar project saves in 83 ms and loads in 87 ms | ✅ | `57d8901` |
| 7 | P-UI: dense-view rendering. Normal views p99 ≤ 6.1 ms; plain egui fails the 100k-note overview (20 ms); GPU-instanced escape hatch brings it to 5.7 ms; idle 0.00%; first frame 545 ms warm; UI Automation tree verified | ✅ | `0c6a15b` |
| 8 | Evidence in ADRs, architecture draft, cargo-deny verified locally, interim report | ✅ | (final M1 prototyping commit) |
| 9 | P-RENDER: `lumen-render` (instanced GPU layers + Vello) measured in a bare winit host and inside egui. Worst case p99 0.84 ms bare / 2.22 ms in egui (egui shapes: 20 ms). Text (Parley) and accessibility (AccessKit) spikes work. egui host with the pipeline cache: first frame 411 ms warm, idle 0.00%. **Owner chose the egui host.** | ✅ | `af2e4bd`, this commit |
| 10 | P-HOST: Test instrument H hosted in-process and in an isolated host process. 66 µs/block round trip incl. processing; real crash detected next block, restarted with last known state in 33 ms, audio resumes; state round trip identical | ✅ | `bd57b9b`, this commit |
| 11 | `lumen` application shell (`crates/lumen-app`): spec layout on egui; arrangement grid drawn by `lumen-render`; GPU pipeline cache in Lumen's cache folder; first frame 394 ms warm / 1,110 ms cold; 21 accessible elements, disabled controls announced as disabled | ✅ | this commit |

---

# Feature tracking against CLAUDE.md

Tier: **M** must ship in 1.0 · **S** should ship in 1.0 · **L** later. Rows
follow the order of `CLAUDE.md`. "Planned" is the roadmap milestone.

## §4 Quality bar

| Requirement | Tier | Status | Planned | Notes |
|---|---|---|---|---|
| No dropouts or clicks during editing, scrolling, scanning, loading, opening plugin windows | M | 🔲 | 2, 10 | |
| 64-track project under 50% of the 128-sample / 48 kHz budget | M | 🔲 | 2, 10 | P-ENGINE measures engine overhead |
| No buffering beyond the driver | M | 🔲 | 2 | ADR-0003 |
| Sample-accurate playback, MIDI, automation, loops, editing | M | 🟡 | 2–7 | Time base done (`lumen_core::time`, ADR-0009): every sample addressable at 999 BPM / 192 kHz. Transport and loops sample-accurate for every block size (`lumen-engine`, slice 3). MIDI, automation, editing later |
| Offline render equals real-time and is repeatable | M | 🔲 | 2, 9 | P-ENGINE block-size invariance |
| UI 60 fps minimum, 120 fps target at 200 tracks / 20,000 clips / 100,000 notes | M | 🟡 | 3, 10 | Clips: met. With clips on the GPU (M3 slice 4b, ADR-0026), the stress project scrolls and zooms at p99 ≤ 2.3 ms at 3440 × 1380 (Ryzen 7 7700X, RTX 4070), measured by `--bench-frames`. With waveforms on every audio clip (4c, ADR-0027): p99 ≤ 2.3 ms at 3440 × 1380, unchanged. At the M3 close (rulers, handles, the grid through clips): p99 ≤ 2.3 ms at 1600 × 900 and 3.3–4.2 ms at 3440 × 1380, inside the 8.3 ms of 120 fps. Notes (M5) still to measure; mid-range hardware in M10 |
| Near-zero idle CPU | M | 🟡 | 3, 10 | 0.0–0.2% measured with audio running (M2 MT-APP-4; M3 slice 2 with the motion system). Animations repaint only while moving. Re-measured at scale in M3 slice 4b: 3 repaints in 3 s idle with the stress project (the once-a-second device check); M10 |
| Cold start under 2 s; scanning and loading never block the UI | M | 🟡 | 3, 6, 10 | First frame 394 ms warm / 1,110 ms cold (M1 shell). Audio files, project open and save, file dialogs and ASIO driver checks all run in the background (M2, M3). Plugin scanning in M6 |
| Autosave, crash recovery, atomic saves, backups | M | ✅ | 3 | Atomic save and rolling backups (`lumen-project`). Autosave a minute after unsaved changes, to the local app data folder; a recovery banner after a crash; tested by killing the process (M3 slice 3d) |
| Crashing plugins isolated, flagged, recoverable; scanning never crashes the app | M | 🟡 | 6, 10 | Built in M6: one host process per plugin (ADR-0066, 0067), scanning in child processes (ADR-0068), a failed card with Reload (slice 2e), kill-on-close job object, the fault soak (slice 7b: crashes, hangs, slow and non-finite blocks, hosts ended from outside; the others never fail). Verified with the test plugin; real plugins are the owner's MT-VST-* runs |
| Device removal, rate and buffer changes, sleep and resume handled | M | 🔲 | 2, 10 | |
| Safe mode without plugins | M | 🟡 | 6 | Built in M6 slice 5e (ADR-0077): `--safe-mode`, Shift at start on Windows, the offer after a start that did not finish; MT-VST-5 is the owner's |
| Automated tests: core logic, editing, serialization and migration, audio (golden renders) | M | 🟡 | all | Unit and integration tests per crate; an end-to-end suite that drives the whole app (CI), and real-binary smoke tests run at the end of every slice (ADR-0029, 2026-10-08). Golden renders with export (M9) |
| Benchmarks with regression gates in CI | M | ✅ | 1–3 | divan wall-clock benchmarks (smoke-run in CI), plus instruction-count gates under Callgrind that fail CI above +2% (M3 slice 1, ADR-0022). Each milestone adds its hot paths |
| CI on Windows, macOS, Linux | M | ✅ | 1 | First run green on 2026-10-06 (run 37396217369): Windows MSVC, macOS, Linux, rustfmt, cargo-deny, VST3 self-test. macOS was paused from 2026-10-07 (Actions minutes ran out) and is **back since 2026-10-10** (owner): every job green on all three systems in run 38070627268, after fixing 20 E2E tests that only failed on macOS |
| Test plugin simulating good and bad behaviour; real-plugin compatibility matrix | M | 🟡 | 1, 6 | M6 slice 0 (ADR-0065): `lumen-test-plugin`, a portable deterministic instrument and effect with injectable faults (factory, processing, state, metadata), tested through the VST3 interfaces and in child processes on Linux and Windows. The host core (`lumen-plugin-host`, ADR-0066) loads it through the OS loader, in process and in child processes; ADR-0067 adds the shared-memory audio path and the measurement harness (84 Linux tests; all 83 Windows release tests passed). The corrected MT-VST-0 full grid ran on Windows 2026-10-15 without an MMCSS warning; event and event/spin had no misses, hybrid and spin missed in several cases. Crash injection still reports two heap operations in Lumen's loop. The scan-stage module and `docs/PLUGIN_COMPATIBILITY.md` (scan stage only) remain; processing, state, editor and routing stages arrive with slices 2 to 6 |
| Real-time correctness enforced by tests | M | 🟡 | 1–2 | `lumen-rt` allocation guard (ADR-0010) |
| High-DPI, 100–200% scaling, light and dark themes, keyboard-navigable, accessible names, colour-blind-safe | M | 🟡 | 3, 10 | Dark, light and match-system themes; interface size 100–200% in the app menu; colour-blind-safe accent; contrast tested for both themes (M3 slice 2, `docs/DESIGN.md`). M3 slice 8: every control in the main window, palette, shortcuts window, clip card, clip menu, audio settings and history has an accessible name, and Tab walks the transport left to right, then the panes (E2E `access`). The full accessibility pass and the Narrator check in M10 |
| No telemetry by default; opt-in only | M | ✅ | — | Nothing collects data. Re-verify each milestone. |

## §6.1 Arrangement and tracks

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Rulers: bars/beats, minutes:seconds, samples, timecode | M | ✅ 2026-10-08 | 3 | M3 slice 5b, layout A (owner): the bar ruler on top; a time ruler along the bottom in minutes and seconds, samples, or timecode at 24, 25, 29.97 drop-frame or 30 fps (`timefmt.rs`, tested), chosen from its corner and saved in settings; the position display follows it. At the deepest zoom the bar ruler pins the position of its left edge |
| Adaptive or fixed grid (triplet, dotted, off) with snap-bypass modifier | M | ✅ 2026-10-08 | 3 | M3 slice 5a: `lumen_core::grid`, adaptive in five widths or fixed 8 bars to 1/64, triplet and dotted, snap on or off (Ctrl+1 to Ctrl+5, the grid menu and button); Alt bypasses; snap guide; saved with the project (format v4). Used by the ruler and dropped files now, by clip edits in slice 6 |
| Anchored zoom, zoom to selection, fit all, fit loop, pan, edge auto-scroll, follow modes. (Ctrl+wheel zoom works in the M1 shell since the 2026-10-06 fix: egui delivers it as a zoom factor, not as scroll.) | M | 🟡 | 3 | M3 slice 4d (ADR-0028): zoom at the pointer to single samples, `+`/`-`, X back, W fit all, H fit heights, Alt+wheel heights, pan, auto-scroll while dragging tracks, Follow page and continuous. Zoom to selection (Z) and fit loop added in slice 7d (ADR-0033), on the bar ruler's menu with the other view commands |
| Track types: audio, MIDI/instrument, group, return, master | M | 🟡 All five exist in the project model and the arrangement. Groups sum their tracks (M4 slices 1 and 3); returns and the master are pinned at the bottom of the arrangement, returns lettered A, B, C with sends from every audio, MIDI and group track (M4 slice 4, ADR-0038). Every kind has a channel strip in the Mixer tab (slice 5, ADR-0040) | 3–4 |
| Per-track name, colour, height, arm, monitor, solo, mute, polarity, I/O routing, inline fader and meter | M | 🟡 Name, type icon, colour, height (drag to resize in 22 pt steps, fold and unfold), arm (glows and breathes), solo and mute in the track header; volume and pan sliders in unfolded headers (M3 slice 4a, ADR-0025) with the level inside the volume slider (the owner's H2, M4 slice 5, ADR-0040); polarity, output routing and a fader on the track's strip in the Mixer tab (slice 5). Monitor and input routing with recording (M9) | 3–4 |
| Group tracks: create, drag in, nest, fold, sum, own devices/sends/fader/automation, one-step ungroup, sensible solo/mute/arm | M | 🟡 M4 slice 3 (format v8, ADR-0037): Ctrl+G and Ctrl+Shift+G (one step each), nesting three deep at most, folding (saved), dragging tracks in and out, groups summing their tracks (slice 1), mute, a group's arm arming its tracks, deleting and duplicating a group with its tracks. Solo across groups and returns by the agreed rules, a soloed group showing its tracks with a hollow S, and a group's sends on its header (slice 4, ADR-0038); a group's devices with M6, automation in M7 | 4 |
| Linked editing across selected tracks | S | 🔲 | 4 |
| Clips: move, copy-drag, trim, slip, fades, gain handle, loop-extend, rename, recolour, mute, lock | M | 🟡 M3 6a: move across tracks and Ctrl+copy-drag by the top half (snapped, one undo step, kinds respected, locked clips stay). 6c: trim edges (within the file), Shift loop-extend, Ctrl+Alt slip, and the clip overlap rule (a clip put down cuts away what it covers). 6d: fade handles with a free bend dot and Alt for S-shape (owner's F3 and C3; curves in format v6, ADR-0031), presets in the menu, a clip gain line (G1), heard and drawn alike; moved clips crossfade while playing. 6e (ADR-0032): 0 mutes (grey, the owner's M1), lock from the menu or card (padlock and clamps, K3; locked clips can still be muted, renamed and recoloured), Ctrl+R or F2 renames in the title bar, clips take their own colour, and the properties card (P1) sets all of it, gain and fades from the keyboard | 3 |
| Selection: clip, marquee, time range with cut/copy/duplicate/delete/split/consolidate | M | 🟡 Clip and marquee selection and time ranges across tracks (M3 6a); cut, copy, paste, duplicate and delete on clips and ranges (6b). From the keyboard (M3 review H2, the owner's choice): with a clip selected the arrows move between clips (←/→ along its track, ↑/↓ to the nearest clip above or below), Shift adds. Split and consolidate in M7b | 3, 7b |
| Loop and punch regions, markers and locators, tempo and time-signature lanes | M | 🟡 Engine side of loop and punch done (slice 3); loop brace and punch (M3 5c) and named locators (M3 5d) done; tempo and time-signature lanes later in M3 | 2–3 |
| Track height presets, collapse, hide, reorder by drag, multi-select | M | 🟡 Fold and unfold, resize by dragging or Alt+wheel, H fits every track, multi-select (Ctrl, Shift), move with Alt+arrows and the menu (4a), reorder by drag with auto-scroll (4d). Hide and height presets later | 3 |
| Track templates, delay offset, lock, overview strip | S | 🔲 | 3, 9 |

## §6.2 Audio

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Import by drag and drop with sample-rate handling and optional copy into project | M | 🟡 Drag and drop from Explorer and automatic rate conversion done (M2 slice 6, ADR-0019), with a supported-file indicator while dragging. Imported audio is copied into `Samples/Imported` on save (setting, on by default; M3 slice 3c). Dropped on an audio track's lane, files land on that track at the pointer, one after another; between tracks they make new tracks there (M3 6e). **Import audio…** (Ctrl+Shift+I; the app menu, the palette, a track's menu and the empty arrangement) opens the system dialog and imports at the playhead onto the selected audio track or new tracks (M3 review H1). Files dropped together keep their order, and later drops never overtake earlier ones (`6ea0681`). Since M5 slice 7 the command is **Import audio or MIDI…** and MIDI files land on MIDI tracks (ADR-0062) | 3 |
| Drop files onto a specific track; dropping between tracks or below them creates a track (owner request, MT-MEDIA-1, 2026-10-06) | M | ✅ Done in M3 6e (`arrangement/drop.rs`): over an audio track's lane, files land on it at the pointer; between tracks, below the last one or over another kind of track, they make new tracks there. MT-APP-14 run 3 found drops between tracks were never offered on Windows (no pointer during a drag from Explorer); fixed by reading the system's cursor (ADR-0032); passed in run 4. macOS and Linux: KNOWN_ISSUES | 3 |
| Clip hover card with file details (format, rate, channels, length, size, path) | S | ✅ 2026-10-08 Basic card in M2 slice 6 (owner request, MT-MEDIA-1); in M3 6e it pins open as the clip's properties card (Alt+Enter; the owner's P1) | 3 |
| Glitch-free playback, seamless loops, instant seeking | M | 🟡 Clips play sample-exactly from memory, with click-free crossfaded loops and locates (slice 6; golden renders; MT-MEDIA-1 passes by ear). Large sessions need disk streaming (M9) | 2 |
| Disk streaming of clip audio with read-ahead, so large sessions need not fit in RAM; in-memory playback becomes a per-clip RAM option (owner decision 2026-10-06: memory in M2, streaming in M9) | M | 🔲 | 9 |
| Crisp, fast waveforms at every zoom | M | ✅ | 3 | M3 slice 4c (ADR-0027): peak pyramids built at load, drawn per pixel on the GPU; no measurable frame cost with 10,000 waveforms. Disk cache deferred to M9 with disk streaming (owner approved); drawn sample by sample below 16 frames per pixel (4d, ADR-0028) |
| Recording: multi-arm, monitoring, multichannel, crash-safe capture, latency compensation and offset, punch, count-in, input meters | M | 🔲 | 9 |
| Loop recording into takes; capture MIDI | S | 🟡 Loop passes layer into one MIDI clip (M5 slice 6, ADR-0055); takes and Capture MIDI are M9 | 9 |
| Clip gain, fade shapes, automatic crossfades, reverse, normalize, polarity, pitch | M | 🟡 Clip gain and fades along free curves, with four presets (M3 6d, ADR-0031; golden renders, and E2E scenarios that listen). Automatic crossfades (model chosen in M7b, X1), reverse, normalize and polarity in M7b. The clip's static pitch needs a pitch engine; the owner deferred it with the audio pitch envelope (2026-10-19, M7b plan, Y1) until warp and time-stretch | 3, 7b |
| Warp and time-stretch with quality modes | S | 🔲 Not in M7 or M7b. It also carries the audio pitch envelope (deferred by the owner, 2026-10-19) and, if the owner defers it, the clip's static pitch | — |
| Take lanes and comping | S | 🔲 | 9 |
| Mono, stereo, multichannel material | M | 🔲 | 2–3 |

## §6.3 MIDI

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Record, overdub, replace, count-in, punch, thru, channel filter, computer keyboard | M | 🔲 | 5 |
| Notes never hang; panic command | M | 🟡 Built in slice 1 (ADR-0045): a tracker per track lets go on stop, jump, loop wrap, edit, mute, change of output and Panic; hardware gets note-offs and All Notes Off / All Sound Off on every channel. Engine tests (14, including 400 random commands with every note-on answered), allocation guard, RTSan, and 11 E2E scenarios on a fake port. Unverified: a real MIDI device and real synth hardware (MT-MIDI-2) | 5 |
| Sustain, pitch bend, aftertouch, program and bank change, MIDI clock out | M | 🟡 Played from clips by the synth and sent to ports (slice 1; bank selects pass through to ports, the synth ignores them); chased on Play and jumps. Clip data can only be made by the demo project until the piano roll (slice 3). **MIDI clock out** built in slice 7 (ADR-0062): Sync per output with a delay, Start, Stop, the song position and Continue, 24 clocks a quarter on their samples; hardware timing unverified (MT-MIDI-8) | 5 |
| Piano roll: draw, select, move, resize, split, glue, mute, duplicate, nudge, transpose, legato, audition, velocity and controller lanes, synced zoom | M | 🟡 Built: the piano roll (slice 3, ADR-0047) and the velocity and controller lanes (slice 4, ADR-0050); MT-MIDI-4 and MT-MIDI-5 wait | 5 |
| Scale highlight and snap, fold, ghost notes, multi-clip editing, step input | S | 🔲 | 5 |
| Quantize: grids, modes, strength, swing, sensitivity, iterative, one-key apply, live-preview popover, humanize, legato, fixed length, transpose, scale-constrain, reverse | M | ✅ 2026-10-12: slice 5 (ADR-0054) and MT-MIDI-6 passed on run 2: the grid, Start, End, Both and Length modes, strength, swing, window, Q with the last settings, the ghost-trail preview, and humanize, fixed length, reverse and constrain to scale. Legato and transpose are in the piano roll's tools (`edit/notes.rs`) | 5 |
| Restore recorded timing, groove templates, audio quantize | S | 🔲 | 5, 7 |
| Standard MIDI File import and export | M | 🟡 Built in M5 slice 7 (ADR-0062): type 0 and 1 in by drop or Import (one clip on a MIDI track, a track per part between tracks; the file's tempo and meter offered), clips (type 0) and the project's MIDI tracks (type 1) out as they play. A file from another DAW is checked by hand in MT-MIDI-8 | 5 |
| MIDI learn with mapping list | M | 🔲 Moved to M7 with automation (owner, 2026-10-13) | 7 |
| MIDI effects (arpeggiator, chord, scale, velocity, length) | S | 🔲 | 8 |
| MPE, MIDI 2.0 | L | 🔲 | — |

## §6.4 Drum maps

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Map entries: name, input and output note, channel, routing, mute, solo, quantize, length, colour, choke group | M | 🔲 | 8 |
| Non-destructive storage in input notes; separate convert command | M | 🔲 | 8 |
| GM, GM2 and 808 presets; user maps; import and export; drum names in piano roll | M | 🔲 | 8 |
| Legacy drum-map format import | L | 🔲 | — |
| Drum editor: lanes with audition, mute, solo, velocity, per-lane quantize, reorder, hide unused | M | 🔲 | 8 |
| Step-sequencer view: step count, resolution, toggle, velocity drag, accent | M | 🔲 | 8 |
| Euclidean and random fill | L | 🔲 | — |
| Per-sound routing to multi-output instrument channels | M | 🔲 | 8 |

## §6.5 Automation and envelopes

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Everything with a value is automatable | M | 🟡 The model holds a lane for a track's fader, pan, trim, mute and sends and for a device's parameters (M7 A0, format v22); the engine plays the first five. Device parameters are slice A3 | 7 |
| Lanes with searchable parameter chooser; show/hide key | M | 🔲 | 7 |
| Point editing, ranges, curve types, shape tools, freehand with simplification, snapping, value tooltip | M | 🔲 | 7 |
| Clip envelopes (audio gain/pan/pitch, MIDI CC/bend/pressure), looping, lock-envelopes option | M | 🟡 MIDI clips already carry looping controller events (M5). Audio gain and pan envelopes and the lock option are M7 slice A7. **The audio pitch envelope is deferred with the owner's approval (2026-10-19)** until a time-stretch and pitch engine exists | 7 |
| Read / Write / Touch / Latch recording with thinning | M | 🔲 | 7 |
| Trim mode | L | 🔲 | — |
| Override state with one-click re-enable | M | 🔲 | 7 |
| Smooth, sample-accurate automation for built-ins and plugins | M | 🟡 Mixer values are sample-exact, the same at any block size, delay-compensated and heap-free (M7 A0; golden renders, allocation guard). Plugin parameters are slice A3; built-in device parameters wait for M8 | 7 |
| Tempo and time-signature changes recalculate the timeline | M | 🟡 The engine keeps the bar and beat across tempo and meter changes, and loop and punch follow (slice 3); clips and automation follow when they exist. The tempo lane edits steps (M7 slice A6); **tempo ramps are deferred with the owner's approval (2026-10-19)** | 7 |

## §6.6 Editing actions

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Split (playhead, cursor, grid, range edges), click-free | M | 🔲 Open for M7b (slice E0). Owner request (2026-10-08): select a clip and drag it to cut it; the gesture is still to be chosen (M3 plan, out of scope) | 7b |
| Join / glue MIDI; consolidate audio (rendered) and MIDI (merged), originals kept | M | 🔲 | 7b |
| Trim, crop, delete, ripple delete, insert silence, cut, copy, paste, duplicate clip and range, loop/unloop, reverse, normalize, nudge, move, rename, recolour, mute, lock | M | 🟡 Trim and loop-extend (6c); move and copy (6a); cut, copy, paste, duplicate, delete, delete time (ripple) and insert time (6b); rename, recolour, mute and lock (6e). Crop, unloop into copies, reverse, normalize and clip nudge are not built yet (M7b) | 3, 7b |
| Bounce in place; render selection to new track | M | 🔲 | 7b |
| Freeze, unfreeze, flatten | M | 🔲 | 7b |
| Select all / none / invert / in track / to start / to end | M | 🟡 Select all (Ctrl+A) and none (Esc) in M3 7d; invert, in track, to start and to end with the editing tools (M7b) | 3 |
| Slice audio to MIDI or drum map | S | 🔲 | 8 |
| Group and ungroup clips | L | 🔲 | — |
| Guarantees: split+join is identity; consolidating unprocessed audio is identical; undo restores exactly | M | 🟡 | 3, 7b | Undo restores exactly by construction (shared snapshots, ADR-0024) and is property-tested; split, join and consolidate in M7b |

## §6.7 Mixer, routing and grouping

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Channel strip: input gain, polarity, devices, pre/post sends, fader, pan law, mute, solo, cue, output | M | 🟡 In the engine (M4 slice 1, ADR-0035): input trim, polarity, mute, pre- and post-fader sends, volume and pan with four pan laws (−3 dB constant power by default), output to a node or a device pair, all gliding over 20 ms. Saved and undoable in the model (slice 2, format v7, ADR-0036). Slice 4 (ADR-0038): sends set from comet knobs on the headers (pre/post from their menu), mute and solo by the agreed rules, cue before the fader to a second output pair. Slice 5 (ADR-0040): the strip itself in the Mixer tab (the owner's S1: output, polarity, trim, two send knobs then "+N", pan, a fader beside its meter, typed volume, held peak, M S arm), the pan law from the mixer's menu and the palette; devices with M6 | 4 |
| Returns, groups, master, sidechain from any track, hardware output routing | M | 🟡 The engine graph sums groups (nested), returns and a master and routes to device pairs (slice 1); a track in a group plays into it. A master track in every project, outputs and sends in the model, loops refused (slice 2, ADR-0036); grouping built (slice 3, ADR-0037); returns and the master pinned at the bottom of the arrangement with sends on the headers (slice 4, ADR-0038). Output choices on the strips, any of the device's pairs, which Lumen then opens (slice 5, B6, ADR-0040); sidechains from any track in the engine graph at the three taps, compensated (slice 6, ADR-0041), chosen in the UI when a device has a sidechain input (M6, M8) | 4 |
| Solo keeps soloed paths audible; exclusive-solo option | M | ✅ Built (M4 slice 4, ADR-0038): the owner's 14 cases as one pure function (`lumen_project::solo`) and its tests: a soloed path stays open, mute beats solo, returns are solo-safe, a soloed return plays wet only. Exclusive by default, additive as a preference; hollow M and S show why a track is silent or heard; the transport's S chip clears every solo. MT-MIX-4 passed | 4 |
| Automatic plugin delay compensation with indicator | M | 🟡 Built in the engine (M4 slice 6, ADR-0041): every node reports its devices' latency; parallel paths, sends and sidechains line up sample for sample through delay lines; the engine renders ahead so what is heard lines up with the playhead; the transport shows the compensation while there is some (the owner's D1). Tested with the synthetic latent device; real latency arrives with VST3 (M6). MT-MIX-6 passed | 4, 6 |
| Peak (hold, clip), RMS; master LUFS and true peak | M | ✅ Built (M4 slice 5, ADR-0040): every node metered after fader and pan on the audio thread (lock-free), peaks falling 20 dB/s with a 1.5 s hold, clips latched until clicked, a 300 ms RMS; the master's momentary, short-term and gated integrated LUFS (BS.1770-4, EBU Tech 3341 cases 1–5 within 0.1 LU) and 4× true peak in the loudness column. MT-MIX-5 passed | 4 |
| Dockable mixer panel; inline fader, pan and meter on track headers | M | ✅ Built (M4 slice 5, ADR-0040): the Mixer tab of the detail panel (strips in the arrangement's order and folding, returns after, the master pinned with the loudness column, strips dragged to reorder, sections folding in a short panel); meters inside the headers' volume sliders. Detachable mixer windows are S (M10). MT-MIX-5 passed | 4 |
| Routing matrix; master mono-check and dim | S | 🔲 | 4 |
| Reference-track A/B | L | 🔲 | — |

## §6.8 Plugins and devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Non-blocking, crash-safe, cached scanning; blocklist; rescan; custom paths | M | 🟡 Built in M6 slice 1 (ADR-0068); MT-VST-1 passed on Windows with real plugins (owner-reported) | 6 |
| Multi-output instruments, sidechain inputs, latency and tail handling, bypass | M | 🟡 Built in M6 slice 6 (ADR-0078); MT-VST-6 is the owner's | 6 |
| Full state save and restore; preset browsing, loading and saving | M | 🟡 Built in M6 slices 2 and 5 (ADR-0071, 0077): state saved with the project and read from every running plugin before a save; programs and `.vstpreset` files; MT-VST-5 is the owner's | 6 |
| Every parameter automatable and MIDI-mappable; generic parameter UI | M | 🟡 The generic parameter UI (the card's strip and the All parameters panel) and plugin-side edits as undo steps are built (M6 slice 4, ADR-0076); automation and MIDI learn are M7 | 6–7 |
| Floating editors: remembered position and size, resizing, high-DPI, keyboard focus | M | 🟡 Built in M6 slice 4f (ADR-0067, 0076): a host-owned window per plugin, its place kept per plugin, hidden with its track, computer-keyboard notes pass through; the real window is verified with a few real plugins by the owner (MT-VST-0 and 4), not on macOS or Linux | 6 |
| Crash and hang isolation with recovery; separate-process isolation by 1.0 | M | 🟡 Separate process per plugin, recovery by Reload from the last good state (M6 slices 0 to 5, 7b); the quality bar's measurements with real plugins are MT-VST-7 | 6, 10 |
| Device chain: cards, bypass, presets, parameter strip, reorder, copy between tracks, ordering rules | M | 🟡 Built in M6 slices 2 to 5: cards, bypass, add, remove, ordering rules, reorder, copy and paste, fold, the parameter strip, the preset menu; MT-VST-2 to 5 | 6 |
| Built-ins: utility, parametric EQ with analyzer, compressor with sidechain, limiter, delay, reverb, polysynth, sampler | M | 🟡 The polysynth exists as a fixed-preset instrument (M5 slice 1, ADR-0045: 16 voices, five presets, no knobs yet); the rest in M8 | 8 |
| Spectrum analyzer, tuner | S | 🔲 | 8 |
| Device browser: categories, vendors, search, favourites, recents, drag, double-click | M | 🟡 Built in M6 slices 1 and 2d (ADR-0068, 0072): vendors, type and category filters, search, favourites, recents, drag to a track or the chain, double-click to add | 6 |
| CLAP hosting; racks with macros | L | 🔲 | — |
| VST3 scanning and hosting on macOS and Linux, with the isolation self-test in CI on all platforms (owner, 2026-10-06) | L | 🔲 | Stretch |

## §6.9 Audio and MIDI devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Driver types: ASIO and WASAPI shared (Windows), CoreAudio, PipeWire/JACK/ALSA | M | 🟡 Output works on ASIO, WASAPI shared (hardware-tested) and cpal (CoreAudio/ALSA, compiled but not hardware-tested). Missing: WASAPI and cpal input (slice 8), JACK/PipeWire (ADR-0016) | 2 |
| WASAPI exclusive | S | 🟡 Output tested on hardware: 3 ms on HDMI with events; the Focusrite switches to polling automatically (MT-AUDIO-4, -5). Input in slice 8 | 2 |
| Device, rate, buffer selection with input/output/round-trip latency display | M | ✅ Audio settings window (M2 slice 8, ADR-0021): driver, device, rate, buffer and live latencies. ASIO buffer sizes are set in the driver's control panel; WASAPI shared follows Windows. MT-APP-6 passes | 2 |
| Device loss and return: keep the session, show the lost state, reconnect automatically (found by MT-ASIO-5) | M | ✅ Detection and auto-reconnect, tested with the fake backend and on hardware (MT-AUDIO-1, -4); lost, busy and failed banners with a way forward (slice 8) | 2 |
| Channel selection with friendly names; mono and stereo configurations | M | 🟡 Output pairs and ASIO input pairs by name (mono for an odd last channel), one pair at a time; more routing with the mixer (M4) | 2 |
| Audio input on WASAPI, CoreAudio and Linux, with drift compensation between separate devices (owner decision 2026-10-06: deferred from M2 slice 8; ASIO input works in M2) | M | 🔲 | 9 |
| Test tone, CPU load, dropout counter, Audio On/Off | M | ✅ All four in the audio panel (M2 slice 7, ADR-0020); test tone also in the audio settings window (slice 8) | 2 |
| ASIO control panel; driver resets and rate/buffer changes without losing the session; busy/missing messages | M | ✅ Control panel button; resets (MT-AUDIO-2); busy, lost and failed banners with Try again / Audio settings; missing saved device falls back with a notice; a device that fails to start returns to the previous one. MT-APP-6 passes | 2 |
| Runtime changes never crash; restore a working configuration; persisted settings | M | ✅ Driver and device switches at run time move the engine to a fresh control thread; refused changes revert with the reason; settings saved atomically once a setup works. MT-APP-6 passes | 2 |
| MIDI devices: enable toggles, hot-plug, per-track MIDI From/To, virtual ports | M | 🟡 Device layer and the MIDI page of the settings are built (M5 slice 0, ADR-0044): inputs and outputs listed, Track and Remote switches saved, hot-plug and replug, opened off the UI thread, a busy port says so. **MIDI To** per track is built (slice 1, ADR-0045: a port and channel from the track menu, timed against the audio clock, outputs switched on automatically; tested on the fake backend, hardware timing unverified). **MIDI From** per track (all inputs, the computer keyboard, or a port and channel) and Monitor In/Auto/Off are built (slice 2, ADR-0046), with the MIDI chip's popover for quick switches; virtual ports are not built (WinMM has none; recorded in KNOWN_ISSUES) | 5 |

## §6.10 Projects, media and export

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| New, open, save, save as, copy, template, revert, recent, unsaved indicator | M | 🟡 | 3, 9 | New, open, save, save as, open recent, unsaved dot in the title, Save / Don't save / Cancel before closing (M3 slice 3c). Save a copy, templates and revert in M9 |
| Human-friendly, versioned format that opens old projects forever; relative paths | M | ✅ | 1, 3 | Format v2 with a migration from v1 and golden fixtures for both; media paths relative to the project folder (M3 slice 3) |
| Autosave, recovery, rolling backups, collect and save, relink, media pool with purge | M | 🟡 | 3, 9 | Autosave, recovery and rolling backups done (M3 slice 3); missing media is reported on open. Collect and save, relink and the media pool in M9 |
| Export: project, loop or selection; master or stems; WAV 16/24/32f, FLAC; SRC; dither; normalize; tails; offline or real-time; progress and cancel; loudness report | M | 🔲 | 9 |
| MP3 and AAC export | L | 🔲 | — |
| Project settings | M | 🔲 | 9 |
| Project notes | S | 🔲 | 9 |

## §7 Standard DAW checklist

| Item | Tier | Status |
|---|---|---|
| Transport: play, stop, record, loop, return to start, go to start/end, previous/next marker. **Owner request:** transport controls use vector icons, not text (M2). | M | 🟡 Engine transport done (slice 3, ADR-0017). Transport bar with vector icons, Space / Shift+Space / Home, go to start, stop, play/pause and loop in the app (slice 4, ADR-0018). Recording MIDI (F9, the Record button, Overdub) in M5 slice 6 (ADR-0055); audio recording in M9; markers and go to end in M3 |
| Tempo 20–999 BPM and time signature with change maps; tap tempo | M | 🟡 Tempo and time-signature maps with changes (`lumen-core`), swapped into the engine live (slice 3). Tap tempo and the UI later |
| Metronome: accent, volume, sound, count-in, pre-roll, record-only, output routing | M | 🟡 On/off, accented downbeat, volume and two synthesized sounds, sample-accurate (M2 slice 7, ADR-0020). Output routing: Main or Cue from its menu, never through the master (M4 slice 4, ADR-0038). Count-in (Off, 1, 2 or 4 bars) and record-only for MIDI recording built in M5 slice 6 (ADR-0055); pre-roll is not built and comes with audio recording (M9) |
| Loop region, punch in/out, auto-punch | M | 🟡 Sample-accurate loop wrap and punch-region reporting in the engine (M2 slice 3). M3 slice 5c: the loop brace is edited on the strip (draw, move, resize, snapped; click toggles), undoable; punch in and out at the loop's edges, from the loop menus. Recording through punch in M9 |
| Markers with names and navigation | M | ✅ 2026-10-08 M3 slice 5d: named locators on the strip, set or removed at the playhead (M, Set), jumped between (Ctrl+Left/Right, ◂ ▸, click a flag), dragged (snapped), renamed (double-click, F2), deleted; undoable, saved (format v4) |
| Multiple time formats; editable position field | M | 🟡 Time formats in M3 5b (`timefmt.rs`: bars and beats, minutes and seconds, samples, timecode), and a Go to position field in the transport bar |
| Edit cursor separate from playhead; scrubbing. **Owner request (2026-10-06), raised from S to M, arrangement-focused:** click in the scrub strip to set the playback position; drag to scrub audibly; drag in the beat ruler vertically to zoom and horizontally to scroll. Locate-on-click in M2; scrub strip, audible scrubbing and ruler zoom in M3. | M | 🟡 Click-to-locate on the ruler done (M2 slice 5), snapping to the editing grid (M3 5a). Ruler drag: down or up zooms, sideways scrolls, the grabbed beat stays under the pointer (M3 5b). M3 5e: the scrub strip (the strip's top, around the locator flags): click jumps (snapped); drag plays the grid step under the pointer in a loop at normal pitch, Alt scrubs like tape (speed and pitch follow the hand); while playing, a drag jumps playback. An engine scrub mode, real-time safe (ADR-0030). An edit cursor separate from the playhead is still to come |
| MIDI clock / MTC out | S | 🟡 Clock out built in M5 slice 7 (ADR-0062; MT-MIDI-8 waits); MTC out deferred, unscheduled |
| Clock / MTC in, network tempo synchronization | L | 🔲 |
| Tracks: add, delete, duplicate, rename, recolour, reorder, multi-select | M | 🟡 All of these from the track menu, the "+ Add track" menu button and keys (Ctrl+T, Ctrl+Shift+T, Ctrl+D, Delete, F2, Alt+arrows), each one undo step (M3 slice 4a). Drag to reorder (slice 4d) |
| Track types with routing, arm, monitor, polarity | M | 🔲 |
| Freeze, unfreeze, flatten | M | 🔲 Planned in M7b (slice E5) |
| Track delay, lock, icons, templates | S | 🔲 |
| Select, marquee, range; cut/copy/paste/duplicate/delete/ripple delete/insert silence | M | 🟡 M3 6a: click, Ctrl and Shift selection, marquee, time ranges across tracks (a clip's bottom half). M3 6b (`lumen_project::edit`): cut, copy, paste (at the playhead or into a range, overwriting the destination range), duplicate, delete, delete time (ripple, the whole song) and insert time, on clips and ranges, each one undo step, in the lanes' right-click menu. Crop and the remaining range edits come with M7b's split |
| Split, join, consolidate, trim, crop, slip, nudge, move, loop/unloop | M | 🟡 Trim, slip, move and loop-extend (M3 6a to 6c). Split (with the drag-to-cut gesture the owner asked for), join, consolidate, crop, unloop into copies and clip nudge in M7b |
| Fades, crossfades, clip gain, reverse, normalize, polarity | M | 🟡 Fades with free curves and four presets, and clip gain (M3 6d, ADR-0031). Moved clips crossfade while playing (5 ms, engine). Automatic crossfades (M7b, model chosen at X1), reverse, normalize and polarity in M7b |
| Snap and grid; zoom; clip name/colour/mute/lock; clip properties | M | ✅ | Grid and snapping (M3 5a) and zoom (4d) done; clip name, colour, mute, lock and the properties card done in M3 6e (ADR-0032; MT-APP-14 passed in run 4) |
| Tool model: pointer, draw, erase, split, mute and zoom, with single-key switching and temporary modifiers (CLAUDE.md §5). Not built, and in no milestone plan yet; planned for M7b with split | M | 🔲 M7b slice E0; keys follow the CLAUDE.md §5 convention (B, Esc, Ctrl+E, 0, Z and X, Del), chosen 2026-10-19 (K1) |
| Warp, transient and tempo detection, pitch shift | S | 🔲 |
| Take lanes and comping | S | 🔲 |
| Unlimited undo/redo with visible history | M | 🟡 Undo and redo for imports, tempo and time signature (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y), a History window, and a note after each undo (M3 slice 3, ADR-0024). Every later edit joins it as it is built |
| Audio recording with monitoring, multitrack, input meters | M | 🔲 |
| Latency compensation with manual offset; crash-safe capture | M | 🟡 MIDI recording takes off the output latency automatically and commits takes about once a second (M5 slice 6, ADR-0055); the manual offset and audio capture are M9 |
| MIDI recording: overdub/replace, count-in, punch | M | 🟡 Built in M5 slice 6 (ADR-0055): Record (F9 or the button) on armed MIDI tracks, a count-in of 1, 2 or 4 bars, Replace and Overdub, punch from the loop brace's switches, loop passes that layer, commits about once a second, one undo step per take, and autosave every 10 s while recording. Real-controller timing is unverified: MT-MIDI-7 waits for the owner |
| Computer keyboard as MIDI input | M | 🟡 Built in M5 slice 2 (ADR-0046): the computer-keyboard MIDI layout in a mode (Ctrl+Shift+K), octave and velocity keys, lit transport button and readout; E2E hears it. MT-MIDI-3 waits |
| Loop recording into takes; capture MIDI | S | 🟡 Loop passes layer into one MIDI clip (M5 slice 6, ADR-0055); takes and Capture MIDI are M9 |
| Piano roll, velocity and controller lanes, quantize, humanize, transpose, legato | M | 🟡 Built: the piano roll (slice 3, ADR-0047), the velocity and controller lanes (slice 4, ADR-0050) and quantize with humanize (slice 5, ADR-0054). MT-MIDI-4, MT-MIDI-5 and MT-MIDI-6 passed 2026-10-12; the row stays 🟡 until the rest of M5 is done |
| MIDI thru, channel filter, panic, sustain/bend/aftertouch, program/bank | M | 🟡 Panic, sustain, bend, pressure and program done (slice 1); thru (to the instrument or the MIDI To port) and the channel filter in MIDI From done (slice 2, ADR-0046). Real-controller timing unverified (MT-MIDI-3) |
| SMF import/export; MIDI learn | M | 🟡 SMF built in M5 slice 7 (ADR-0062; MT-MIDI-8 waits); MIDI learn moved to M7 (owner, 2026-10-13) |
| Drum maps, drum editor, step sequencer | M | 🔲 |
| MIDI effects; on-screen keyboard | S | 🔲 |
| MPE, MIDI 2.0 | L | 🔲 |
| Automation: all parameters, curves, R/W/T/L, clip envelopes, tempo and signature, override, simplification | M | 🟡 Lanes, curves and steps play for the mixer (M7 A0), and point simplification is in the model; a lane can be shown, and points added, moved and deleted, from the mouse and the keys (A1, ADR-0082), and edited as sets with shapes, drawing, copy and paste (A2, ADR-0083); a lane drives a hosted plugin's parameter, chosen from the plugin's own list (A3, ADR-0084); controls follow their lane and a moved control takes its lane over, with Re-enable (A4a, ADR-0086); recording modes are A4b and the rest is A5 to A7 |
| Mixer: fader, pan law, mute, solo, sends, returns, groups, master, routing, sidechain, PDC, meters, cue | M | 🟡 The engine graph (M4 slice 1, ADR-0035); routing in the model (slice 2); groups (slice 3); mute, solo, sends, returns, the master and cue (slice 4, ADR-0038); the Mixer tab, meters, the pan law and output pairs (slice 5, ADR-0040); sidechains and delay compensation in the engine with the transport's readout (slice 6, ADR-0041) |
| LUFS and true-peak metering; mono-check and dim | S | 🟡 LUFS (momentary, short-term, integrated) and true peak on the master (M4 slice 5, ADR-0040); mono-check and dim later |
| Reference track | L | 🔲 |
| Async cached VST3 scanning, custom paths, blocklist, search, favourites, recents | M | 🟡 Built and tested on Linux with the test plugin (M6 slice 1, ADR-0068); MT-VST-1 passed on Windows with real plugins (owner-reported, no measurements); not run on macOS |
| Instruments and effects; multi-out; sidechain; bypass; presets; state; automation; generic UI; floating editors with DPI and focus | M | 🟡 Built (M6 slice 6, ADR-0078; format v21, protocol version 8): the host lists and turns on a plugin's audio buses (a block carries up to 64 channels); a sidechain from any track at three taps with a row and a light on the effect's card; a plugin instrument's outputs as tracks of their own (the card's Outputs row); a plugin instrument's latency is compensated, effects flush on a locate and a scrub is not late. Verified on Windows with the test plugin through real host processes, a real sampler's outputs and a real orchestra plugin's buses; MT-VST-6 (real plugins) is the owner's; not run: smoke, WSL gates, macOS |
| Latency compensation; crash isolation; safe mode | M | 🟡 A plugin instrument's reported latency is compensated like an effect's, a locate flushes effect tails, a scrub is not late (M6 slice 6, ADR-0078); crash isolation and safe mode are built (slices 0 and 5); MT-VST-6 is the owner's |
| Built-in essentials | M | 🔲 |
| Spectrum analyzer, tuner | S | 🔲 |
| CLAP, racks with macros | L | 🔲 |
| Browser: devices, samples, projects, user places; search; audition; drag and drop | M | 🟡 Devices tab with search, type, category and favorites (M6 slice 1); adding a device to a track is slice 2; samples, projects, places, audition and drag and drop are later |
| Tags and favourites; tempo-synced audition | S | 🟡 Favorites on devices (M6 slice 1); tags and audition are later |
| Media pool with relink and purge | M | 🔲 |
| Project management: new/open/save/as/copy/template/revert/recent; autosave and recovery; backups; collect; relink; migrations; settings | M | 🔲 |
| Project notes and metadata | S | 🔲 |
| Export: master and stems; WAV/FLAC; bit depth; dither; offline and real-time; scope; tails; normalize | M | 🔲 |
| MP3/AAC | L | 🔲 |
| Preferences: audio, MIDI, plugin paths, file/cache locations, theme, scale, autosave interval, undo depth, track defaults, record settings | M | 🟡 Audio settings window (M2); theme, interface size, Reduce motion and copy-imported-audio in the app menu (M3). The Preferences window in M9 |
| Shortcut editor with conflict detection; default keymap | M | 🟡 Built in M3 7c (ADR-0033): the Keyboard shortcuts window (keyboard map by area with modifier layers over the list), clashes with Reassign or Cancel, reset one or all, rebinding from the palette, saved in the settings. MT-APP-15 passed |
| alternative keymap preset | L | 🔲 |
| First-run wizard (audio device, plugin paths) | M | 🔲 |
| Preferences import/export; localization-ready text | S | 🔲 |
| Command palette; tooltips with shortcuts; context menus everywhere | M | 🟡 Built in M3 7b and 7d (ADR-0033): the palette (fuzzy search, context names, reasons, typed values), every menu and tooltip shortcut drawn from the keymap as keycaps, right-click menus on the rulers and track names added. MT-APP-15 passed |
| Design system with a crisp vector icon set (spec §5): transport icons (play, pause, stop, record, loop, metronome) in M2, full set with the design system in M3 | M | 🟡 Design system v1 (`docs/DESIGN.md`, `theme.rs`): colour tokens for both themes, type, spacing, shape, widget states, key flows; M2 UI moved onto it (M3 slice 2). Icons grow with each slice's new controls |
| Motion system: tokens, easing, what animates, reduce-motion; never delays input, no idle redraws; M2 widgets retrofitted (owner request 2026-10-06: fluid feel). Built in M3 with the design system; final polish pass in M10 | M | 🟡 Built (M3 slice 2, `motion.rs`): FAST/BASE/GLIDE tokens, ease-out and critically damped spring, Reduce motion; panels slide, popovers and windows fade, notices and banners animate, transport buttons fade on hover, playhead and start marker glide after a locate, clips fade in. Idle CPU unchanged (0.2%). Owner signed off (MT-APP-7, 2026-10-07); M10 polish |
| Reset to default, type-in, fine adjust, wheel adjust, parameter right-click menus | M | 🟡 Value controls reset on double-click, take typed values, Shift for fine, arrow keys (M3); the latching wheel on every slider, knob, fader and field (the owner's B3, M4 slice 5, ADR-0040); a send's menu switches pre and post fader. Parameter menus for automation and MIDI mapping with M5 and M7 |
| Drag and drop for files, devices, clips, tracks; status-bar hints | M | 🟡 Files from Explorer, with a live indicator of what will be added (M2). Clips and tracks in M3, devices in M6 |
| Resizable, collapsible panels with remembered layout; full screen; high-DPI; light and dark | M | 🟡 Resizable panels that slide open and closed; layout saved with each project; dark, light and match-system themes; interface size 100–200% (M3 slices 2–3). Full screen later |
| Keyboard-only core flows; accessible control names. Shift+Tab settled by the owner (ADR-0015): it moves focus back in a text field or an open window, and toggles the detail panel elsewhere. | M | 🟡 M3 7d: F6 and Shift+F6 move the keyboard between named panes; shortcuts keep working with a control focused (only its own keys stay with it). M3 slice 8 and its review: the exit flow works from the keyboard alone (Import audio…, clips chosen with the arrows); Tab order is transport, browser, detail, arrangement (egui's panel order, recorded in ADR-0033), and F6 goes in reading order. The accessibility pass and the Narrator check in M10 |
| Detachable mixer and editor windows; hover help; welcome screen | S | 🔲 |
| CPU, disk and dropout indicators; Audio On/Off; panic | M | 🟡 CPU (peak per second), dropouts, Audio On/Off and panic done (M2 slice 7). Disk indicator with disk streaming (M9). MIDI all-notes-off joined panic in M5 slice 1: the engine cuts every instrument and sends CC 123 and 120 on every channel of every MIDI output in use, and the app sends them directly too (a palette command and the audio panel button) |
| Crash report file; logging with "reveal logs" | M | 🟡 Crash reports (owner request, M3 6c): a panic on any thread writes what failed, where, the backtrace, the version, the system and the thread to `%LOCALAPPDATA%\Lumen\Crashes`; the next start offers it (Show report, Dismiss). Logging and "reveal logs" later |
| Device hot-unplug, sleep/resume, rate-change handling | M | 🟡 Unplug and replug recover on ASIO and WASAPI (MT-AUDIO-1, -4); driver resets on ASIO (MT-AUDIO-2). Sleep/resume and Windows format and rate changes recover too (MT-AUDIO-6). Banners for these states (slice 8). macOS and Linux untested on hardware |
| Single instance; file association; open by double-click or drop | M | 🟡 `lumen <file>.lumen` opens a project (M3 slice 3c). File association with a project file icon in M9 (per user), system-wide by the installer in M10 |
| App icon and `.lumen` file icon (owner request, 2026-10-07) | S | 🔲 M9, shown as specimens first |
| Opt-in telemetry only | M | ✅ |
| Update checker; licensing/activation | L | 🔲 |
| Windows installer and portable build; third-party licence listing; VST and ASIO notices; SDK compliance | M | 🔲 |
| Notarized macOS app | S | 🔲 |
| Linux packages | L | 🔲 |

---

## Milestone reports

### Milestone 6 — report (2026-10-19)
**Status:** Built, not signed off. Every slice is built and merged into `main`
(slices 0 to 7; the last is PR #38). The owner's Windows runs of MT-VST-3 to
MT-VST-7 are not recorded as passed, so this report names what was verified and
what was not. The owner's approval is still required before M7.

**Shipped** (ADRs 0065 to 0080; formats v17 to v21; host protocol version 8).
- **Slice 0, the host and the audio path.** Deterministic test plugins with
  chosen faults (ADR-0065), a host process per plugin with a bounded versioned
  protocol (ADR-0066), a shared-memory audio path and measurement harness
  (ADR-0067), and the decision that the audio callback may wait a bounded time on
  a host (ADR-0069, **on trial**). The slice 0 report is below.
- **Slice 1, discovery** (ADR-0068). Scanning in a process of its own with a
  deadline, a cache keyed by file stamp, a blocklist with the reason, and the
  browser's Devices tab. MT-VST-1 passed on Windows (owner-reported).
- **Slice 2, a device on a track** (ADRs 0070 to 0072). Hosted instruments and
  effects in the engine with a fallback of the reported latency, the hosting
  manager, and the Devices tab with cards for ready, loading, failed and missing
  states. The slice 2 walkthrough (MT-VST-2) passed (owner-reported, no step detail).
- **Slice 3, chain editing** (ADR-0073). Move, copy, paste, duplicate and fold
  cards; the master takes effects; the plugin's own picture.
- **Slice 4, parameters and editors** (ADR-0076). Parameter lists, the card's strip
  of eight knobs and the All parameters panel, plugin-side edits as undo steps, and
  native editor windows that follow their track.
- **Slice 5, presets and resilient state** (ADR-0077). Programs and preset files
  in one menu, loading as one undo step, state read about every 30 s and before every
  save, failures counted per plugin, and safe mode.
- **Slice 6, production routing** (ADR-0078). Multi-bus plugins, sidechain from any
  track, an instrument's other outputs as tracks, and plugin latency compensated.
- **Slice 7, compatibility and closeout** (ADR-0080). The probe `lumen-host-probe`,
  the fault soak, `lumen --bench-callback`, the combined walkthrough
  `walkthrough_plugins`, and the repository review.
- **Fixes from the owner's runs.** Plugins ring out until quiet (ADR-0074); missed
  plugin blocks (ADR-0075), where the cause was plugins configured for 8,192-frame
  blocks while the engine gives them at most 256; saving reads every running plugin
  first; a plugin's window follows its track; stepped controls on the strip.

**Measured.**
- Owner's machine, MT-VST-B run 1: the block budget used fell from 1.56 of 2.90 ms
  (54%, CPU peak 70%) to 0.31 ms (11%, peak 20 to 24%) once plugins were promised the
  device's block. One run; steps 2 to 6 and 8 and whether pops are gone were not
  reported.
- Hand-over of a block to a host and back: 12 µs median and 34 µs p99 on the owner's
  machine (`lumen-host-bench`). Slice 0's 64-host numbers are in its report below.
- Performance gate after the review's fixes: largest change +0.11% (WSL); the
  `engine_hosted_64` baseline was updated twice, each with its reason recorded.
- Tests at the review's close: 2,207 workspace tests on Windows, 495 E2E on Linux,
  6 smoke tests. Mutation checks are recorded per slice.
- The soak ran five random 90-second runs on one machine (67 to 98 rounds each); it
  found nothing in the product.

**Review.** `lumen-review` ran on 2026-10-18 (`docs/audits/2026-10-18-m6.md`): 7 High,
14 Medium and a list of Low findings. All Highs, Mediums and Lows are fixed or
recorded (the audit's Status table), each with a test that fails without the fix.
The owner's naming rule now reaches the code: a test scans the sources for
workstation names.

**Not verified, and where it goes.**
- **The owner's Windows runs:** MT-VST-3, MT-VST-4's re-check steps, MT-VST-5,
  MT-VST-6 and MT-VST-7. The real Windows editor window is compile-checked only in
  the cloud container; its behavior was seen by the owner in MT-VST-4 run 1.
- **ADR-0069's measurements.** The callback numbers at 64 and 128 frames with 16 and
  64 hosted tracks (MT-VST-7 steps 7 to 9). **Monitoring through an effect waits for
  audio input in M9.** Until both exist the callback's wait stays on trial.
- **Installed plugins.** Twelve anonymous labels (RP-1 to RP-12) have been tried in
  parts; the probe has not been run over the installed library. This is not a
  compatibility claim.
- **Not run:** RealtimeSanitizer after the review's fixes, macOS tests (cross-clippy
  only), Linux and macOS timing and editors.
- **Moved to later milestones:** automation and MIDI learn (M7), built-in devices and
  the sampler (M8), audio recording and export (M9), hosting on macOS and Linux
  (stretch).

**Risks.**
1. The callback's wait is on trial; if the numbers fail, the pipelined path adds one
   block of latency to monitored tracks.
2. One machine and mostly synthetic load. The working budget (64 tracks at 100 µs of
   plugin work) is an assumption, not a promise about real plugins.
3. A plugin the test plugins cannot imitate may behave differently in its editor,
   state or latency. A saved state that fails to restore is reported per plugin.
4. Windows is the only system with timing and editor results.

**Next.** The owner runs MT-VST-7 (or waives items, recorded in DECISIONS), then
approves or rejects M6. Then M7 (automation) and M7b (editing and render), planned in `docs/milestones/M7.md` and
`docs/milestones/M7b.md`.

### Milestone 6, slice 0 — report (2026-10-16)
**Status:** Complete. Slice 0 settled how plugins are hosted before any
device workflow is built: separate host processes, a shared-memory audio
path, and the audio callback's wait (ADR-0069, accepted, **the wait on
trial**). Branches: `m6-s0-test-plugin`, `m6-s0-host-core`,
`m6-s0-shm-transport` (merged) and `m6-s0-audio-path` (this report). M5's
closeout is still pending, as it was when M6 started.

**Shipped.**
- **Deterministic test plugins** (ADR-0065, `lumen-test-plugin`): an
  instrument and an effect that sound the same every time, with faults chosen
  by environment variable (crash, hang, error, NaN, slow, bad state, bad
  buses, bad parameters) and, new, an editor view (`editor`, `editor_hang=MS`).
- **The host** (ADR-0066, `lumen-plugin-host`): OS-loader module loading, one
  instance per process, a bounded versioned protocol, stale-block rejection,
  deadlines on everything, exit classification, parent-death cleanup.
- **The audio path** (ADR-0067): shared memory with one block in flight per
  host, untrusted-region rules on both sides, per-side wake-up settings, the
  host's audio thread, a Windows job object so hosts end with Lumen, and the
  measurement harness `lumen-host-bench`.
- **This branch:** the two heap operations the crash runs reported are fixed
  (the loop asked `ended`, which words a crash in a `String`; it asks
  `has_ended`); the 64-host budget and process grouping measured; a latency
  change proven to reach the loop in the same block; the editor-focus spike
  (`lumen-editor-spike`); and the decision, ADR-0069. The guarded real-time
  tests for the transport run under RealtimeSanitizer in CI.
- **Records:** MT-VST-0 runs 1 to 6 (`docs/manual-tests/M6.md`); ADRs 0065 to
  0067 and 0069; `m6-grid-mmcss-fixed.csv` and the other data files.

**Measured** (Windows 11, 8 cores and 16 threads, Balanced, the test plugin
with a synthetic busy loop; one run per scenario; the machine was in use).
- Wake-up, 90-scenario grid: `event` and `event/spin` zero misses; hybrid 14
  and 12; spin 114 and 86 to 98% of a core per host.
- 64 hosts at 128 frames, 48 kHz, half-period budget 1.333 ms, `event`: 100 µs
  of plugin work per track, p99 0.63 ms, slowest block 0.88 ms, no misses; 200
  µs, p99 1.16 ms, p99.9 1.66 ms, misses varying between runs (0 and 14).
- Grouping (modelled): no gain; past about four tracks per process the
  serialised work passes the budget.
- A crash was seen in 1 block (2.7 ms), a hang in 3 (8 ms); unrelated hosts
  had zero misses; a replacement answered in 5 to 9 ms.
- A latency change is in the answer to the same block. Lumen's side allocates
  nothing, including noticing a crash or a latency change.
- Editors: a hung editor is noticed in 0.6 s; Lumen's window stalls 31 ms
  with an unowned editor and **freezes (16 s over two checks, until the host
  is killed) with one owned across processes**, which is rejected.
- Verified on Windows, and on Linux under the allocation guard and
  RealtimeSanitizer (WSL); macOS type-checks and passes clippy. Owner runs:
  MT-VST-0 "Editors" with one real plugin and MT-VST-1 (slice 1) on Windows,
  both reported passed.

**Deferred, and where it goes.**
- Real chain processes (not needed: the model gives no case); a cap on the
  process count if 64 hosts cost too much memory (not measured).
- A restart request raised by a plugin's controller and a bus-change trigger
  in the test plugin: slices 4 and 6 (the same-block latency news is done).
- The whole callback with the real engine, the audio device and the hosts
  together, **including recording with monitoring through an effect at 64 and
  128 frames**: slice 2 (MT-VST-2). ADR-0069's numbers are the transport only.
- macOS and Linux timing and editors; a second monitor or display scale; more
  than one real plugin; the `moduleinfo.json` fast path (slice 1).

**Risks.**
1. **The callback's wait is on trial** (ADR-0069). If slice 2's measurements
   fail, the pipelined path (one block of extra latency) replaces it, and
   monitored tracks get 2.7 ms heavier at 128 frames.
2. **One machine, a synthetic load, one run each.** At 200 µs of work per
   track the tail passes the budget and results vary, so the working budget
   (64 tracks, 100 µs each) is a documented assumption, not a promise about
   real plugins.
3. **Windows only for timing and editors.** macOS polls instead of a timed
   wait, so the wake-up decision is Windows's until measured there.
4. **Real plugins:** one tried. A real plugin's editor, state or latency may
   behave in ways the test plugin cannot show.

**Next.** Slice 2, the first complete device workflow (a device on a track,
hearing it, bypass, save and reopen), pitching behaviour variants first. It
needs the audio-path ADR, now accepted.

### Milestone 5 — report (2026-10-19)
**Status:** Built, not signed off. All nine slices are built and merged. MT-MIDI-1
and 4 to 6 passed; MT-MIDI-2 and 3 passed functionally, with follow-ups in KNOWN_ISSUES. MT-MIDI-7, 8 and 9 and the WSL gates on the final branch are the
owner's and are not recorded as passed. The owner's approval is still required.

**Shipped** (ADRs 0044 to 0064; formats v11 to v16).
- **Devices and the model** (slice 0). `lumen-midi`, the MIDI settings page, and
  controller, bend, pressure and program events in clips.
- **Hearing MIDI** (slice 1). A sample-exact event path with a note tracker, chase and
  every release case; a built-in synth with five presets; MIDI To for hardware.
- **Playing live** (slice 2). MIDI From, monitoring, the computer keyboard as a MIDI
  keyboard, and the MIDI activity chip.
- **MIDI clips and the piano roll** (slice 3). Notes drawn on the GPU, the clip editor
  in the Clip tab and a pop-out window, and note mute (format v14).
- **Velocity and controller lanes** (slice 4).
- **Quantize and its tools** (slice 5). Grid, modes, strength, swing, a sensitivity
  window, a live-preview popover, humanize, fixed length, scale-constrain, reverse.
- **Recording MIDI** (slice 6). Count-in, Replace and Overdub, loop passes, punch,
  takes committed about once a second, and autosave while recording.
- **MIDI files and clock out** (slice 7). Standard MIDI File import and export at 960
  PPQ and MIDI clock with a delay per output.
- **Closeout** (slice 8). The combined E2E `midi_walkthrough` (record, quantize, save,
  reopen, play) and the repository review (`docs/audits/2026-10-13-m5.md`).
- **Corrections after owner runs.** The clip window model (ADR-0056); overdub and
  seams (ADRs 0057 to 0060); resizing a playing clip (ADR-0061); the recording window
  and punch masking (ADR-0064).

**Measured.**
- With a 100,000-note clip open, frame time p99 is at most 1.82 ms at 1600 × 900 and
  2.22 ms at 3440 × 1380; three idle repaints in three seconds (`--bench-editor`).
- The built-in synth costs 0.36 ms a block (13.5%) with 64 tracks (`engine_synth_64`).
- At the review's close: 1,436 tests, 333 E2E, 6 smoke. The WSL performance gate
  passed all nine scenarios after the recording-window fix (largest +1.50%, limit 2%).

**Review.** Two High findings, an audio-thread free and a UI-thread disk check, are
fixed, along with module splits and bounds on positions in projects and MIDI files.

**Not verified.**
- **MT-MIDI-7** (recording with a physical controller, count-in, overdub, replace, loop
  and punch), **MT-MIDI-8** (a file from another workstation in and out, and clock to
  real hardware) and **MT-MIDI-9** (the closeout run) are the owner's.
- Timing on real MIDI hardware, macOS tests, and the WSL gates on the final M5 branch.

**Moved.** MIDI learn moved to M7 (owner, 2026-10-13). Restore recorded timing,
groove templates and MIDI effects are not built. Manual latency offset waits for M9.

**Risks.** MIDI port identity, overflow note-off loss and edits to a recording track
during a take remain in KNOWN_ISSUES.

### Milestone 4.5 — report (2026-10-10)
**Status:** Built 2026-10-10, a mid milestone between M4 and M5. All six
slices are built and every finding of the interface audit
(`docs/audits/2026-10-10-design.md`, 20 findings) is built; none was
deferred. MT-POL-1 to MT-POL-4 passed (owner). MT-POL-5 (motion) and
MT-POL-6 (the close) wait for the owner, as does MT-APP-18 (light and the
mark, merged from `main`).

**Shipped:** 12 commits on `m4.5-polish` (and one merge from `main`), no format change, nothing heard
differently.
- **Bugs** (slice 1): value fields follow the surface (T1), a `well` token
  for grooves and meters (T2), the scroll kept on the tracks (U1), the
  shots harness repaired (U3).
- **Shared widgets and sizes** (slice 2): one size scale (S4), the slider
  handle that opens near its value (S2, the owner's own variant C), scale
  numbers thinned by priority (S3), switches, segmented controls and
  choice rows in place of egui's checkboxes and radios (T3), menus that
  stay open through changes and fade when left (the owner's C).
- **Chrome** (slice 3): toggles and underlined tabs (T4), the arm grey at
  rest (T5), one empty-state widget and the mixer's ghost strip (S5), the
  clip card's swatches and shortened path (S6), one time-signature field
  (S7).
- **Martian Mono** for numbers (slice 4, S1).
- **Motion** (slice 5): clips glide and undo washes what changed (M1, M4),
  rows grow in, glide and fold shut (M2, M3), fading scroll bars (U2), the
  palette's sliding highlight, cross-fading lit states and windows fading
  out (M5).
- **The close** (slice 6): the design sweep over every changed view, in
  both themes at 100%, 150% and 200%, found two things, both fixed:
  switches, segmented controls and captions sat at a menu's edge, about
  1.5 pt left of its labels (they now line up, `widgets::menu_caption`);
  and the audit's mixer edge fades (U2's second half) had not been built
  (strips cut at an edge now fade into the ground).
- **Merged from `main`:** the CI fixes, macOS back in CI, and the light
  layer and the Dawn mark (ADR-0043); the empty arrangement's empty state
  shows the mark as its icon.

**Automated tests:** 687 in the workspace, including 157 end-to-end
scenarios, all passing, plus 6 real-binary smoke tests. clippy
(`-D warnings`) and rustfmt are clean. New shots: `polish_sweep` and
`mixer_edges` (renders for checking by eye, run on demand).

**Verified by hand:** MT-POL-1 to MT-POL-4 (owner). Not yet: MT-POL-5,
MT-POL-6, MT-APP-18. Screen readers are still unchecked (Narrator
deferred); names and Tab order are tested (E2E `access`).

**Key measurements** (Ryzen 7 7700X, RTX 4070; stress project, 200
tracks and 20,000 clips, Mixer tab open):

| What | Result | Bar |
|---|---|---|
| Frame time p99, scroll time / scroll tracks / zoom, 1600 × 900 | 1.5 / 1.7 / 1.6 ms (M4: 2.4–3.2 ms) | 16.7 ms (60 fps); 8.3 ms target |
| Frame time p99, 3440 × 1380 | 3.6 / 3.6 / 3.3 ms | same |
| Idle repaints | 3 in 3 s, as in M3 and M4 | near-zero idle CPU |

The first run reported 146 idle repaints at 1600 × 900. It was the
benchmark, not the app: idle was counted from 0.1 s after the last scroll,
while the scroll bar still lingered and faded (with vsync off, its fade
alone was 30 to 140 frames). Idle is now counted from 1.5 s after.

**Exit criterion:** *every audit finding built or explicitly deferred;
each slice's manual test passed; idle repaints and frame times unchanged;
no format change.* All met except the two manual tests that wait.

**Not done or deferred:**
- Found by the sweep and recorded (KNOWN_ISSUES): the pinned clip card
  covers windows opened after it, older than M4.5; a fix is to pitch.

**Risks:**
- Motion adds per-frame work while it runs; at rest it costs nothing
  (measured). M5's piano roll will be the next large draw.
- The ASIO SDK agreement is still to be signed (owner action, before any
  distribution).

**Proposed next:** M5, MIDI and the piano roll, in
`docs/milestones/M5.md`, with five choices for the owner (§4).

### Milestone 4 — report (2026-10-10)
**Status:** Complete 2026-10-10. All eight slices are built, and every
manual test passed (MT-APP-17 and MT-MIX-1 to MT-MIX-8, owner). The
review, `docs/audits/2026-10-10-m4.md`, found no High issues. Its four
Medium and eight Low findings are fixed on a follow-up branch, after this
merge, at the owner's request.

**Shipped:** 65 commits on `m4` (ADR-0034 to ADR-0042, formats v7 to v10).
- **The info line** (slice 0, ADR-0034): hints along the bottom of the
  window with K5 keycaps. Only live values follow the pointer.
- **The mixer graph in the engine** (slice 1, ADR-0035): a node per
  track with trim, polarity, mute, sends, volume, pan and output, compiled
  into a plan off the audio thread. Plans swap at a block edge, levels
  glide over 20 ms, and a new shape crossfades over 5 ms.
- **Routing in the model** (slice 2, format v7, ADR-0036): a master track
  in every project, outputs, sends, polarity, trim and the pan law. One
  rule for destinations and loops serves validation, edits and the engine.
- **Group tracks** (slice 3, formats v8 and v9, ADR-0037): Ctrl+G and
  Ctrl+Shift+G, nesting up to three deep, folding, dragging tracks in and
  out, nested rails and lane bars, and a resizable header column.
- **Mute, solo, returns, master and cue** (slice 4, format v10,
  ADR-0038): the owner's 14 solo cases as one pure function. Returns and
  the master are pinned at the bottom, with comet send knobs and a
  return's loudest senders. A pre-fader cue goes to a second output pair.
- **The mixer panel and meters** (slice 5, ADR-0040): meters and
  loudness are measured on the audio thread (peak with hold, clips, RMS,
  gated LUFS and 4× true peak, checked against EBU Tech 3341). Meters sit
  inside the headers' volume sliders. The Mixer tab has console strips, a
  loudness column, output pairs, strips dragged to reorder, the pan law
  menu and the latching wheel.
- **Sidechains and delay compensation** (slice 6, ADR-0041): devices and
  the three sidechain taps in the graph, delay lines that line up
  every path, the engine rendering ahead of the playhead, and the D1
  readout in the transport bar.
- **The exit flow** (slice 7, ADR-0042): E2E `walkthrough_mix` and
  MT-MIX-7. It found that edits made while stopped never reached the
  engine (fixed). Session locks stop a second Lumen from offering a
  running session's autosave. The pan law now applies to mono tracks
  only. Clipboard keys work in the real window.

**Automated tests:** 661 in the workspace, including 144 end-to-end
scenarios, all passing, plus 6 real-binary smoke tests. clippy
(`-D warnings`) and rustfmt are clean on Windows, in the WSL Linux mirror
and for the macOS target. The WSL performance gate passes with every
scenario at ±0.00% against the baseline, which was updated with reasons
in slices 1, 5 and 6.

**Verified by hand** (owner, Focusrite USB on ASIO and WASAPI): MT-APP-17
and MT-MIX-1 to MT-MIX-8, including the dropout count on the Focusrite.

**Not verified:**
- The cue bus on real hardware: the owner's interface has one output
  pair. It is checked on the four-output test device (KNOWN_ISSUES).
- Delay compensation with real plugin latency (M6). Only the synthetic
  latent device exists.
- macOS and Linux by hand, and screen readers (Narrator deferred). Names
  and Tab order are tested.

**Key measurements** (Ryzen 7 7700X, RTX 4070):

| What | Result | Bar |
|---|---|---|
| 64 routed tracks in 8 groups, two sends each | about 1% of a 128-frame block at 48 kHz | under 50% |
| Frame time with the Mixer tab open, stress project (200 tracks, 20,000 clips), p99 | 2.4–3.2 ms (1.5–1.8 ms without the mixer) | 16.7 ms (60 fps); 8.3 ms target |
| Idle repaints, stress project | 3 in 3 s, as in M3 | near-zero idle CPU |
| Performance gate (instructions, WSL) | `engine_routed_64` and `engine_latent_64` added; every scenario ±0.00% at the close | +2% fails CI |

**Exit criterion:** *a multi-track project can be routed through groups,
returns and the master bus, with predictable solo and mute behaviour.*

| Step | Status |
|---|---|
| Automated | ✅ E2E `walkthrough_mix`: a drum group and a vocal through a latent return. Solo and mute are heard at every step, then the project is saved and reopened |
| By hand | ✅ MT-MIX-7 (owner, 2026-10-10) |

**Not done or deferred** (as planned in M4.md §6):
- Plugins and plugin latency → M6. The built-in compressor, reverb and
  delay → M8.
- Automation of mixer controls and the override state → M7.
- Recording, input monitoring and input meters → M9.
- Routing matrix, mono-check and dim [S], detachable mixer windows [S]
  → M10.
- Recorded for later (KNOWN_ISSUES): header drag for returns, sends
  between returns, scrubbing heard late under delay compensation, a
  crowded transport bar at 150%, and browser audition through the cue.

**Risks:**
- A plan swap copies every delay line on the audio thread (review M3).
  Nothing can trigger it before M6, and it is fixed on the review branch.
- The frame budget keeps growing: the mixer added about 1 ms at p99. M5's
  piano roll is the next large draw.
- Four app modules passed the 800-line limit during M4 (review M1). They
  are split before M5 builds on them.
- The ASIO SDK agreement is still to be signed (owner action, before any
  distribution).

**Proposed next:** the review's fixes on their own branch. Then M5, MIDI
and the piano roll, planned in `docs/milestones/M5.md` for the owner's
approval.

### Milestone 3 — report (2026-10-09)
**Status:** Complete; approved by the owner 2026-10-09 (MT-APP-16 passed).
All 8 slices are built. Slices 1 to 7 passed their manual tests
(MT-APP-7 to MT-APP-15). Slice 8 adds the exit flow as an automated test,
an accessibility audit, the end-of-milestone review and its fixes; its
manual test, MT-APP-16, is the exit flow by hand.

**Shipped:** 93 commits on `m3-slice-7` and `m3-slice-8` (from `03e2228`).
- **Performance gates** (slice 1, ADR-0022): five instruction-count
  scenarios under Callgrind fail CI at +2%.
- **Design system and motion** (slice 2, ADR-0023, `docs/DESIGN.md`):
  semantic tokens with dark, light and match-system themes; interface size
  100–200%; Inter and Unbounded; springs and fades with Reduce motion; the
  M2 UI retrofitted.
- **Project model, undo and files** (slice 3, ADR-0024): undo by shared
  snapshots (property-tested), new, open, save, save as, open recent,
  project folders with imported audio copied in, autosave and crash
  recovery. Format v2 to v6, each with a migration and a golden fixture.
- **Tracks and the arrangement at scale** (slice 4, ADR-0025 to 0028):
  the owner-chosen track headers, folding and resizing in 22 pt steps,
  clip bodies and waveforms on the GPU, zoom to single samples, the default
  navigation keys, Follow, dragging tracks to reorder. A smooth playhead
  while playing (the owner found Follow choppy; ADR-0028 amendment).
- **Grid, rulers, loop, locators and scrubbing** (slice 5, ADR-0030):
  adaptive and fixed grids with triplets, ruler layout A with a time ruler
  in three formats, the loop brace and punch as undoable edits, named
  locators, loop and tape scrubbing.
- **Clip editing** (slice 6, ADR-0031, ADR-0032): the split-clip model
  (move from the top half, time ranges from the bottom, marquees in empty
  lanes), cut, copy, paste, duplicate, delete, delete and insert time,
  trim, loop-extend, slip, fades on free curves, clip gain, mute, lock,
  rename, colour, the pinned properties card, files dropped on a chosen
  track or between tracks, 5 ms crossfades when playing clips change.
- **Commands** (slice 7, ADR-0033): one table of 56 commands run by one
  dispatcher; the command palette with context names, reasons and typed
  values; remappable shortcuts with clash handling and a keyboard map;
  menus and tooltips drawn from the keymap; F6 between panes.
- **Exit flow and review** (slice 8): the exit flow as an E2E test, every
  control named for screen readers, and the review's fixes: Import
  audio… (Ctrl+Shift+I), clips chosen with the arrows (the owner's
  choice), the time signature as an undoable method clamped to what a
  project can hold (a 65–99/x meter used to save a song that would not
  open), files dropped together kept in order (the flaky E2E runs), and
  three oversized modules split.
- An end-to-end suite (ADR-0029) that drives the whole app headless in CI
  through its accessibility tree, and real-binary smoke tests.

**Automated tests:** 434 in the workspace, all passing, plus 6 real-binary
smoke tests (passing) and 10 screenshot renders run on demand. By crate:
`lumen-app` 231 (143 unit, 88 end-to-end), `lumen-engine` 55 (with the
allocation-guard tests), `lumen-project` 50, `lumen-core` 30,
`lumen-audio` 25, `lumen-media` 12, `lumen-rt` 9, `lumen-render` 9, and 13
in tools and the M1 prototypes. clippy (`-D warnings`) and rustfmt are clean on Windows,
in the WSL Linux mirror and for the macOS target; the WSL performance
gate passes (engine +0.94% at most).

**Verified by hand** (owner, Focusrite USB on ASIO and WASAPI): MT-APP-7
to MT-APP-16, including the real Import audio dialog, each recorded with its runs in `docs/manual-tests/M3.md`.

**Not verified:**
- macOS and Linux by hand (CI and the cross-target clippy only; macOS CI
  was paused from 2026-10-07 to 2026-10-10). Dropping files on a chosen
  track may not follow the pointer there (KNOWN_ISSUES).
- Screen readers (the Narrator check was deferred by the owner; names and
  focus order are tested).
- A mid-range machine and long sessions (M10).

**Key measurements** (Ryzen 7 7700X, RTX 4070; details in the ADRs):

| What | Result | Bar |
|---|---|---|
| Arrangement frame time, stress project (200 tracks, 20,000 clips, waveforms), p99 scroll time / scroll tracks / zoom | 1600 × 900: 2.2 / 2.0 / 2.3 ms; 3440 × 1380: 3.3–3.7 / 3.8–4.2 / 3.2–3.4 ms | 16.7 ms (60 fps); 8.3 ms target |
| Same at the slice 4 close, 3440 × 1380 | 2.3–3.1 / 2.8–2.9 / 2.2–2.6 ms | — |
| Idle repaints, stress project | 3 in 3 s (the once-a-second device check); one hover card fading in when the pointer rests on a clip | near-zero idle CPU |
| First egui baseline before clips moved to the GPU | 13.4 ms p99 zooming | — |
| Waveform peaks, 5-minute stereo file | 21 ms of a 100–140 ms load | backgrounded |
| Performance gate (instructions, WSL) | engine 64 clips +0.94%, transport +0.35%, project serialize +0.55%, parse −0.23% against the baseline | +2% fails CI |

The frame time grew by about 0.6–0.9 ms at p99 since slice 4 (rulers,
the scrub strip, handles, the grid through clip bodies); still under half
of the 120 fps budget.

**Exit criterion (ROADMAP):** *Create → add track → import audio →
arrange clips → edit → save → close → reopen, with the same project
state.*

| Step | Status |
|---|---|
| Automated | ✅ E2E `walkthrough`: create, add a track, import onto it and below it, drag and duplicate, rename, mute, tempo, a locator and a loop; save, close, reopen: field for field the same project, and the same audio sample for sample |
| From the keyboard alone | ✅ since the review (Import audio…, the arrows between clips); E2E `audio_imports_from_the_keyboard_onto_the_selected_track`, `arrows_move_between_clips_when_a_clip_is_selected` |
| By hand | ✅ MT-APP-16 (owner, 2026-10-09) |

**Not done or deferred:**
- Deferred with owner approval: the waveform peak disk cache and
  relinking missing media → M9; app and file icons and file association
  → M9; the drag-to-cut gesture and the tool model → M7b with split; group
  summing, sends and routing → M4; a UX pass on hints and tooltips →
  scheduled when the next milestones are planned.
- Recorded for later (KNOWN_ISSUES): a second instance offering to recover
  a running one's autosave (single instance, M9); audio clips not
  following tempo (warp); submenu names read with their arrow, the Tab
  order of the panes, the cramped shortcuts window at 200% in the
  smallest window, a hint under the pinned card, and the pipeline cache
  read on the UI thread (M10).
- Undo depth is fixed at 10,000 steps until Preferences (M9).

**Risks:**
- egui: Tab order follows panel declaration, file drags give no pointer
  on Windows (worked around), and upgrades break often. The arrangement's
  dense drawing is on our own GPU path, which limits the exposure.
- The frame budget is used a little more with each slice; the gate is by
  hand (`--bench-frames`) because CI has no GPU. M4's inline meters and
  M5's notes are the next big draws.
- The engine still mixes every clip into one stereo bus; M4 replaces that
  with a routing graph, the largest engine change since M2.
- The ASIO SDK agreement is still to be signed (owner action, before any
  distribution).

**Proposed next:** M4, Mixer, Routing & Groups, planned in
`docs/milestones/M4.md` (a proposal for the owner's approval).

### Milestone 2 — report (2026-10-06)
**Status:** Complete; approved by the owner 2026-10-07. All 8 slices are done, and
every M2 manual test passes on the owner's hardware. One planned item was not
built: the benchmark regression gates (below).

**Shipped:** about 56 commits on `main`.
- `lumen-audio`: a device layer with a supervisor that recovers from device
  loss. A restored device is confirmed before it is reported. Backends:
  native ASIO, WASAPI shared and exclusive (with a device-clock check and a
  polling fallback), and cpal for macOS and Linux. Run-time driver switching
  hands the engine to a new control thread.
- `lumen-engine`: a sample-accurate transport (play from the start marker,
  stop, pause, locate, loop, punch region) with live tempo and meter maps,
  2 ms declick crossfades, the clip player, metronome, test tone and panic.
- `lumen-media`: decoding (WAV, AIFF, FLAC, MP3, Ogg Vorbis) and sample-rate
  conversion, off the audio thread.
- `lumen-app`:
  - transport bar with vector icons;
  - click-to-locate ruler with start marker and playhead;
  - drag-and-drop audio files;
  - audio panel (CPU, dropouts, Audio On/Off, panic);
  - audio settings window;
  - device banners;
  - saved settings;
  - ASIO driver checks in a helper process.
- RealtimeSanitizer in Linux CI with live canaries (allocation, file open,
  held lock).
- Docs: ADR-0016 to ADR-0021, manual tests MT-AUDIO-1 to -6, MT-ENGINE-1,
  MT-APP-2 to -6 and MT-MEDIA-1, and third-party notices (symphonia is
  MPL-2.0).

**Automated tests:** 148 in the workspace, all passing. By crate:
`lumen-engine` 37, `lumen-audio` 24, `lumen-app` 24, `lumen-project` 22,
`lumen-core` 14, `lumen-rt` 9, `lumen-media` 6, and the M1 prototypes. They
include golden renders at block sizes 1–4096, zero heap operations over
thousands of mixed-command blocks, and fake-backend tests for loss, restore,
starvation and dropouts. clippy (`-D warnings`), rustfmt and cargo-deny are
clean.

**Verified on hardware** (Focusrite USB, NVIDIA HDMI; owner and agent):
- unplug and replug on ASIO and WASAPI;
- driver resets from the ASIO control panel;
- sleep and resume;
- Windows format and rate changes;
- transport, loop and stop by ear (no pops);
- file playback;
- metronome, test tone, Audio On/Off and panic;
- audio settings, including a driver with no hardware attached.

**Not verified:**
- macOS and Linux audio on real hardware (CI compiles and unit-tests it;
  stretch milestone);
- WASAPI and cpal input (deferred to M9, owner decision);
- a mid-range machine;
- long-session soak testing (M10).

**Key measurements** (Ryzen 7 7700X; details in the ADRs and manual tests):

| What | Result | Bar |
|---|---|---|
| 64 stereo clips at 128 frames / 48 kHz | 25 µs median = 0.9% of the 2.67 ms budget | < 50% |
| App idle CPU (owner, MT-APP-4) | 0.0–0.2% | near zero |
| Output latency, Focusrite ASIO 128 / 48 kHz | 8.3 ms (397 samples, as the driver reports) | no added buffering |
| Round trip, Focusrite ASIO 256 / 48 kHz | 25.9 ms | — |
| WASAPI exclusive, NVIDIA HDMI | 144 frames (3 ms), 331 callbacks/s of 333 | — |
| Recovery after replug | ASIO about 2 s; WASAPI exclusive about 4 s | automatic |
| ASIO rate change, Focusrite | about 1.3 s, of which Lumen about 20 ms (the rest is the driver re-locking its clock) | never crash; progress shown |
| ASIO driver check in a helper | 115–206 ms per driver | never block or crash |
| Dropouts on NVIDIA HDMI, shared and exclusive | 0 | 0 |
| Audio file ready after drop (5 s file) | 38 ms | backgrounded |

**Exit criteria (ROADMAP):**

| Criterion | Status |
|---|---|
| Lumen opens an audio device | ✅ ASIO and WASAPI, chosen in the app and remembered |
| Plays audio at low latency | ✅ 128 frames on ASIO; 3 ms WASAPI exclusive |
| Stops safely | ✅ declicked stop, pause and locate; panic |
| Recovers from device changes | ✅ unplug, reset, sleep, rate changes, failed switches |
| Stable transport behaviour | ✅ sample-accurate at every block size (tests) and by ear |

**Not done or deferred:**
- **Benchmark regression gates** (ADR-0011 planned them for M2): not built.
  CI runs every benchmark once as a smoke test only. Proposed as M3 slice 1.
- Deferred with owner approval:
  - disk streaming, WASAPI and cpal input, count-in and record-only → M9;
  - metronome output routing → M4;
  - motion system and the retrofit of the M2 UI → M3;
  - AAC/M4A → pending a licensing check.

**CI:** macOS and Linux were red from slice 8 part 3 until the close: a
helper function used only on Windows was dead code there, and clippy denies
it. Fixed after the report. Once the tests ran again, they exposed a real
bug: backups saved within the same millisecond could be pruned in the wrong
order. That is fixed too, with a test that forces the collision. Linux
clippy and tests now run in the WSL mirror before pushing platform-specific
code.

**Risks:**
- A 64-track project has not yet run on mid-range hardware. M2 measured the
  engine, not a full project.
- Clip audio is held in memory until M9; large sessions will need streaming.
- The ASIO SDK agreement is still to be signed (owner action); it gates
  distribution. Lumen's licence is GPL-3.0-or-later (ADR-0039, supersedes ADR-0012).
- egui upgrades break every couple of months; the M3 arrangement grows the
  UI surface they can break.
- Other ASIO drivers may behave differently from the Focusrite's; only the
  Focusrite, ASIO4ALL and FlexASIO have been exercised.

### Milestone 1 — interim report (2026-10-05)
**Status:** Prototyping complete, paused for decisions. Not closed: two exit
criteria are only partly met (table below).

**Shipped:** 8 commits on `main`.
- Cargo workspace (Rust 1.99.0, edition 2024); CI workflow (Windows MSVC,
  macOS, Linux: rustfmt, clippy with warnings denied, tests, benchmark smoke
  runs, cargo-deny, VST3 scanner self-test); permissive-only licence policy.
- Production seeds: `lumen-core` (product name, time base), `lumen-rt`
  (real-time allocation guard), `lumen-project` (versioned, crash-safe
  project format).
- Prototypes: `engine-core`, `asio-probe`, `isolation-ipc`, `vst3-scan` with
  `vst3-test-module`, `ui-stress`.
- Docs: M1 plan and risk register, 14 ADRs with evidence, this tracker,
  manual tests, plugin compatibility matrix (scan stage), architecture
  draft, developer setup.

**Automated tests (all pass locally, GNU toolchain):** 47 tests: `lumen-core` 9,
`lumen-rt` 8, `lumen-project` 22, `engine-core` 5, `vst3-scan` 3. Every
benchmark also runs once under `cargo test --all-targets`. The VST3 isolation
self-test passes (3 checks). `cargo-deny check` passes advisories, bans,
licences and sources. clippy (`-D warnings`) and rustfmt are clean.

**Verified on this machine with real hardware and plugins:** sandboxed query
of 7 ASIO drivers; ASIO streaming on a Focusrite interface (128 frames,
2 × 30 s) and on ASIO4ALL (64 frames); the isolation matrix with crash and
hang injection; scanning of 40 real VST3 modules; the UI benchmark; the
accessibility tree through UI Automation.

**Not verified:** ~~CI, MSVC, macOS and Linux~~ (all green in the first CI run on
2026-10-06; no local MSVC build yet); tests that need a person (MT-ASIO-4/5/6,
MT-ISO-2, MT-UI-2 with Narrator, MT-UI-3 visual quality); mid-range hardware.

**Key measurements** (Ryzen 7 7700X, RTX 4070; details in the ADRs):

| What | Result | Bar |
|---|---|---|
| Engine, 64 tracks × 128 frames (hot benchmark) | 22.7 µs = 0.85% of budget | < 50% (whole project) |
| Same load inside the real Focusrite ASIO callback | p50 34 µs, p99 61 µs, max 301 µs (11.3%) | < 50% |
| ASIO streaming, 30 s | 0 discontinuities, 0 real-time heap operations | no dropouts |
| Plugin host round trip, 1 / 8 / 16 hosts (128 frames) | p99 19.5 / 35.5 / 53.3 µs | p99 ≤ 50 µs (1), ≤ 150 µs (8) |
| Crash / hang in a plugin host | detected same block / in 5.3 ms; back in service 10.7 / 13.3 ms; audio uninterrupted | survive without stalling |
| VST3 scan, 40 real modules | 39 enumerated, 1 cleanly classified failure, 10.4 s wall time | never crash or block |
| Project save / load, quality-bar size | 83 ms / 87 ms (12 MB) | < 300 ms |
| UI, normal editing views (p99 CPU per frame) | ≤ 6.1 ms | ≤ 8.3 ms (120 fps) |
| UI, 100k-note overview: egui shapes / GPU instanced | 20.0 ms / 5.7 ms | ≤ 8.3 ms |
| UI idle CPU; first frame | 0.00%; 545 ms warm (1,007 ms cold) | ≈ 0; < 2 s usable |

**Exit criteria (ROADMAP):**

| Criterion | Status |
|---|---|
| A minimal application launches | ✅ `lumen` shell launches (slice 11, 2026-10-06) |
| UI approach validated | ✅ P-UI, with the GPU escape hatch proven; awaiting approval |
| Audio architecture proven with a prototype | ✅ Engine core plus native ASIO on real hardware. Other backends are M2. |
| Project format saves and loads a minimal project | ✅ |
| VST3 and isolation approach demonstrated | ✅ Real plugin (Test instrument H) processes audio in an isolated host process, survives a crash and is restored from its last known state (P-HOST, 2026-10-06) |

**Deferred, proposed to finish M1 after approval:** slice 9 (P-HOST), slice
10 (application shell), the first CI run, and a local MSVC build.

**Risks:**
- Plugin editor windows across processes (R12) are untested.
- The POSIX isolation transport and the macOS/Linux audio backends are
  unproven.
- 120 fps margins were measured on a high-end machine.
- egui ships breaking releases about every two months.
- ASIO licensing is undecided.
- Local GNU and CI MSVC builds differ.
