# Lumen — Progress

## Status

**Current milestone:** 3 — Arrangement & Project System (plan:
[milestones/M3.md](milestones/M3.md)). Slices 1–3 are done; slice 4
(tracks and the arrangement view at scale) is in progress: part 4a (track
headers) is built and the architecture audit after it is fixed
([audits/2026-10-07-architecture.md](audits/2026-10-07-architecture.md)).
**Previous:** M2 complete and approved 2026-10-07 (report below); M1 complete 2026-10-06 (report below; [milestones/M1.md](milestones/M1.md))

Statuses: 🔲 Not started · 🟡 In progress · ✅ Complete · ⚠️ Partial / known limitation · ⏸️ Deferred with approval · ❌ Blocked

## Milestones

| # | Milestone | Status | Notes |
|---|---|---|---|
| 1 | Foundations & Architecture | ✅ Complete | Plan and risk register in `docs/milestones/M1.md` |
| 2 | Audio Engine, Devices & Transport | ✅ Complete, approved 2026-10-07 | Plan: `docs/milestones/M2.md` |
| 3 | Arrangement & Project System | 🟡 In progress (slices 1–3 of 8 done) | Plan: `docs/milestones/M3.md` |
| 4 | Mixer, Routing & Groups | 🔲 | |
| 5 | MIDI & Piano Roll | 🔲 | |
| 6 | VST3 Hosting & Device System | 🔲 | |
| 7 | Automation, Envelopes & Advanced Editing | 🔲 | |
| 8 | Drum Maps, Drum Editor & Built-In Devices | 🔲 | |
| 9 | Recording, Media, Export & Production Workflow | 🔲 | |
| 10 | Hardening, Performance & Release | 🔲 | |
| S | Stretch goals (unscheduled; pulled into a milestone only with owner approval) | 🔲 | VST3 scanning and hosting on macOS and Linux |

## Milestone 3 slices

