# Lumen — Progress

## Status

**Current milestone:** 1 — Foundations & Architecture
**Overall status:** In progress (see [milestones/M1.md](milestones/M1.md))

Statuses: 🔲 Not started · 🟡 In progress · ✅ Complete · ⚠️ Partial / known limitation · ⏸️ Deferred with approval · ❌ Blocked

## Milestones

| # | Milestone | Status | Notes |
|---|---|---|---|
| 1 | Foundations & Architecture | 🟡 In progress | Plan and risk register in `docs/milestones/M1.md` |
| 2 | Audio Engine, Devices & Transport | 🔲 | |
| 3 | Arrangement & Project System | 🔲 | |
| 4 | Mixer, Routing & Groups | 🔲 | |
| 5 | MIDI & Piano Roll | 🔲 | |
| 6 | VST3 Hosting & Device System | 🔲 | |
| 7 | Automation, Envelopes & Advanced Editing | 🔲 | |
| 8 | Drum Maps, Drum Editor & Built-In Devices | 🔲 | |
| 9 | Recording, Media, Export & Production Workflow | 🔲 | |
| 10 | Hardening, Performance & Release | 🔲 | |

## Milestone 1 slices

| # | Slice | Status | Commit |
|---|---|---|---|
| 1 | Repository, workspace, CI, license policy, RT allocation guard, benchmark harness, plan and ADRs | ✅ | `8d8f0a2` |
| 2 | P-ENGINE: real-time core prototype. All criteria met: 0 heap operations, bit-identical across block sizes, 64 tracks × 128 frames = 0.85% of budget | ✅ | `5084725` |
| 3 | P-ASIO: native ASIO probe. 7 drivers queried safely; 30 s Focusrite streams with 0 discontinuities and 0 heap operations; 64-frame ASIO4ALL stable | ✅ | `7a85a6f` |
| 4 | P-ISO: isolation transport. 1 host p99 19.5 µs, 8 hosts p99 35.5 µs at 128 frames; crash and hang detected and replaced in about 11–13 ms with audio uninterrupted | ✅ | `0fbaf7d` |
| 5 | P-VST3: out-of-process scanning. 39 of 40 real modules enumerated in 10.4 s; moduleinfo matches 4 of 4; crash, hang and no-factory test modules contained (CI self-test) | ✅ | (this commit) |
| 6 | P-FORMAT: project format and time base | 🔲 | |
| 7 | P-UI: dense-view rendering stress test | 🔲 | |
| 8 | Evidence in ADRs, architecture doc, milestone report | 🔲 | |

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
| Sample-accurate playback, MIDI, automation, loops, editing | M | 🔲 | 2–7 | ADR-0009 time base |
| Offline render equals real-time and is repeatable | M | 🔲 | 2, 9 | P-ENGINE block-size invariance |
| UI 60 fps minimum, 120 fps target at 200 tracks / 20,000 clips / 100,000 notes | M | 🔲 | 3, 10 | P-UI |
| Near-zero idle CPU | M | 🔲 | 3, 10 | P-UI |
| Cold start under 2 s; scanning and loading never block the UI | M | 🔲 | 3, 6, 10 | |
| Autosave, crash recovery, atomic saves, backups | M | 🔲 | 3, 9 | P-FORMAT (atomic save) |
| Crashing plugins isolated, flagged, recoverable; scanning never crashes the app | M | 🔲 | 6, 10 | P-ISO, P-VST3 |
| Device removal, rate and buffer changes, sleep and resume handled | M | 🔲 | 2, 10 | |
| Safe mode without plugins | M | 🔲 | 6 | |
| Automated tests: core logic, editing, serialization and migration, audio (golden renders) | M | 🟡 | all | Harness in place |
| Benchmarks with regression gates in CI | M | 🟡 | 1–2 | divan harness; gates in M2 (ADR-0011) |
| CI on Windows, macOS, Linux | M | 🟡 | 1 | Workflow written; no remote yet, so never run |
| Test plugin simulating good and bad behaviour; real-plugin compatibility matrix | M | 🟡 | 1, 6 | Misbehaving test module (scan stage) + `docs/PLUGIN_COMPATIBILITY.md` (scan stage only) |
| Real-time correctness enforced by tests | M | 🟡 | 1–2 | `lumen-rt` allocation guard (ADR-0010) |
| High-DPI, 100–200% scaling, light and dark themes, keyboard-navigable, accessible names, colour-blind-safe | M | 🔲 | 3, 10 | P-UI checks AccessKit |
| No telemetry by default; opt-in only | M | ✅ | — | Nothing collects data. Re-verify each milestone. |

