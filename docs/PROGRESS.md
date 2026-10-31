# Lumen — Progress

Every requirement of `CLAUDE.md` with its tier and status (feature tracking,
below) and every milestone's report (at the end). Plans and slice lists are in
`docs/milestones/`, the reasoning in `docs/DECISIONS.md`, by-hand test runs in
`docs/manual-tests/`, open problems in `docs/KNOWN_ISSUES.md`.

## Now (2026-10-29)

M7 waits for the owner (MT-EDIT-2, MT-EDIT-7, MT-UX-2, then the approval); its
planned E7 (audio clip editor) and E8 (warp and time-stretch) start when the
owner chooses. M9 continues with R7 to R9; R6 is merged and R2b (the loopback helper, ADR-0110) is built on its branch. Details: the table below
and `CLAUDE.md` §12.

Statuses: 🔲 Not started · 🟡 In progress · ✅ Complete · ⚠️ Partial / known limitation · ⏸️ Deferred with approval · ❌ Blocked

## Milestones

| # | Milestone | Status | Plan and notes |
|---|---|---|---|
| 1 | Foundations & Architecture | ✅ Complete 2026-10-06 | [M1](milestones/M1.md) |
| 2 | Audio Engine, Devices & Transport | ✅ Approved 2026-10-07 | [M2](milestones/M2.md) |
| 3 | Arrangement & Project System | ✅ Approved 2026-10-09 | [M3](milestones/M3.md) |
| 4 | Mixer, Routing & Groups | ✅ Complete 2026-10-10 (review fixes, PR #3) | [M4](milestones/M4.md) |
| 4.5 | Polish pass (the interface audit's fixes) | ✅ Built 2026-10-10; MT-POL-1 to 6 passed (owner, 2026-10-10 and 2026-10-12); MT-APP-18 has no recorded run | [M4.5](milestones/M4.5.md); audit `docs/audits/2026-10-10-design.md` |
| 5 | MIDI & Piano Roll | ✅ Signed off 2026-10-26; MT-MIDI-7 to 9 out of scope for now (owner) | [M5](milestones/M5.md) |
| 6 | VST3 Hosting & Device System | ✅ Signed off 2026-10-26; MT-VST-3 to 7 and the ADR-0069 callback numbers are out of scope for now (owner), so the callback's wait stays on trial | [M6](milestones/M6.md); review `docs/audits/2026-10-18-m6.md` |
| 7 | Automation, Editing & Render | 🟡 A0 to A8 and E0 to E6 merged (ADR-0081 to 0098; formats v22 to v29); MT-EDIT-2, MT-EDIT-7 and MT-UX-2 and the approval wait; E7 and E8 not started | [M7](milestones/M7.md); reviews `docs/audits/2026-10-22-m7.md`, `docs/audits/2026-10-24-m7b.md` |
| 9 | Recording, Media, Export & Production Workflow | 🟡 R0 to R5, R1b and R4b merged (ADR-0099 to 0107; formats v30 to v32); MT-EXPORT-1 waits; R6 merged (PR #74, ADR-0109), MT-STREAM-1 passed; R2b (loopback helper, ADR-0110) built on its branch, MT-REC-5 waits; R7 to R9 not started | Before M8 (owner, 2026-10-23). [M9](milestones/M9.md) |
| 8 | Drum Maps, Drum Editor & Built-In Devices | 🔲 | After M9 (owner, 2026-10-23) |
| 10 | Hardening, Performance & Release | 🔲 | |
| S | Stretch goals (only with the owner's approval) | 🟡 | VST3 hosting: macOS approved 2026-10-19 and in progress ([plan](milestones/macOS-VST3.md), ADR-0085); Linux not scheduled |

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
| Sample-accurate playback, MIDI, automation, loops, editing | M | 🟡 | 2–7 | Time base done (`lumen_core::time`, ADR-0009): every sample addressable at 999 BPM / 192 kHz. Transport and loops sample-accurate for every block size (`lumen-engine`, slice 3); MIDI events (M5) and automation (M7) on their exact sample |
| Offline render equals real-time and is repeatable | M | 🔲 | 2, 9 | P-ENGINE block-size invariance |
| UI 60 fps minimum, 120 fps target at 200 tracks / 20,000 clips / 100,000 notes | M | 🟡 | 3, 10 | Clips: met. With clips on the GPU (M3 slice 4b, ADR-0026), the stress project scrolls and zooms at p99 ≤ 2.3 ms at 3440 × 1380 (Ryzen 7 7700X, RTX 4070), measured by `--bench-frames`. With waveforms on every audio clip (4c, ADR-0027): p99 ≤ 2.3 ms at 3440 × 1380, unchanged. At the M3 close (rulers, handles, the grid through clips): p99 ≤ 2.3 ms at 1600 × 900 and 3.3–4.2 ms at 3440 × 1380, inside the 8.3 ms of 120 fps. Notes: with the 100,000-note track, p99 ≤ 4.5 ms at 3440 × 1380 (M5 slice 3). Mid-range hardware in M10 |
| Near-zero idle CPU | M | 🟡 | 3, 10 | 0.0–0.2% measured with audio running (M2 MT-APP-4; M3 slice 2 with the motion system). Animations repaint only while moving. Re-measured at scale in M3 slice 4b: 3 repaints in 3 s idle with the stress project (the once-a-second device check); M10 |
| Cold start under 2 s; scanning and loading never block the UI | M | 🟡 | 3, 6, 10 | First frame 394 ms warm / 1,110 ms cold (M1 shell). Audio files, project open and save, file dialogs and ASIO driver checks all run in the background (M2, M3). Plugin scanning runs in child processes (M6, ADR-0068) |
| Autosave, crash recovery, atomic saves, backups | M | ✅ | 3 | Atomic save and rolling backups (`lumen-project`). Autosave a minute after unsaved changes, to the local app data folder; a recovery banner after a crash; tested by killing the process (M3 slice 3d) |
| Crashing plugins isolated, flagged, recoverable; scanning never crashes the app | M | 🟡 | 6, 10 | Built in M6: one host process per plugin (ADR-0066, 0067), scanning in child processes (ADR-0068), a failed card with Reload (slice 2e), kill-on-close job object, the fault soak (slice 7b: crashes, hangs, slow and non-finite blocks, hosts ended from outside; the others never fail). Verified with the test plugin; real plugins are the owner's MT-VST-* runs |
| Device removal, rate and buffer changes, sleep and resume handled | M | 🟡 | 2, 10 | Recovered on Windows (MT-AUDIO-1, 2, 4 and 6; §6.9). macOS and Linux untested on hardware |
| Safe mode without plugins | M | 🟡 | 6 | Built in M6 slice 5e (ADR-0077): `--safe-mode`, Shift at start on Windows, the offer after a start that did not finish; MT-VST-5 is the owner's |
| Automated tests: core logic, editing, serialization and migration, audio (golden renders) | M | 🟡 | all | Unit and integration tests per crate; an end-to-end suite that drives the whole app (CI), and real-binary smoke tests run at the end of every slice (ADR-0029, 2026-10-08). Golden renders since M2, export's in M9 R5 |
| Benchmarks with regression gates in CI | M | ✅ | 1–3 | divan wall-clock benchmarks (smoke-run in CI), plus instruction-count gates under Callgrind that fail CI above +2% (M3 slice 1, ADR-0022). Each milestone adds its hot paths |
| CI on Windows, macOS, Linux | M | ✅ | 1 | First run green on 2026-10-06 (run 37396217369): Windows MSVC, macOS, Linux, rustfmt, cargo-deny, VST3 self-test. macOS was paused from 2026-10-07 (Actions minutes ran out) and is **back since 2026-10-10** (owner): every job green on all three systems in run 38070627268, after fixing 20 E2E tests that only failed on macOS |
| Test plugin simulating good and bad behaviour; real-plugin compatibility matrix | M | 🟡 | 1, 6 | `lumen-test-plugin`, a deterministic instrument and effect with injectable faults (ADR-0065), hosted through real host processes (ADR-0066, 0067); the probe `lumen-host-probe` and the fault soak (M6 slice 7, ADR-0080). The real-plugin matrix is `docs/PLUGIN_COMPATIBILITY.md`; the real-plugin runs (MT-VST-7) are out of scope for now (owner, 2026-10-26) |
| Real-time correctness enforced by tests | M | 🟡 | 1–2 | `lumen-rt` allocation guard (ADR-0010); RealtimeSanitizer in CI |
| High-DPI, 100–200% scaling, light and dark themes, keyboard-navigable, accessible names, colour-blind-safe | M | 🟡 | 3, 10 | Dark, light and match-system themes; interface size 100–200% in the app menu; colour-blind-safe accent; contrast tested for both themes (M3 slice 2, `docs/DESIGN.md`). M3 slice 8: every control in the main window, palette, shortcuts window, clip card, clip menu, audio settings and history has an accessible name, and Tab walks the transport left to right, then the panes (E2E `access`). The full accessibility pass and the Narrator check in M10 |
| No telemetry by default; opt-in only | M | ✅ | — | Nothing collects data. Re-verify each milestone. |

## §6.1 Arrangement and tracks

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Rulers: bars/beats, minutes:seconds, samples, timecode | M | ✅ 2026-10-08 M3 slice 5b, layout A (owner): the bar ruler on top; a time ruler along the bottom in minutes and seconds, samples, or timecode at 24, 25, 29.97 drop-frame or 30 fps (`timefmt.rs`, tested), chosen from its corner and saved in settings; the position display follows it. At the deepest zoom the bar ruler pins the position of its left edge | 3 |
| Adaptive or fixed grid (triplet, dotted, off) with snap-bypass modifier | M | ✅ 2026-10-08 M3 slice 5a: `lumen_core::grid`, adaptive in five widths or fixed 8 bars to 1/64, triplet and dotted, snap on or off (Ctrl+1 to Ctrl+5, the grid menu and button); Alt bypasses; snap guide; saved with the project (format v4). Used by the ruler and dropped files now, by clip edits in slice 6 | 3 |
| Anchored zoom, zoom to selection, fit all, fit loop, pan, edge auto-scroll, follow modes. (Ctrl+wheel zoom works in the M1 shell since the 2026-10-06 fix: egui delivers it as a zoom factor, not as scroll.) | M | 🟡 M3 slice 4d (ADR-0028): zoom at the pointer to single samples, `+`/`-`, X back, W fit all, H fit heights, Alt+wheel heights, pan, auto-scroll while dragging tracks, Follow page and continuous. Zoom to selection (Z) and fit loop added in slice 7d (ADR-0033), on the bar ruler's menu with the other view commands | 3 |
| Track types: audio, MIDI/instrument, group, return, master | M | 🟡 All five exist in the project model and the arrangement. Groups sum their tracks (M4 slices 1 and 3); returns and the master are pinned at the bottom of the arrangement, returns lettered A, B, C with sends from every audio, MIDI and group track (M4 slice 4, ADR-0038). Every kind has a channel strip in the Mixer tab (slice 5, ADR-0040) | 3–4 |
| Per-track name, colour, height, arm, monitor, solo, mute, polarity, I/O routing, inline fader and meter | M | 🟡 Name, type icon, colour, height (drag to resize in 22 pt steps, fold and unfold), arm (glows and breathes), solo and mute in the track header; volume and pan sliders in unfolded headers (M3 slice 4a, ADR-0025) with the level inside the volume slider (the owner's H2, M4 slice 5, ADR-0040), since M9 R1b a level strip beside the rail, folded or not, with M, S and arm in one well and sends in a well of slots (ADR-0101); polarity, output routing and a fader on the track's strip in the Mixer tab (slice 5). monitor In/Auto/Off and input routing built in M9 R1 (ADR-0100) | 3–4, 9 |
| Group tracks: create, drag in, nest, fold, sum, own devices/sends/fader/automation, one-step ungroup, sensible solo/mute/arm | M | 🟡 M4 slice 3 (format v8, ADR-0037): Ctrl+G and Ctrl+Shift+G (one step each), nesting three deep at most, folding (saved), dragging tracks in and out, groups summing their tracks (slice 1), mute, a group's arm arming its tracks, deleting and duplicating a group with its tracks. Solo across groups and returns by the agreed rules, a soloed group showing its tracks with a hollow S, and a group's sends on its header (slice 4, ADR-0038); a group's devices with M6, automation in M7 | 4 |
| Linked editing across selected tracks | S | 🔲 | 4 |
| Clips: move, copy-drag, trim, slip, fades, gain handle, loop-extend, rename, recolour, mute, lock | M | 🟡 M3 6a: move across tracks and Ctrl+copy-drag by the top half (snapped, one undo step, kinds respected, locked clips stay). 6c: trim edges (within the file), Shift loop-extend, Ctrl+Alt slip, and the clip overlap rule (a clip put down cuts away what it covers). 6d: fade handles with a free bend dot and Alt for S-shape (owner's F3 and C3; curves in format v6, ADR-0031), presets in the menu, a clip gain line (G1), heard and drawn alike; moved clips crossfade while playing. 6e (ADR-0032): 0 mutes (grey, the owner's M1), lock from the menu or card (padlock and clamps, K3; locked clips can still be muted, renamed and recoloured), Ctrl+R or F2 renames in the title bar, clips take their own colour, and the properties card (P1) sets all of it, gain and fades from the keyboard | 3 |
| Selection: clip, marquee, time range with cut/copy/duplicate/delete/split/consolidate | M | 🟡 Clip and marquee selection and time ranges across tracks (M3 6a); cut, copy, paste, duplicate and delete on clips and ranges (6b). From the keyboard (M3 review H2, the owner's choice): with a clip selected the arrows move between clips (←/→ along its track, ↑/↓ to the nearest clip above or below), Shift adds. Split (M7 E0, MT-EDIT-1 passed) and consolidate of clip selections (E3). Time-range consolidate is not built (KNOWN_ISSUES) | 3, 7 |
| Loop and punch regions, markers and locators, tempo and time-signature lanes | M | 🟡 Engine side of loop and punch done (slice 3); loop brace and punch (M3 5c) and named locators (M3 5d) done; tempo and time-signature lanes built in M7 A6 (ADR-0089) | 2–3, 7 |
| Track height presets, collapse, hide, reorder by drag, multi-select | M | 🟡 Fold and unfold, resize by dragging or Alt+wheel, H fits every track, multi-select (Ctrl, Shift), move with Alt+arrows and the menu (4a), reorder by drag with auto-scroll (4d). Hide and height presets later | 3 |
| Track templates, delay offset, lock, overview strip | S | 🔲 | 3, 9 |

## §6.2 Audio

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Import by drag and drop with sample-rate handling and optional copy into project | M | 🟡 Drag and drop from Explorer and automatic rate conversion done (M2 slice 6, ADR-0019), with a supported-file indicator while dragging. Imported audio is copied into `Samples/Imported` on save (setting, on by default; M3 slice 3c). Dropped on an audio track's lane, files land on that track at the pointer, one after another; between tracks they make new tracks there (M3 6e). **Import audio…** (Ctrl+Shift+I; the app menu, the palette, a track's menu and the empty arrangement) opens the system dialog and imports at the playhead onto the selected audio track or new tracks (M3 review H1). Files dropped together keep their order, and later drops never overtake earlier ones (`6ea0681`). Since M5 slice 7 the command is **Import audio or MIDI…** and MIDI files land on MIDI tracks (ADR-0062) | 3 |
| Drop files onto a specific track; dropping between tracks or below them creates a track (owner request, MT-MEDIA-1, 2026-10-06) | M | ✅ Done in M3 6e (`arrangement/drop.rs`): over an audio track's lane, files land on it at the pointer; between tracks, below the last one or over another kind of track, they make new tracks there. MT-APP-14 run 3 found drops between tracks were never offered on Windows (no pointer during a drag from Explorer); fixed by reading the system's cursor (ADR-0032); passed in run 4. macOS and Linux: KNOWN_ISSUES | 3 |
| Clip hover card with file details (format, rate, channels, length, size, path) | S | ✅ 2026-10-08 Basic card in M2 slice 6 (owner request, MT-MEDIA-1); in M3 6e it pins open as the clip's properties card (Alt+Enter; the owner's P1) | 3 |
| Glitch-free playback, seamless loops, instant seeking | M | 🟡 Clips play sample-exactly from memory, with click-free crossfaded loops and locates (slice 6; golden renders; MT-MEDIA-1 passes by ear). From disk (M9 R6, ADR-0109): loops and jumps to the start, the loop and markers are read ahead; other locates wait at most 150 ms; the automated soak of 64 tracks has no late read. MT-STREAM-1 passed on the owner's interface at 64 and 128 samples (2026-10-28) | 2, 9 |
| Disk streaming of clip audio with read-ahead, so large sessions need not fit in RAM; in-memory playback stays automatic for short files (owner decision 2026-10-06, replaced by M B 2026-10-27: files under 30 s stay in memory, no per-clip switch) | M | 🟡 Built in M9 R6 (ADR-0109): "Play audio from disk", on by default since MT-STREAM-1 passed (2026-10-28, the owner's S6 A), a 96 MB page pool, a disk thread, converted copies in a 20 GB cache. 64 two-minute files: 3,085 MB in memory, 170 MB from disk; renders equal from disk and memory | 9 |
| Crisp, fast waveforms at every zoom | M | ✅ M3 slice 4c (ADR-0027): peak pyramids built at load, drawn per pixel on the GPU; no measurable frame cost with 10,000 waveforms. Drawn sample by sample below 16 frames per pixel (4d, ADR-0028). Peaks of files played from disk are kept in the cache and read back on open (M9 R6, ADR-0109: 64 files ready in 0.03 s), deep-zoom samples are read on a worker, and the atlas reclaims what is no longer drawn | 3, 9 |
| Audio clip editor in the detail panel (CLAUDE.md §5) | M | 🔲 Planned as M7 E7 (`docs/milestones/M7.md` §5); today the Clip tab shows a static wave beside the clip's properties | 7 |
| Recording: multi-arm, monitoring, multichannel, crash-safe capture, latency compensation and offset, punch, count-in, input meters | M | 🟡 Input, per-track input routing (format v30), input meters with clip lights and monitoring In/Auto/Off through the track's chain built in M9 R1 (ADR-0100; MT-REC-1 passed). Audio recording is built in M9 R2 (ADR-0102): capture in the engine into a ring, a worker writing one float WAV per armed track that is valid at every moment, latency compensation plus a manual offset (audio window), punch, count-in, loop (one long clip), one undo step, a growing clip while recording, `Samples/Recorded` on Save, and recovery of a take after a crash; E2E scenarios on a ramp input pass; MT-REC-2 passed (owner, 2026-10-26) | 9 |
| Loop recording into takes; capture MIDI | S | 🟡 Built in M9 R3 (ADR-0104, format v31): loop recording keeps every pass as a take of one clip (audio always; MIDI in Replace, Overdub still layers), a Takes menu and "n/m" chip on the clip, commands to play the next or previous take, extract one to a new track (audio) and keep only one; Capture MIDI (Shift+F9) turns the last phrase played while not recording into a clip, from a ten-minute in-memory buffer with a switch. E2E scenarios pass; MT-REC-3 passed (owner, 2026-10-26) | 9 |
| Clip gain, fade shapes, automatic crossfades, reverse, normalize, polarity, pitch | M | 🟡 Clip gain and fades along free curves, with four presets (M3 6d, ADR-0031; golden renders, and E2E scenarios that listen). Reverse, polarity and normalize are built (M7 E1, ADR-0093; MT-EDIT-2 waits for the owner); automatic join crossfades are built (M7 E2, ADR-0094; MT-EDIT-3 passed, owner, 2026-10-24). The clip's static pitch needs a pitch engine; the owner deferred it with the audio pitch envelope (2026-10-19, Y1) to warp and time-stretch, M7 E8 | 3, 7 |
| Warp and time-stretch with quality modes | S | 🔲 Planned as M7 E8 (`docs/milestones/M7.md` §5), with the clip's static pitch and the audio pitch envelope (deferred by the owner, 2026-10-19) | 7 |
| Take lanes and comping | S | 🔲 Deferred by the owner until after M9's must-have slices (2026-10-26; DECISIONS, owner table). Loop passes are takes inside one clip until then (ADR-0104) | 9 |
| Mono, stereo, multichannel material | M | 🔲 | 2–3 |

## §6.3 MIDI

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Record, overdub, replace, count-in, punch, thru, channel filter, computer keyboard | M | 🟡 Built in M5 slices 2 and 6 (ADR-0046, ADR-0055); MT-MIDI-3 passed. Real-controller recording (MT-MIDI-7) is out of scope for now (owner, 2026-10-26) | 5 |
| Notes never hang; panic command | M | 🟡 Built in slice 1 (ADR-0045): a tracker per track lets go on stop, jump, loop wrap, edit, mute, change of output and Panic; hardware gets note-offs and All Notes Off / All Sound Off on every channel. Engine tests (14, including 400 random commands with every note-on answered), allocation guard, RTSan, and 11 E2E scenarios on a fake port. Unverified: a real MIDI device and real synth hardware (MT-MIDI-2) | 5 |
| Sustain, pitch bend, aftertouch, program and bank change, MIDI clock out | M | 🟡 Played from clips by the synth and sent to ports (slice 1; bank selects pass through to ports, the synth ignores them); chased on Play and jumps. Clip data can only be made by the demo project until the piano roll (slice 3). **MIDI clock out** built in slice 7 (ADR-0062): Sync per output with a delay, Start, Stop, the song position and Continue, 24 clocks a quarter on their samples; hardware timing unverified (MT-MIDI-8) | 5 |
| Piano roll: draw, select, move, resize, split, glue, mute, duplicate, nudge, transpose, legato, audition, velocity and controller lanes, synced zoom | M | 🟡 Built: the piano roll (slice 3, ADR-0047) and the velocity and controller lanes (slice 4, ADR-0050); MT-MIDI-4 and MT-MIDI-5 passed (2026-10-12) | 5 |
| Scale highlight and snap, fold, ghost notes, multi-clip editing, step input | S | 🔲 | 5 |
| Quantize: grids, modes, strength, swing, sensitivity, iterative, one-key apply, live-preview popover, humanize, legato, fixed length, transpose, scale-constrain, reverse | M | ✅ 2026-10-12: slice 5 (ADR-0054) and MT-MIDI-6 passed on run 2: the grid, Start, End, Both and Length modes, strength, swing, window, Q with the last settings, the ghost-trail preview, and humanize, fixed length, reverse and constrain to scale. Legato and transpose are in the piano roll's tools (`edit/notes.rs`) | 5 |
| Restore recorded timing, groove templates, audio quantize | S | 🔲 | 5, 7 |
| Standard MIDI File import and export | M | 🟡 Built in M5 slice 7 (ADR-0062): type 0 and 1 in by drop or Import (one clip on a MIDI track, a track per part between tracks; the file's tempo and meter offered), clips (type 0) and the project's MIDI tracks (type 1) out as they play. A file from another DAW is checked by hand in MT-MIDI-8 | 5 |
| MIDI learn with mapping list | M | 🟡 Built in M7 slice A5 (ADR-0088, format v23): a tap on the live feed, mappings saved with the project, the right-click Learn menu on mixer, header, send and plugin controls, the Mappings window; a mapped move is a hand on the control (undo, override, Touch recording). Pickup and relative encoders are not built; MT-AUTO-7 passed (owner, 2026-10-20, with a virtual MIDI port) | 7 |
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
| Everything with a value is automatable | M | 🟡 Built for what exists: a track's fader, pan, trim, mute and sends (A0), plugin parameters of effects and instruments on their exact sample (A3, ADR-0084), tempo and time signature (A6), clip gain and pan (A7). Built-in device parameters wait for the built-in devices (M8) | 7 |
| Lanes with searchable parameter chooser; show/hide key | M | ✅ The A key shows the lane over the track's clips (A1, ADR-0082), the chip's chooser lists a track's parameters and the plugin chooser searches a list of thousands (A3); MT-AUTO-2 and 4 passed (owner) | 7 |
| Point editing, ranges, curve types, shape tools, freehand with simplification, snapping, value tooltip | M | ⚠️ Built (A2, ADR-0083; MT-AUTO-3 passed, owner): box select, move, linear / step / curved segments, five shape tools, the pencil with simplification, snapping, a live value. **Copy, paste and duplicate act on picked points, not on a time range** (the owner's note, KNOWN_ISSUES): to be revisited after the core product is finished | 7 |
| Clip envelopes (audio gain/pan/pitch, MIDI CC/bend/pressure), looping, lock-envelopes option | M | 🟡 MIDI clips already carry looping controller events (M5). Audio gain and pan envelopes and the lock option were built in M7 slice A7 (ADR-0090; format v25; MT-AUTO-9 passed, owner, 2026-10-21), the envelope edited on the clip with the lane gestures and kept from the clip's start. **The audio pitch envelope is deferred with the owner's approval (2026-10-19)** until a time-stretch and pitch engine exists | 7 |
| Read / Write / Touch / Latch recording with thinning | M | ✅ Per track, from the lane chip, from Lumen's controls, a plugin's own gestures and mapped controllers; thinned on release; a Touch glides back (A4b, ADR-0087; MT-AUTO-6 passed, owner). A long pass is written into the project in parts every 5 s and is still one undo step (ADR-0091). Accuracy is measured on the silent test device only | 7 |
| Trim mode | L | 🔲 | — |
| Override state with one-click re-enable | M | ✅ A control moved under its lane takes the lane over; amber ring, chip and a Re-enable button; `edit.automation_reenable` (A4a, ADR-0086; MT-AUTO-5 passed, owner). The master and returns have no lane view, so no button there (KNOWN_ISSUES) | 7 |
| Smooth, sample-accurate automation for built-ins and plugins | M | 🟡 Mixer values and plugin parameters are sample-exact, the same at any block size, delay-compensated and heap-free (A0, A3; golden renders, allocation guard, link tests). Built-in device parameters wait for M8 | 7 |
| Tempo and time-signature changes recalculate the timeline | M | 🟡 The engine keeps the bar and beat across tempo and meter changes, and loop and punch follow (slice 3); clips and automation follow. Tempo and meter lanes, with ramps, and the time-signature map are built (M7 A6, ADR-0089, format v24). Audio clips do not follow tempo until warp (M7 E8) | 7 |

## §6.6 Editing actions

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Split (playhead, cursor, grid, range edges), click-free | M | 🟡 Built in M7 E0 (ADR-0092): press-aim-release or click with Split, Ctrl+E on clips or a range, snapped with Alt bypass; one undo step, original source phase and fade windows preserved. MT-EDIT-1 passed (owner, 2026-10-23) | 7 |
| Join / glue MIDI; consolidate audio (rendered) and MIDI (merged), originals kept | M | 🟡 Built for clip selections (M7 E3, ADR-0095): audio is rendered through the clip player into a float WAV (gain, polarity, reverse, fades, crossfades, envelopes, gaps as silence), MIDI is unlooped and merged; one undo step, originals stay in the media pool. Time-range consolidate is not built (KNOWN_ISSUES). MT-EDIT-4 passed (owner) | 7 |
| Trim, crop, delete, ripple delete, insert silence, cut, copy, paste, duplicate clip and range, loop/unloop, reverse, normalize, nudge, move, rename, recolour, mute, lock | M | 🟡 Trim and loop-extend (6c); move and copy (6a); cut, copy, paste, duplicate, delete, delete time (ripple) and insert time (6b); rename, recolour, mute and lock (6e). Crop, unloop into copies, reverse, normalize and clip nudge are built (M7 E1, ADR-0093; MT-EDIT-2 waits for the owner) | 3, 7 |
| Bounce in place; render selection to new track | M | 🟡 Built (M7 E4, ADR-0096 and ADR-0097): the track is rendered through the real mixer with plugins in host processes of the bounce's own, to a float WAV, over the range, the selected clips or the whole track, with its effects (devices, fader, pan, lanes) and an automatic tail as chosen; Replace or New track; one undo step; Esc stops. MT-EDIT-5 passed (owner) | 7 |
| Freeze, unfreeze, flatten | M | 🟡 Built (M7 E5, ADR-0098; format v29): a frozen track plays a rendered file and runs no plugin; read-only with a Frozen badge; Flatten converts it to audio. MT-EDIT-6 passed (owner) | 7 |
| Select all / none / invert / in track / to start / to end | M | 🟡 Select all (Ctrl+A) and none (Esc) in M3 7d; invert, in track, to start and to end in M7 E1 (ADR-0093) | 3, 7 |
| Slice audio to MIDI or drum map | S | 🔲 | 8 |
| Group and ungroup clips | L | 🔲 | — |
| Guarantees: split+join is identity; consolidating unprocessed audio is identical; undo restores exactly | M | 🟡 Undo restores exactly by construction (shared snapshots, ADR-0024) and is property-tested; split and join (M7 E0, E1) and consolidate (E3) are built | 3, 7 |

## §6.7 Mixer, routing and grouping

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Channel strip: input gain, polarity, devices, pre/post sends, fader, pan law, mute, solo, cue, output | M | 🟡 In the engine (M4 slice 1, ADR-0035): input trim, polarity, mute, pre- and post-fader sends, volume and pan with four pan laws (−3 dB constant power by default), output to a node or a device pair, all gliding over 20 ms. Saved and undoable in the model (slice 2, format v7, ADR-0036). Slice 4 (ADR-0038): sends set from comet knobs on the headers (pre/post from their menu), mute and solo by the agreed rules, cue before the fader to a second output pair. Slice 5 (ADR-0040): the strip itself in the Mixer tab (the owner's S1: output, polarity, trim, two send knobs then "+N", pan, a fader beside its meter, typed volume, held peak, M S arm), the pan law from the mixer's menu and the palette; devices with M6 | 4 |
| Returns, groups, master, sidechain from any track, hardware output routing | M | 🟡 The engine graph sums groups (nested), returns and a master and routes to device pairs (slice 1); a track in a group plays into it. A master track in every project, outputs and sends in the model, loops refused (slice 2, ADR-0036); grouping built (slice 3, ADR-0037); returns and the master pinned at the bottom of the arrangement with sends on the headers (slice 4, ADR-0038). Output choices on the strips, any of the device's pairs, which Lumen then opens (slice 5, B6, ADR-0040); sidechains from any track in the engine graph at the three taps, compensated (slice 6, ADR-0041), chosen in the UI when a device has a sidechain input (M6, M8) | 4 |
| Solo keeps soloed paths audible; exclusive-solo option | M | ✅ Built (M4 slice 4, ADR-0038): the owner's 14 cases as one pure function (`lumen_project::solo`) and its tests: a soloed path stays open, mute beats solo, returns are solo-safe, a soloed return plays wet only. Exclusive by default, additive as a preference; hollow M and S show why a track is silent or heard; the transport's S chip clears every solo. MT-MIX-4 passed | 4 |
| Automatic plugin delay compensation with indicator | M | 🟡 Built in the engine (M4 slice 6, ADR-0041): every node reports its devices' latency; parallel paths, sends and sidechains line up sample for sample through delay lines; the engine renders ahead so what is heard lines up with the playhead; the transport shows the compensation while there is some (the owner's D1). Tested with the synthetic latent device; real latency arrives with VST3 (M6). MT-MIX-6 passed | 4, 6 |
| Peak (hold, clip), RMS; master LUFS and true peak | M | ✅ Built (M4 slice 5, ADR-0040): every node metered after fader and pan on the audio thread (lock-free), peaks falling 20 dB/s with a 1.5 s hold, clips latched until clicked, a 300 ms RMS; the master's momentary, short-term and gated integrated LUFS (BS.1770-4, EBU Tech 3341 cases 1–5 within 0.1 LU) and 4× true peak in the loudness column. MT-MIX-5 passed | 4 |
| Dockable mixer panel; inline fader, pan and meter on track headers | M | ✅ Built (M4 slice 5, ADR-0040): the Mixer tab of the detail panel (strips in the arrangement's order and folding, returns after, the master pinned with the loudness column, strips dragged to reorder, sections folding in a short panel); level strips beside the headers' rails (ADR-0101; inside the volume sliders before M9 R1b). Detachable mixer windows are S (M10). MT-MIX-5 passed | 4 |
| Routing matrix; master mono-check and dim | S | 🔲 | 4 |
| Reference-track A/B | L | 🔲 | — |

## §6.8 Plugins and devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Non-blocking, crash-safe, cached scanning; blocklist; rescan; custom paths | M | 🟡 Built in M6 slice 1 (ADR-0068); MT-VST-1 passed on Windows with real plugins (owner-reported) | 6 |
| Multi-output instruments, sidechain inputs, latency and tail handling, bypass | M | 🟡 Built in M6 slice 6 (ADR-0078); MT-VST-6 is the owner's | 6 |
| Full state save and restore; preset browsing, loading and saving | M | 🟡 Built in M6 slices 2 and 5 (ADR-0071, 0077): state saved with the project and read from every running plugin before a save; programs and `.vstpreset` files; MT-VST-5 is the owner's | 6 |
| Every parameter automatable and MIDI-mappable; generic parameter UI | M | 🟡 The generic parameter UI (the card's strip and the All parameters panel) and plugin-side edits as undo steps are built (M6 slice 4, ADR-0076); automation and MIDI learn were built in M7 | 6–7 |
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
| Driver types: ASIO and WASAPI shared (Windows), CoreAudio, PipeWire/JACK/ALSA | M | 🟡 Output works on ASIO, WASAPI shared (hardware-tested) and cpal (CoreAudio/ALSA, compiled but not hardware-tested). WASAPI shared input since M9 R1 (ADR-0100); cpal input compiles but is untested. Missing: JACK/PipeWire (ADR-0016) | 2, 9 |
| WASAPI exclusive | S | 🟡 Output tested on hardware: 3 ms on HDMI with events; the Focusrite switches to polling automatically (MT-AUDIO-4, -5). Exclusive input is not built | 2 |
| Device, rate, buffer selection with input/output/round-trip latency display | M | ✅ Audio settings window (M2 slice 8, ADR-0021): driver, device, rate, buffer and live latencies. ASIO buffer sizes are set in the driver's control panel; WASAPI shared follows Windows. MT-APP-6 passes | 2 |
| Device loss and return: keep the session, show the lost state, reconnect automatically (found by MT-ASIO-5) | M | ✅ Detection and auto-reconnect, tested with the fake backend and on hardware (MT-AUDIO-1, -4); lost, busy and failed banners with a way forward (slice 8) | 2 |
| Channel selection with friendly names; mono and stereo configurations | M | 🟡 Output pairs and ASIO input pairs by name (mono for an odd last channel), one pair at a time; more routing with the mixer (M4) | 2 |
| Audio input on WASAPI, CoreAudio and Linux, with drift compensation between separate devices (owner decision 2026-10-06: deferred from M2 slice 8; ASIO input works in M2) | M | 🟡 WASAPI shared input kept in step with the render clock (rubato's `Slip`; M9 R1, ADR-0100; MT-REC-1 passed). CoreAudio and Linux input (cpal) compile only | 9 |
| Test tone, CPU load, dropout counter, Audio On/Off | M | ✅ All four in the audio panel (M2 slice 7, ADR-0020); test tone also in the audio settings window (slice 8) | 2 |
| ASIO control panel; driver resets and rate/buffer changes without losing the session; busy/missing messages | M | ✅ Control panel button; resets (MT-AUDIO-2); busy, lost and failed banners with Try again / Audio settings; missing saved device falls back with a notice; a device that fails to start returns to the previous one. MT-APP-6 passes | 2 |
| Runtime changes never crash; restore a working configuration; persisted settings | M | ✅ Driver and device switches at run time move the engine to a fresh control thread; refused changes revert with the reason; settings saved atomically once a setup works. MT-APP-6 passes | 2 |
| MIDI devices: enable toggles, hot-plug, per-track MIDI From/To, virtual ports | M | 🟡 Device layer and the MIDI page of the settings are built (M5 slice 0, ADR-0044): inputs and outputs listed, Track and Remote switches saved, hot-plug and replug, opened off the UI thread, a busy port says so. **MIDI To** per track is built (slice 1, ADR-0045: a port and channel from the track menu, timed against the audio clock, outputs switched on automatically; tested on the fake backend, hardware timing unverified). **MIDI From** per track (all inputs, the computer keyboard, or a port and channel) and Monitor In/Auto/Off are built (slice 2, ADR-0046), with the MIDI chip's popover for quick switches; virtual ports are not built (WinMM has none; recorded in KNOWN_ISSUES) | 5 |

## §6.10 Projects, media and export

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| New, open, save, save as, copy, template, revert, recent, unsaved indicator | M | 🟡 New, open, save, save as, open recent, unsaved dot in the title, Save / Don't save / Cancel before closing (M3 slice 3c). Built in M9 R4 (ADR-0105, format v32): Save a copy (the open project is untouched), Save as template and New from template (whole projects with their audio in a templates folder; palette entry per template), Revert to saved (reloads the file; the unsaved version is kept in `Backups/Reverted`). E2E scenarios pass; MT-PROJ-1 passed (owner, 2026-10-26) | 3, 9 |
| Human-friendly, versioned format that opens old projects forever; relative paths | M | ✅ Format v2 with a migration from v1 and golden fixtures for both; media paths relative to the project folder (M3 slice 3) | 1, 3 |
| Autosave, recovery, rolling backups, collect and save, relink, media pool with purge | M | 🟡 Autosave, recovery and rolling backups done (M3 slice 3); missing media is reported on open. Collect and save built in M9 R4 (ADR-0105: a background copy with progress and Esc, temporary names, nothing changes if stopped, missing files listed and left). Relink, the media pool and purge built in M9 R4b (ADR-0106; the Project media tab, Locate, Search a folder, Remove unused, Clear to the system trash, a banner on open); MT-PROJ-2 passed (owner, 2026-10-26) | 3, 9 |
| Export: project, loop or selection; master or stems; WAV 16/24/32f, FLAC; SRC; dither; normalize; tails; offline or real-time; progress and cancel; loudness report | M | 🟡 Built in M9 R5 (ADR-0107), with snapshot renders, a callback-driven real-time sink and reports measured from the encoded files; MT-EXPORT-1 pending. Since M9 R6 (ADR-0109) export and its loudness report read in blocks, and WAV past 4 GiB is written as RF64 (tested with a lowered limit). | 9 |
| MP3 and AAC export | L | 🔲 | — |
| Project settings | M | 🟡 Built in M9 R4 (ADR-0105, format v32): a window with the project's sample rate, pan law, automatic crossfades, the colour new tracks take and notes, each change one undo step; a new project takes the device's rate; join, bounce and freeze write their files at the project rate. Tempo and time signature stay on the transport. MT-PROJ-1 passed (owner, 2026-10-26) | 9 |
| Project notes | S | 🟡 Notes in the Project settings window (M9 R4, ADR-0105; MT-PROJ-1 passed) | 9 |

## §7 Standard DAW checklist

| Item | Tier | Status |
|---|---|---|
| Transport: play, stop, record, loop, return to start, go to start/end, previous/next marker. **Owner request:** transport controls use vector icons, not text (M2). | M | 🟡 Engine transport done (slice 3, ADR-0017). Transport bar with vector icons, Space / Shift+Space / Home, go to start, stop, play/pause and loop in the app (slice 4, ADR-0018). Recording MIDI (F9, the Record button, Overdub) in M5 slice 6 (ADR-0055); audio recording in M9 R2 (ADR-0102); markers and go to end in M3 |
| Tempo 20–999 BPM and time signature with change maps; tap tempo | M | 🟡 Tempo and time-signature maps with changes (`lumen-core`), swapped into the engine live (slice 3); the tempo and meter lanes (M7 A6, ADR-0089). Tap tempo: the Tap piece after the tempo and the Tap tempo command (M9 R7, ADR-0112) |
| Metronome: accent, volume, sound, count-in, pre-roll, record-only, output routing | M | 🟡 On/off, accented downbeat, volume and two synthesized sounds, sample-accurate (M2 slice 7, ADR-0020). Output routing: Main or Cue from its menu, never through the master (M4 slice 4, ADR-0038). Count-in (Off, 1, 2 or 4 bars) and record-only for MIDI recording built in M5 slice 6 (ADR-0055); pre-roll is not built |
| Loop region, punch in/out, auto-punch | M | 🟡 Sample-accurate loop wrap and punch-region reporting in the engine (M2 slice 3). M3 slice 5c: the loop brace is edited on the strip (draw, move, resize, snapped; click toggles), undoable; punch in and out at the loop's edges, from the loop menus. Recording punches: MIDI since M5 slice 6, audio since M9 R2 |
| Markers with names and navigation | M | ✅ 2026-10-08 M3 slice 5d: named locators on the strip, set or removed at the playhead (M, Set), jumped between (Ctrl+Left/Right, ◂ ▸, click a flag), dragged (snapped), renamed (double-click, F2), deleted; undoable, saved (format v4) |
| Multiple time formats; editable position field | M | 🟡 Time formats in M3 5b (`timefmt.rs`: bars and beats, minutes and seconds, samples, timecode), and a Go to position field in the transport bar |
| Edit cursor separate from playhead; scrubbing. **Owner request (2026-10-06), raised from S to M, arrangement-focused:** click in the scrub strip to set the playback position; drag to scrub audibly; drag in the beat ruler vertically to zoom and horizontally to scroll. Locate-on-click in M2; scrub strip, audible scrubbing and ruler zoom in M3. | M | 🟡 Click-to-locate on the ruler done (M2 slice 5), snapping to the editing grid (M3 5a). Ruler drag: down or up zooms, sideways scrolls, the grabbed beat stays under the pointer (M3 5b). M3 5e: the scrub strip (the strip's top, around the locator flags): click jumps (snapped); drag plays the grid step under the pointer in a loop at normal pitch, Alt scrubs like tape (speed and pitch follow the hand); while playing, a drag jumps playback. An engine scrub mode, real-time safe (ADR-0030). An edit cursor separate from the playhead is still to come |
| MIDI clock / MTC out | S | 🟡 Clock out built in M5 slice 7 (ADR-0062; MT-MIDI-8 out of scope for now); MTC out deferred, unscheduled |
| Clock / MTC in, network tempo synchronization | L | 🔲 |
| Tracks: add, delete, duplicate, rename, recolour, reorder, multi-select | M | 🟡 All of these from the track menu, the "+ Add track" menu button and keys (Ctrl+T, Ctrl+Shift+T, Ctrl+D, Delete, F2, Alt+arrows), each one undo step (M3 slice 4a). Drag to reorder (slice 4d) |
| Track types with routing, arm, monitor, polarity | M | 🟡 All five track types (M3, M4); output routing and polarity on the strip (M4 slice 5); arm and monitor for MIDI (M5 slice 2) and audio, with input routing (M9 R1) |
| Freeze, unfreeze, flatten | M | 🟡 Built in M7 E5 (ADR-0098); MT-EDIT-6 passed (owner) |
| Track delay, lock, icons, templates | S | 🔲 |
| Select, marquee, range; cut/copy/paste/duplicate/delete/ripple delete/insert silence | M | 🟡 M3 6a: click, Ctrl and Shift selection, marquee, time ranges across tracks (a clip's bottom half). M3 6b (`lumen_project::edit`): cut, copy, paste (at the playhead or into a range, overwriting the destination range), duplicate, delete, delete time (ripple, the whole song) and insert time, on clips and ranges, each one undo step, in the lanes' right-click menu. Crop in M7 E1 (ADR-0093) |
| Split, join, consolidate, trim, crop, slip, nudge, move, loop/unloop | M | 🟡 Trim, slip, move and loop-extend (M3 6a to 6c). Split with the drag-to-cut gesture (E0); join (seamless), crop, unloop into copies and clip nudge (E1); consolidate (E3, ADR-0095) |
| Fades, crossfades, clip gain, reverse, normalize, polarity | M | 🟡 Fades with free curves and four presets, and clip gain (M3 6d, ADR-0031). Moved clips crossfade while playing (5 ms, engine). Reverse, normalize and polarity (M7 E1); automatic and custom join crossfades (M7 E2, ADR-0094; MT-EDIT-3 passed, owner, 2026-10-24) |
| Snap and grid; zoom; clip name/colour/mute/lock; clip properties | M | ✅ Grid and snapping (M3 5a) and zoom (4d) done; clip name, colour, mute, lock and the properties card done in M3 6e (ADR-0032; MT-APP-14 passed in run 4) |
| Tool model: pointer, draw, erase, split, mute and zoom, with single-key switching and temporary modifiers (CLAUDE.md §5) | M | 🟡 Built in M7 E0 (ADR-0092; MT-EDIT-1 passed); keys follow the CLAUDE.md §5 convention (B, Esc, Ctrl+E, 0, Z and X, Del), chosen 2026-10-19 (K1) |
| Warp, transient and tempo detection, pitch shift | S | 🔲 Planned as M7 E8 |
| Take lanes and comping | S | 🔲 Deferred by the owner until after M9's must-have slices (2026-10-26) |
| Unlimited undo/redo with visible history | M | 🟡 Undo and redo for imports, tempo and time signature (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y), a History window, and a note after each undo (M3 slice 3, ADR-0024). Every later edit joins it as it is built |
| Audio recording with monitoring, multitrack, input meters | M | 🟡 Monitoring, input routing and input meters (M9 R1, ADR-0100); recording is built in R2 (ADR-0102), MT-REC-2 passed (owner, 2026-10-26) |
| Latency compensation with manual offset; crash-safe capture | M | 🟡 MIDI recording takes off the output latency automatically and commits takes about once a second (M5 slice 6, ADR-0055). Audio capture takes off the input and output latency plus a manual offset (audio window, M9 R2, ADR-0102); the file is valid at every moment and recovered after a crash. MT-REC-2 passed (owner, 2026-10-26) but reported no measured figures. The loopback helper (M9 R2b, ADR-0110) measures the real round trip through a cable and offers the difference as the offset, remembering the setup it was measured on; MT-REC-5 (hardware) waits. The manual offset does not yet apply to MIDI |
| MIDI recording: overdub/replace, count-in, punch | M | 🟡 Built in M5 slice 6 (ADR-0055): Record (F9 or the button) on armed MIDI tracks, a count-in of 1, 2 or 4 bars, Replace and Overdub, punch from the loop brace's switches, loop passes that layer, commits about once a second, one undo step per take, and autosave every 10 s while recording. Real-controller timing is unverified: MT-MIDI-7 is out of scope for now (owner, 2026-10-26) |
| Computer keyboard as MIDI input | M | 🟡 Built in M5 slice 2 (ADR-0046): the computer-keyboard MIDI layout in a mode (Ctrl+Shift+K), octave and velocity keys, lit transport button and readout; E2E hears it. MT-MIDI-3 passed (2026-10-11) |
| Loop recording into takes; capture MIDI | S | 🟡 Built in M9 R3 (ADR-0104, format v31): loop recording keeps every pass as a take of one clip (audio always; MIDI in Replace, Overdub still layers), a Takes menu and "n/m" chip on the clip, commands to play the next or previous take, extract one to a new track (audio) and keep only one; Capture MIDI (Shift+F9) turns the last phrase played while not recording into a clip, from a ten-minute in-memory buffer with a switch. E2E scenarios pass; MT-REC-3 passed (owner, 2026-10-26) |
| Piano roll, velocity and controller lanes, quantize, humanize, transpose, legato | M | 🟡 Built: the piano roll (slice 3, ADR-0047), the velocity and controller lanes (slice 4, ADR-0050) and quantize with humanize (slice 5, ADR-0054). MT-MIDI-4, MT-MIDI-5 and MT-MIDI-6 passed 2026-10-12; the row stays 🟡 until the rest of M5 is done |
| MIDI thru, channel filter, panic, sustain/bend/aftertouch, program/bank | M | 🟡 Panic, sustain, bend, pressure and program done (slice 1); thru (to the instrument or the MIDI To port) and the channel filter in MIDI From done (slice 2, ADR-0046). Real-controller timing unverified (MT-MIDI-3) |
| SMF import/export; MIDI learn | M | 🟡 SMF built in M5 slice 7 (ADR-0062; MT-MIDI-8 out of scope for now); MIDI learn built in M7 slice A5 (ADR-0088) |
| Drum maps, drum editor, step sequencer | M | 🔲 |
| MIDI effects; on-screen keyboard | S | 🔲 |
| MPE, MIDI 2.0 | L | 🔲 |
| Automation: all parameters, curves, R/W/T/L, clip envelopes, tempo and signature, override, simplification | M | 🟡 Lanes, curves and steps play for the mixer (M7 A0), and point simplification is in the model; a lane can be shown, and points added, moved and deleted, from the mouse and the keys (A1, ADR-0082), and edited as sets with shapes, drawing, copy and paste (A2, ADR-0083); a lane drives a hosted plugin's parameter, chosen from the plugin's own list (A3, ADR-0084); controls follow their lane and a moved control takes its lane over, with Re-enable (A4a, ADR-0086); Touch, Latch and Write record a track's lanes from its controls and a plugin's gestures (A4b, ADR-0087); clip envelopes and the lock option are on the clip (A7, ADR-0090); tempo and meter lanes (A6, ADR-0089); MT-AUTO-1 to 10 passed |
| Mixer: fader, pan law, mute, solo, sends, returns, groups, master, routing, sidechain, PDC, meters, cue | M | 🟡 The engine graph (M4 slice 1, ADR-0035); routing in the model (slice 2); groups (slice 3); mute, solo, sends, returns, the master and cue (slice 4, ADR-0038); the Mixer tab, meters, the pan law and output pairs (slice 5, ADR-0040); sidechains and delay compensation in the engine with the transport's readout (slice 6, ADR-0041) |
| LUFS and true-peak metering; mono-check and dim | S | 🟡 LUFS (momentary, short-term, integrated) and true peak on the master (M4 slice 5, ADR-0040); mono-check and dim later |
| Reference track | L | 🔲 |
| Async cached VST3 scanning, custom paths, blocklist, search, favourites, recents | M | 🟡 Built and tested on Linux with the test plugin (M6 slice 1, ADR-0068); MT-VST-1 passed on Windows with real plugins (owner-reported, no measurements); not run on macOS |
| Instruments and effects; multi-out; sidechain; bypass; presets; state; automation; generic UI; floating editors with DPI and focus | M | 🟡 Built (M6 slice 6, ADR-0078; format v21, protocol version 8): the host lists and turns on a plugin's audio buses (a block carries up to 64 channels); a sidechain from any track at three taps with a row and a light on the effect's card; a plugin instrument's outputs as tracks of their own (the card's Outputs row); a plugin instrument's latency is compensated, effects flush on a locate and a scrub is not late. Verified on Windows with the test plugin through real host processes, a real sampler's outputs and a real orchestra plugin's buses; MT-VST-6 (real plugins) is the owner's; not run: smoke, WSL gates, macOS |
| Latency compensation; crash isolation; safe mode | M | 🟡 A plugin instrument's reported latency is compensated like an effect's, a locate flushes effect tails, a scrub is not late (M6 slice 6, ADR-0078); crash isolation and safe mode are built (slices 0 and 5); MT-VST-6 is the owner's |
| Built-in essentials | M | 🔲 |
| Spectrum analyzer, tuner | S | 🔲 |
| CLAP, racks with macros | L | 🔲 |
| Browser: devices, samples, projects, user places; search; audition; drag and drop | M | 🟡 Devices tab with search, type, category and favorites (M6 slice 1), added by double-click or drag (slice 2); the Project media tab (M9 R4b). Samples, projects, places and audition are not built |
| Tags and favourites; tempo-synced audition | S | 🟡 Favorites on devices (M6 slice 1); tags and audition are later |
| Media pool with relink and purge | M | 🟡 Built in M9 R4b (ADR-0106): the browser's Project media tab lists every audio file with its state; Locate and Search a folder relink missing files (checked first; not an undo step); Remove unused is one undo step; Clear moves leftover files to the system trash after two presses; a banner offers Relink on open. E2E scenarios pass; MT-PROJ-2 passed (owner, 2026-10-26) |
| Project management: new/open/save/as/copy/template/revert/recent; autosave and recovery; backups; collect; relink; migrations; settings | M | 🟡 Copy, template, revert, collect and settings built in M9 R4 (ADR-0105); relink, the media pool and purge in R4b (ADR-0106); MT-PROJ-1 and 2 passed |
| Project notes and metadata | S | 🟡 Notes in Project settings (M9 R4, ADR-0105); metadata is not built |
| Export: master and stems; WAV/FLAC; bit depth; dither; offline and real-time; scope; tails; normalize | M | 🟡 Built in M9 R5 (ADR-0107, PR #72): background snapshot render, actual device-callback real-time mode, master/track/group/return files, project/loop/time selection, 16/24-bit WAV and FLAC plus float WAV, output-rate conversion, TPDF dither, −1 dBFS peak normalization, tails, cancellation, gated LUFS and true-peak report. Reads in blocks, RF64 past 4 GiB (M9 R6, ADR-0109). MT-EXPORT-1 pending. |
| MP3/AAC | L | 🔲 |
| Preferences: audio, MIDI, plugin paths, file/cache locations, theme, scale, autosave interval, undo depth, track defaults, record settings | M | ✅ One Preferences window with eight pages and a search (M9 R7, ADR-0112): audio and MIDI devices, recording, editing (undo steps, autosave interval, new-track monitoring and instrument), mixing, plugin folders with Remove and the blocked plugins with Retry, files (projects folder, copy imported audio, the cache's folder, limit and Clear), appearance. MT-PREF-1 pending |
| Shortcut editor with conflict detection; default keymap | M | 🟡 Built in M3 7c (ADR-0033): the Keyboard shortcuts window (keyboard map by area with modifier layers over the list), clashes with Reassign or Cancel, reset one or all, rebinding from the palette, saved in the settings. MT-APP-15 passed |
| alternative keymap preset | L | 🔲 |
| First-run wizard (audio device, plugin paths) | M | 🔲 |
| Preferences import/export; localization-ready text | S | 🔲 |
| Command palette; tooltips with shortcuts; context menus everywhere | M | 🟡 Built in M3 7b and 7d (ADR-0033): the palette (fuzzy search, context names, reasons, typed values), every menu and tooltip shortcut drawn from the keymap as keycaps, right-click menus on the rulers and track names added. MT-APP-15 passed |
| Design system with a crisp vector icon set (spec §5): transport icons (play, pause, stop, record, loop, metronome) in M2, full set with the design system in M3 | M | 🟡 Design system v1 (`docs/DESIGN.md`, `theme.rs`): colour tokens for both themes, type, spacing, shape, widget states, key flows; M2 UI moved onto it (M3 slice 2). Icons grow with each slice's new controls |
| Motion system: tokens, easing, what animates, reduce-motion; never delays input, no idle redraws; M2 widgets retrofitted (owner request 2026-10-06: fluid feel). Built in M3 with the design system; final polish pass in M10 | M | 🟡 Built (M3 slice 2, `motion.rs`): FAST/BASE/GLIDE tokens, ease-out and critically damped spring, Reduce motion; panels slide, popovers and windows fade, notices and banners animate, transport buttons fade on hover, playhead and start marker glide after a locate, clips fade in. Idle CPU unchanged (0.2%). Owner signed off (MT-APP-7, 2026-10-07); M10 polish |
| Reset to default, type-in, fine adjust, wheel adjust, parameter right-click menus | M | 🟡 Value controls reset on double-click, take typed values, Shift for fine, arrow keys (M3); the latching wheel on every slider, knob, fader and field (the owner's B3, M4 slice 5, ADR-0040); a send's menu switches pre and post fader. Controls take a right-click Learn menu for MIDI mapping (M7 A5, ADR-0088) |
| Drag and drop for files, devices, clips, tracks; status-bar hints | M | 🟡 Files from Explorer, with a live indicator of what will be added (M2). Clips and tracks in M3, devices in M6 |
| Resizable, collapsible panels with remembered layout; full screen; high-DPI; light and dark | M | 🟡 Resizable panels that slide open and closed; layout saved with each project; dark, light and match-system themes; interface size 100–200% (M3 slices 2–3). Full screen later |
| Keyboard-only core flows; accessible control names. Shift+Tab settled by the owner (ADR-0015): it moves focus back in a text field or an open window, and toggles the detail panel elsewhere. | M | 🟡 M3 7d: F6 and Shift+F6 move the keyboard between named panes; shortcuts keep working with a control focused (only its own keys stay with it). M3 slice 8 and its review: the exit flow works from the keyboard alone (Import audio…, clips chosen with the arrows); Tab order is transport, browser, detail, arrangement (egui's panel order, recorded in ADR-0033), and F6 goes in reading order. The accessibility pass and the Narrator check in M10 |
| Detachable mixer and editor windows; hover help; welcome screen | S | 🔲 |
| CPU, disk and dropout indicators; Audio On/Off; panic | M | 🟡 CPU (peak per second), dropouts, Audio On/Off and panic done (M2 slice 7). Disk: a Disk row in the audio panel (read load, late reads, unreadable pages) and a "Disk late" chip after an underrun, while audio plays from disk (M9 R6, ADR-0109). MIDI all-notes-off joined panic in M5 slice 1: the engine cuts every instrument and sends CC 123 and 120 on every channel of every MIDI output in use, and the app sends them directly too (a palette command and the audio panel button) |
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

### Milestone 7 — report (2026-10-22)
**Status:** Built, not signed off. This report covers the automation part (A0 to A8, merged). The owner's
MT-AUTO-1 to 10 passed (most reported without step detail; MT-AUTO-8 and MT-AUTO-10 on 2026-10-22). The owner's
run found two things, logged in KNOWN_ISSUES for after the core milestones: a plugin parameter does not follow its
lane when the playhead is moved while stopped, and a recorded fade leaves 10 to 15 points.

**Editing and render (E0 to E6, built as "M7b", PR #51 to #58).** Tools and split, clip commands, crossfades,
consolidate, bounce and freeze (ADR-0092 to 0098; formats v26 to v29); the slice list is in
`docs/milestones/M7.md` §3. E6's audit (`docs/audits/2026-10-24-m7b.md`) is this part's report: the combined
editing walkthrough, an app-wide UX review and its fixes. MT-EDIT-1 and 3 to 6 passed; MT-EDIT-2 and 7 wait.

**Shipped** (ADRs 0081 to 0091; formats v22 to v25; no host protocol change).
- **A0 hear a lane.** `lumen_core::automation` (points, segment shapes, `value_at`, simplification), lanes on a
  track in the model (format v22), Document methods, the mixer playing fader, pan, trim, mute and send lanes on
  their exact sample, the same at any block size, delay-compensated, heap-free (ADR-0081).
- **A1 and A2 see, add and edit a lane.** The A key shows the lane over the track's clips; click, drag, delete;
  box select, five shape tools, curved and step segments, the pencil with simplification, snapping, a live value
  (ADR-0082, ADR-0083).
- **A3 plugin parameters.** A lane drives a hosted plugin's parameter on its exact sample, for effects and
  instruments; the chooser searches thousands of parameters; counted overflow (ADR-0084).
- **A4a and A4b follow, override and record.** Controls show their lane's value; moving one takes the lane over,
  with one-click Re-enable; Read, Touch, Latch and Write per track, from Lumen's controls and plugin gestures
  (ADR-0086, ADR-0087).
- **A5 MIDI learn.** A tap on the live feed, mappings saved with the project (format v23), the right-click menu
  and the Mappings window (ADR-0088).
- **A6 tempo and meter lanes.** The tempo lane (with ramps, at the owner's request) and the time-signature map
  (format v24); every reader of tempo and meter moved to the maps, SMF through them (ADR-0089).
- **A7 clip envelopes.** Gain and pan on audio clips, kept from the clip's start, looped with the clip, played by
  the clip player, and the Lock envelopes option (format v25; ADR-0090).
- **A8 close.** The combined walkthrough (`walkthrough_automation`: lanes drawn and heard, a plugin lane from the
  chooser, a Touch pass and its undo, a mapped controller's sweep, a tempo and meter change, save, reopen, play),
  the accessibility check for the M7 views, the review and its fixes (ADR-0091).

**Verified.**
- Gates on Linux in the cloud container (2026-10-22): fmt, workspace clippy `-D warnings`, the workspace tests
  (2,635 tests across the workspace: 801 unit and 602 E2E in `lumen-app`), 6 of 6 smoke tests, macOS cross-clippy. Four timing-sensitive hosted-plugin and playhead scenarios failed under load in an earlier full run; they now hold the plugin turn, wait for what the engine answers and discard a playhead read the test thread was held up across, and pass four times beside four busy loops. The earlier slices ran
  the WSL performance gate: largest change +0.69% (`engine_routed_64`, A7), none over +2%.
- New in A8: the walkthrough (mutation checked: a gain lane kept shut makes it fail), the accessibility test for
  lane, chip menu, taken-over lane, control menu, Mappings window, tempo and meter lanes and clip envelope (no
  unnamed controls), and the review's fixes with tests that fail without them (below).
- Real plugins: A3's lanes were heard by the owner with real plugins (MT-AUTO-4), recorded without step detail.

**Review.** `lumen-review` ran on 2026-10-22 (`docs/audits/2026-10-22-m7.md`), narrower than M5's and M6's (one
reviewer, mechanical scans over all 165 changed source files, targeted reading of the recording, tap and
validation code): 1 High, 3 Medium, 7 Low. The High was a real bug, reproduced: opening another project during a
lane pass wrote the pass into the new project; fixed with the controller and gesture state that crossed the
swap (ADR-0091). A pass is now written into the project every 5 s and recovered after a crash. Three files were
split by concern. The documents drifted (this file's §6.5, the architecture page, the plan, the known issues)
and are brought up to date. Each fix has a test that fails without it.

**Not verified, and where it goes.**
- **MT-AUTO-8 and MT-AUTO-10 passed** (owner, 2026-10-22, no step detail). The release frame times with 200 tracks
  and visible lanes at 100% and 200% were not reported as numbers, so they stay unmeasured here (KNOWN_ISSUES, A1).
- **Real hardware controllers:** MIDI learn was tried with a virtual cable and an on-screen keyboard only.
- **Recorded accuracy** (the playhead within 12 ms of a straight line) is measured on the silent test device,
  not a real one.
- **RealtimeSanitizer, Windows and macOS test runs, the WSL performance gate for A8:** not run (A8 changed no
  engine, device or link code; CI's `rtsan` job covers them).
- **Deferred with approval:** the audio pitch envelope and the static clip pitch (a time-stretch engine, [S]);
  copy, paste and duplicate on a time range rather than picked points (the owner's note); a lane view for the
  master and returns; remembering which lanes are shown.

**Risks.**
1. The lane pass committed every 5 s re-simplifies the whole pass each time; a very long pass (tens of minutes)
   is not measured.
2. An edit with another merge key between two parts (a fader dragged in Lumen, a plugin's own gesture) makes the
   pass more than one undo step, as a MIDI take does (KNOWN_ISSUES).
3. Accuracy of recorded points on a real device and with a real controller is unknown.
4. Windows is the only system with the owner's results; Linux and macOS have the automated ones.

**Next.** The owner's MT-EDIT-2, MT-EDIT-7 and MT-UX-2, then the approval of M7. E7 and E8 run when the owner
chooses (`docs/milestones/M7.md` §5).

### Milestone 6 — report (2026-10-19)
**Status:** Complete; signed off by the owner 2026-10-26. Every slice is built
and merged into `main` (slices 0 to 7; the last is PR #38). MT-VST-3 to MT-VST-7
and the ADR-0069 callback numbers are not signed off and are out of scope for now
(owner, 2026-10-26), so this report names what was verified and what was not.

**Shipped** (ADRs 0065 to 0080; formats v17 to v21; host protocol version 8).
- **Slice 0, the host and the audio path.** Deterministic test plugins with
  chosen faults (ADR-0065), a host process per plugin with a bounded versioned
  protocol (ADR-0066), a shared-memory audio path and measurement harness
  (ADR-0067), and the decision that the audio callback may wait a bounded time on
  a host (ADR-0069, **on trial**). Slice 0's measurements are in ADR-0069 and
  `docs/manual-tests/M6.md`.
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
  machine (`lumen-host-bench`). Slice 0's 64-host numbers are in ADR-0069.
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
  64 hosted tracks (MT-VST-7 steps 7 to 9). Monitoring through an effect was part of
  MT-REC-1 (M9 R1), passed as a whole with no figures (owner, 2026-10-26). The
  callback's wait stays on trial until those numbers are in.
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

### Milestone 5 — report (2026-10-19)
**Status:** Complete; signed off by the owner 2026-10-26. All nine slices are built
and merged. MT-MIDI-1 and 4 to 6 passed; MT-MIDI-2 and 3 passed functionally, with
follow-ups in KNOWN_ISSUES. MT-MIDI-7, 8 and 9 are not signed off and are out of
scope for now (owner, 2026-10-26).

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
- The stress project with the 100,000-note track, release frame times p99 (scroll time,
  scroll tracks, zoom): arrangement only 1.62, 1.77, 1.53 ms at 1600 × 900 and 3.08,
  4.47, 3.11 ms at 3440 × 1380; with the clip open in the editor (`--bench-editor`) at
  most 1.82 ms and 2.22 ms. Three idle repaints in three seconds.
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

**Risks.** MIDI port identity, overflow note-off loss and a take's undo steps around
other edits remain in KNOWN_ISSUES.

### Milestone 4.5 — report (2026-10-10)
**Status:** Built 2026-10-10, a mid milestone between M4 and M5. All six
slices are built and every finding of the interface audit
(`docs/audits/2026-10-10-design.md`, 20 findings) is built; none was
deferred. MT-POL-1 to MT-POL-6 passed (owner, 2026-10-10 and 2026-10-12).
MT-APP-18 (light and the mark, merged from `main`) waits.

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

**Verified by hand:** MT-POL-1 to MT-POL-6 (owner). Not yet: MT-APP-18.
Screen readers are still unchecked (Narrator
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

**Exit criterion (the plan):** *Create → add track → import audio →
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
  → M9; the drag-to-cut gesture and the tool model → M7 E0 with split; group
  summing, sends and routing → M4; a UX pass on hints and tooltips →
  scheduled when the next milestones are planned.
- Recorded for later (KNOWN_ISSUES): a second instance offering to recover
  a running one's autosave (single instance, M9); audio clips not
  following tempo (warp); submenu names read with their arrow, the Tab
  order of the panes, the cramped shortcuts window at 200% in the
  smallest window, a hint under the pinned card, and the pipeline cache
  read on the UI thread (M10).
- Undo depth is fixed at 10,000 steps until Preferences (M9; a setting since R7).

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
- cpal (macOS, Linux) input: compile-only (M9 D8); WASAPI input is built (M9 R1) and MT-REC-1 passed (owner, 2026-10-26);
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

**Exit criteria (the plan):**

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
**Status:** Complete 2026-10-06. Written on 2026-10-05, when two exit criteria were
only partly met; P-RENDER (the owner chose the egui host), P-HOST and the `lumen`
shell (first frame 394 ms warm, 1,110 ms cold) closed them the next day (ADR-0002,
ADR-0005).

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

**Exit criteria (the plan):**

| Criterion | Status |
|---|---|
| A minimal application launches | ✅ `lumen` shell launches (slice 11, 2026-10-06) |
| UI approach validated | ✅ P-UI, with the GPU escape hatch proven; awaiting approval |
| Audio architecture proven with a prototype | ✅ Engine core plus native ASIO on real hardware. Other backends are M2. |
| Project format saves and loads a minimal project | ✅ |
| VST3 and isolation approach demonstrated | ✅ Real plugin (Test instrument H) processes audio in an isolated host process, survives a crash and is restored from its last known state (P-HOST, 2026-10-06) |

**Risks:**
- Plugin editor windows across processes (R12) are untested.
- The POSIX isolation transport and the macOS/Linux audio backends are
  unproven.
- 120 fps margins were measured on a high-end machine.
- egui ships breaking releases about every two months.
- ASIO licensing is undecided.
- Local GNU and CI MSVC builds differ.