| # | Slice | Status | Commits |
|---|---|---|---|
| 1 | Performance gates by instruction count (ADR-0022) | ✅ 2026-10-07 | `03e2228` |
| 2 | Design system, motion system, M2 retrofit; typefaces (ADR-0023, MT-APP-7) | ✅ 2026-10-07 | `dbe4777`, `392a5f8`, `d320cf0` |
| 3 | Project model and undo, save/open, autosave and recovery (ADR-0024, MT-APP-8) | ✅ 2026-10-07 | `fd7583b`, `e2768fb`, `43c86f1`, `578bc6f`, `7780b11`, `4a7f615` |
| 4 | Tracks and the arrangement view at scale: groundwork (format v3), 4a track headers (ADR-0025, MT-APP-9 passed), architecture audit fixed, 4b clips on the GPU with a frame-time benchmark (ADR-0026, MT-APP-10 passed), 4c waveforms (ADR-0027, MT-APP-11 passed), 4d navigation (ADR-0028, MT-APP-12 passed) | ✅ 2026-10-08 | 4a and audit through `eea085a`; 4b from `397f846` |
| 5 | Grid, snapping, rulers, scrub, markers, loop: groundwork (format v4), 5a grid and snapping, 5b rulers (layout A), 5c loop and punch, 5d locators, 5e scrubbing | ✅ 2026-10-08 (ADR-0030, MT-APP-13 passed) | |
| 6 | Clip editing and selection: groundwork (format v5), 6a selection and moving, 6b time ranges and edits, 6c trim, loop-extend and slip, 6d fades and clip gain (format v6, ADR-0031; MT-APP-14 run 1 passed its steps), 6e clip properties (mute, lock, rename, colour, the pinned card) and drops on a chosen track (ADR-0032) | 🟡 built; MT-APP-14 run 2 (6e steps) to come | `f66c647` to `a6a2164`; 6e uncommitted |
| 7 | Command palette, shortcuts, context menus | 🔲 | |
| 8 | End-to-end walkthrough and report | 🔲 | |

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
| 10 | P-HOST: TyrellN6 hosted in-process and in an isolated host process. 66 µs/block round trip incl. processing; real crash detected next block, restarted with last known state in 33 ms, audio resumes; state round trip identical | ✅ | `bd57b9b`, this commit |
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
| UI 60 fps minimum, 120 fps target at 200 tracks / 20,000 clips / 100,000 notes | M | 🟡 | 3, 10 | Clips: met. With clips on the GPU (M3 slice 4b, ADR-0026), the stress project scrolls and zooms at p99 ≤ 2.3 ms at 3440 × 1380 (Ryzen 7 7700X, RTX 4070), measured by `--bench-frames`. With waveforms on every audio clip (4c, ADR-0027): p99 ≤ 2.3 ms at 3440 × 1380, unchanged. Notes (M5) still to measure; mid-range hardware in M10 |
| Near-zero idle CPU | M | 🟡 | 3, 10 | 0.0–0.2% measured with audio running (M2 MT-APP-4; M3 slice 2 with the motion system). Animations repaint only while moving. Re-measured at scale in M3 slice 4b: 3 repaints in 3 s idle with the stress project (the once-a-second device check); M10 |
| Cold start under 2 s; scanning and loading never block the UI | M | 🟡 | 3, 6, 10 | First frame 394 ms warm / 1,110 ms cold (M1 shell). Audio files, project open and save, file dialogs and ASIO driver checks all run in the background (M2, M3). Plugin scanning in M6 |
| Autosave, crash recovery, atomic saves, backups | M | ✅ | 3 | Atomic save and rolling backups (`lumen-project`). Autosave a minute after unsaved changes, to the local app data folder; a recovery banner after a crash; tested by killing the process (M3 slice 3d) |
| Crashing plugins isolated, flagged, recoverable; scanning never crashes the app | M | 🔲 | 6, 10 | P-ISO, P-VST3 |
| Device removal, rate and buffer changes, sleep and resume handled | M | 🔲 | 2, 10 | |
| Safe mode without plugins | M | 🔲 | 6 | |
| Automated tests: core logic, editing, serialization and migration, audio (golden renders) | M | 🟡 | all | Unit and integration tests per crate; an end-to-end suite that drives the whole app (CI), and real-binary smoke tests run at the end of every slice (ADR-0029, 2026-10-08). Golden renders with export (M9) |
| Benchmarks with regression gates in CI | M | ✅ | 1–3 | divan wall-clock benchmarks (smoke-run in CI), plus instruction-count gates under Callgrind that fail CI above +2% (M3 slice 1, ADR-0022). Each milestone adds its hot paths |
| CI on Windows, macOS, Linux | M | ⚠️ | 1 | First run green on 2026-10-06 (run 37396217369): Windows MSVC, macOS, Linux, rustfmt, cargo-deny, VST3 self-test. **macOS paused 2026-10-07** (owner: Actions minutes ran out); Windows and Linux run, macOS is checked by cross-target clippy until it returns |
| Test plugin simulating good and bad behaviour; real-plugin compatibility matrix | M | 🟡 | 1, 6 | Misbehaving test module (scan stage) + `docs/PLUGIN_COMPATIBILITY.md` (scan stage only) |
| Real-time correctness enforced by tests | M | 🟡 | 1–2 | `lumen-rt` allocation guard (ADR-0010) |
| High-DPI, 100–200% scaling, light and dark themes, keyboard-navigable, accessible names, colour-blind-safe | M | 🟡 | 3, 10 | Dark, light and match-system themes; interface size 100–200% in the app menu; colour-blind-safe accent; contrast tested for both themes (M3 slice 2, `docs/DESIGN.md`). Keyboard navigation of the new views and the full accessibility pass in M3 slice 8 and M10 |
| No telemetry by default; opt-in only | M | ✅ | — | Nothing collects data. Re-verify each milestone. |