## §6.1 Arrangement and tracks

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Rulers: bars/beats, minutes:seconds, samples, timecode | M | 🔲 | 3 |
| Adaptive or fixed grid (triplet, dotted, off) with snap-bypass modifier | M | 🔲 | 3 |
| Anchored zoom, zoom to selection, fit all, fit loop, pan, edge auto-scroll, follow modes | M | 🔲 | 3 |
| Track types: audio, MIDI/instrument, group, return, master | M | 🔲 | 3–4 |
| Per-track name, colour, height, arm, monitor, solo, mute, polarity, I/O routing, inline fader and meter | M | 🔲 | 3–4 |
| Group tracks: create, drag in, nest, fold, sum, own devices/sends/fader/automation, one-step ungroup, sensible solo/mute/arm | M | 🔲 | 4 |
| Linked editing across selected tracks | S | 🔲 | 4 |
| Clips: move, copy-drag, trim, slip, fades, gain handle, loop-extend, rename, recolour, mute, lock | M | 🔲 | 3 |
| Selection: clip, marquee, time range with cut/copy/duplicate/delete/split/consolidate | M | 🔲 | 3, 7 |
| Loop and punch regions, markers and locators, tempo and time-signature lanes | M | 🔲 | 2–3 |
| Track height presets, collapse, hide, reorder by drag, multi-select | M | 🔲 | 3 |
| Track templates, delay offset, lock, overview strip | S | 🔲 | 3, 9 |

## §6.2 Audio

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Import by drag and drop with sample-rate handling and optional copy into project | M | 🔲 | 3 |
| Glitch-free playback, seamless loops, instant seeking | M | 🔲 | 2 |
| Crisp, fast waveforms at every zoom | M | 🔲 | 3 |
| Recording: multi-arm, monitoring, multichannel, crash-safe capture, latency compensation and offset, punch, count-in, input meters | M | 🔲 | 9 |
| Loop recording into takes; capture MIDI | S | 🔲 | 9 |
| Clip gain, fade shapes, automatic crossfades, reverse, normalize, polarity, pitch | M | 🔲 | 3, 7 |
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
| Tempo and time-signature changes recalculate the timeline | M | 🔲 | 7 |

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
| Guarantees: split+join is identity; consolidating unprocessed audio is identical; undo restores exactly | M | 🔲 | 7 |

## §6.7 Mixer, routing and grouping

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Channel strip: input gain, polarity, devices, pre/post sends, fader, pan law, mute, solo, cue, output | M | 🔲 | 4 |
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

## §6.9 Audio and MIDI devices

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| Driver types: ASIO and WASAPI shared (Windows), CoreAudio, PipeWire/JACK/ALSA | M | 🔲 | 2 |
| WASAPI exclusive | S | 🔲 | 2 |
| Device, rate, buffer selection with input/output/round-trip latency display | M | 🔲 | 2 |
| Channel selection with friendly names; mono and stereo configurations | M | 🔲 | 2 |
| Test tone, CPU load, dropout counter, Audio On/Off | M | 🔲 | 2 |
| ASIO control panel; driver resets and rate/buffer changes without losing the session; busy/missing messages | M | 🔲 | 2 |
| Runtime changes never crash; restore a working configuration; persisted settings | M | 🔲 | 2 |
| MIDI devices: enable toggles, hot-plug, per-track MIDI From/To, virtual ports | M | 🔲 | 5 |

## §6.10 Projects, media and export