## §6.1 Arrangement and tracks

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Rulers: bars/beats, minutes:seconds, samples, timecode | M | ✅ 2026-10-08 | 3 | M3 slice 5b, layout A (owner): the bar ruler on top; a time ruler along the bottom in minutes and seconds, samples, or timecode at 24, 25, 29.97 drop-frame or 30 fps (`timefmt.rs`, tested), chosen from its corner and saved in settings; the position display follows it. At the deepest zoom the bar ruler pins the position of its left edge |
| Adaptive or fixed grid (triplet, dotted, off) with snap-bypass modifier | M | ✅ 2026-10-08 | 3 | M3 slice 5a: `lumen_core::grid`, adaptive in five widths or fixed 8 bars to 1/64, triplet and dotted, snap on or off (Ctrl+1 to Ctrl+5, the grid menu and button); Alt bypasses; snap guide; saved with the project (format v4). Used by the ruler and dropped files now, by clip edits in slice 6 |
| Anchored zoom, zoom to selection, fit all, fit loop, pan, edge auto-scroll, follow modes. (Ctrl+wheel zoom works in the M1 shell since the 2026-10-06 fix: egui delivers it as a zoom factor, not as scroll.) | M | 🟡 | 3 | M3 slice 4d (ADR-0028): zoom at the pointer to single samples, `+`/`-`, X back, W fit all, H fit heights, Alt+wheel heights, pan, auto-scroll while dragging tracks, Follow page and continuous. Zoom to selection with selection (slice 6); fit loop with the command palette (slice 7) |
| Track types: audio, MIDI/instrument, group, return, master | M | 🟡 All five exist in the project model; audio tracks are created by importing (M3 slice 3). Track headers and adding each type in M3 slice 4; group summing, returns and master in M4 | 3–4 |
| Per-track name, colour, height, arm, monitor, solo, mute, polarity, I/O routing, inline fader and meter | M | 🟡 Name, type icon, colour, height (drag to resize in 22 pt steps, fold and unfold), arm (glows and breathes), solo and mute in the track header; interim volume and pan sliders in unfolded headers (M3 slice 4a, ADR-0025). Monitor, polarity, routing, real fader and meter with the mixer (M4) | 3–4 |
| Group tracks: create, drag in, nest, fold, sum, own devices/sends/fader/automation, one-step ungroup, sensible solo/mute/arm | M | 🔲 | 4 |
| Linked editing across selected tracks | S | 🔲 | 4 |
| Clips: move, copy-drag, trim, slip, fades, gain handle, loop-extend, rename, recolour, mute, lock | M | 🟡 M3 6a: move across tracks and Ctrl+copy-drag by the top half (snapped, one undo step, kinds respected, locked clips stay). 6c: trim edges (within the file), Shift loop-extend, Ctrl+Alt slip, and Ableton's overlap rule (a clip put down cuts away what it covers). 6d: fade handles with a free bend dot and Alt for S-shape (owner's F3 and C3; curves in format v6, ADR-0031), presets in the menu, a clip gain line (G1), heard and drawn alike; moved clips crossfade while playing. 6e (ADR-0032): 0 mutes (grey, the owner's M1), lock from the menu or card (padlock and clamps, K3; locked clips can still be muted, renamed and recoloured), Ctrl+R or F2 renames in the title bar, clips take their own colour, and the properties card (P1) sets all of it, gain and fades from the keyboard | 3 |
| Selection: clip, marquee, time range with cut/copy/duplicate/delete/split/consolidate | M | 🔲 | 3, 7 |
| Loop and punch regions, markers and locators, tempo and time-signature lanes | M | 🟡 Engine side of loop and punch done (slice 3); loop brace and punch (M3 5c) and named locators (M3 5d) done; tempo and time-signature lanes later in M3 | 2–3 |
| Track height presets, collapse, hide, reorder by drag, multi-select | M | 🟡 Fold and unfold, resize by dragging or Alt+wheel, H fits every track, multi-select (Ctrl, Shift), move with Alt+arrows and the menu (4a), reorder by drag with auto-scroll (4d). Hide and height presets later | 3 |
| Track templates, delay offset, lock, overview strip | S | 🔲 | 3, 9 |

## §6.2 Audio

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Import by drag and drop with sample-rate handling and optional copy into project | M | 🟡 Drag and drop from Explorer and automatic rate conversion done (M2 slice 6, ADR-0019), with a supported-file indicator while dragging. Imported audio is copied into `Samples/Imported` on save (setting, on by default; M3 slice 3c). Dropped on an audio track's lane, files land on that track at the pointer, one after another; between tracks they make new tracks there (M3 6e) | 3 |
| Drop files onto a specific track; dropping between tracks or below them creates a track (owner request, MT-MEDIA-1, 2026-10-06) | M | 🔲 | 3 |
| Clip hover card with file details (format, rate, channels, length, size, path) | S | ✅ 2026-10-08 Basic card in M2 slice 6 (owner request, MT-MEDIA-1); in M3 6e it pins open as the clip's properties card (Alt+Enter; the owner's P1) | 3 |
| Glitch-free playback, seamless loops, instant seeking | M | 🟡 Clips play sample-exactly from memory, with click-free crossfaded loops and locates (slice 6; golden renders; MT-MEDIA-1 passes by ear). Large sessions need disk streaming (M9) | 2 |
| Disk streaming of clip audio with read-ahead, so large sessions need not fit in RAM; in-memory playback becomes a per-clip RAM option (owner decision 2026-10-06: memory in M2, streaming in M9) | M | 🔲 | 9 |
| Crisp, fast waveforms at every zoom | M | ✅ | 3 | M3 slice 4c (ADR-0027): peak pyramids built at load, drawn per pixel on the GPU; no measurable frame cost with 10,000 waveforms. Disk cache deferred to M9 with disk streaming (owner approved); drawn sample by sample below 16 frames per pixel (4d, ADR-0028) |
| Recording: multi-arm, monitoring, multichannel, crash-safe capture, latency compensation and offset, punch, count-in, input meters | M | 🔲 | 9 |
| Loop recording into takes; capture MIDI | S | 🔲 | 9 |
| Clip gain, fade shapes, automatic crossfades, reverse, normalize, polarity, pitch | M | 🟡 Clip gain and fades along free curves, with four presets (M3 6d, ADR-0031; golden renders, and E2E scenarios that listen). Automatic crossfades, reverse, normalize, polarity and pitch in M7 | 3, 7 |
| Warp and time-stretch with quality modes | S | 🔲 | 7 |
| Take lanes and comping | S | 🔲 | 9 |
| Mono, stereo, multichannel material | M | 🔲 | 2–3 |

## §6.3 MIDI

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Record, overdub, replace, count-in, punch, thru, channel filter, computer keyboard | M | 🔲 | 5 |
| Notes never hang; panic command | M | 🔲 | 5 |
| Sustain, pitch bend, aftertouch, program and bank change, MIDI clock out | M | 🔲 | 5 |
| Piano roll: draw, select, move, resize, split, glue, mute, duplicate, nudge, transpose, legato, audition, velocity and controller lanes, synced zoom | M | 🔲 | 5 |
| Scale highlight and snap, fold, ghost notes, multi-clip editing, step input | S | 🔲 | 5 |
| Quantize: grids, modes, strength, swing, sensitivity, iterative, one-key apply, live-preview popover, humanize, legato, fixed length, transpose, scale-constrain, reverse | M | 🔲 | 5 |
| Restore recorded timing, groove templates, audio quantize | S | 🔲 | 5, 7 |
| Standard MIDI File import and export | M | 🔲 | 5 |
| MIDI learn with mapping list | M | 🔲 | 5 |
| MIDI effects (arpeggiator, chord, scale, velocity, length) | S | 🔲 | 8 |
| MPE, MIDI 2.0 | L | 🔲 | — |

## §6.4 Drum maps

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Map entries: name, input and output note, channel, routing, mute, solo, quantize, length, colour, choke group | M | 🔲 | 8 |
| Non-destructive storage in input notes; separate convert command | M | 🔲 | 8 |
| GM, GM2 and 808 presets; user maps; import and export; drum names in piano roll | M | 🔲 | 8 |
| Cubase drum map import | L | 🔲 | — |
| Drum editor: lanes with audition, mute, solo, velocity, per-lane quantize, reorder, hide unused | M | 🔲 | 8 |
| Step-sequencer view: step count, resolution, toggle, velocity drag, accent | M | 🔲 | 8 |
| Euclidean and random fill | L | 🔲 | — |
| Per-sound routing to multi-output instrument channels | M | 🔲 | 8 |