| Requirement | Tier | Status | Planned |
|---|---|---|---|
| New, open, save, save as, copy, template, revert, recent, unsaved indicator | M | 🔲 | 3, 9 |
| Human-friendly, versioned format that opens old projects forever; relative paths | M | 🔲 | 1, 3 |
| Autosave, recovery, rolling backups, collect and save, relink, media pool with purge | M | 🔲 | 3, 9 |
| Export: project, loop or selection; master or stems; WAV 16/24/32f, FLAC; SRC; dither; normalize; tails; offline or real-time; progress and cancel; loudness report | M | 🔲 | 9 |
| MP3 and AAC export | L | 🔲 | — |
| Project settings | M | 🔲 | 9 |
| Project notes | S | 🔲 | 9 |

## §7 Standard DAW checklist

| Item | Tier | Status |
|---|---|---|
| Transport: play, stop, record, loop, return to start, go to start/end, previous/next marker | M | 🔲 |
| Tempo 20–999 BPM and time signature with change maps; tap tempo | M | 🔲 |
| Metronome: accent, volume, sound, count-in, pre-roll, record-only, output routing | M | 🔲 |
| Loop region, punch in/out, auto-punch | M | 🔲 |
| Markers with names and navigation | M | 🔲 |
| Multiple time formats; editable position field | M | 🔲 |
| Edit cursor separate from playhead; scrubbing | S | 🔲 |
| MIDI clock / MTC out | S | 🔲 |
| Clock / MTC in, Ableton Link | L | 🔲 |
| Tracks: add, delete, duplicate, rename, recolour, reorder, multi-select | M | 🔲 |
| Track types with routing, arm, monitor, polarity | M | 🔲 |
| Freeze, unfreeze, flatten | M | 🔲 |
| Track delay, lock, icons, templates | S | 🔲 |
| Select, marquee, range; cut/copy/paste/duplicate/delete/ripple delete/insert silence | M | 🔲 |
| Split, join, consolidate, trim, crop, slip, nudge, move, loop/unloop | M | 🔲 |
| Fades, crossfades, clip gain, reverse, normalize, polarity | M | 🔲 |
| Snap and grid; zoom; clip name/colour/mute/lock; clip properties | M | 🔲 |
| Warp, transient and tempo detection, pitch shift | S | 🔲 |
| Take lanes and comping | S | 🔲 |
| Unlimited undo/redo with visible history | M | 🔲 |
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
| Preferences: audio, MIDI, plugin paths, file/cache locations, theme, scale, autosave interval, undo depth, track defaults, record settings | M | 🔲 |
| Shortcut editor with conflict detection; Ableton-compatible default keymap | M | 🔲 |
| FL-style keymap preset | L | 🔲 |
| First-run wizard (audio device, plugin paths) | M | 🔲 |
| Preferences import/export; localization-ready text | S | 🔲 |
| Command palette; tooltips with shortcuts; context menus everywhere | M | 🔲 |
| Reset to default, type-in, fine adjust, wheel adjust, parameter right-click menus | M | 🔲 |
| Drag and drop for files, devices, clips, tracks; status-bar hints | M | 🔲 |
| Resizable, collapsible panels with remembered layout; full screen; high-DPI; light and dark | M | 🔲 |
| Keyboard-only core flows; accessible control names | M | 🔲 |
| Detachable mixer and editor windows; hover help; welcome screen | S | 🔲 |
| CPU, disk and dropout indicators; Audio On/Off; panic | M | 🔲 |
| Crash report file; logging with "reveal logs" | M | 🔲 |
| Device hot-unplug, sleep/resume, rate-change handling | M | 🔲 |
| Single instance; file association; open by double-click or drop | M | 🔲 |
| Opt-in telemetry only | M | ✅ |
| Update checker; licensing/activation | L | 🔲 |
| Windows installer and portable build; third-party licence listing; VST and ASIO notices; SDK compliance | M | 🔲 |
| Notarized macOS app | S | 🔲 |
| Linux packages | L | 🔲 |

---

## Milestone reports

### Milestone 1
**Status:** In progress. The report is written when the milestone ends.