## §6.5 Automation and envelopes

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Everything with a value is automatable | M | 🔲 | 7 |
| Lanes with searchable parameter chooser; show/hide key | M | 🔲 | 7 |
| Point editing, ranges, curve types, shape tools, freehand with simplification, snapping, value tooltip | M | 🔲 | 7 |
| Clip envelopes (audio gain/pan/pitch, MIDI CC/bend/pressure), looping, lock-envelopes option | M | 🔲 | 7 |
| Read / Write / Touch / Latch recording with thinning | M | 🔲 | 7 |
| Trim mode | L | 🔲 | — |
| Override state with one-click re-enable | M | 🔲 | 7 |
| Smooth, sample-accurate automation for built-ins and plugins | M | 🔲 | 7 |
| Tempo and time-signature changes recalculate the timeline | M | 🟡 The engine keeps the bar and beat across tempo and meter changes, and loop and punch follow (slice 3); clips and automation follow when they exist | 7 |

## §6.6 Editing actions

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Split (playhead, cursor, grid, range edges), click-free | M | 🔲 | 7 |
| Join / glue MIDI; consolidate audio (rendered) and MIDI (merged), originals kept | M | 🔲 | 7 |
| Trim, crop, delete, ripple delete, insert silence, cut, copy, paste, duplicate clip and range, loop/unloop, reverse, normalize, nudge, move, rename, recolour, mute, lock | M | 🔲 | 3, 7 |
| Bounce in place; render selection to new track | M | 🔲 | 7 |
| Freeze, unfreeze, flatten | M | 🔲 | 7 |
| Select all / none / invert / in track / to start / to end | M | 🔲 | 3 |
| Slice audio to MIDI or drum map | S | 🔲 | 8 |
| Group and ungroup clips | L | 🔲 | — |
| Guarantees: split+join is identity; consolidating unprocessed audio is identical; undo restores exactly | M | 🟡 | 3, 7 | Undo restores exactly by construction (shared snapshots, ADR-0024) and is property-tested; split, join and consolidate in M7 |

## §6.7 Mixer, routing and grouping

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Channel strip: input gain, polarity, devices, pre/post sends, fader, pan law, mute, solo, cue, output | M | 🔲 | 4 | Must smooth gain and pan, and fade mute and solo, as the interim clip-level glide does in M3 (ADR-0025) |
| Returns, groups, master, sidechain from any track, hardware output routing | M | 🔲 | 4 |
| Solo keeps soloed paths audible; exclusive-solo option | M | 🔲 | 4 |
| Automatic plugin delay compensation with indicator | M | 🔲 | 4, 6 |
| Peak (hold, clip), RMS; master LUFS and true peak | M | 🔲 | 4 |
| Dockable mixer panel; inline fader, pan and meter on track headers | M | 🔲 | 4 |
| Routing matrix; master mono-check and dim | S | 🔲 | 4 |
| Reference-track A/B | L | 🔲 | — |

## §6.8 Plugins and devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Non-blocking, crash-safe, cached scanning; blocklist; rescan; custom paths | M | 🔲 | 6 |
| Multi-output instruments, sidechain inputs, latency and tail handling, bypass | M | 🔲 | 6 |
| Full state save and restore; preset browsing, loading and saving | M | 🔲 | 6 |
| Every parameter automatable and MIDI-mappable; generic parameter UI | M | 🔲 | 6–7 |
| Floating editors: remembered position and size, resizing, high-DPI, keyboard focus | M | 🔲 | 6 |
| Crash and hang isolation with recovery; separate-process isolation by 1.0 | M | 🔲 | 6, 10 |
| Device chain: cards, bypass, presets, parameter strip, reorder, copy between tracks, ordering rules | M | 🔲 | 6 |
| Built-ins: utility, parametric EQ with analyzer, compressor with sidechain, limiter, delay, reverb, polysynth, sampler | M | 🔲 | 8 |
| Spectrum analyzer, tuner | S | 🔲 | 8 |
| Device browser: categories, vendors, search, favourites, recents, drag, double-click | M | 🔲 | 6 |
| CLAP hosting; racks with macros | L | 🔲 | — |
| VST3 scanning and hosting on macOS and Linux, with the isolation self-test in CI on all platforms (owner, 2026-10-06) | L | 🔲 | Stretch |

## §6.9 Audio and MIDI devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Driver types: ASIO and WASAPI shared (Windows), CoreAudio, PipeWire/JACK/ALSA | M | 🟡 Output works on ASIO, WASAPI shared (hardware-tested) and cpal (CoreAudio/ALSA, compiled but not hardware-tested). Missing: WASAPI and cpal input (slice 8), JACK/PipeWire (ADR-0016) | 2 |
| WASAPI exclusive | S | 🟡 Output tested on hardware: 3 ms on HDMI with events; the Focusrite switches to polling automatically (MT-AUDIO-4, -5). Input in slice 8 | 2 |
| Device, rate, buffer selection with input/output/round-trip latency display | M | ✅ Audio settings window (M2 slice 8, ADR-0021): driver, device, rate, buffer and live latencies. ASIO buffer sizes are set in the driver's control panel, as in Ableton; WASAPI shared follows Windows. MT-APP-6 passes | 2 |
| Device loss and return: keep the session, show the lost state, reconnect automatically (found by MT-ASIO-5) | M | ✅ Detection and auto-reconnect, tested with the fake backend and on hardware (MT-AUDIO-1, -4); lost, busy and failed banners with a way forward (slice 8) | 2 |
| Channel selection with friendly names; mono and stereo configurations | M | 🟡 Output pairs and ASIO input pairs by name (mono for an odd last channel), one pair at a time; more routing with the mixer (M4) | 2 |
| Audio input on WASAPI, CoreAudio and Linux, with drift compensation between separate devices (owner decision 2026-10-06: deferred from M2 slice 8; ASIO input works in M2) | M | 🔲 | 9 |
| Test tone, CPU load, dropout counter, Audio On/Off | M | ✅ All four in the audio panel (M2 slice 7, ADR-0020); test tone also in the audio settings window (slice 8) | 2 |
| ASIO control panel; driver resets and rate/buffer changes without losing the session; busy/missing messages | M | ✅ Control panel button; resets (MT-AUDIO-2); busy, lost and failed banners with Try again / Audio settings; missing saved device falls back with a notice; a device that fails to start returns to the previous one. MT-APP-6 passes | 2 |
| Runtime changes never crash; restore a working configuration; persisted settings | M | ✅ Driver and device switches at run time move the engine to a fresh control thread; refused changes revert with the reason; settings saved atomically once a setup works. MT-APP-6 passes | 2 |
| MIDI devices: enable toggles, hot-plug, per-track MIDI From/To, virtual ports | M | 🔲 | 5 |

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
| Transport: play, stop, record, loop, return to start, go to start/end, previous/next marker. **Owner request:** transport controls use vector icons, not text (M2). | M | 🟡 Engine transport done (slice 3, ADR-0017). Transport bar with vector icons, Space / Shift+Space / Home, go to start, stop, play/pause and loop in the app (slice 4, ADR-0018). Record in M9; markers and go to end in M3 |
| Tempo 20–999 BPM and time signature with change maps; tap tempo | M | 🟡 Tempo and time-signature maps with changes (`lumen-core`), swapped into the engine live (slice 3). Tap tempo and the UI later |
| Metronome: accent, volume, sound, count-in, pre-roll, record-only, output routing | M | 🟡 On/off, accented downbeat, volume and two synthesized sounds, sample-accurate (M2 slice 7, ADR-0020). Count-in, pre-roll and record-only with recording (M9); output routing with the mixer (M4); settings saved with preferences (M9) |
| Loop region, punch in/out, auto-punch | M | 🟡 Sample-accurate loop wrap and punch-region reporting in the engine (M2 slice 3). M3 slice 5c: the loop brace is edited on the strip (draw, move, resize, snapped; click toggles), undoable; punch in and out at the loop's edges, as in Ableton, from the loop menus. Recording through punch in M9 |
| Markers with names and navigation | M | ✅ 2026-10-08 M3 slice 5d: named locators on the strip, set or removed at the playhead (M, Set), jumped between (Ctrl+Left/Right, ◂ ▸, click a flag), dragged (snapped), renamed (double-click, F2), deleted; undoable, saved (format v4) |
| Multiple time formats; editable position field | M | 🔲 |
| Edit cursor separate from playhead; scrubbing. **Owner request (2026-10-06), raised from S to M, Ableton-style:** click in the scrub strip to set the playback position; drag to scrub audibly; drag in the beat ruler vertically to zoom and horizontally to scroll. Locate-on-click in M2; scrub strip, audible scrubbing and ruler zoom in M3. | M | 🟡 Click-to-locate on the ruler done (M2 slice 5), snapping to the editing grid (M3 5a). Ruler drag: down or up zooms, sideways scrolls, the grabbed beat stays under the pointer (M3 5b). M3 5e: the scrub strip (the strip's top, around the locator flags): click jumps (snapped); drag plays the grid step under the pointer in a loop at normal pitch, Alt scrubs like tape (speed and pitch follow the hand); while playing, a drag jumps playback. An engine scrub mode, real-time safe (ADR-0030). An edit cursor separate from the playhead is still to come |
| MIDI clock / MTC out | S | 🔲 |
| Clock / MTC in, Ableton Link | L | 🔲 |
| Tracks: add, delete, duplicate, rename, recolour, reorder, multi-select | M | 🟡 All of these from the track menu, the "+ Add track" menu button and keys (Ctrl+T, Ctrl+Shift+T, Ctrl+D, Delete, F2, Alt+arrows), each one undo step (M3 slice 4a). Drag to reorder (slice 4d) |
| Track types with routing, arm, monitor, polarity | M | 🔲 |
| Freeze, unfreeze, flatten | M | 🔲 |
| Track delay, lock, icons, templates | S | 🔲 |
| Select, marquee, range; cut/copy/paste/duplicate/delete/ripple delete/insert silence | M | 🟡 M3 6a: click, Ctrl and Shift selection, marquee, time ranges across tracks (a clip's bottom half). M3 6b (`lumen_project::edit`): cut, copy, paste (at the playhead or into a range, overwriting as Ableton), duplicate, delete, delete time (ripple, the whole song) and insert time, on clips and ranges, each one undo step, in the lanes' right-click menu. Crop and the remaining range edits come with M7's split |
| Split, join, consolidate, trim, crop, slip, nudge, move, loop/unloop | M | 🔲 |
| Fades, crossfades, clip gain, reverse, normalize, polarity | M | 🔲 |
| Snap and grid; zoom; clip name/colour/mute/lock; clip properties | M | 🟡 | Grid and snapping (M3 5a) and zoom (4d) done; clip name, colour, mute, lock and the properties card built in M3 6e (ADR-0032), awaiting MT-APP-14 run 2 |
| Warp, transient and tempo detection, pitch shift | S | 🔲 |
| Take lanes and comping | S | 🔲 |
| Unlimited undo/redo with visible history | M | 🟡 Undo and redo for imports, tempo and time signature (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y), a History window, and a note after each undo (M3 slice 3, ADR-0024). Every later edit joins it as it is built |
| Audio recording with monitoring, multitrack, input meters | M | 🔲 |
| Latency compensation with manual offset; crash-safe capture | M | 🔲 |
| MIDI recording: overdub/replace, count-in, punch | M | 🔲 |
| Computer keyboard as MIDI input | M | 🔲 |
| Loop recording into takes; capture MIDI | S | 🔲 |
| Piano roll, velocity and controller lanes, quantize, humanize, transpose, legato | M | 🔲 |
| MIDI thru, channel filter, panic, sustain/bend/aftertouch, program/bank | M | 🔲 |
| SMF import/export; MIDI learn | M | 🔲 |
| Drum maps, drum editor, step sequencer | M | 🔲 |
| MIDI effects; on-screen keyboard | S | 🔲 |
| MPE, MIDI 2.0 | L | 🔲 |
| Automation: all parameters, curves, R/W/T/L, clip envelopes, tempo and signature, override, simplification | M | 🔲 |
| Mixer: fader, pan law, mute, solo, sends, returns, groups, master, routing, sidechain, PDC, meters, cue | M | 🔲 |
| LUFS and true-peak metering; mono-check and dim | S | 🔲 |
| Reference track | L | 🔲 |
| Async cached VST3 scanning, custom paths, blocklist, search, favourites, recents | M | 🔲 |
| Instruments and effects; multi-out; sidechain; bypass; presets; state; automation; generic UI; floating editors with DPI and focus | M | 🔲 |
| Latency compensation; crash isolation; safe mode | M | 🔲 |
| Built-in essentials | M | 🔲 |
| Spectrum analyzer, tuner | S | 🔲 |
| CLAP, racks with macros | L | 🔲 |
| Browser: devices, samples, projects, user places; search; audition; drag and drop | M | 🔲 |
| Tags and favourites; tempo-synced audition | S | 🔲 |
| Media pool with relink and purge | M | 🔲 |
| Project management: new/open/save/as/copy/template/revert/recent; autosave and recovery; backups; collect; relink; migrations; settings | M | 🔲 |
| Project notes and metadata | S | 🔲 |
| Export: master and stems; WAV/FLAC; bit depth; dither; offline and real-time; scope; tails; normalize | M | 🔲 |
| MP3/AAC | L | 🔲 |
| Preferences: audio, MIDI, plugin paths, file/cache locations, theme, scale, autosave interval, undo depth, track defaults, record settings | M | 🟡 Audio settings window (M2); theme, interface size, Reduce motion and copy-imported-audio in the app menu (M3). The Preferences window in M9 |
| Shortcut editor with conflict detection; Ableton-compatible default keymap | M | 🔲 |
| FL-style keymap preset | L | 🔲 |
| First-run wizard (audio device, plugin paths) | M | 🔲 |
| Preferences import/export; localization-ready text | S | 🔲 |
| Command palette; tooltips with shortcuts; context menus everywhere | M | 🔲 |
| Design system with a crisp vector icon set (spec §5): transport icons (play, pause, stop, record, loop, metronome) in M2, full set with the design system in M3 | M | 🟡 Design system v1 (`docs/DESIGN.md`, `theme.rs`): colour tokens for both themes, type, spacing, shape, widget states, key flows; M2 UI moved onto it (M3 slice 2). Icons grow with each slice's new controls |
| Motion system: tokens, easing, what animates, reduce-motion; never delays input, no idle redraws; M2 widgets retrofitted (owner request 2026-10-06: fluid, Apple-like feel). Built in M3 with the design system; final polish pass in M10 | M | 🟡 Built (M3 slice 2, `motion.rs`): FAST/BASE/GLIDE tokens, ease-out and critically damped spring, Reduce motion; panels slide, popovers and windows fade, notices and banners animate, transport buttons fade on hover, playhead and start marker glide after a locate, clips fade in. Idle CPU unchanged (0.2%). Owner signed off (MT-APP-7, 2026-10-07); M10 polish |
| Reset to default, type-in, fine adjust, wheel adjust, parameter right-click menus | M | 🔲 |
| Drag and drop for files, devices, clips, tracks; status-bar hints | M | 🟡 Files from Explorer, with a live indicator of what will be added (M2). Clips and tracks in M3, devices in M6 |
| Resizable, collapsible panels with remembered layout; full screen; high-DPI; light and dark | M | 🟡 Resizable panels that slide open and closed; layout saved with each project; dark, light and match-system themes; interface size 100–200% (M3 slices 2–3). Full screen later |
| Keyboard-only core flows; accessible control names. Open conflict: Shift+Tab is the spec's detail-panel shortcut and also the standard backward-focus key (ADR-0015). | M | 🔲 |
| Detachable mixer and editor windows; hover help; welcome screen | S | 🔲 |
| CPU, disk and dropout indicators; Audio On/Off; panic | M | 🟡 CPU (peak per second), dropouts, Audio On/Off and panic done (M2 slice 7). Disk indicator with disk streaming (M9); MIDI all-notes-off joins panic in M5 |
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
  distribution. Lumen's own licence is decided: closed source (ADR-0012).
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
| VST3 and isolation approach demonstrated | ✅ Real plugin (TyrellN6) processes audio in an isolated host process, survives a crash and is restored from its last known state (P-HOST, 2026-10-06) |

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
