# Build "Lumen": a production-grade DAW in Rust

> **How to use this file**
> - Save it in the repo root as `CLAUDE.md` (Claude Code loads it every session), or paste it as your first message.
> - This document defines **what** to build and **how good it must be**. You own the **how**: architecture, libraries, structure and algorithms are your decisions.
> - "Lumen" is the product name. Keep it defined in one place in the code and docs so it is easy to change if needed.
> - Tier legend: **[M]** must ship in 1.0 · **[S]** should ship in 1.0 · **[L]** later / post-1.0.

---

## Documentation naming rule (owner, 2026-10-14)

Before writing or reviewing any documentation, read
[lumen-documentation](.agents/skills/lumen-documentation/SKILL.md).
Use product-neutral language: do not name third-party software products,
plugins or vendors in comparisons, design references, endorsements or
examples. State the behavior directly and use consistent anonymous labels
for external plugin test records. Retain technical identifiers needed for
accuracy (platforms, languages, tools, dependencies, APIs, formats,
commands, paths and hardware identities), plus required legal attribution,
license and trademark notices. This owner rule supersedes older naming
and comparison wording without changing feature requirements.

## 1. Mission

Build **Lumen**, a professional-grade desktop digital audio workstation written in **Rust**.

It combines **an arrangement-first workflow** with a **clean, polished interface**. There is no Session view and no clip launcher: the arrangement is the whole product. It must host **VST3** instruments and effects through a robust, horizontal device-chain system, support **ASIO** with a proper device selector, work with **MIDI devices and audio instruments**, and provide a full MIDI editor with **quantize**, **per-sound drum maps**, **group tracks**, **automation and envelopes**, and the standard editing actions (split, consolidate, fades and more).

**Platforms:** Windows 10/11 is first-class (ASIO is mandatory). macOS and Linux follow from the same codebase.

**Priority order when requirements conflict:**
1. Never lose user work; never interrupt the audio.
2. UX quality and responsiveness.
3. Performance.
4. Feature completeness and standard-DAW behavior.
5. Internal elegance.

---

## 2. Product principles

- **UX is the most important requirement.** Every feature is judged by how few steps it takes, how discoverable it is, and whether it keeps the producer in flow.
- **One view, done extremely well.** The arrangement timeline is the center of the app. Browser, device chain, clip editors and mixer exist to support it.
- **Minimalist and clean, but never limiting.** Quiet by default, with progressive disclosure. The design and the product must be flexible enough that new capabilities can be pulled in later without redesign.
- **Fast.** The app feels instant: startup, scrolling, zooming, dragging, loading, editing.
- **Production grade.** Stable under long sessions and big projects. Plugins misbehaving must not take down the session. Work is never lost.
- **Keyboard and mouse are equal citizens.** Everything is reachable by shortcut and by a searchable command palette.
- **Everything is undoable. Nothing blocks.** No modal dialogs for edits; long operations run in the background with progress and cancel.

---

## 3. Working agreement

- **You make the technical decisions.** Choose the architecture, libraries, UI approach, project format and algorithms. Constraints that are fixed: Rust, VST3 hosting, ASIO support on Windows. Everything else is open.
- **Record significant decisions** as short decision records (context, options, choice, consequences), including the UI approach and anything with licensing implications.
- **Plan before building.** For each milestone write a short plan, build in vertical slices that work end to end, keep the main branch green, and finish with a brief report: what shipped, what was deferred and why, measured performance, open risks.
- **Validate risky choices early** with small prototypes and measurements, especially the UI approach for the arrangement and piano roll, ASIO integration, and plugin hosting.
- **No fake completeness.** Don't stub a feature and call it done. Track every feature in `docs/PROGRESS.md` against this document, with its status and tier.
- **Be honest about testing.** Say what was verified and what could not be (for example ASIO hardware, physical MIDI controllers, specific third-party plugins). Put manual test scripts in `docs/MANUAL_TESTS.md`.
- **Licensing.** Prefer permissively licensed dependencies. Flag anything needing a human decision (ASIO and VST3 SDK terms, time-stretch engines, GPL components, trademark notices for VST and ASIO).
- **Ask the human only when blocked** on something irreversible or external (naming, licensing, signing certificates, paid SDKs). Otherwise choose the most standard, most user-friendly behavior, log the assumption in `docs/DECISIONS.md`, and continue.
- **Pause for approval** at the end of each milestone before starting the next.

---

## 4. Quality bar

These are outcomes to meet and measure, not instructions on how to reach them.

**Audio**
- No dropouts, clicks or glitches during normal use, including while editing, scrolling, scanning plugins, loading projects or opening plugin windows.
- Stable at small buffer sizes (64–128 samples) on a mid-range modern machine with a 64-track project: audio processing stays comfortably under half of its time budget at 128 samples/48 kHz.
- Lowest practical monitoring latency; the app adds no buffering beyond what the driver requires.
- Sample-accurate timing for playback, MIDI, automation, loops and editing.
- Offline rendering (export, bounce, freeze) produces the same result as real-time playback and is repeatable.

**Responsiveness**
- UI at 60 fps minimum, 120 fps target, while scrolling, zooming and dragging in a project with 200 tracks, 20,000 clips and 100,000 notes.
- Near-zero CPU use when idle.
- Cold start to usable window in under 2 seconds; plugin scanning and project loading never block the UI.

**Reliability**
- Autosave, crash recovery, safe atomic saves, backups. A crash must never cost more than the last couple of minutes of work.
- A misbehaving or crashing plugin must not crash the app or corrupt the project; it is isolated, flagged, and recoverable. Scanning plugins can never crash the app.
- Graceful handling of audio device removal, sample-rate or buffer-size changes, and sleep/resume.
- A safe mode that starts without loading plugins.

**Quality assurance**
- Automated tests for core logic, editing operations, serialization and migration, and audio correctness (including golden renders), plus performance benchmarks with regression gates in CI.
- Continuous integration on Windows, macOS and Linux.
- A test plugin that simulates good and bad plugin behavior, and a compatibility matrix of real-world plugins.
- Real-time correctness enforced by tests, not by convention.

**Accessibility and scale**
- High-DPI, UI scaling (100–200%), light and dark themes, keyboard-navigable core workflows, accessible control names, color-blind-safe defaults.

**Privacy**
- No telemetry by default. Any analytics or crash reporting is opt-in.

---

## 5. UX and design direction

**Layout.** A single window:
- Slim **transport bar** at the top: play, stop, record, loop, tempo, time signature, position, metronome, CPU/audio status.
- Collapsible **browser** on the left: devices, samples, projects, user locations; search; audition.
- **Arrangement** in the center: track headers, ruler with markers/loop/tempo lanes, timeline, optional automation lanes.
- Collapsible **detail panel** at the bottom with tabs for device chain, clip editor (piano roll, drum editor, audio editor) and mixer.
- Plugin editors open in floating windows.

**Look and feel.** Minimalist and clean, dark by default with a light theme. Strong contrast on content (clips, notes, waveforms), quiet chrome, crisp vector icons, a single accent color, a muted track-color palette, restrained and purposeful motion. Clear content and restrained visual density. Before building the UI, produce a short **design system and key-flow spec** (layout, color, type, spacing, core widgets, the main workflows) and use it consistently everywhere.

**Interaction principles**
- Direct manipulation first: drag to move, trim edges, drag fade handles, draw notes and envelopes, double-click to open editors, right-click for context menus.
- A simple tool model (pointer, draw, erase, split, mute, zoom) with single-key switching and temporary modifier overrides.
- Identical snapping, selection, zoom and drag behavior in every editor.
- Immediate feedback: hover states, cursor changes, live value tooltips, ghost previews, visible snap guides.
- Designed empty, error and warning states (missing media with relink, plugin crashed with reload, audio device lost with recovery).
- Double-click resets a control to default; type-in values; fine adjustment with a modifier; mouse wheel adjustment; right-click a parameter for automation and MIDI mapping.
- Searchable **command palette** (Ctrl/Cmd+K) covering every command; tooltips show shortcuts; fully customizable shortcuts with conflict detection.

**Starting shortcuts** (all remappable; default where sensible): Space play/stop · Ctrl/Cmd+Z / Shift+Z undo/redo · Ctrl/Cmd+D duplicate · Ctrl/Cmd+E split · Ctrl/Cmd+J consolidate · Ctrl/Cmd+G group / Shift ungroup · Q quantize, Shift+Q options · B draw mode · A automation view · Z / X zoom to selection / back · Ctrl/Cmd+L loop selection · Ctrl/Cmd+K command palette · Ctrl/Cmd+S save · Shift+Tab toggle detail panel · F9 record · 0 mute clip.

**UX acceptance checklist (applies to every feature)**
1. Works with mouse only and with keyboard only.
2. Discoverable: tooltip, context menu entry, command-palette entry.
3. Undoable as one coherent step.
4. Responsive: input is handled instantly; long work is backgrounded with progress and cancel.
5. Clear feedback during and after the action.
6. Sensible defaults; state persists where it should.
7. Empty and error states are designed.
8. Visually consistent with the design system.
9. Verified at 100%, 150% and 200% scale in both themes.
10. Accessible name and sensible focus order.

---

## 6. Feature requirements

### 6.1 Arrangement and tracks
- A single arrangement timeline with rulers in bars/beats, minutes:seconds, samples and timecode. [M]
- Grid that adapts to zoom or is fixed (whole note down to very fine divisions, triplet and dotted variants, or off), with a modifier to bypass snapping temporarily. [M]
- Smooth horizontal and vertical zoom (anchored at the pointer), zoom to selection, fit all, fit loop; scroll and pan; auto-scroll when dragging near edges; follow-playhead modes (off, page, continuous). [M]
- Track types: **audio, MIDI/instrument, group, return, master**. Per track: name, color, height, record arm, monitor mode (In/Auto/Off), solo, mute, polarity invert, input and output routing, inline fader and meter. [M]
- **Group tracks** [M]: create by selecting tracks and pressing Ctrl/Cmd+G or by dragging tracks into a group; groups nest; fold and unfold; the group sums its children and has its own devices, sends, fader and automation; ungroup is one step; solo, mute and arm behave sensibly across the group. Linked editing across selected tracks [S].
- Clip interactions: move (across tracks, with snap), copy-drag, trim edges, slip content, fade handles, clip gain handle, loop-extend, rename, recolor, mute, lock. [M]
- Selection: clip selection, marquee, and **time-range selection** that is independent of clips and supports cut, copy, duplicate, delete, split and consolidate on the range. [M]
- Loop region, punch in/out region, named markers and locators, tempo and time-signature lanes. [M]
- Track height presets, collapse, hide, reorder by drag, multi-select, track templates [S], track delay offset [S], track lock [S], overview strip [S].

### 6.2 Audio
- Import by drag and drop from the OS or the browser, with automatic sample-rate handling and optional copy into the project. [M]
- Smooth, glitch-free playback of large projects, seamless loops and instant seeking. [M]
- Waveform display that stays crisp and fast at every zoom level. [M]
- **Recording** [M]: arm one or many tracks, input monitoring, multichannel inputs, crash-safe writing to disk, automatic latency compensation with manual offset, punch in/out, count-in, input level meters with clip indicators. Loop recording into takes [S]. Retroactive "capture MIDI" [S].
- Clip properties: gain, fade in/out with selectable shapes, automatic short crossfades on overlaps, reverse, normalize, polarity, pitch. [M]
- **Warp and time-stretch** [S]: warp markers, transient and tempo detection, quality modes for rhythmic, melodic and polyphonic material, clips that follow project tempo.
- Take lanes and comping [S].
- Mono, stereo and multichannel material supported. [M]

### 6.3 MIDI
- Record, overdub and replace; count-in; punch; MIDI thru and monitoring on armed tracks; channel filtering; computer keyboard as a MIDI keyboard. [M]
- Notes never hang: stopping, jumping, looping, muting and editing always release sounding notes; a panic command exists. [M]
- Sustain pedal, pitch bend, aftertouch, program and bank change, MIDI clock out. [M]
- **Piano roll** [M]: draw, select, move, resize, split, glue, mute, duplicate, nudge, transpose, legato; note audition while editing; velocity lane; controller lanes (CC, pitch bend, pressure) with pencil, line and curve tools; zoom synchronized with the arrangement. Scale highlighting and snap-to-scale [S]; fold to used notes [S]; ghost notes from other clips [S]; editing several clips together [S]; step input [S].
- **Quantize** [M]:
  - Grid choices from whole note to very fine, with triplet, dotted and custom values.
  - Modes: note start, note end, start and end, note length.
  - **Strength** (0–100%), **swing**, a sensitivity window (only quantize notes near a grid line), and an iterative mode.
  - One-key apply with the last settings, plus an options popover with live preview.
  - Related tools: humanize (timing and velocity), legato, fixed length, transpose, scale-constrain, reverse.
  - "Restore recorded timing" after quantizing [S]; groove templates [S]; audio quantize through warping [S].
- Standard MIDI File import and export (clips and tracks). [M]
- MIDI learn for any parameter, with a mapping list. [M]
- MIDI effects (arpeggiator, chord, scale, velocity, note length). [S]
- MPE and MIDI 2.0. [L]

### 6.4 Drum maps (per-sound)
- A **drum map** assigns each sound: name, input note, output note, MIDI channel, output routing (the instrument or a specific instrument output), mute, solo, per-sound quantize, length override, color, and a choke group (for example open and closed hi-hat). [M]
- Clips store the performance in terms of the drum map's input notes, so attaching, swapping or editing a map **never destroys the notes**. A separate destructive "convert to another map" command exists. [M]
- Presets for General MIDI, GM2 and a common 808-style layout; user-created maps; import and export; the piano roll shows drum names instead of piano keys when a map is attached. Legacy drum-map format import [L]. [M]
- **Drum editor** [M]: one lane per sound with audition, mute, solo, per-hit velocity and per-lane quantize; reorderable lanes; option to hide unused lanes.
- **Step-sequencer view** [M] of the same clip: configurable step count and resolution, click to toggle, drag for velocity, accent modifier. Euclidean and random fill [L].
- Multi-output instruments can route each sound to its own mixer track. [M]

### 6.5 Automation and envelopes
- **Everything that has a value can be automated**: mixer controls, built-in device parameters, hosted plugin parameters, sends, tempo and time signature. [M]
- Lanes on the arrangement with a searchable parameter chooser (device → parameter), including for plugins that expose hundreds of parameters. Show and hide with a key. [M]
- Editing: add, move, delete and box-select points; copy, paste and duplicate ranges; linear, curved and step segments; shape tools (sine, triangle, saw, square, ramp); freehand drawing with automatic point simplification; snapping in time and value; live value tooltip. [M]
- **Clip envelopes** [M]: gain, pan and pitch for audio clips; CC, pitch bend and pressure for MIDI clips; envelopes loop with their clip. A "lock envelopes" option controls whether track automation moves with clips.
- **Recording automation** [M]: Read, Write, Touch and Latch modes, capturing moves from Lumen's controls, from plugin editors, and from mapped MIDI controllers; recorded points are thinned automatically. Trim mode [L].
- When the user manually overrides an automated control, it shows a clear override state with one-click re-enable. [M]
- Automation is smooth and sample-accurate for built-in devices and plugins alike. [M]
- Tempo and time-signature changes recalculate the whole timeline correctly. [M]

### 6.6 Editing actions (audio and MIDI wherever they make sense)
All are commands with shortcuts, work on clip selection or time-range selection, and are undoable. [M] unless tagged.
- **Split** at the playhead, cursor or grid; on a time range, split all affected clips at both edges. Splitting audio never causes clicks.
- **Join / glue** MIDI clips. **Consolidate**: combine selected clips into one new clip; for audio, it renders gain, fades, envelopes and warping into a new file spanning the selection (gaps become silence); for MIDI it merges clips and bakes loop repeats. Originals remain available in the media pool.
- Trim, crop to selection, delete, **ripple delete**, insert silence, cut, copy, paste at cursor, duplicate clip and duplicate range, loop and unloop (turn a loop into real copies), reverse, normalize, nudge by grid and finely, move to another track, rename, recolor, mute, lock.
- **Bounce in place** and **render selection to a new track**, optionally including effects and tails.
- **Freeze, unfreeze and flatten** tracks to save CPU.
- Select all / none / invert / all in track / to start / to end.
- Slice audio to MIDI or drum map. [S]
- Group and ungroup clips. [L]
- Guarantees: split followed by join returns the original; consolidating unprocessed audio sounds identical to the original; undo restores the exact previous state.

### 6.7 Mixer, routing and grouping
- Channel strip: input gain, polarity, device chain, pre- and post-fader sends, fader in dB, pan with selectable pan law, mute, solo, cue/preview, output selection. [M]
- Return tracks, group tracks, master track, sidechain routing from any track, output routing to any hardware output pair. [M]
- Solo that keeps the signal paths of soloed tracks audible (groups, returns); exclusive-solo option. [M]
- **Automatic plugin delay compensation** across the entire project, with an indicator. [M]
- Metering: peak with hold and clip indicators, RMS; on the master, LUFS (momentary, short-term, integrated) and true peak. [M]
- Mixer as a dockable panel, with fader, pan and meter also inline on track headers. [M]
- Routing matrix view [S]. Master mono-check and dim [S]. Reference-track A/B [L].

### 6.8 Plugins and devices
- **VST3** instruments and effects. [M]
  - Non-blocking, crash-safe scanning with caching, a blocklist for bad plugins, rescan, and user-configurable search paths.
  - Multi-output instruments, sidechain inputs, automatic latency and tail handling, bypass.
  - Full state save and restore inside projects, preset browsing, loading and saving of VST3 presets.
  - Every plugin parameter available for automation and MIDI mapping, with a generic parameter UI as fallback.
  - Plugin editors in floating windows that remember position and size, handle resizing, high-DPI and keyboard focus correctly (typing in plugin fields works).
  - Plugins that crash, hang or misbehave are isolated, flagged and recoverable with last known state. Strong isolation (separate process) is required by 1.0.
- **Device chain** [M]: horizontal chain per track in the detail panel; each device is a collapsible card with power/bypass, name, preset menu and "open editor"; a compact parameter strip where the user chooses which parameters to show; drag to reorder; copy between tracks; instruments first, then MIDI and audio effects.
- **Built-in essentials** [M]: utility (gain, pan, width, mono, polarity), parametric EQ with analyzer, compressor with sidechain, limiter, delay, reverb, a simple polyphonic synth, and a sampler (single- and multi-sample, drum-map aware). Spectrum analyzer and tuner [S].
- Browser tab for devices with categories, vendors, search, favorites, recents; drag to a track or chain, double-click to add to the selected track. [M]
- CLAP hosting, device racks with macros [L].

### 6.9 Audio and MIDI devices
- **Audio device selector** [M]:
  - Driver type: ASIO, WASAPI (shared, and exclusive [S]) on Windows; CoreAudio on macOS; PipeWire/JACK/ALSA on Linux.
  - Input and output device, sample rate, buffer size, with live display of input, output and round-trip latency.
  - Selectable active input and output channels with friendly names; stereo and mono configurations.
  - Test tone, CPU load, dropout counter, and an Audio On/Off switch.
  - ASIO: open the driver's control panel, and handle driver resets and sample-rate or buffer-size changes without losing the session. Clear messages when a driver is busy or missing.
  - Changing device, rate or buffer at runtime never crashes; on failure it restores a working configuration. Settings persist and start-up falls back gracefully.
- **MIDI devices** [M]: list inputs and outputs with enable toggles for track use, sync and remote control; hot-plug support; per-track "MIDI From" (all inputs, a device, a channel) and "MIDI To" for external hardware instruments; virtual ports where the OS supports them.

### 6.10 Projects, media and export
- New, open, save, save as, save a copy, save as template, revert, recent projects, unsaved-changes indicator. [M]
- A **human-friendly, versioned project format** that opens older projects forever, with relative paths. [M]
- Autosave and recovery, rolling backups, **collect and save**, **relink missing media**, a **media pool** listing used and unused assets with purge. [M]
- **Export** [M]: whole project, loop region or time selection; master or stems (per track, group, return); WAV (16/24/32-bit float) and FLAC; sample-rate conversion; dither; normalize option; include effect tails; offline or real-time; progress and cancel; post-export loudness report. MP3 and AAC [L].
- Project settings (sample rate, tempo, time signature, defaults) and project notes. [M]/[S]

---

## 7. Standard DAW checklist

Everything every professional DAW is expected to have. Items marked [M] block 1.0. Track status in `docs/PROGRESS.md`.

**Transport and timing**
- [ ] [M] Play, stop, record, loop, return to start, go to start/end, previous/next marker
- [ ] [M] Tempo (20–999 BPM) and time signature with change maps; tap tempo
- [ ] [M] Metronome: on/off, accented downbeat, volume, sound choice, count-in (off/1/2/4 bars), pre-roll, record-only option, output routing
- [ ] [M] Loop region, punch in/out, auto-punch
- [ ] [M] Markers with names and navigation
- [ ] [M] Multiple time-display formats; editable position field
- [ ] [S] Edit cursor distinct from playhead; scrubbing
- [ ] [S] MIDI clock/MTC out · [L] clock/MTC in, network tempo synchronization

**Tracks and arrangement**
- [ ] [M] Add, delete, duplicate, rename, recolor, reorder, multi-select
- [ ] [M] Audio, MIDI/instrument, group, return and master tracks; routing; arm; monitor; polarity
- [ ] [M] Freeze, unfreeze, flatten
- [ ] [S] Track delay, lock, icons, templates

**Clip editing**
- [ ] [M] Select, marquee, range select; cut/copy/paste/duplicate/delete/ripple delete/insert silence
- [ ] [M] Split, join, consolidate, trim, crop, slip, nudge, move, loop/unloop
- [ ] [M] Fades, crossfades, clip gain, reverse, normalize, polarity
- [ ] [M] Snap and grid; zoom; clip name/color/mute/lock; clip properties
- [ ] [S] Warp/time-stretch, transient and tempo detection, pitch shift
- [ ] [S] Take lanes and comping
- [ ] [M] Unlimited undo/redo with a visible history

**Recording**
- [ ] [M] Audio recording with monitoring, multi-track simultaneous record, input meters
- [ ] [M] Latency compensation with manual offset; crash-safe capture
- [ ] [M] MIDI recording with overdub/replace, count-in and punch
- [ ] [M] Computer keyboard as MIDI input
- [ ] [S] Loop recording into takes · [S] Capture MIDI

**MIDI**
- [ ] [M] Piano roll, velocity and controller lanes, quantize, humanize, transpose, legato
- [ ] [M] MIDI thru, channel filter, panic, sustain/pitch bend/aftertouch, program/bank change
- [ ] [M] SMF import/export; MIDI learn
- [ ] [M] Drum maps, drum editor, step sequencer view
- [ ] [S] MIDI effects; on-screen keyboard · [L] MPE, MIDI 2.0

**Automation**
- [ ] [M] Lanes for all parameters; curves; Read/Write/Touch/Latch; clip envelopes; tempo and time-signature automation; override and re-enable; point simplification

**Mixer and routing**
- [ ] [M] Fader, pan law, mute, solo, sends, returns, groups, master, output routing, sidechain, delay compensation, meters, cue/preview
- [ ] [S] LUFS and true-peak metering; mono-check/dim · [L] reference track

**Plugins and devices**
- [ ] [M] Async cached VST3 scanning, custom paths, blocklist, search, favorites, recents
- [ ] [M] Instruments and effects; multi-out; sidechain; bypass; presets; state; parameter automation; generic UI; floating editors with correct DPI and keyboard focus
- [ ] [M] Latency compensation; crash isolation; safe mode
- [ ] [M] Built-in essentials (utility, EQ, compressor, limiter, delay, reverb, synth, sampler)
- [ ] [S] Spectrum analyzer, tuner · [L] CLAP, racks with macros

**Browser and media**
- [ ] [M] Browser with devices, samples, projects, user places; search; audition; drag and drop
- [ ] [S] Tags/favorites; tempo-synced audition · [M] media pool with relink and purge

**Project management**
- [ ] [M] New/open/save/save as/copy/template/revert/recent; autosave and recovery; backups; collect and save; relink; migrations; project settings
- [ ] [S] Project notes and metadata

**Export**
- [ ] [M] Master and stems; WAV/FLAC; bit depth; dither; offline and real-time; scope options; tails; normalize
- [ ] [L] MP3/AAC

**Preferences**
- [ ] [M] Audio and MIDI pages; plugin paths; file/cache locations; theme and UI scale; autosave interval; undo depth; default track behavior; record settings
- [ ] [M] Shortcut editor with conflict detection and keymap presets (default) · [L] alternative preset
- [ ] [M] First-run wizard (choose and test audio device, set plugin paths)
- [ ] [S] Preferences import/export; localization-ready text

**UI and UX**
- [ ] [M] Command palette; tooltips with shortcuts; context menus everywhere
- [ ] [M] Reset-to-default, type-in values, fine adjust, wheel adjust, parameter right-click menus
- [ ] [M] Drag and drop for files, devices, clips and tracks; status bar hints
- [ ] [M] Resizable, collapsible panels with remembered layout; full screen; high-DPI; light and dark themes
- [ ] [M] Keyboard-only navigation for core flows; accessible control names
- [ ] [S] Detachable mixer/editor windows for multi-monitor setups; hover help panel; welcome screen

**System and robustness**
- [ ] [M] CPU, disk and dropout indicators; Audio On/Off; panic button
- [ ] [M] Crash report file; logging with a "reveal logs" menu item
- [ ] [M] Device hot-unplug, sleep/resume and sample-rate-change handling
- [ ] [M] Single instance; file association; open by double-click or drop
- [ ] [M] Opt-in telemetry only
- [ ] [L] Update checker; licensing/activation

**Distribution and legal**
- [ ] [M] Windows installer and portable build; third-party license listing; VST and ASIO trademark notices; SDK license compliance
- [ ] [S] Notarized macOS app · [L] Linux packages

---

## 8. Out of scope

**Not in 1.0:** Session view and clip launching, notation, video, surround and Atmos, cloud collaboration, scripting, ARA, AU, LV2, mobile, bundled sample libraries.

**Post-1.0 candidates:** CLAP hosting, MPE, device racks with macros, network tempo synchronization, control-surface support (Mackie/HUI), spectral editing, advanced comping, MP3/AAC export.

---

## 9. Suggested delivery sequence

Each step must end with something usable and tested. Reorder or split as you see fit, and say why.

1. **Foundations and decisions:** repository, CI, test and benchmark setup, plus early prototypes that settle the UI approach, audio backend strategy, project format and plugin-isolation strategy.
2. **Sound and device selector:** ASIO and other drivers, device selector, test tone, metering, transport, metronome, audio playback on a timeline.
3. **Arrangement UI:** design system, tracks, clips, selection, snapping, zoom, undo/redo, command palette, shortcuts, browser basics, save and load.
4. **Mixer, routing and group tracks.**
5. **MIDI:** devices, recording, piano roll, quantize, SMF.
6. **VST3 hosting:** scanning, instruments, effects, editors, state, presets, delay compensation, sidechain, multi-out.
7. **Automation and envelopes.**
8. **Editing power tools:** split, consolidate, fades, crossfades, warp, bounce, freeze, takes.
9. **Drum maps, drum editor, step sequencer.**
10. **Built-in devices.**
11. **Projects, recording polish, export, preferences, first-run experience.**
12. **Hardening and release:** plugin isolation, performance tuning to the quality bar, long-session soak testing, accessibility, installers, documentation.

---

## 10. Definition of done

**For every feature**
- Works end to end, is covered by automated tests, and meets the UX acceptance checklist.
- Is a registered command with a shortcut where sensible, visible in the command palette.
- Is undoable, persists correctly in projects or settings, and survives save/load and version migration.
- Does not regress performance or audio stability (measured where relevant).
- Is documented for users and tracked in `docs/PROGRESS.md`.

**For 1.0**
- Every [M] item in sections 6 and 7 is complete, or explicitly deferred with human approval.
- The quality bar in section 4 is met and the measurements are recorded.
- A scripted end-to-end walkthrough succeeds: create a project, play a drum pattern with a drum map, record a MIDI part and quantize it, record audio, load a VST3 synth and effect, group the drums, automate a filter, split and consolidate clips, bounce, and export stems.

---

## 11. Kickoff

Begin with milestone 1.

1. Read this entire document.
2. Create `docs/PROGRESS.md`, `docs/DECISIONS.md` and a plan for milestone 1, including the prototypes and measurements you will use to settle the early technical decisions.
3. Execute the plan, record decisions, and deliver the milestone.
4. Report what shipped, what was deferred, measured results and risks, propose the plan for milestone 2, and wait for approval.

---

## 12. Where the project stands (keep this section current)

The specification above does not change. This section is the handoff: read
it first in a new session, and update it at the end of every slice.

**Last updated:** 2026-10-21 (latest of all): **M7 slice A7 (clip envelopes) is built** on the branch `m7-sa7-clip-envelopes` (cut from `main` after the owner merged A6, PR #48; pushed; ADR-0090; the owner chose E1 A (the envelope drawn and edited on the clip, from rendered specimens of three places), E2 (points measured from the clip's start) and E3 (Lock envelopes off by default, so the track's automation under a clip goes with it); **format v25**, so M7b's crossfade flag is v26; an engine change (the clip player), no host or protocol change: rebuild only the app). New: `lumen_project::envelope` (`ClipEnvelopes` on `Clip.envelopes`, `Kind`, `window`, `windowed`; the golden `v25/golden.lumen`; `edit::slice` and `edit::trim` shift the points), `lumen_project::edit::carry` (the track's automation under a clip moved, copied, pasted, cut or deleted with it; `Clipboard.lanes`), `lumen_engine::ClipEnvelope` (`Clip.envelope`, baked on the clip's own clock by `lane_bake::bake_from` in `lanes.rs::envelope_of`; `AutoLane::fill` is the gain function `Stage::render` now shares; `placed_as` compares it so an edit crossfades), `Target::ClipGain` and `ClipPan` (addresses for the lane editor, never saved in a track's lanes; `Track::lane_view` gives the points on the song's clock, `Document::write_envelope` stores them from the clip's start), the E key and the clip menu (`app/clip_envelopes.rs`, `view.clip_envelope`), `edit.lock_envelopes` (`settings.editing.lock_envelopes`, `Document::set_lock_envelopes`, set each frame), both at the end of `Command` and `ALL`, `draw_repeats` in `auto_view.rs`. A fresh worktree `D:/Projects/lumen-m7-sa7` holds the slice (the app deleted the first one after the branch was renamed). Tests: units for each piece, 5 engine renders and the allocation guard (`crates/lumen-engine/tests/clip_envelopes.rs`), 12 E2E (`tests/e2e/clip_envelopes.rs`, mutation checked), a shot (`clip_envelope`, looked at). **The WSL performance gate passes** (largest change +0.69% on `engine_routed_64`, none over +2%; `tempo_conversions` +1.85% is A6's; no scenario plays clip envelopes yet, so none was added). The branch is pushed. **Not run:** RealtimeSanitizer, the WSL clippy and Linux tests, macOS runs, playback by ear (**MT-AUTO-9 in `docs/manual-tests/M7.md` is the owner's**). Next: A8 (close M7). Before it, 2026-10-21: **M7 slice A6 (tempo and meter lanes) is built** on the branch `m7-sa6-tempo-meter-lanes` (cut from `main` after the owner merged A5, PR #47; not pushed; ADR-0089; the owner chose C1 to C4, all recommendations, after a rendered specimen of three places: two slim lanes under the bar ruler, shown with T; the bar fields edit the segment at the playhead; click adds, drag moves, double-click types; a meter edit moves later changes to the nearest bar line; **format v24**, so M7b's crossfade flag is v25; **no engine, host or protocol change**, rebuild only the app). New: `Project.meter` (a list of `lumen_core::meter::MeterPoint`) replaces `time_signature` (the migration keeps the old one as the first change; golden `v24/golden.lumen`; `validate` refuses a map that is not a valid `MeterMap`, over 4,096 changes, a bar over 64 beats), `lumen_core::meter::tidy` and `MeterMap::nearest_bar_start`, `lumen_project::edit::maps` (the pure edits, a 2,000-step random walk keeps both maps valid), `document/song_maps.rs` (`set_tempo_at`, `put_`, `move_`, `remove_tempo_change`, `set_time_signature_at`, `put_`, `set_`, `move_`, `remove_meter_change`; `set_tempo` and `set_time_signature` are gone), the top-level `song_lanes/` (`mod.rs` state, `scale.rs` pure hit tests and scales, `paint.rs`), `app/song_lanes.rs` (pointer, keys, the typing box, menus), three commands at the end of `Command` and `ALL` (`view.song_lanes` on T, `edit.tempo_change`, `edit.meter_change`), `size::SONG_*` tokens, DESIGN §7i. **Added the same day at the owner's word (ADR-0089 amendment, still v24): tempo ramps** (`TempoPoint.ramp`, a straight line in music time to the next change; the conversion is in `lumen_core::time`, so every reader of the tempo map keeps its calls; `TempoMap::{bpm_at, ramps_between, stepped}`; set from a point's right-click menu or `edit.tempo_ramp`, last in `Command` and `ALL`; MIDI files get steps), **and a Tempo button in the ruler corner plus a ruler-menu toggle** to show the lanes (`size::SONG_BUTTON`); MT-AUTO-8 steps 16 and 17 are the owner's. Every reader moved to the map: the transport timeline, count-in, new MIDI clip bars, import and export, SMF (`MidiFile.meter` is a `Vec<MeterPoint>`). Tests: core `tidy` and bar-line units, 12 `maps` units, 7 Document units, the v24 golden and refusals (`tests/meter.rs`), SMF round trip, 2 engine tests (accents across a change of meter, a loop across one), 10 E2E (`tests/e2e/song_lanes.rs`), a shot (`song_lanes`, looked at). **Not run:** the WSL gate and performance gate, RealtimeSanitizer, macOS runs, any playback by ear (**MT-AUTO-8 in `docs/manual-tests/M7.md` is the owner's**). The regenerated demo projects (`tests/projects/*.lumen`) are v24. **The D: drive filled up during this slice (my `target/` is about 9 GB); builds here use `CARGO_INCREMENTAL=0`.** Next: A7 (clip envelopes). Before it, 2026-10-20: **M7 slice A5 (MIDI learn) is built** on the branch `m7-sa5-midi-learn` (cut from `main` after the owner merged A4, PR #45; not pushed; ADR-0088; the owner chose N1 to N4, all recommendations: a mapping is a controller number and channel from any input, absolute values that jump, a mapped move acts as a hand on the control, a right-click menu plus a Mappings window; **format v23**, so A6's time-signature map is v24 and M7b's crossfade flag v25; **no engine, host or protocol change**, rebuild only the app). New: `lumen_project::mapping` and `Project.mappings` (the golden `v23/golden.lumen`), `Document::{set_mapping, clear_mapping, clear_all_mappings, move_control}` (`document/mappings.rs`), `live_tap.rs` (the tap: `Heard`, `Tap`, a 2,048-bit set of taken controllers the audio thread reads, overflow counted) and `LiveFeed::pull`, which copies controller messages to the tap and keeps taken ones from the engine, `app/controllers.rs` (drains the tap each frame, learns, applies mapped moves through the control's own setter, holds a controller for 300 ms so Touch and undo see a gesture), `app/controller_requests.rs`, `app/mappings_window.rs`, `arrangement/control_menu.rs` and `widgets/control_items.rs` (the menu rows every automatable control ends with: header volume, pan and mute, strip fader, pan, trim and the master's mute, send knobs, a plugin card's knobs and the All parameters rows), three commands at the end of `Command` and `ALL` (`midi.learn`, `midi.clear_mapping`, `midi.mappings`). The lib test binary now installs the guarded allocator (`lib.rs`). Tests: 13 `live` unit tests incl. the allocation guard, 7 for the document, project v23 golden and refusals, 9 E2E (`tests/e2e/controllers.rs`), mutation checked, a shot (`controller_mappings`, both themes, 100/150/200%, looked at). **Recorded at the playhead of the frame that applies the move, not on the message's tick** (KNOWN_ISSUES). **Not run:** the WSL gate, RealtimeSanitizer, macOS runs, any hardware controller; **MT-AUTO-7 (`docs/manual-tests/M7.md`) passed (owner, 2026-10-20) with a virtual MIDI cable and an on-screen MIDI keyboard, not a hardware controller**. Next: A6 (tempo and meter lanes, format v24). Before it, 2026-10-20: **M7 slice A4b (recording a lane) is built** on the branch `m7-sa4-follow-and-record` (one branch for A4a and A4b, on the owner's word; pushed; ADR-0087; **no format, engine or host change**; the owner chose R1 B (a 100 ms glide back at the end of a Touch) and K1 B (one chip with a red dot, the lane tinted red) from rendered specimens, and took M2 (modes not saved), W1 (Write from where playing starts) and L1b (a pass ends at the loop wrap)). A track has a mode, Read, Touch, Latch or Write (`auto_record::Mode`, `AutoView.modes`, session state; the chip's menu and `edit.automation_mode_*`, four commands at the end of `Command` and `ALL`, the order test's last variant moved). `auto_record.rs` is a pure state machine (`Recorder::frame`: the playhead, the controls touched, the pointer, the open plugin gestures in; `TakeOver` and finished passes out); `app/auto_recording.rs` runs it each frame after the interface is drawn (`lib.rs`), writes a pass with `lumen_project::edit::automation::recorded` (the freehand stroke with the end of the pass: a Touch glides back, a steps lane jumps back, a Stop holds) through `Document::record_lane` (one undo step, "Record automation"), and tracks plugin gestures (`note_gestures` from `plugin_edits.rs`). `Document::moved` (the setters and plugin controls call it) queues the moves for `take_touched`. A lane being written is shown as recording, not overridden (`Arrangement::taken_over` leaves out `AutoView.recording`). Accuracy: a point is stamped with the frame's playhead (at most one interface frame late); the playhead is within 11 to 12 ms of a straight line on the silent test device (measured in E2E), a real device not measured. Tests: 5 units for `recorded`, 10 for the state machine, 8 E2E in `tests/e2e/lane_record.rs`, a shot (`automation_modes`), mutation checked. Gates on Windows: fmt, workspace clippy, workspace tests (all pass but `the_automation_demo_project_file_is_current_and_valid`, which fails only because `tests/projects/automation-demo.lumen` has an unrelated uncommitted local edit: leave it out of commits), 6 smoke, macOS cross-clippy. **Not run:** the WSL and performance gates, RealtimeSanitizer, macOS runs, any real plugin, device or controller; **MT-AUTO-6 (`docs/manual-tests/M7.md`) passed (owner, 2026-10-20, no step detail), after two fixes from the run: a Touch on a control with no lane yet returns to where the control rested (`Document::take_rests`, `Action::Write.rest`), and Write overwrites only the lane the chip shows.** Next: A5 (MIDI learn). Before it, 2026-10-20: **M7 slice A4a (controls follow their lane, and taking a lane over) is built** on the branch `m7-sa4-follow-and-record` (cut from `main` after A3 merged, PR #43; now part of the single A4 branch; ADR-0086; A4 was split on the owner's word into A4a and A4b (recording); the owner chose the override look A from rendered specimens; **no format, engine or host change**). A control whose lane plays shows `Lane::value_at` at the playhead (`arrangement/follow.rs`: `Arrangement::shown`, `AutoView::following`, `AutoView.at` set each frame in `lib.rs`; headers, strip, send knobs, M, the card's strip and the All parameters window); moving a control takes its lane over (`document/overrides.rs`: `Overrides` kept in the `Document` beside the history, session state, not saved, not undone; `Document::override_lane`, `override_param`, `reenable_lane`, `reenable_all`; the setters `set_volume`, `set_pan`, `set_trim`, `set_send`, `set_mute` and the plugin controls call it; a program the user chose does not; editing, switching or removing a lane ends it). `Overrides::plays` is the one rule that the routing (`routing.rs`, through `Devices.overridden`), `lane_bake::device_lanes`, `midi_play::with_lanes` (its key in `lanes.rs`) and the hosting bridge (`Hosting::set_overridden`) all read, so a taken-over lane plays nothing and the control's own value is heard. Look A: amber ring (`follow::ring`, `ring_round`), amber A button, amber chip with a filled **Re-enable** button (`auto_tools.rs`), dashed curve, a dotted amber line at the control's value; commands `edit.automation_reenable` (Ctrl+Shift+R) and `edit.automation_reenable_all` at the end of `Command` and `ALL` (the order test's last variant moved). Tests: 8 units, routing, MIDI-list and plugin-edit tests, 6 E2E scenarios in `tests/e2e/lane_override.rs` and one in `plugin_lanes.rs` (A3's put-back scenario was rewritten), a shot (`automation_override`), mutation checked. Gates on Windows: fmt, workspace clippy, workspace tests (all pass but `the_automation_demo_project_file_is_current_and_valid`, which fails only because `tests/projects/automation-demo.lumen` has an unrelated uncommitted local edit; it passes against the committed file, so leave that file out of commits), 6 smoke, macOS cross-clippy. **Not run:** the WSL gate and performance gate (no engine or project change), RealtimeSanitizer, macOS runs, any real plugin; **MT-AUTO-5 (`docs/manual-tests/M7.md`) passed (owner, 2026-10-20; no step detail).** KNOWN_ISSUES: the master has no lane view, so no Re-enable button (the All parameters window's bars carry the ring too). Next: A4b (Read, Write, Touch, Latch recording; the owner asked for specimens of how a control returns to the lane at the end of a Touch first). Before it, 2026-10-20: **A3 is done**: the owner's MT-AUTO-4 passed again after a second fix for the same report (a parameter could not be changed after its lane was switched off or removed, with a plugin instrument): the engine offered a hosted instrument the removed lane's last block with every block, because `automate` was only called for a track that still had lanes; it now says "no parameters" every block with no lane to play (`midi/tracks.rs` `gather_track`; ADR-0084's second amendment; engine test `a_lane_that_is_taken_away_stops_reaching_a_plugin_instrument` fails without it; no format or host change, so only the app needs a rebuild). Next: A4 (record and override), whose override state is also what makes a knob moved under a lane visible. Before it, 2026-10-20: **M7 slice A3 (plugin parameter lanes) is built** on the branch `m7-sa3-device-targets` (cut from `main` after the owner merged A2, PR #42; pushed; ADR-0084; **no format change** (still v22) and **no host protocol change**, so no host rebuild is needed). A lane now drives a parameter of a hosted plugin effect or instrument on its exact sample. New: `lumen_devices::ParamBlock` (`hosted/params.rs`: the parameter changes of one block, fixed room of 64 queues and 1,024 points, filled on the audio thread), `Hosted::submit` and `Guard::begin` take `&ParamBlock` (every fake host passes or ignores it), `Instrument::automate` (a default that does nothing; `HostedInstrument` keeps the block for its next offer), `lumen_plugin_host::protocol::ParamQueues` (the shared region's write is generic over it, so no allocation), `Link` writes whole queues and counts what the region cannot take (`Counts::dropped_params`), `lumen-engine/src/mixer/params.rs` (`ParamLane`, `fill`: a pure function of a baked lane and the block's place: the value at the block start, each point on its sample, a step as a hold and a jump, curves by knots every `KNOT` frames, straight segments by their ends; nothing is remembered between blocks), `NodeAutomation::plugins` and `NodeStages::plugin_block` for effects (read at the node's input time less the latency of devices ahead), `MidiTrack::automation` and `set_automation` for instruments (filled in `gather_track`), `Status::params_cut` (lanes cut to their ends or left out), `lumen-app/src/lane_bake.rs` (`bake`, moved out of `routing.rs`, and `device_lanes`), `midi_play::with_lanes` (the MIDI list is rebuilt when the device's rate changes), `Track::drives` and the bridge skip in `hosting/update.rs` (a parameter an enabled lane drives is not sent from the project's resting value), `arrangement/auto_params.rs` (pure: which parameters a lane can drive, the search, the groups), `app/auto_params.rs` (the chooser dialog: a pill per plugin, a search, group chips, only the rows in view, arrows and Enter; `lane_names` gives lanes the plugin's words, "Bus Comp · Cutoff"), `ParamChooser` and `AutoView::{chooser, names, name_of}` in `auto_state.rs`, `scale::offered` lists plugin lanes that exist, perf scenario `engine_plugin_lanes_64` (baseline added by hand from the cloud container). The lane chip's menu ends with "Parameters of *plugin*…". **Built-in devices have no engine lanes until M8**, and a lane for one is kept and plays nothing. Tests: 7 units for `ParamBlock`, 12 for the generator, 5 for the chooser's pure part, engine tests (`hosted_effects.rs`, `hosted.rs`: a step lands on its exact sample at five block sizes, behind an earlier device's latency, for an instrument; the heap guard, mutation checked), 4 real-host tests through the link (`crates/lumen-hosting/tests/automation.rs`) and the link test under the allocation guard carries parameters, 7 E2E (`tests/e2e/plugin_lanes.rs`, mutation checked), a shot (`automation_plugin_chooser`, both themes, 100/150/200%, looked at). Gates run in the Linux cloud container: fmt, workspace clippy, the workspace tests (2,424 passed; the one failure was the region test that expects "point offset", fixed and re-run), 6 smoke under `xvfb-run`, macOS and Windows (GNU) cross-clippy, the Valgrind performance gate (`engine_hosted_64` +1.62%, none over 2%; `project_parse` +5.37% is this machine's known offset). **MT-AUTO-4 (`docs/manual-tests/M7.md`) passed (owner, 2026-10-20; no step detail)**, so real plugins' lanes work. The owner then found one bug, fixed (ADR-0084 amendment): after a lane was switched off its parameter could not be changed (a stale value queued in the plugin host overwrote the new one, Lumen's idea of the plugin's value was stale, and the engine's plan lagged a block); `Parameters::driven` and `released` in `hosting/`, `Handler::is_queued` and `Instance::take_heard` in the host. **Rebuild `lumen-plugin-host`** (`cargo build --release -p lumen-plugin-host`) to get the host fix. **Not run:** the WSL gate, RealtimeSanitizer, the cloud's Windows or macOS runs, the release frame benchmark with lanes (still due from A1). **Known limits** (KNOWN_ISSUES): a knob moved under a lane does nothing audible and the plugin's own window is overridden at the next block (A4's override); lane cuts are counted and not shown; a point reads as a percentage; pasting a device does not bring its lanes. Next: A4 (record and override). Before it, 2026-10-20: **M7 slice A2 (full lane editing) is built** on the branch `m7-sa2-edit-a-lane` (cut from `main` after the owner merged A1, PR #41; pushed; ADR-0083; **no format change**, still v22). New: `lumen_project::edit::automation` (pure: `shifted`, `with_shape`, `replace_span`, `copy`, `paste`, `duplicate`, `wave`, `stroke`), `arrangement/auto_state.rs` (the lane view's state: `Picks`, `Tool`, one `Gesture` value, `AutoView`, moved out of `auto_view.rs`), `arrangement/auto_tools.rs` (the tool strip, the box, the bend dot, the live value chip), `app/auto_gestures.rs` (move, box, pencil, shape and bend, each rebuilt from the lane at the press; resting places), `app/auto_keys.rs` (the keys and commands, moved from `auto_editing.rs`, which keeps the pointer routing), `auto_scale::snap_height`, `lane_overlay` takes a `Scene`, eight new commands at the end of `Command` and `ALL` (`edit.automation_segment_{linear,step,curve}`, `edit.automation_tool_{ramp,sine,triangle,saw,square}`; the last is `AutomationToolSquare` in the order test), B is the existing `DrawTool` key and toggles the pencil when a lane is shown, Shift+click and Shift+drag pick (a plain drag on empty lane still adds a point, A1's behaviour), Ctrl+A, Ctrl+C, Ctrl+V and Ctrl+D act on picked points while a lane has the keyboard (`AutoView.focus`). Copy, paste and duplicate act on picked points, not a time range (the plan said ranges; ADR-0083 choice 4); there is no right-click menu on a point. Tests: 22 pure, unit tests for the state, scale and bend dot, 19 E2E (`tests/e2e/lane_edit.rs`, mutation checked), a shot (`automation_editing`, both themes at 100/150/200%, looked at). Gates run in the Linux cloud container: fmt, workspace clippy, the workspace tests (2,375 passed; `lumen-hosting`'s fault soak failed once under load and passes alone), 6 smoke, macOS cross-clippy. **Not run:** the Windows cross-clippy, the WSL gate, RealtimeSanitizer, any Windows or macOS run, the release frame benchmark with lanes (still due). **MT-AUTO-3 (`docs/manual-tests/M7.md`) passed (owner, 2026-10-20; no step detail)**, so the looks and gestures built without specimens stand. **Owner note, to revisit after the core product is finished:** copy, paste and duplicate on a lane should work on time ranges, not on picked points (a range to copy may have no points at its edges; ADR-0083 choice 4 is the interim, KNOWN_ISSUES has the row). KNOWN_ISSUES: a point at tick 0 is hard to grab. Next: A3 (device and plugin targets). Before it, 2026-10-19: **M7 slice A1 (see and add a lane) is built** on the branch `m7-sa1-see-a-lane` (cut from `m7-sa0-hear-a-lane`, which is not merged; the owner said go ahead; ADR-0082; **no format change**, still v22). The lane is drawn over the track's clip lane (the owner's L1 B): `arrangement/auto_view.rs` (state `Arrangement.auto`, drawing, the chip and its chooser, the pure point edits `inserted`/`moved`/`removed`, `Frame` mapping time and value to the screen), `auto_scale.rs` (the one place values meet heights), `app/auto_editing.rs` (the pointer before the clip gestures: a click adds or picks a point, a drag moves it or adds one and moves it, Ctrl reaches the clip underneath, Alt places off the grid; the keys), the A button in the unfolded header's second row, five new commands at the end of `Command` and `ALL` (`view.automation` on A, `view.automation_parameter`, `edit.automation_point`, `view.automation_on`, `edit.automation_remove`), new `size::AUTO_*` tokens, test seams `automation_*` in `testing.rs`, `--bench-lanes` (with `--stress`). **Which lanes are shown is not saved**, the master has no lane view and controls do not follow a playing lane (KNOWN_ISSUES; the last waits for A4). Tests: 13 E2E (`tests/e2e/lane_view.rs`, mutation checked), 16 unit, a shot (`automation_lanes`, both themes at 100/150/200%, checked by eye). Windows gates: fmt, workspace clippy, the whole workspace test run, 6 smoke, macOS cross-clippy. **Not run:** the WSL gate and Linux and macOS test runs, RealtimeSanitizer, the release frame benchmark with lanes (**MT-AUTO-2 passed (owner, 2026-10-19) except step 12**: `--stress` on a real device at another rate shows 100 missing-file warnings, a separate bug in KNOWN_ISSUES; the numbers are still due). Next: A2 (full lane editing: box select, shapes, freehand, value snapping, tooltip). Open items unchanged below: M5 and M6 are not signed off. Before it, 2026-10-19: **M7 slice A0 (hear a lane) is built** on the branch `m7-sa0-hear-a-lane` (A0 and the M7 planning docs before it; cut from a generated name, renamed 2026-10-19) (the owner told it to start, with Y1 deferred; ADR-0081; plan and the changes from it in `docs/milestones/M7.md`; **format v22**, lanes only: the time-signature map moved to A6 as v23 and M7b's crossfade flag is v24). New: `lumen_core::automation` (points, segment shapes, `value_at`, `simplify`), `lumen_project::automation` (`Target`, `AutomationLane`, ranges) and `Track.automation`, `Document::{set_lane, set_lane_enabled, remove_lane}` (`document/automation.rs`; a device's lanes follow it through move, copy and remove, a send's through its return), `routing.rs` bakes lanes to samples through the tempo map (a segment a tempo change falls inside is split or resampled), and the mixer's `mixer/auto.rs` plays trim, mute, fader, pan and send lanes on their sample (knots every 32 frames, a 1 ms ramp at a step, read at the time of the audio each stage acts on, a 20 ms glide when a lane is edited under the playhead; `Plan::process` takes a `Pass` with the timeline sample). `NodeSpec.automation` is new (use `..NodeSpec::unity`). Tests: 11 core, golden v22, 11 engine render tests (`tests/automation.rs`), the allocation guard (mutation checked), routing bake tests including a property test across a tempo change, 5 E2E scenarios that play lanes through the whole app (`tests/e2e/automation.rs`, failing without the bake), perf scenario `engine_automated_64`. **There is no lane interface until A1:** `App::give_lane` is the test seam and `tests/projects/automation-demo.lumen` the example. **MT-AUTO-1 (`docs/manual-tests/M7.md`) passed (owner, by ear, 2026-10-19; reported as passed with no step detail).** Next: A1 (see and add a lane: the overlay on the clip lane, the A button and key). Open items unchanged below: M5 and M6 are not signed off. Before it, 2026-10-19: **M5 and M6 reports are written and the M7 milestone is split into two plans** on the session branch `m7-sa0-hear-a-lane` (docs only; ADR-0080 for slice 7's tools; the reports in `docs/PROGRESS.md`; `docs/milestones/M7.md` (automation and envelopes, slices A0 to A8, decisions D1 to D9) and `docs/milestones/M7b.md` (advanced editing and render, slices E0 to E6, decisions R1, R2, E0, X1), built from three read-only explorations; neither is approved or started. **Owner decisions of 2026-10-19:** the milestone is split (S1), tempo ramps are deferred (T1), the audio pitch envelope is deferred (P1); crossfades stay in scope. **Chosen from the specimens (2026-10-19):** L1 B (the lane drawn over the track's clip lane), X1 A (a crossfade at a join), K1 the cut gestures (press, aim, release; the edit cursor with Ctrl+E) and tool keys that follow the conventional shortcuts of §5 with no new single-letter tool keys. **Y1:** the static clip pitch is deferred too (owner, same day). Neither M5 nor M6 is signed off: the owner's MT-MIDI-7 to 9, MT-VST-3 to 7 and the ADR-0069 callback numbers are still open, and M7 starts only after the owner approves. Before it: **M13 is fixed, so the review's findings are all done** on `m6-s7-compatibility-closeout`: the owner's naming rule reaches the code. About 80 comments, doc comments and test names that pointed at another workstation now state the behavior; the Shortcuts window says "Keymap: default"; two real plugins named in a test comment, an ADR and this file are the anonymous RP-11 and RP-12 (their names are not recorded, so those reports cannot be reproduced from the text); `crates/lumen-app/tests/third_party_names.rs` scans every crate's Rust sources for ten workstation names (mutation checked). Hardware, driver and legal names stay (the ASIO driver lists in the M1 and M2 records, the ASIO SDK agreement). The audit's Status table has the row. **Still open:** the owner's MT-VST-7 runs, one ADR for slice 7's tools, the M6 report in PROGRESS and the owner's approval before M7. Before it, 2026-10-18: **the test gaps (M14) and the Low findings L1 to L6 of the review are fixed** on `m6-s7-compatibility-closeout` (pushed; the audit's Status table, "Test gaps and Low findings"). M14: tests for the link's MIDI mapping and offset clamp, the output pairs, the manager's stale starts, a removed sidechain source. L1: the scan program, presets folder and safe-mode marker are looked for off the UI thread, the browser's list is rebuilt a few times a second during a scan, asks for parameter words are coalesced and time out. L2: tokens for the Devices tab's greys, widths and timings (`tests/design_tokens.rs` keeps them so). L3: a module whose entry returns false is refused, controller events past the room are counted, a stray `printf` cannot corrupt the protocol. L4: engine details (true-peak `expect`, zero-channel clip, NaN average, per-channel All Notes Off, delay-line carry, event sort). L5: the event cap is the region's room, discovery says when it stopped at 4,096 modules, a scan thread or reload that cannot start is shown, the view's and settings' lists are bounded. L6: the built-in vendor label is a label. Each has a test (mutation checked where it applies). Gates: Windows fmt, workspace clippy, workspace tests (one full run lost eight `editors` E2E scenarios to the plugin-load timing wait under load; they pass alone and in the next full run), 6 smoke, macOS cross-clippy; WSL clippy and Linux tests (495 E2E) and the performance gate (largest change +0.11%). Not run: RealtimeSanitizer, macOS tests, any real plugin. **Still open (then):** M13 (since fixed), the owner's MT-VST-7 runs, one ADR for slice 7's tools, the M6 report in PROGRESS and the owner's approval before M7. Before it, the same day: **the review's Medium findings M1 to M6 and M8 to M12 are fixed too** on `m6-s7-compatibility-closeout` (pushed; the audit's Status table lists each fix, test and commit). Fixed: a pipe reader no longer holds up dropping a host (M1); a panic in a device's thread is said and gives its load permit back (M2); the host's `process_into` runs under the allocation guard (M3) and its SAFETY note is true (M4); other outputs are never a previous block's (M5) and the retry waits to the callback's shared deadline (`Instrument::render_by`, M6); guarded tests for the slice 6 paths (M8); `hosting.rs` is `hosting/` with `update()` split into named parts, and `protocol/`, `hosted/`, `process/`, `com/`, `instance/`, `chain/card/` and `midi/tracks.rs` split by concern (M9; `chain/presets.rs`, `engine.rs` and `hosting/mod.rs` stay near 800 to 860 lines); dead code and stale `allow`s removed (M10); the settings write is flushed and ADR-0079 records that added sections keep the version (M11); ARCHITECTURE, PROGRESS, the ADR index and KNOWN_ISSUES match the code (M12). **M7 was measured and then fixed** (the owner's go-ahead): the chase read every note before the position, 2.25 ms of a 2.67 ms block at 128 frames with 100,000 notes at the end of a song, and 2.5 ms for a one-note edit to the clip playing; a per-clip index (`midi/index.rs`) brought both to 1.4 µs and 75 µs (`tests/midi_scale.rs`; two tests in `hold.rs`, one against the unindexed reading, one counting notes read; the WSL performance gate stayed flat). **Not fixed, by the owner's word:** M13 (the naming rule's reach in code); M14 and the Low findings were fixed after this. The WSL performance gate read `engine_hosted_64` at +2.01% (the fixes' flags), so `perf/baseline.json` was updated from WSL (the other scenarios moved by under 0.7%). Gates run on Windows: fmt, workspace clippy, 2,207 tests, 6 smoke, macOS cross-clippy; on the pushed branch in WSL, clippy and the Linux tests (494 E2E). Not run: RealtimeSanitizer, macOS tests, any real plugin. Before it: **the slice 7 review (7e) is done and its seven High findings are fixed** on `m6-s7-compatibility-closeout` (pushed; report `docs/audits/2026-10-18-m6.md` with its Status table). The owner approved only the Highs: **H1** closing Lumen no longer waits for a plugin that is still loading (`Manager::shutdown(grace)`, `Hosting::EXIT_GRACE`); **H2** a settings file saved by a newer version is renamed to `settings.json.newer`, not overwritten; **H3 and H4** a block a busy plugin host refused is offered again without its events and counted as missed once (`HostedInstrument::refused`, `Guard::begin_again`, the mixer's `Dev.refused`); **H5** the host's block thread says a slow block after letting go of the instance lock; **H6** the Windows timed wait falls back when the timer cannot be armed; **H7** no user-visible string spells out the product's name (`tests/product_name.rs` scans the sources). Each has a test that fails without it. Gates run on Windows: fmt, workspace clippy, 2,198 tests, 6 smoke, macOS cross-clippy; WSL performance gate (largest change +1.25%, `engine_hosted_64`, none over +2%). **Not run:** RealtimeSanitizer, Linux and macOS test runs, any real plugin. **Remaining:** the Medium (M1 to M14) and Low findings (the owner decides), the owner's MT-VST-7 runs, one ADR for the slice's tools, the M6 report in PROGRESS and the owner's approval before M7. Before it, 2026-10-18: **M6 slice 7 (compatibility and closeout) is under way on `m6-s7-compatibility-closeout`** (cut from `main` after PRs #36 and #37; four commits, not pushed; plan, the owner's choices (every recommendation) and progress in `docs/milestones/M6.md`, "Slice 7"). Built: **7a** `lumen-host-probe` (a binary of the host package: runs installed VST3 plugins through scan, load, buses, configure, process, link, state, parameters, editor and shutdown, each in a host process of its own; `--json`, `--table` with labels and no product names; `tests/probe.rs`); **7b** the fault soak (`crates/lumen-hosting/tests/soak.rs`: six real hosts played as the engine plays them while crashes, hangs, slow and non-finite blocks, hosts ended from outside and failed loads are applied and each victim replaced; checks the others never fail, a failed host is silent and not restarted by itself, the audio work allocates nothing, no orphan process, no growth in memory or handles; `LUMEN_SOAK_SECONDS` and `LUMEN_SOAK_SEED` for the long run, `.github/workflows/soak.yml` starts it by hand; the Windows `exists` check now requires the host's image name because Windows reuses process ids); **7c** `lumen --bench-callback <out.json> [--bench-seconds N] <project>` (`callback_bench.rs`, `app/bench_callback.rs`: plays the project through the real device and reports the callback's time as a share of the block, overloads, dropouts and each plugin's wait and work; exit 0, 3 for an overrun, 2 when it could not run; no change to the audio path); **7d** the combined walkthrough (E2E `walkthrough_plugins`). Gates run on Windows: fmt, workspace clippy and tests (E2E and the new soak pass), the 6 smoke tests, macOS cross-clippy, mutation checks on the probe, the soak, the benchmark and the walkthrough; five random 90 s soaks passed. **Not run:** anything on Linux or macOS, the WSL gates (no engine or project code changed), the long soak on a machine other than this one, any installed plugin or real device (**MT-VST-7 in `docs/manual-tests/M6.md` is the owner's: the probe over the installed library, the callback numbers at 64 and 128 frames with 16 and 64 hosted tracks, the walkthrough with real plugins**). ADR-0069's monitoring-through-an-effect measurement cannot be taken until audio input exists (M9). **Remaining in slice 7 (7e):** the `lumen-review` repository review, one ADR for the slice's tools, PROGRESS (the M6 report) and the owner's approval before M7. Before it, 2026-10-18: **owner's first run of slice 6's sidechain and three fixes, on `m6-s6-buses-sidechain` (the owner asked for them in this PR).** The test effect's sidechain now follows the key's peak (`KEY_RELEASE` 0.1 s, silent from 0.25), because ducking per sample made a kick amplitude-modulate a held pad (a faint kick, not a leak: E2E `the_key_does_not_leak_into_the_keyed_track`). Pressing a record arm arms that track alone, Ctrl keeps the others, a lit arm disarms only its own (`Document::arm_only`, `Arrangement::press_arm`, one undo step; E2E `arming_a_track_disarms_the_others_unless_ctrl_is_held`). A device dragged from the browser onto a track's header or lane (or into the chain) goes there, shows the Devices tab and opens the plugin's window once it runs (`browser_drop`, `Chain::open_when_ready`, `add_device_showing_window`; E2E `editors`). **Fixed after the owner narrowed it down:** a value changed in a plugin's window jumped back to the old one for a moment, because the bridge matched the plugin to the project in the frame the edit arrived, before the project held it (`hosting.rs` skips a device with unrecorded edits; E2E `a_value_changed_in_the_plugins_window_is_never_sent_back_to_it`). A preset change that reloads the plugin itself is not seen with the test plugin. Gates: workspace clippy, `lumen-app` and test-plugin suites. Not run again: WSL gates, smoke. Before it, 2026-10-18: **M6 slice 6 (production routing: buses, sidechain, outputs, plugin latency) is built** on `m6-s6-buses-sidechain` (cut from `main` after PR #35; not pushed; ADR-0078; plan, owner choices and progress in `docs/milestones/M6.md`). Built in parts 6a to 6d: a plugin instrument's latency is compensated and a locate flushes effect tails while a scrub is not late (6a); the host lists a plugin's audio buses and turns on the ones asked for (6b; **protocol version 8, a block carries up to 64 channels: close Lumen and rebuild `lumen-plugin-host`**); **format v20** `Device.sidechain` (a key from any track at pre FX, post FX or post mixer, the Sidechain row and its light on the effect's card, 6c); **format v21** `Track.plugin_output` (an instrument's other outputs as tracks of their own, the Outputs row and menu on the card, `Document::set_outputs`, 6d). Owner's fixes from runs with real plugins: every output of a plugin is on and an output with no track plays on the instrument's own track (a plugin moved a sound routed to an off output to the next one on); a bus's own main flag is not read (an orchestra plugin flags all sixteen outputs main); the menu no longer grows, its scroll bar has room, switches do not glide after a scroll; the empty arrangement's prompt fits short windows (`fix/empty-state-short-window`, its own branch cut from `origin/main`, not pushed). Verified on Windows: workspace clippy, `lumen-app` tests and E2E (`sidechain`, `outputs`, `hosting`), the host and hosting suites with real host processes, mutation checks, screenshots (`sidechain_row`, `outputs_row`) in both themes at 100 and 200%, one real sampler's outputs, the 6 smoke tests, macOS cross-clippy, and on the pushed branch in WSL the CI RealtimeSanitizer steps (realtime, hosted, hosting link, plugin-host audio path: all pass) and the performance gate (largest change +1.06%, `engine_hosted_64`; none over +2%, no baseline change). **Not run:** Linux and macOS app runs, any real plugin beyond those above (**MT-VST-6 in `docs/manual-tests/M6.md` is the owner's**). The ADR-0069 measurements are still due before the M6 report. **Next:** the owner's MT-VST-6 and the gates, then the M6 report. Before it, 2026-10-18: **the plugin window's button moved into the card's header** on `m6-s5-presets-state` (the owner's D from rendered specimens: "Open editor" and "All 2.1k…" were cut short in the row; the header now has a window-glyph button where the kind badge was, `strip::editor_button`, and the row is Learn and "All N parameters"; docs `DESIGN.md` §7f, `milestones/M6.md` 5h). Before it, 2026-10-18: **stepped controls on the card's strip** on `m6-s5-presets-state` (PR #35; the owner's finding after 5f, option A chosen from rendered specimens): a two-state parameter is a switch, a parameter of 2 to 64 steps is a chooser (the plugin's own word, click for the next, an arrow for the list), a knob stays for the rest; each click is one undo step (`chain/stepped.rs`, `Output::Choose`, `Command::Texts`, `Hosting::{choices, ask_choices}`); also the Editor button's spacing, the preset menu staying open while a name is typed, preset names cut to 30 characters, and a refused preset given to the plugin again twice (`PRESET_RETRIES`). **Not run:** any real plugin (a real sample-based instrument, labelled RP-8, refused a saved preset with `setState failed` after its device was re-added: in KNOWN_ISSUES, not reproduced; the retry is the first try). Before it, 2026-10-17: **M6 slice 5 (presets and resilient state) is built** on `m6-s5-presets-state` (cut from `main` after PR #33; `origin/main` through PR #34 is merged in; pushed; ADR-0077; plan, owner choices R1 to R5 and progress in `docs/milestones/M6.md`). Built in parts 5a to 5f: the host lists program lists, reads and sets the edit controller's state and reads and writes bounded `.vstpreset` files (**protocol version 6: close Lumen and rebuild `lumen-plugin-host`**); program change reaches plugin instruments through the program parameter; loading a program or a preset file is one undo step that undo gives back exactly (`Document::load_device_state`, `Device.state_rev`, `hosting.rs` `try_preset`/`PresetNews`); running plugins are read about every 30 s and the last good state is never replaced by a failed read, failures are counted per plugin and nothing restarts a plugin by itself (Reload, Reload without sound); safe mode (`--safe-mode`, Shift at start on Windows, the offer after a start that did not finish, the marker `plugin-loading.marker`; `app/safe_mode.rs`); **5f, the interface:** the card's second row with arrows and a field naming the sound (a dot marks a changed one), a searchable menu of factory programs and preset files, Save, Save as… (never writes over a file) and Show presets folder (`chain/presets.rs`, `preset_files.rs`), the failed card saying which sound Reload starts from and how old it is and "Keeps stopping" with Reload without sound after three failures, commands `devices.preset` and `devices.save_preset` at the end of `Command`/`ALL`; card height 344. Format unchanged (v19). Verified on Windows with the test plugin and the real host: workspace clippy, workspace tests (E2E `presets`, `preset_menu`, `resilience`, `safe_mode`), mutation checks, screenshots (`shots preset_menu`, `safe_mode`) in both themes at 100, 150 and 200%, macOS cross-clippy. **Not run:** any real plugin (**MT-VST-5 in `docs/manual-tests/M6.md` is the owner's**), smoke tests, the WSL performance gate and RealtimeSanitizer (`link.rs` `host_event` changed in 5a), Linux and macOS runs. Known limits in KNOWN_ISSUES (the sound's name is kept for the session only; choosing the program a plugin already has does nothing). **Next:** the owner's MT-VST-5 and the WSL gates, then slice 6 (buses, multi-output, sidechain, dynamic latency) and the M6 report; the ADR-0069 measurements are still due. Before it, 2026-10-17: **saving now reads every running plugin first** (the owner's report: two plugins (anonymous RP-11, an orchestral instrument, and RP-12, a mastering effect) forgot their settings on reopening; their saved project held the state of one of ten plugins, because a plugin was read only after a reported parameter edit and the save did not wait; `Hosting::request_captures`, `App::save_waiting`, ADR "A save reads every running plugin"; MT-VST-4 step 25 is the owner's; the follow-up is built too: a change made only inside a plugin marks the project unsaved and reaches autosave, through `IComponentHandler2::setDirty` and a restart that says the values changed (`EditKind::Dirty`, `Document::mark_changed_inside`, `change_mark`; MT-VST-4 step 26 is the owner's; plugins that say nothing are still found by the save). Before it, on `m6-s4-parameters-editors`, three owner points. **A plugin's window now follows its track:** choosing another track hides every other open editor (hidden, not closed) and choosing the track again shows it as it was; Learn ends when its window is hidden (`ShowEditor`, `EditorStatus::visible`, `Hosting::show_editors_of`, `App::follow_chosen_track`; **protocol version 5: close Lumen and rebuild `lumen-plugin-host`**; MT-VST-4 step 23 is the owner's; the real window's hide and show is compile-checked only, the path above it is E2E-tested on the stand-in window). **Editors stay in front of Lumen** (the owner chose that over sinking them behind Lumen when it is clicked; a try of the other was committed and reverted). **A device card's track-coloured band follows the rounded corner** when a short tray has no picture (`card::track_band`; shots `chain-stopped-*-small`; MT-VST-4 step 24). Gates run: workspace clippy, the host, hosting and `editors` E2E tests (mutation checked). Before it, 2026-10-17: **MT-VST-4 run 1's six findings are fixed** on `m6-s4-parameters-editors` (ADR-0076 "MT-VST-4 run 1"; run record and re-check steps 17 to 22 in `docs/manual-tests/M6.md`; plugins labelled RP-5 to RP-7). Only reload, bus and latency restarts set a plugin up again (`protocol::RESTART_RECONFIGURE`); while a window request is stuck only window requests are answered "not responding" (now after 5 s, `server::EDITOR_WAIT`; tests shorten it with `LUMEN_HOST_EDITOR_WAIT_MS`) and the rest wait; the client drops answers owed to requests it gave up on (`HostProcess::owed`); controller calls run without the audio lock (`instance::Controls`); a relinking plugin is not shown as failed; editors are owned by a hidden host window (no taskbar button, no minimize) and restack only on a change; with the computer keyboard on, its keys pressed in a plugin's window play through Lumen (owner's choice; Space and everything else stay the plugin's; `Request::CaptureKeys`, notice `Response::Key`). **Protocol version 4: close Lumen and rebuild `lumen-plugin-host`.** Verified on Windows: the host, hosting and E2E tests with mutation checks; the real window code (owner, stacking, key capture) is compile-checked only. Before it: **M6 slice 4 (parameters and native editors) is under way** on `m6-s4-parameters-editors` (cut from `main` after PR #31; ADR-0076; plan, owner choices P1 to P4 and progress in `docs/milestones/M6.md`; not pushed). Built and tested on Linux: the host lists parameters with values, flags, groups and the plugin's own text, takes values, keeps a plugin's own edits (begin, perform, end) and gives them to the processor, collects what the processor reports (protocol and shared region **version 3**: rebuild `lumen-plugin-host` with Lumen); CC, bend and pressure reach plugin instruments through `IMidiMapping`; **format v19** (`Device.params`: the values the project holds over a plugin's saved state; `ViewState.shown_params`); `Document::set_device_params`; `Hosting` reconciles the project's values with each host both ways, so a change, undo and redo are heard and a plugin's own gesture is one undo step (`app/plugin_edits.rs`); a changed plugin's state is read once it rests and a save holds it (or the loaded state and the values, saying so when a plugin could not be read); **the card's strip (eight comet knobs, the owner's S2 in the send knobs' style) and the All parameters panel (the owner's L1)** with the command `devices.parameters`. Typing a value into a knob or bar is built too (the plugin reads the text: protocol `ParamValue`). **The editor window is built** (4f: the host's main thread pumps the window, `serve_pumped`; the card's button row with Learn and End plugin; the window's place kept per plugin in the settings), with the real Windows window only compile-checked here: everything above it runs on Linux against the host's stand-in window (`LUMEN_HOST_HEADLESS_EDITOR`), and the window is MT-VST-4 steps 5 to 8 and 14 to 16 for the owner. Gates (Linux container): fmt, workspace clippy, Windows (GNU) and macOS cross-clippy, the whole workspace suite (90 suites; the only failure is the timing-sensitive scenario below, which passes alone). **Not run:** smoke, the WSL performance gate, the real Windows editor window, any real plugin. A timing-sensitive E2E (`hosting::the_audio_panel_shows_each_plugins_work_in_the_block_budget`) fails under load and passes alone. Before it, 2026-10-17: **fix: missed plugin blocks** on `fix/plugin-host-misses` (cut from `main` after PRs #29 and #30; ADR-0075, proposed). The owner saw CPU 70 to 100% and pops with a plugin synth into a plugin amp at 128 frames/44.1 kHz. Measured on their machine: `lumen-host-bench` with each real plugin, 14 to 66 µs a round trip; the amp's host 9 to 15% of a core. So the time is not the plugins' work nor the bench's handover, and a block past the 75% deadline is silence (instrument) or dry (effect), shown nowhere. Built (owner's fixes 1 to 3): a busy host's events go with the next block (`HostedInstrument::begin`); host sessions run at `HIGH_PRIORITY_CLASS` with power throttling off (`process::run_at_full_speed`); each block's plugin work comes back in the answer (`Answer::work_ns`, shared region **version 2**: rebuild `lumen-plugin-host` with Lumen) and `Guard` times its wait (`Status::take_cost`, `hosted/timing.rs`). **The readout is built** (the owner's B, the block budget bar in the audio panel: `transport/budget.rs` the model, `budget_view.rs` the drawing, `Status::take_block_time`, `Hosting::take_costs`, tokens `budget` and `size::BUDGET_*`, DESIGN §7g; the five costliest plugins as segments, every plugin in a list that scrolls and gives way first in a short window; mock-ups use generic device names). **Cause of the owner's CPU found and fixed:** plugins were configured for 8192-frame blocks while the engine gives them at most 256; the amp plugin did six times the work on every 128-frame block (1.36 against 0.23 ms, measured in process). They are now promised the device's block, at most `lumen_devices::MOST_FRAMES` (256), and set up again when the buffer size changes (the owner's B; `hosting::promised_frames`, `Manager::relink_all(rate, frames)`); the host splits a longer block than promised into pieces (ADR-0075 "Cause found", "The device's block"). **MT-VST-B run 1 (owner):** 0.31 of 2.90 ms a block (11%, CPU peak 20 to 24%) with option B, from 1.56 ms (54%, peak 70%); the amp plugin's work 0.29 ms from 1.54. The audio-path change is not needed on these numbers. Steps 2 to 6 and 8, and whether pops are gone, were not reported. Readout gates (Windows): workspace clippy, 1,894 tests (393 E2E), 6 smoke, macOS cross-clippy; not run: WSL (no platform or engine-path change), any real plugin. Owner's power-plan comparison (High performance against Balanced) pending. Gates (Windows): fmt, workspace clippy, 1,888 tests, macOS cross-clippy, WSL clippy and tests; WSL perf gate `engine_hosted_64` +3.90% (clock reads and counters per hosted block), baseline updated. Before it, 2026-10-17: **fix: plugins ring out until their output is quiet** on `fix/plugins-ring-until-silent` (cut from `main` after PR #28 merged slice 3; ADR-0074, the owner's option B). The owner heard a plugin synth into a plugin amp cut out a second or two after each note: a hosted instrument was given blocks only for its reported tail (usually none) or one second, and stopped effects with it. Now `Guard` listens to each host's output (−90 dBFS, `QUIET_LEVEL`) and a plugin is given blocks until it has been quiet for two seconds (`QUIET_SECONDS`) or its reported tail, at most ten (`Guard::sounding`, `wake`, `hush`, `age`); the mixer rings while any plugin effect that is on is sounding (`HostPool::sounding`), so effect tails ring out after Stop (part of B2 brought forward). The CPU reading in the report was looked at, not changed: it is a one-second peak of the callback's wall time including the plugins' own work; the handover measured 12 µs median, 34 µs p99 a round trip on the owner's machine (`lumen-host-bench`). The readout's average and per-plugin time are to be pitched as a mock-up. The test plugin has a `release=MS` fault. New tests: devices units, `tests/hosted_effects.rs` (two), E2E `hosting::a_plugin_instruments_release_is_heard_until_it_ends_not_cut_after_a_second` (engine test and E2E fail without the fix). Gates (Windows): fmt, workspace clippy, 1,884 workspace tests (392 E2E; `midi::remote_and_track_are_separate_switches` failed once under the full suite's load and passed alone three times and in the next full run). macOS cross-clippy clean; WSL performance gate passes (`engine_hosted_64` +0.78%, the rest within ±0.21%). **Not run:** smoke tests, any real plugin (MT-VST-R in `docs/manual-tests/M6.md`), RTSan, Linux and macOS runs. Pushed; the owner merges. The MIDI settings E2E race found on the way is fixed in its own session (`fix/` branch). Before it, 2026-10-17: **M6 slice 3 (chain editing) is built** on `m6-s3-chain-editing` (cut from `m6-s2-chain-states`, which is not merged into `main` yet; merge `main` into this branch once it is; ADR-0073; plan and progress in `docs/milestones/M6.md`). Owner choices: E1 (the preset menu, editor button and parameter strip move to slices 5 and 4) and E2 (Alt-drag copies, plus a menu and the clipboard keys). **Format v18** (`view.folded_devices`; identity migration, frozen golden). `Document::{move_device, copy_device, paste_device}` (`document/devices.rs`: one undo step each; a gap counts the chain as it stands; the master is addressed like any track and takes effects, no instrument). The chain tab: drag a card to reorder or onto a track's header (Alt copies; `Arrangement::device_drop_target`), Alt+Left and Alt+Right, Left and Right, Ctrl+C, Ctrl+V and Ctrl+D act on cards while the tab has the keyboard (the existing commands guarded by `chain.live`, as Delete is), six new commands at the end of `Command` and `ALL` (`devices.move_earlier`, `move_later`, `copy`, `paste`, `duplicate`, `fold`), the card menu, double-click folds a card to a 48 pt strip (look chosen without specimens), a click on the master's header chooses it. `hosting.rs` restarts an instrument that moves to another track (the engine keeps an instrument by its track); an effect keeps its host. The plugin's own picture: `chain/pictures.rs` reads `Contents/Resources/Snapshots/<class id>_snapshot[_2.0x].png` off the UI thread (1 MiB, 2048 px; deviation from the plan, which had the scan child return it: ADR-0073 item 6). Tests: 10 unit tests for the edits, 4 E2E for hosting under moves (`tests/e2e/hosting.rs`: mutation checked), device E2E for drag, keys, folding, the master and pictures (`tests/e2e/devices.rs`). Gates (Windows): fmt, workspace clippy, macOS cross-clippy, 1,876 tests (391 E2E) and 6 smoke. **Not run:** the WSL performance gate and Linux and macOS runs (nothing pushed), any real plugin; MT-VST-3 is the owner's. **Found, not fixed:** a plugin instrument that finishes loading while a note is held does not sound it until the next note (KNOWN_ISSUES; an engine change in `midi/player.rs`, proposed). Two unrelated E2E scenarios (`dropdown`, `clock`) each failed once in a full run and passed on rerun. **Next:** the owner's MT-VST-3 and the carried-over ADR-0069 measurements and real-plugin reload result; then slice 4 (parameters and native editors), cut from `main` once the slice 2 and 3 branches merge. Before it, 2026-10-17: **M6 slice 2 is closed** (2f: the owner passed the MT-VST-2 walkthrough, no step detail; the report is in `docs/PROGRESS.md`). **The ADR-0069 measurements were not taken**, so the callback's wait stays on trial (KNOWN_ISSUES); the real-plugin reload result (RP-1 to RP-4) is unreported. Branch `m6-s2-chain-states` holds 2e and these records, not pushed. **Next: slice 3 (chain editing), cut from `main` once `m6-s2-chain-states` merges; the measurements are due before the M6 report.** Before it, 2026-10-17: **M6 slice 2e, wider tests for the failed and missing cards, is built** on `m6-s2-chain-states` (cut from `main` after PR #26 merged slice 2d; progress in `docs/milestones/M6.md`). Four new E2E scenarios in `tests/e2e/devices.rs` (a real host death shown on the card with Reload; remove then undo of a failed device; remove while loading; names and Tab stops on every card face; `access::{unnamed, focused}` are now `pub(super)`), and the card shots cover both themes at 100, 150 and 200%. No product code changed. Gates (Windows): workspace clippy, macOS cross-clippy, 374 E2E, 6 smoke. **Not run:** the WSL performance gate, Linux or macOS runs of the new scenarios, any real plugin. **Next: the owner reloads the real plugins with the latest host build (MT-VST-2 step 9a: RP-1 to RP-4 in KNOWN_ISSUES) and looks at the cards (step 10); then 2f** (walkthrough, ADR-0069 measurements, report). Not pushed; the owner merges. Before it, 2026-10-17: **M6 slice 2d, the Devices tab, is built** on `m6-s2-device-chain` (ADR-0072; the owner's C4 card design, "a defined chain with lit cables"; progress in `docs/milestones/M6.md`, DESIGN.md §7f). New `crates/lumen-app/src/chain/`: `place.rs` (where a chosen device goes, pure), `geometry.rs`, `art.rs`, `card.rs` (one card in four faces: ready, loading, failed with Reload, missing with Rescan plugins), `view.rs` (tray, cables lit by the Beam while playing, chips, ports, the drag's drop mark) and `actions.rs`. The detail panel's Devices tab is live and grows to fit a card. Double-click or Enter in the browser adds to the selected track (an effect after the chosen card, an instrument replacing the synth, a new track in one undo step when none fits: `Document::add_device_on_new_track`); dragging a browser row shows a line and a plus at the gap; the power button, Delete and the new commands `devices.add`, `devices.remove`, `devices.bypass` (at the end of `Command` and `ALL`). New icons Power, Sliders, PlugOff; `widgets::{shorten, fit}`; test seams `testing::{select_tracks, chosen_card, pretend_device_state}`. Gates (Linux container): workspace clippy, 606 unit tests, 370 E2E (10 new, `tests/e2e/devices.rs`), 6 smoke, macOS cross-clippy; two mutations checked. **Not run:** the WSL performance gate (no engine or project change), any real plugin, Windows or macOS runs of the interface. The failed and missing faces came early (2e keeps the polish and wider tests). **Next: the owner's look at the cards (MT-VST-2 interface steps in `docs/manual-tests/M6.md`), then 2e, then 2f** (walkthrough, the ADR-0069 measurements, report). Pushed to the branch; the owner merges. Before it, 2026-10-16: **M6 slice 2c, the hosting manager, is built** on `m6-s2-device-chain` (ADR-0071; plan, owner choices C1 to C3 and progress in `docs/milestones/M6.md`). New crate `lumen-hosting`: `Link` (a `Hosted` over the shared-memory `BlockLink`; notes only, other messages dropped and counted), one thread per host that owns its `HostProcess` (load, restore state, configure, attach, each with a deadline; then it watches the process and sets the flag the callback reads), and `Manager` (`want`, `reload`, `forget`, `relink`, `poll`; four loads at a time). The app's `hosting.rs` is the bridge: plugin effects are named in the routing (`Device::Hosted`) from the moment the project holds them, at their last known latency, so loading, failed or missing devices keep their time; instruments are stand-ins until the host loads; bypass, latency news, restart requests and failed states flow from the engine's `Status`; the reported latency and tail are stamped at save like the view, not written as an edit (C2, refined). Two engine fixes: a host brought by a new or skipped track list is never given away. Gates: workspace clippy and tests (1,803 passed), WSL performance gate (+0.00%), RealtimeSanitizer over the link and the hosted tests, macOS cross-clippy, the Linux E2E suite twice (360). **You cannot add a plugin to a track from the interface yet: that is 2d** (browser attach, commands, cards); the E2E scenarios open projects that already hold the devices. Not run: any real plugin, macOS app runs, smoke tests (slice end). **Next: 2d** (cards, browser double-click and drag, the three commands; remove the `allow(dead_code)` on `document::devices`), then 2e (failed and missing cards, Reload), 2f (walkthrough, MT-VST-2 by the owner, report). Before it: **M6 slice 2, the first complete device workflow, is under way on `m6-s2-device-chain`** (cut from `main` after slice 1 merged; plan, owner choices D1–D9 and B1–B3, and progress in `docs/milestones/M6.md`). **2a is built:** format v17 (optional `DeviceKind`, reported latency, controller state, plugin version; identity migration, frozen golden) and `Document::{add_device, remove_device, set_device_bypass}` (`document/devices.rs`, one undo step each, `Refusal` says why; an `allow(dead_code)` stays until 2d calls them). **2b is built with fake hosts** (ADR-0070): the `Hosted` contract and `Guard` in `lumen-devices` (the engine depends only on core, rt and devices), hosted instruments begun before the clips render (`TrackSource::begin`), hosted effects in the mixer (`Device::Hosted`, a fixed host pool, an always-running delay-line fallback of the reported latency, a 20 ms wet glide, early hand-over of the first hosted device of nodes nothing feeds), attached with `MixerHandle::attach_host` / `detach_host`; the MIDI player carries instruments by `kind`, so the app builds `HostedInstrument::placeholder()` in later track lists. Chains of two or more hosted effects on a track, and effects on groups, run one after another (B1); an effect's tail is cut at Stop until slice 6 (B2); the clock is read only when a host is attached (B3). Gates run: workspace tests, clippy, WSL performance gate (all +0.00% against the regenerated baseline, new `engine_hosted_64`), RTSan over `realtime`, `hosted`, `hosted_effects` (CI's `rtsan` job runs the two new files), macOS cross-clippy. **Not run, and not claimed:** any real host process or plugin, a whole callback with hosts, E2E and smoke for the slice (due at 2f). The unrelated `lumen-midi` flaky test is in KNOWN_ISSUES. Card design for the Devices tab is chosen (design system v3, C with C2's kind treatment, 96 pt artwork band with fade; plugin snapshots are slice 3). **Next: 2c, the hosting manager** (one `lumen-plugin-host` process per VST3 device, a shared-memory adapter over `BlockLink` implementing `Hosted`), then 2d (cards, browser attach, commands), 2e (failed and missing states), 2f (walkthrough, MT-VST-2 owned by the owner, report). Not pushed; the owner merges.

Before it, 2026-10-16: **M6 slice 0 is complete but for its report (MT-VST-0 "Editors" passed with one real plugin, run 6; slice 1's MT-VST-1 passed on Windows, owner-reported, no measurements):**
**ADR-0069 is Accepted** (the audio-path ADR). The owner chose option A:
the audio callback may make a bounded wait on a plugin host's completion event,
under the ADR's contract, **on trial** (the pipelined path, one block of extra
latency, is the fallback). Slice 2's MT-VST-2 decides whether it stays and must
include recording with monitoring through an effect at 64 and 128 frames. The
slice 0's report remains, then slice 2. Before it, 2026-10-15: **M6 slice 0's last measurements, on
`m6-s0-audio-path`** (cut from `main` after slice 1 merged). The two heap
operations the crash runs reported are fixed: the loop asked
`HostProcess::ended`, which words a crash in a `String`; it asks the
allocation-free `has_ended` (guarded test
`shm_rt::noticing_a_crashed_host_does_not_allocate`; crash and hang benchmark
runs now show 0 heap operations). The 64-host budget is measured on Windows
(MT-VST-0 run 3, `m6-budget-64.csv`): at 128 frames/48 kHz `event` stays
inside the 1.333 ms half-period budget up to 100 µs of plugin work per track
(and at p99 at 200 µs); `event/spin` missed 40 deadlines at 200 µs. One run,
machine in use, synthetic load. **The editor-focus spike is done on Windows with the test plugin**
(`lumen-editor-spike`, MT-VST-0 "Editors", run 5; ADR-0067 "Editors"): owned
windows across processes are rejected (a hung editor freezes Lumen's window
moves and minimizes); the default is an unowned window on top only while
Lumen or an editor has the foreground; no shortcuts forwarded; a hung editor
is flagged and the user decides. The owner ran the manual steps with one real plugin (run 6: loads, typing
fires no Lumen shortcut, the rest passed); a second monitor and more plugins are untried. The owner accepted the representative workload (64 tracks, 100 µs
each; 200 µs is the documented limit). `shm_rt` passes under RTSan in WSL and
is a new CI step. **Still open before the audio-path ADR can be written:**
process grouping (modelled, run 3: no case for chains; real chain code unbuilt), and
dynamic latency, partly done (`shm_latency`: the news reaches the loop in the same block, allocation-free;
controller-origin restarts and bus changes have no trigger yet: slices 4 and 6). Not run: macOS.
Before it, 2026-10-16: **M6 slice 1, plugin discovery, is
built** on `m6-s1-plugin-scan` (ADR-0068; plan and progress in
`docs/milestones/M6.md`). Started while slice 0's audio-path ADR is still open
(the owner ran MT-VST-0 and said to go on). Owner choices: scan at startup in
the background; a plugin that crashes or hangs is blocked with its reason and
retried only when its file changes or on Retry; browser layout B (by vendor,
category chips) plus a type filter, from the "Lumen Plugin Browser Specimens"
canvas. `lumen-plugin-host scan <path>` (`scan/{report, host, run}.rs`) is one
bounded, versioned reply per module in its own process with a deadline;
the new crate `lumen-plugin-catalog` finds modules, scans them four at a
time, caches answers by file stamp (a blocked module is an entry with its
reason), words the reasons and answers the browser's queries; in the app,
`plugins/` owns the scan thread (polled each frame, saves the cache
atomically beside `settings.json` as `plugin-cache.json`) and the settings
section (folders, favorites, recents, scan at startup; no version change) and
`browser/` is the Devices tab (`rows.rs` pure and tested, `list.rs` paints
only rows in view, rows are clickable but not Tab stops: the list is the
pane's one stop, F6, then arrows, Left/Right fold, F stars, Enter chooses).
Commands: Ctrl+F search, rescan, rescan all, add folder, favorite, scan at
startup (area Devices; **new commands go at the end of `Command` and `ALL`**).
Flags and test seams: `--plugins off`, `--plugin-roots/--plugin-host/
--plugin-cache`, `Launch::with_plugins` (tests are off by default). **Scan
program location:** beside the Lumen executable, so `cargo run -p lumen-app`
needs `cargo build --release -p lumen-plugin-host` first (the browser says
so; KNOWN_ISSUES). Verified on Linux with the test plugin: catalog 30 unit
tests and 11 scanner scenarios, host 9 reply tests and 10 scan tests, app 580
unit tests, **354 E2E (15 new in `tests/e2e/plugins.rs`), 6 smoke**; clippy
clean for Windows (GNU) and macOS. **Not run, and not claimed:** anything on
Windows or macOS, any real plugin, the Windows job object for scan
processes, frame times with a large library. **Next: the owner's MT-VST-1
(Windows)**; then slice 2 (the first complete device workflow: a device on a
track, hearing it, bypass, save and reopen), which needs the audio-path ADR
from slice 0's open gate. Cloud container: `apt-get update`, then
`libasound2-dev libudev-dev pkg-config mesa-vulkan-drivers xvfb
libxkbcommon-x11-0`; the smoke tests run under `xvfb-run -a`.

Before it, 2026-10-15: **M6 slice 0, part 3: the shared-memory
audio path and the measurement harness** on `m6-s0-shm-transport` (ADR-0067;
the branch was renamed from a generated `claude/…` name, which never existed
on the remote). `lumen_plugin_host::shm`: one block in flight per host (a
late host can never write into a block that has moved on; its answer is
dropped by sequence and counted), a region whose every count is checked on
both sides, wake-up settings per side (`Wake { host, lumen }`, each spin,
event or hybrid), `BlockLink` allocation-free and lock-free on Lumen's side
(tested under the allocation guard), the host's audio thread (`blocks.rs`,
one lock shared with the control thread, abort on panic),
`Instance::process_into`, `HostProcess::attach`, protocol v2
(`Attach`/`Attached`), and on Windows a kill-on-close job object. The harness
is `lumen-host-bench` (MT-VST-0, `docs/manual-tests/M6.md`). 84 tests pass in
the crate on Linux (47 before); mutation checks on the generation and busy
rules fail as they should; Windows (GNU) and macOS cross-clippy are clean.
**Not run, and not claimed:** anything on Windows (file mapping, events,
timers, MMCSS, the job object) and every measurement; the Linux harness run
was a four-core sandbox check, no numbers from it are used. **Next: the owner
runs MT-VST-0 on Windows**; its numbers decide the wake-up, whether the
callback may wait on the operating system, and the process grouping (the
next ADR). Slice 1 (plugin discovery) follows slice 0's gate. To work on the
crate in the cloud container nothing beyond Rust is needed
(`cargo test -p lumen-plugin-host`; `rustup target add
x86_64-pc-windows-gnu aarch64-apple-darwin` for the cross clippy).

Before it, 2026-10-15 (later): **M6 slice 0, part 2: the production host
core** on `m6-s0-host-core` (ADR-0066; the branch was renamed from a generated
name, and the old remote name never existed). New crate `crates/lumen-plugin-host`
and its binary: `Module` loads a VST3 module with the OS loader, `Instance`
runs one plugin, `protocol`/`wire` are the versioned, bounded messages (a
block carries a configuration generation and a sequence number; stale blocks
and answers are rejected and counted), `client::HostProcess` talks to a host
process with a deadline on everything and says how it ended, `process` makes
the host die with its parent (Linux). 47 tests pass on Linux against the test
plugins, in process and in real child processes, with every fault; the crate
passes clippy for Windows (GNU) and macOS but was **not run** there. **Not
done, and not claimed:** the shared-memory block transport, the signalling and
process-grouping measurements (they need Windows), the Windows job object, the
editor, the scanner. Next: the shared-memory transport behind the same
messages and the measurement harness for the owner to run on Windows, then the
architecture ADR for the audio path; slice 1 (plugin discovery) follows slice
0's gate. Packages for the cloud build are listed below.

Before it, 2026-10-15: **M6 (VST3 hosting and the device system)
started** at the owner's instruction, with M5's closeout (MT-MIDI-7 to 9, the
WSL gates, the report) still open and not marked passed. Slice 0, part 1:
`crates/lumen-test-plugin` (ADR-0065), a portable deterministic VST3
instrument and effect with faults chosen by `LUMEN_TEST_PLUGIN_FAULT`; tested
on Linux through the VST3 interfaces and in child processes (crash, hang).
ADR-0005's section status now matches its index entry; M6.md says v16. **Not
done, and not claimed:** the signalling and process-grouping measurements,
which need Windows (`docs/milestones/M6.md`, "Slice 0 progress"); the
architecture ADR must follow them. Branch: `m6-s0-test-plugin` (renamed from a generated name; the old remote
name `claude/eloquent-knuth-oapshw` is for the owner to delete). Next: the production host crate and a
loader for the test plugins, then the measurement harness for the owner to
run on Windows. Cloud container: the packages listed below are needed for
the workspace build.

Before it, PR 16 review corrections: recording eligibility shares
capture's compensated positions and uses engine-side loop-pass bounds.
Disabling Loop before the first wrap preserves the saved visible window.
Format v16 marks hidden captured notes so they cannot continue an old
note across a seam; older notes retain their existing behavior. See
ADR-0064 and `docs/milestones/M5-recording-window.md` for verification.
M5 closeout remains pending.

Before it, 2026-10-14: **MIDI recording window correction**
(ADR-0064; `docs/milestones/M5-recording-window.md`). Capture continues
until Stop or Record-off; loop/punch boundaries mask full hidden MIDI
content rather than discard it. Replace playback resumes at punch-out;
first loop entry preserves delayed pre-loop messages. No format change.
MT-MIDI-7 step 7 has the owner verification steps. M5 closeout remains
pending; no new milestone has started.

Follow-up: the first note could land slightly before a nonzero recording
start after latency compensation. `Capture::recording` floors only the
initial pass at Record start, independently of the punch/loop mask. Later
passes retain their positions. MT-MIDI-7 step 17 covers the owner recheck.
The owner reports the fix works after `409212c` and authorizes pushing
`codex/midi-recording-window`. The pre-push WSL performance gate passes all
nine scenarios (largest increase +1.50%; +2% limit); no baseline update.

Before it, 2026-10-14: **CI fixes on `m5-midi`** (PR 14;
ADR-0063). Run 166 failed on all three systems because the silent test
device ran one block per poll and fell behind the wall clock on a busy
runner. It now keeps its pace (`FakeDevice::pace`: `Pace::Every` for `test`,
`Pace::RealTime` for `test-paced`, which `with_audio_buffer` selects). E2E
rule: wait for what the engine answers (`wait_for`), never a count of
frames; write chords through `keys()`. Also fixed: canonical paths in the
Save As unit test, the MIDI sender's lateness bounds (median 8 ms, worst
25 ms), `smf::write` returning `Result` (`WriteError::TooSparse`), and
Export MIDI building its file on a worker. Verified on Linux (the whole
suite, and the record scenarios beside four busy loops); macOS could not be
run here, so the next macOS CI run is the check. Before it, 2026-10-14:
**M5 slice 8: the review is done
with its fixes** (`docs/audits/2026-10-13-m5.md`, Status table). Every gate
ran in the cloud container: 1,436 tests, 333 E2E and 6 smoke tests pass
(smoke under `xvfb-run` with `libxkbcommon-x11-0` and lavapipe installed).
Fixed: an audio-thread free in `take_new` (clips, MIDI, mixer: skip a
queued list only while `retired.slots() >= 2`); the synth under the
allocation guard; `validate::LONGEST` (2^52 ticks) bounds positions in
projects and MIDI files; splits into `widgets/{dialog, slider, format}`,
`app/startup.rs` (`App::new`), `note_editor/pointer.rs` and
`lumen-devices/src/synth/{mod, voice}`; `lumen-audio`'s unused `lumen-rt`
dependency removed. The perf gate here reads `project_parse` +5.84%, but
+4.90% at the baseline's own commit: this machine's offset (KNOWN_ISSUES).
Next: the owner's MT-MIDI-7, 8 and 9 and the WSL gates, then the M5 report.
Before it, 2026-10-13: **M5 slice 8 began**. The new
`tests/e2e/midi_walkthrough.rs` covers record → quantize → save/reopen → play;
the preliminary review moved Save As folder probing to the save worker. **M5 slice 7 is built** (MIDI files
and clock out, ADR-0062; owner's F1–F4 A). `lumen_project::smf` (on `midly`,
Unlicense with a deny exception) reads type 0 and 1 into parts split by
channel and writes type 0 or 1 at 960 PPQ (`tests/smf/`: hand-made files from
`make.py`, the golden `lumen-960.mid`, written once with
`LUMEN_WRITE_FIXTURES=1`). Import: `midi_files.rs` reads off the UI thread;
`app/midi_files.rs` places files (on a MIDI track one clip,
`Document::import_midi_clip`; between tracks a track per part,
`import_midi_tracks`) and offers the file's tempo in a banner
(`use_file_tempo`); drops keep a place per kind (`document::Places`; `drop_place`
takes the kind). **Import audio or MIDI…** is `Command::Import` (id still
`file.import_audio`). Export: `midi_export.rs` (what plays: windows, seams,
loops baked), **Export MIDI clip…** and **Export MIDI file…**, written with
`lumen_project::write_atomic`. **Round-trip fix (owner, 2026-10-13):** SMF
reading queues overlapping note-ons per key/channel and pairs offs FIFO,
instead of shortening the earlier note at a new attack (ADR-0062 follow-up;
project regressions and app export/reimport E2E). Source-file recheck pending.
Clock: `Use::sync` and a delay per output
(`settings.midi` ports' `sync`, `sync_delay_ms`; `midi_settings::Shown`), the
engine's `midi/clock.rs` (`MidiHandle::set_sync`, a port bit mask), the
sender's delay (`Output::set_sync_delays`) and stall Stop. **The command enum
must stay in `commands::ALL`'s order** (`ALL[command as usize]`; a test
checks it: a misplaced variant ran Create MIDI clip for Export). E2E `smf`
(4) and `clock` (1); E2E 331, smoke 6 pass (`xvfb-run -a`; the container
needed `libxkbcommon-x11-0` for smoke). Valgrind is in this container:
`cargo build --release -p lumen-perf && target/release/perf-gate` runs the
gate; slice 7 is flat against its base, but `project_parse` reads +5.83%
over the baseline here at the base too (KNOWN_ISSUES). Not run: macOS
cross-clippy. **MT-MIDI-8 waits for the owner** (with MT-MIDI-7). Commits on
this branch carry no `Claude-Session:` trailer (the owner's PR rule).
Before it, 2026-10-13: **MIDI learn moved to M7** (owner),
built with automation; M5 slice 7 became MIDI files and clock out.
Before it, 2026-10-13: **resizing a playing MIDI clip no
longer strikes its notes again** (ADR-0061, owner's report). The engine knew a
held note by its clip's start and the note's length, which a resize changes
on every frame of the drag; a held note is now known by the tick it began on
and its key (`hold::Held::start`, `hold::renewed`), so a resize only moves
its end. Engine test and E2E `midi_play::resizing_a_playing_clip_…` (both fail
without the fix). **MT-MIDI-7 step 37 waits for the owner.** Before it,
2026-10-13: **MT-MIDI-7 run 7's finding is
fixed** (ADR-0060; owner: "looking great", one bug). Overdub begun before a
clip and reaching it, or begun at its start, merged into that clip
(ADR-0057); it now records a new clip wherever it begins,
carrying what the clips under it played (`edit::record::overdub`, the old
"begun inside" rule for every take), and what it covers is cut away so the
old clips stay their own. Clip growing and merging are gone. E2E `record`
(two scenarios fail without the fix). **MT-MIDI-7 run 8 (step 36) waits for
the owner.** Before it, 2026-10-13: **the owner's two clip points are
fixed** (ADR-0059). A note carried on across a seam belongs to the clip it
began in: the engine knows where a run's clips meet (`MidiClip::seams`) and a
chase starts only notes that began in the clip the position is in
(`hold::owning_start`), so playing from the next clip does not strike it.
The clip editor shows every note, those outside the window too, in its
dimmed time (`EditorView::shows`, `ClipNotes::outside`, the lane, Ctrl+A);
note edits let a note stay as far outside as it is and come back in
(`edit::notes::time_room`, `resize`). The arrangement still hides them. E2E
`hidden_notes`, `overdub_seams` (fail without the fix); E2E 325, smoke 6;
shot `hidden_notes_editor`. Not run: WSL perf gate, macOS cross-clippy.
MT-MIDI-7 run 7 passed but for ADR-0060's finding. Before it,
2026-10-13: **the owner's Overdub report is fixed** (ADR-0058). Overdub over an empty lane makes its clip at once, empty
until a note is played. Notes **carry on across seams**: where a clip begins
exactly where the one before it ends and holds the same note (hidden before
its start), the note sounds and is drawn through (`edit::seams`). Overdub
begun inside a clip carries what was heard whole (`record::carry`: a note
sounding at the record start keeps its start before the new clip; nothing is
cut at the take's end), so it sustains through recording and its commits.
Playback joins touching clips into one engine clip, a run known by its first
clip (`midi_runs.rs`; `midi_play` uses it, the engine is unchanged), so a
held note keeps its owner while the seams after it move. Mini notes draw
carried notes (`ClipNotes::carried`); the clip editor does not yet
(KNOWN_ISSUES). E2E `overdub_seams` (both fail without the fix); E2E 324,
smoke 6 pass. Not run: the WSL perf gate, macOS cross-clippy. **MT-MIDI-7 run
6 (steps 31 to 33) waits for the owner.** Before it, 2026-10-13: **the
owner's four recording and trim points are fixed** (ADR-0057). MIDI clip edges keep the distance they
were picked up at (`Gesture::Trim::grab`), trims do not glide, and a track's
note layout counts hidden notes (`mini::track_range`). Overdub begun inside a
non-looping MIDI clip is a new clip from the playhead to the stop carrying the
old notes there; begun at or before a clip, the clip grows (start back, end
on) and takes the take (`edit::record`: `begun_inside`, `extended`,
`heard_in`). With Loop on a take begins at the playhead (`Session::from`).
While recording the arrangement draws `Document::preview_takes` every frame
(the project as the next commit makes it); the recording tracks are on their
own GPU layers (`canvas.rs` `live`, `live_mini`), the main layers skip
commits. `live_take.rs` is gone; E2E use `drawn_clips`. E2E 322, smoke 6
pass (`xvfb-run -a`); `midi_out` sender timing flakes in this container on
the unchanged code too (KNOWN_ISSUES). Not run: the WSL perf gate, macOS
cross-clippy. **MT-MIDI-7 runs 4 and 5 (steps 22 to 30) wait for the
owner.** Before it, 2026-10-13 (after run 3): **a MIDI clip is a window
onto its notes** (format v15, ADR-0056; the owner chose the clip window over
stacked clips). Trims, covers (moves, copies, Replace recording), splits and
slips only move notes in clip time; notes before the clip's start (negative)
or past its content are kept, hidden (`edit::in_window`, `shown_notes`).
Playback (`midi_play`), the clip editor (`EditorView::window`), the lane
(`LaneView::window`) and "every note" (Ctrl+A, tools with no picks) read only
the window. Overdub across clips already merges at the
project level; the owner will reproduce what they saw (MT-MIDI-7 step 26).
**MT-MIDI-7 run 4 (steps 22 to 26) waits for the owner.** The WSL perf gate
was not run. Before it, 2026-10-13 (run 3): **one note layout per track.**
Clips fitted on their own drew one-note clips an octave apart at the same
height; now every MIDI clip on a track shares one `mini::Fit` over the
pitches all of them play (`mini::track_range`, at least 48 semitones,
centred), and the take uses it too (`LiveTrack::range`; `live_ranges` is
gone). Shot `mini_notes_octaves`. Also: Overdub over the same pitch stacked
notes; now a note played wins its pitch
(`edit::record::settle`, also between loop passes), and the engine applies
the rule as keys are played while recording (`TrackState::yields`,
`give_way`), so the old note stops sounding too. **MT-MIDI-7 run 4 (steps
22 to 24) waits for the owner.** The WSL perf gate was not run for the
`lumen-project` change. Before it, 2026-10-13 (latest): **MT-MIDI-7 run 2 fixes, clip
previews with shared track pitch ranges** on `m5-midi` (ADR-0055 amendment
on clip previews). Notes inside arrangement clips now have the piano roll's
proportions: 128 bands a track (`mini::BANDS`), a clip's range divided evenly
by `mini::Fit` (padded to 16, centred), a note spanning its row's bands (a box
may span row slots: the top 8 bits of `BoxInstance::row`,
`lumen_render::spanning`). The take is drawn from the same fit
(`LiveNote::range`, `record.rs` `live_ranges`), so nothing moves on stop; the
fixed `live_band` layout is gone. Nothing glides while recording, nor for the
commit that ends it (`ClipGlides::update`). E2E 314, smoke 6 (under
`xvfb-run -a` in the container). **MT-MIDI-7 run 3 (steps 19 to 21) waits for
the owner.** Open: the clip editor follows commits (about once a second), not
every frame. Before it, 2026-10-13 (later): **MT-MIDI-7 run 1 fixes** on `m5-midi`
(ADR-0055 amendment). The owner's findings: the take painted in chunks,
recording over a clip redrew it at every commit, and held keys and notes held
across the loop wrap were lost. Now: the take is drawn every frame from the
capture (`arrangement/live_take.rs`, `clips.rs` `draw_live_take`; commits stay
about once a second and at each wrap); commits give the pieces they cut stable
ids (`Recording::pieces`); the engine keeps the keys a recording track's input
holds and queues them as note-ons when recording begins, numbers loop passes
(`Captured::pass`, `WRAP` markers), and the app's `Capture::looping` cuts a
note held across a wrap at the loop end and carries it on from the loop start.
MT-MIDI-7 run 2 (steps 15 to 18) waits for the owner. Before it, 2026-10-13:
**M5 slice 6 (recording MIDI) is built** on `m5-midi` (ADR-0055; the plan and what changed from it are in `docs/milestones/M5.md`). Record (F9 or the button) keeps what the armed MIDI tracks play, after a count-in of none, 1, 2 or 4 bars; takes land as Replace or Overdub, one undo step per recording, committed about once a second, with autosave every 10 s while recording. Owner choices: loop passes layer, a clip runs from record start to stop, settings in the app file (no format change, still v14), and the 1-A, 2-B, 3-A look. Built by Haiku agents (take builder, settings, E2E, the bar and the clip look) and the parent (engine and recorder). Open: with Loop on a take starts at the loop's start; edits to the same track during a take are lost; no manual latency offset until M9 (KNOWN_ISSUES). Not run here: the WSL perf gate and the macOS cross-clippy. **MT-MIDI-7 waits for the owner.** Cloud container: `apt-get update`, then `libasound2-dev libudev-dev pkg-config mesa-vulkan-drivers xvfb libxkbcommon-x11-0`. Next: MT-MIDI-7, then slice 7 (files, learn and clock).

Before it, 2026-10-12: **M5 slice 5 (quantize and its tools) is
built** on `m5-midi` (ADR-0054; plan in `docs/milestones/M5.md`; owner:
ghost-trail preview, comet sliders in the popover, Q on every note when none
are picked, restore-recorded-timing deferred). Pure edits in
`lumen_project::edit::{quantize, note_tools}`; `Document::{quantize,
humanize, fixed_length, scale_constrain, reverse}_notes`; the UI is
`note_editor/quantize.rs` (the Quantize button and popover, `Options`, the
stand-in clip the roll draws while the popover is open, ghost trails in
`paint.rs`); `widgets::comet_slider` (`SliderStyle::comet`) and
`Menu::{staying, open_for, close_for}` are new shared pieces. Q applies the
last settings, Shift+Q opens the popover; there is no Iterative switch
(strength below 100% applied again is iterative). Options are not saved yet
(KNOWN_ISSUES). E2E `quantize` (7), shot `quantize_popover`, workspace tests
pass (E2E 296). **Not run:** the WSL perf gate and the macOS cross-clippy.
**MT-MIDI-6 waits for the owner.** Built with Haiku subagents for the pure
edits, Document tests and E2E; the parent did the UI. Cloud container: run
`apt-get update` first, then install `libasound2-dev libudev-dev pkg-config
mesa-vulkan-drivers xvfb libxkbcommon-x11-0`. Next: slice 6 (recording MIDI).

Before it, 2026-10-12: **M5 slices 0 to 4 are built and their manual
tests have passed** (MT-MIDI-1 to 5; follow-up findings are in
MANUAL_TESTS and KNOWN_ISSUES). The slice 3 and 4 plans are folded into
`docs/milestones/M5.md` (the separate files are gone). `m5-midi` was
squashed and force-updated upstream; an older local copy is the branch
`backup/m5-midi-pre-sync`, safe to delete. **Next: slice 5, quantize and
its tools** (C5: the current grid, start only, 100%, no swing; Q applies
the last settings, Shift+Q opens the options popover with a live preview;
humanize, legato, fixed length, transpose, scale-constrain, reverse).
Specimens first (the quantize popover), as at the start of each slice.
Reuse: slice 3's legato; slice 4's `velocity::set`, `velocity::scale` and
`events::set_point` are tested and uncalled, humanize is their first user.

Before it, 2026-10-12: **owner bug fixes** on `m5-midi`: a click on
a clip's header while stopped locates to the clip's start; the bar ruler's
cursor is a grab hand; dropdown and sends lists pad their scrolled rows by
`raised::FOCUS_OUTSET` (`raised::ring_room`) so focus rings are whole; tabs
and segmented controls glide their chosen *index* between live rects
(`toggle::slide_between`), so a moving strip carries them; the velocity
lane draws a head per note, a press on a head drags it (and every picked
note, relative), `LaneView::head_at` picks among overlapping heads; a
Pointer double-click on empty roll space draws a note (ADR-0047 and
ADR-0050 amendments). E2E 289 pass. Built by Sonnet subagents.

Before it, 2026-10-12: **the owner's Lumen design system is
applied** on `m5-midi` (ADR-0052; DESIGN.md v2, §5a Material). The design
system is a Design System artifact (tokens, a Material card, a card per
component); where it and DESIGN.md differ, it wins. Asked where it and
ADR-0051 differed, the owner chose it on all four: on = `Tone::On`
(`accent_muted`, a 38% accent hairline, no glow); solo, the primary and the
chosen tool = `Tone::Accent` (filled, no halo; amber only warns); a lit M =
`Tone::Mute`; raised controls = `control_hi` to `control`, edge light,
border, Ember hover, no drop shadow, radius by height
(`radius::for_height`); the arm is round again. New tokens in `theme.rs`
(`ground`, `control_hi`, `thumb_hi`/`thumb`, `edge_light`, `shade`, `ember`,
`accent_hi`/`accent_lo`/`on_accent`, `mute`/`on_mute`, radius `XL`/`FULL`,
`stroke::*`, `CONTROL_LARGE`, `TRANSPORT_ICON`, `TRANSPORT_BAR` 44,
`TRACK_STEP`, `HEADER_COLUMN`, `MONO_SMALL`, `KEYCAP`; `motion::{WASH,
BREATH}`); `widgets/raised.rs` paints the material (`paint`, `paint_sunken`,
`paint_field`, `lip`, `primary_button`, `paint_focus`). Built by Sonnet and
Haiku subagents (audits, fixes, visual checks of the shots). Also fixed: the
piano roll's B|C and E|F lines sat one row low. 1157 tests, E2E 273, smoke 6
and all shots pass. Open: MT-DS-1 (the owner's look check), the record icon's
2.7:1 and menu rows' 33 pt and 4 pt gap (KNOWN_ISSUES). Use
`transport::divider` for dividers in the bar (egui's adds 6 pt and the bar
must fit at 960).

Before it, 2026-10-11: **M5 slice 4 (velocity and controller lanes)
is built** on `m5-midi` (ADR-0050; the owner took every recommendation from
the live "Lumen Controller Lane Specimens" artifact: V A stems with round
heads, L A one lane with a tab switcher, T A Pencil, Line and Curve in the
lane's header, R B a 1/64 event density for Line, Curve and bend with the
Pencil on the grid; plan `docs/milestones/M5.md`). **No format change**
(v14; controller events have been in clips since v11). Pure edits:
`lumen_project::edit::velocity` and `edit::events` (a drawn line is plain
events, only repeated values dropped: MIDI has no ramps); undoable
`Document` methods in `document/lanes.rs` (`Drag`, `Line`, `Shift` structs;
one undo step per gesture, the document changes on release as for notes);
the lane itself is `note_editor/lane/` (`geometry`, pure `gesture`, `paint`,
`ui`, state in `mod.rs`), under the roll with its own tabs and tools, sharing
the roll's time view; ten palette commands (the tools on keys 1, 2, 3, which
work whenever a clip is open; Shift in the lane starts a box rather than
drawing a line, to confirm with the owner), Delete removes picked lane points
when the lane was the last place pressed. Engine golden check for controller
and bend timing: `tests/midi.rs`
`drawn_controllers_and_bends_land_on_their_exact_sample_in_order`. E2E
`lanes` (18 scenarios), shot `clip_lane`. **Frame times with the 100,000-note
clip and the lane open are not measured here** (see below); MT-MIDI-5 passed 2026-10-12.
Built by **Haiku subagents** under the new delegation policy
(`.claude/skills/references/delegation.md`: the parent specifies, integrates
and verifies; Haiku for bounded work), the parent wrote the UI wiring. In a
Linux cloud container, install `libasound2-dev libudev-dev pkg-config` to
build, `mesa-vulkan-drivers` for GPU tests and shots, and `xvfb`,
`libxkbcommon-x11-0` to run the smoke tests under `xvfb-run -a`. Next: slice 5
(quantize and its tools), specimens first; (MT-MIDI-4 and 5 have passed).

Before it, 2026-10-11: **M5 slice 3 (MIDI clips and the piano
roll) is built** on `m5-midi` (ADR-0047; the owner's Look A studio keys,
Draw A click-adds-then-drag-length, Live A preview-then-commit, chosen from
the "Lumen Piano Roll Specimens" artifact; plan `docs/milestones/M5.md`).
Format **v14** (`Note` has a mute flag: `Note::new` for unmuted;
`note.muted()`); note edits are pure transforms in
`lumen_project::edit::notes` returning the new picks, applied through
`document/notes.rs` and `midi_clips.rs`; picks are indices checked by
`NoteSelection::refresh`. Notes draw on the GPU in `piano_roll/` (a layer
per `canvas::Surface`, rebuilt only on content or pick changes; mini notes
on arrangement clips). The editor is `note_editor/` (Clip tab and pop-out,
`app/detail.rs`); while it holds the keyboard, Delete, Ctrl+D, 0, Ctrl+A,
Esc and the arrows act on notes. Auditioning is the live source
`AUDITION` (`audition.rs`); its editor hook takes the clip, because the
editor's state is lifted out of `App` during a gesture. `--bench-editor`
measures with the 100,000-note clip open: p99 at most 1.82 ms at
1600 × 900 and 2.22 ms at 3440 × 1380, idle 3 repaints in 3 s. Built by Sonnet agents in worktrees
(created from `main`: reset them to `m5-midi` first), merged here.
MT-MIDI-4 passed 2026-10-12.

Before it, 2026-10-11: **M5 slice 2 (playing live) is built** on
`m5-midi` (ADR-0046; owner's L A, J A, C B, K A, H B, R A and Ctrl+Shift+K,
chosen on the "MIDI: Playing Live" design canvas). Live input reaches the
engine through `LiveInput` (`lumen-engine/src/midi/live.rs`: `Listen`, a
live note tracker; stop and jumps leave live notes alone) and the app's
`LiveFeed` (`live.rs`: driver stamps to device frames, one block later;
`RenderContext::device`); **format v13** (`Track::midi_from`,
`Track::monitor`); who listens is `midi_play::listening` (selected or armed
MIDI tracks, Monitor Auto); the computer keyboard (`app/keyboard.rs`); the
MIDI chip (`transport/midi_chip.rs`; activity counted in `lumen-midi`, the
driver wakes the UI, never poll); header lights
(`arrangement/listen_light.rs`); the transport bar's room rule
(`transport/fit.rs`: say where a new bar control falls in its order). The
slice was built by **Sonnet agents in worktrees** (the owner's request:
orchestrate with Sonnet), reviewed and merged here; worktrees build their
own `target/`, so watch the disk (`.claude/worktrees/` is excluded in
`.git/info/exclude`). The perf gate reads ~+5% high in this cloud sandbox
(Ubuntu 24.04 glibc); trust CI's 26.04 container. MT-MIDI-3 passed. Next: slice 3 (MIDI clips and the piano roll),
specimens first; the "MIDI Clips & Editing" canvas holds earlier
explorations for it.

Before it, 2026-10-10: **M5 (MIDI and the piano roll) is approved and
under way** on `m5-midi` (plan `docs/milestones/M5.md`; owner choices: C1 a
small built-in synth plus MIDI out, C2 the detail panel's Clip tab *and* a
way to pop the piano roll out into its own window, C3 the computer-keyboard MIDI layout
layout as a transport mode, C4 replace with an overdub switch, C5 the
current grid). **Slice 0 is built** (ADR-0044): `crates/lumen-midi` (midir;
`MidiSystem` worker thread, fake backend), `app/midi.rs` and
`midi_settings.rs` (the MIDI tab of "Audio and MIDI settings", saved in
`settings.midi`), and **format v11** (`ClipContent::Midi { notes, events }`,
`ClipEvent`/`EventKind`; use `ClipContent::midi()` for an empty clip; edits
cut events like notes). E2E `midi`; `--midi off|test:fake`;
`Launch::with_midi(FakeControl)`. MT-MIDI-1 waits for the owner (needs a
MIDI device). **Slice 1 is built** (ADR-0045; owner's S1, E1, T1, H1): `lumen_core::midi`
(`Msg`), the new `crates/lumen-devices` (the `Instrument` trait and the
synth with five presets; the engine may depend on it: layering amended),
`lumen-engine/src/midi/` (`hold.rs` the note tracker, chase and every
release; `player.rs`; `Layered` plays it beside the clip player;
`Source::{stopped, panic, ringing}`, `RenderContext::{mode, stream}` and
`Mode::Ring`, which renders tails while stopped), **format v12** (the
synth is a built-in device named `synth` with its preset in `state`;
`Track::midi_to`), `Document::set_instrument` and `set_midi_to`,
`midi_play.rs` (project to engine, pure, with a port table), `midi_out.rs`
(hardware timing: `StreamClock`, `ClockMap`, `Scheduler`, the sender
thread), `app/hardware.rs`, the track menu's Instrument and MIDI To and the
palette's presets and Panic. E2E `midi_play` watches a fake port: every
note-on needs its off. `tests/projects/midi-demo.lumen` (from
`lumen_project::testing::midi_demo`; regenerate with
`LUMEN_WRITE_FIXTURES=1`) is for MT-MIDI-2. Perf scenario `engine_synth_64`
(0.36 ms a block, 13.5 %; baseline added by hand). Unverified: timing on
real MIDI hardware. Next: the owner's MT-MIDI-1 and MT-MIDI-2, then slice 2
(MIDI From, monitoring, the computer keyboard, the MIDI chip).

Before it, 2026-10-10: **M4.5, a polish pass, is approved and
under way** on `m4.5-polish` (plan `docs/milestones/M4.5.md`). The owner
merged the M4 review fixes (PR #3; CI did not run because of GitHub
billing, so the full suite was run locally first), then asked for an
interface audit (`docs/audits/2026-10-10-design.md`). Every proposal was
chosen from interactive mock-ups (an artifact; the owner-decisions table,
"Interface audit"); the slider handle is the owner's own variant (a solid
line that opens into ticks near its value). M4.5 runs before M5 so the
piano roll inherits the new widgets, and is recorded as a mid milestone
when finished. Six slices: bugs (value field fill, a `well` token, the
scroll clamp, the shots harness), shared widgets and sizes, chrome,
Martian Mono (ask before downloading), motion, close. **Slice 1 is
built** (T1 fields take `theme::controls`, the `well` token, the scroll
clamp in `rows.rs` `keep_in_content`, the shots harness); MT-POL-1 waits
for the owner (passed). **Slice 2 is built**: size tokens
(`size::CONTROL`, `CONTROL_SMALL`, `KNOB`, `KNOB_SMALL`, `LABEL_COLUMN*`,
`space::XXS`), the handle that opens near its value
(`widgets::draw_handle`), scale numbers thinned (`meter::place`), and
`widgets/toggle.rs` (`switch`, `segmented`, `choice_row`): use these,
never egui's checkbox or radio. MT-POL-2 passed (run 2; menus fade,
`widgets::Menu`, the owner's C). **Slice 3 is built**: `toggle_button`,
`tabs` (names end "tab"), the grey arm at rest, `widgets::empty_state`
and the mixer's ghost strip ("Add track in the mixer"), the clip card's
swatches and `lanes::shorten_middle`, `widgets::time_signature`,
`SliderKind::Percent`. MT-POL-3 passed. **Slice 4 is built**: Martian
Mono (downloaded with the owner's approval) is the monospace, at width
87.5 (`fonts.rs`). MT-POL-4 passed. **Slice 5 is built**: clip glides
and the undo wash (`arrangement/glide.rs`, `Document::session`), rows
that grow in, glide and fold shut (`arrangement/row_glide.rs`; `Row`
has `clipped`), `widgets::scroll_bar`, the palette's sliding highlight
(`motion::glide_over`), lit states cross-fading, `widgets::paint_gone`
for closing windows. E2E that acts right after adding, moving or folding
tracks must let rows settle (`frames(30)`); the harness clock moves 1/60 s
a frame. MT-POL-5 waits. **Slice 6 (close) is built**: E2E `access` and
the walkthroughs re-run; the design sweep (`shots::polish_sweep`, both
themes at 100/150/200%) found menu switches, segmented controls and
captions off the label line (they now inset themselves inside a menu,
`toggle::menu_inset`; captions are `widgets::menu_caption`) and the
audit's mixer edge fades unbuilt (built in `strips.rs`). The frame
benchmark counts idle from 1.5 s after the last driven frame (the scroll
bar's fade had read as 146 idle repaints). Frame times p99 1.5–1.7 ms at
1600 × 900, 3.3–3.6 ms at 3440 × 1380, idle 3 in 3 s. The M4.5 report is
in PROGRESS.md; MT-POL-6 waits. The pinned clip card covering windows
opened after it is in KNOWN_ISSUES, to pitch. **M5 is proposed** in
`docs/milestones/M5.md` with five owner choices (C1 what plays MIDI
before VST3, C2 where the piano roll opens, C3 the computer keyboard, C4
replace or overdub, C5 quantize defaults). Next: the owner's MT-POL-5,
MT-POL-6 and MT-APP-18, merge `m4.5-polish`, then M5 once approved. E2E must not flip process-wide
settings (theme, Reduce motion): tests run side by side.
`tests/shots.rs` renders mixed themes when run in parallel (the theme is
process-global): run it with `--test-threads=1`.
`origin/main` (PRs #4 to #6: CI fixes, macOS CI back, the light layer
and the mark) is merged into `m4.5-polish`; the empty arrangement's
empty state shows the mark, half risen, as its icon (`EmptyState::icon_size`).
Before it, 2026-10-10: **the light layer and the mark are built**
on `claude/keen-einstein-2fjiuy` (ADR-0043, DESIGN.md §6a and §6b; the
owner approved them on the identity canvas, a private Design artifact,
and chose the logo, Dawn in Sunrise). `light.rs` holds the five levels
(Unlit, Ember, `LIT`, `BEAM`, `FLARE`): **every glow is one of them,
never a one-off**; new tokens `core`, `sun`, `sun_core`. The playhead is a
beam while playing, Play lights, meters have heads, clip lights flare.
`widgets/mark.rs` draws the mark: a meter of the master in the transport
bar (red when recording arrives in M9), half risen over an empty
arrangement. `brand.rs` draws the window icon as pixels (installer icons
stay M9). E2E `light`, shots `light_layer`. MT-APP-18 waits for the owner.
A guitarist-first direction was pitched and set aside by the owner. Before it, 2026-10-10: **macOS CI is back** (owner) on
`claude/busy-pascal-pbswo9`: `macos-latest` (Apple silicon) is in the test
matrix again, and a new push to a pull request cancels its superseded run.
Its first run failed 20 E2E tests, both kinds fixed: chords are written
Cmd and Option on macOS (write E2E text with chords through `keys("Ctrl+D
…")` in `tests/e2e/main.rs`), and `Lumen::listen` settled for a wall-clock
time while the test device plays a block per 10 ms poll, so slow sleeps
measured levels mid-glide; it now counts the settle in frames played. CI
tests run with `--no-fail-fast`. CI is green on all three systems (run
38070627268). Before it, 2026-10-10: **CI fixes** on
`claude/gallant-newton-feg0a0`. `main` was red in Tests on both systems:
- Linux: the waveform atlas test ran on OpenGL (llvmpipe), which reads
  negative peaks back as 0. It now asks for Vulkan, DX12 or Metal, and the
  Linux job installs lavapipe (`mesa-vulkan-drivers`) so it runs;
- Windows: an E2E listener heard 768 frames in a fixed 150 ms. Every level
  check now uses `Lumen::listen(settle, frames)`, which waits for frames.
Both follow-ups are in KNOWN_ISSUES. Before it, 2026-10-10: **the M4
review's fixes are done on `m4-review`** (`docs/audits/2026-10-10-m4.md`, Status table). Changes:
- the five oversized app modules split: `arrangement/rows.rs`,
  `tracks.rs`, `header_row.rs`, `strip_output.rs`, `selection_look.rs`
  and `app/test_modes.rs`;
- `widgets::value_slider_styled` takes a `SliderStyle`;
- plan swaps take delay lines over (`Lines::take_over`), with the new perf
  scenario `engine_latent_swaps_64`;
- the access E2E covers M4's views;
- new tokens: `size::SCALE`, `size::READOUT`, `motion::BREATH_FRAME`;
- a file's size is read in the loader thread.

The WSL scripts test `origin/main` unless run with
`env LUMEN_REF=<branch>`. Next: merge `m4-review`, then propose M5 (MIDI
and the piano roll). Before it, 2026-10-10: **M4 is complete and merged
into `main`.** Every manual test passed (MT-APP-17 and MT-MIX-1 to 8,
owner), including the Focusrite's dropout count. The report is in
PROGRESS.md. Before it, 2026-10-10: **M4 slice 7 under way** (ADR-0042). The
exit walkthrough is E2E `walkthrough_mix` (drum group and vocal through a
return with the synthetic latent device; solo and mute heard at every
step; save and reopen) and MT-MIX-7. It found a bug, fixed: stopped, the
engine took no clip lists or routings (`Source::idle` only let meters
fall), so after four edits `lanes.dirty` stuck and Play started on stale
routings; `Mixer::idle` and `TrackSource::idle` now take them. Known
issues fixed: session locks (`<session>.lock`) so a second Lumen skips a
running session's autosave; `widgets::submenu` (names without the
arrow; use it for every submenu); `canvas::PipelineCache` reads and
writes the cache off the UI thread; the M3 walkthrough's Linux flake (the
Stop's block). E2E levels after a change: measure until two stretches
agree (`walkthrough_mix::hears`), never one fixed wait. Next: the owner's
manual tests (MT-APP-17, MT-MIX-1, 2, 5, 6, 7, and the Focusrite dropout
count, due before the review), then the `lumen-review` review and the M4
report. Before it, 2026-10-10: **M4 slice 6 built** (sidechains and delay compensation, ADR-0041; below). MT-MIX-6 waits for the owner. Before it, 2026-10-10: mixer fixes after MT-MIX-5 run 1, approved from mock-ups (ADR-0040 amendment): strips 96 pt wide, the header's value text clear of its level bars, whole-dB peak readouts from 10 dB up; MT-MIX-5 run 2 waits for the owner (header meter to re-check with audio). Before it, 2026-10-09: M4 slice 5 built (below). Before it, M3 slice 8 built (the exit flow, the
accessibility audit, the end-of-milestone review and every finding fixed
or recorded). MT-APP-16 passed and **M3 is approved** (2026-10-09). The M3
report is in PROGRESS.md and M4 is proposed in `docs/milestones/M4.md`.
M4 is approved (exclusive solo by default, −3 dB pan law). **Slice 0,
the UX pass on hints, is built** (2026-10-09, ADR-0034): the owner chose an
info line along the bottom of the window, with K5 keycaps, over chips by
the pointer. Hovers call `App::set_info` (`app/info.rs`); only live values
follow the pointer while dragging (DESIGN.md §2). MT-APP-17 waits for the
owner. **Slice 1, the mixer graph in the engine, is built** (2026-10-09,
ADR-0035): `lumen_engine::mixer` compiles a `Routing` (a node per track:
trim, polarity, mute, sends, volume, pan law, output) into an ordered plan
on the UI side; clips play into their track's bus; plans swap at a block
edge, levels glide over 20 ms, a new shape crossfades over 5 ms. The app
builds the routing in `routing.rs` and sends it after the clip list. 64
routed tracks cost about 1% of a 128-frame block. Owner choices: centre
−3 dB, balance panning, mute silences every send, post-fader sends after
the pan. MT-MIX-1 waits for the owner. The dropout readout became the
owner's A (a chip in words that fades; ADR-0020 update); the Focusrite's
count still needs testing (KNOWN_ISSUES). **Slice 2, routing in the model,
is built** (format v7, ADR-0036): `Project::master` (apart from
`tracks`), `pan_law`, and per track `output`, `sends`, `invert`,
`trim_db`; one rule in `lumen_project::routing` (destination, loops) for
validation, edits (`document/mixer.rs`, refused when they would feed back)
and the engine routing. `history::unchanged` destructures projects and
tracks: add every new persisted field there too (the compiler insists).
MT-MIX-2 waits for the owner. **Slice 3, group tracks, is built**
(format v8, ADR-0037; owner's H1 nested rails, L1 lane bars, deleting a
group deletes its tracks, a group's arm arms them): one order rule in
`lumen_project::groups` (a group's tracks follow it; edits set parents
then `normalize`), edits in `document/groups.rs` (`group_tracks`,
`ungroup`, `move_tracks(ids, Place)`), folding in
`Arrangement::collapsed` (rows skip hidden tracks; the GPU row table
keeps a slot per track index), the look in `arrangement/nesting.rs`, the
menus in `arrangement/menus.rs`. The perf baseline was updated (parse
+1.71% over slices 2 and 3; reason in ADR-0037). MT-MIX-3 run 1's
findings are fixed (ADR-0037 amendment): groups go three deep at most
(`groups::MAX_NESTING`, refusals via `Document::trial` and a note), a
resizable header column saved per project (format v9), and solo keeps a
soloed track's groups open (`document::audible`). Run 2 found a crash
deleting from a track's menu (header rows now skip tracks gone mid-frame)
and grouped tracks 3 dB quieter a level (every bus pans with the balance
law); both fixed. **MT-MIX-3 passed; slice 3 is done.** **Slice 4, mute, solo, returns,
master and cue, is built** (format v10, ADR-0038; the owner agreed 14
solo cases and chose D1–D7, A1, K1 comet knobs, hollow M, and O1 for the
cue outputs): the rules are one pure function, `lumen_project::solo`
(its tests are the cases); the engine gates a node's output apart from
its sends (wet-only returns) and has a pre-fader cue bus
(`Routing::cue`, channels 3/4: the stream opens the main pair then the
cue pair); rows carry a `Zone` and returns and the master are pinned at
the bottom (`arrangement/pinned.rs`; returns keep any list order);
`widgets/knob.rs` and `arrangement/sends.rs` are the comet sends (tracks
with sends unfold five steps); `app/cue.rs` lends the cue settings to the
headers. The owner's Focusrite has two outputs: cue is verified on the
four-output test device (`App::heard_all`). MT-MIX-4 runs 1–5 (ADR-0038
amendments): knobs step up on selected rows (C4); a "+N ▾" list of every
send (O1); an unfolded return lists its four loudest senders, then
"+N more" (R2, L4), silent ones leaving when let go (S1); at most 12
returns (`mixer::MAX_RETURNS`); a knob drag keeps its own travel in
widget memory (slow drags from −∞ were lost); a knob's live value is a
chip above it, none in lists; senders re-sort only when one joins or
leaves. The owner keeps the standard +6 dB of an empty return. **MT-MIX-4
passed; slice 4 is done** (2026-10-09). **Slice 5, the mixer panel and
meters, is built** (ADR-0040; chosen from interactive specimens, an
artifact: S1 console strips, O2 sends (two knobs then "+N"), M1 accent
meters with M2 track colours as a setting, L2 loudness column, H2 meters
inside the headers' volume sliders, and B1–B8 as recommended, recorded in
M4.md and the owner-decisions table). In parts: 5a meters in the engine
(`mixer/meter.rs`: a lock-free bank, peaks taken with swaps, 300 ms RMS,
clips; `lumen_core::loudness`: K-weighting, gated LUFS, 4x true peak;
`Routing::measure` names the master; `Source::idle` lets RMS fall while
stopped); 5b header meters (`meters.rs` ballistics, `arrangement/
levels.rs`; integrated loudness resets on Play from stop via
`Transport::played_from_stop`); 5c the Mixer tab (`arrangement/strips.rs`,
`strip.rs`, `buttons.rs` shared M/S; `widgets/fader.rs`, `meter.rs`;
strip controls are named "… in the mixer"; the mixer draws after the
arrangement for Tab order); strips drag to reorder (B8, `reorder.rs`
`in_mixer`) and the pan law menu; 5d the latching wheel (`widgets/
wheel.rs`: route first thing each frame, controls take notches; wheel
gestures stay one undo step); 5e output pairs (`Output::Device(p)` is the
device's pair; `routing::Outputs`; `Audio::want_pairs` reopens the stream
with the pairs the project uses). Found and fixed: saved panel sizes never
came back (egui sizes a panel by its contents; the detail panel now
claims its area and has a min and max). Frame times with the mixer on the
200-track stress project p99 2.4 to 3.2 ms (was 1.5 to 1.8), idle 3
repaints in 3 s as before; engine perf baseline updated for meters
(+21.8% routed). MT-MIX-5 waits for the owner. **Slice 6, sidechains and
delay compensation, is built** (ADR-0041; the owner chose read ahead,
the three taps, saving sidechains with the first device that uses
them (no format change now), and the D1 readout from mock-ups).
`lumen_engine::mixer` is now `spec.rs` (routing types: `Device`,
`SidechainSpec`, `Tap`), `compile.rs` (order, loops, compensation: every
early path and a late-waiting node's own input go through a line),
`plan.rs` (runs it; mute comes after devices and taps only where a node
has them, so plain routings cost what they did), `delay.rs` (lines
carried across plans by key). The engine renders ahead by
`Source::latency` (`Engine::ahead`); the playhead follows through
`playhead.rs` only while latent; latent jumps blend at the tracks' inputs
in `Mixer::locate`. A latency change while playing skips or repeats the
difference, without a step (the pitch said "no gap"; corrected in the
ADR). The only device is the synthetic delay: `testing::set_test_latency`,
`--test-latency <ms>` (first track), `routing::Devices` in `Lanes`.
Readout: `transport/compensation.rs`; the bar is drawn before the info
line, so E2E checks a bar hover by its "Info: …" node, not `info()`. Perf
gate: new `engine_latent_64`; keep the non-latent path free of per-block
work (the transport gate caught +3%). The E2E walkthrough is flaky on
Linux from before this slice (KNOWN_ISSUES). MT-MIX-6 waits for the
owner. Next: slice 7 (walkthrough, review and report). The owner asks for mock-ups before
any visual change, bug fixes included, and checks findings fast: run the
full suite before every push (a push once went out with two failing E2E
tests). The owner's drag-to-cut request is recorded for M7b with split
(the M3 plan, out of scope). Editing from scripts: write Python edits to
a file with raw strings (`r'''…'''`); inline heredocs broke on `\u{…}`
escapes and quotes several times.

**Status**
- M1 Foundations ✅ and M2 Audio Engine, Devices & Transport ✅ (approved 2026-10-07).
- **M4.5 Polish pass: built 2026-10-10; slices 1–4 done, slices 5 and 6
  built (MT-POL-5 and MT-POL-6 wait).** M5 under way: slices 0 to 6 built; MT-MIDI-7 waits for the owner, then slice 7 (`docs/milestones/M5.md`).
- **M4 Mixer, Routing & Groups: complete 2026-10-10**; the review's fixes
  are merged (PR #3).
- **M3 Arrangement & Project System: complete, approved 2026-10-09.** Slices 1–3 done:
  - performance gates;
  - design system, motion and typefaces;
  - project model, undo, save/open, autosave and recovery.
- **M3 slices 4 and 5 are done.**
  - Groundwork (format v3) and part 4a are built: track headers in the
    owner-chosen design, refined after MT-APP-9 run 1 (DESIGN §7a,
    ADR-0025):
    - type icons, sliders with values inside;
    - a record arm that glows and breathes;
    - 22 pt height steps, a glowing resize handle;
    - a "+ Add track ▾" menu button.
  - An architecture audit after 4a (`docs/audits/2026-10-07-architecture.md`)
    found 3 high, 4 medium and 4 low issues; all are fixed and pushed
    (`eea085a`). `main.rs` and `arrangement.rs` are now split into modules
    (see ARCHITECTURE.md), so 4b–4d land in clean files.
  - MT-APP-9 run 2 (owner) found four polish issues, all fixed (`a28ee60`):
    - one shared `widgets::Dialog` for every window;
    - pixel-snapped M, S and arm buttons;
    - typed tempo applies on Enter;
    - controls step up on selected rows.
  - Run 3 passed, with audio fixes found during it: no resent timeline
    on every edit, and clip levels that glide (ADR-0025).
  - **4b built** (ADR-0026): clip bodies on the GPU (a box layer with a
    per-frame row table) and a frame-time benchmark
    (`--stress --bench-frames`; DEVELOPMENT.md). MT-APP-10 passed.
  - **4c done** (ADR-0027): waveforms from peak pyramids built at load,
    drawn per pixel on the GPU; MT-APP-11 passed. The peak disk cache
    moved to M9 (owner approved).
  - **4d built** (ADR-0028): zoom to single samples (f64 view, split
    beats on the GPU, sample-level waveforms), the default keys (`+`/`-`, X,
    W, H, Alt+wheel), Follow (page by default, on), drag to reorder tracks.
    MT-APP-12 passed (run 2). **Slice 4 is done** (2026-10-08).
  - MT-APP-12 run 1: zoom-out now stops at the song and
    the grid covers any view (`28b40d4`).
  - **End-to-end tests** (ADR-0029, 2026-10-08): `crates/lumen-app/tests/e2e/`
    drives the whole app headless in CI; `tests/smoke.rs` runs the real
    binary at the end of every slice.
  - Shaders are `.wgsl` files checked at build time (`lumen-shader-check`).
  - Problems found and not fixed yet: `docs/KNOWN_ISSUES.md`.
  - **Slice 5 built** (ADR-0030, format v4): the editing grid and
    snapping (`lumen_core::grid`, Ctrl+1 to Ctrl+5, Alt bypasses);
    rulers in layout A (bar ruler on top, drag to zoom; a time ruler at
    the bottom in minutes and seconds, samples or timecode); the loop
    brace and punch as undoable document edits; named locators (M,
    Ctrl+Left/Right, Set ◂ ▸); the scrub strip (short loops of the
    grid step, Alt for tape), with an engine scrub mode.
    MT-APP-13 passed in run 2 (after drag hints, the snap magnet and
    locators added on the strip). **Slice 5 is done.**
  - **Slice 6 done** (clip editing; owner's split-clip drag model,
    S3 and H1 selection look, L3 handles, the clip overlap rule, F3 and
    G1 for fades and gain): 6a selection and moving, 6b time ranges and
    edits, 6c trim, loop-extend and slip, 6d fades and clip gain are
    built. Fades bend along free curves (the owner's C3; format v6,
    `lumen_core::curve`, ADR-0031), the model automation reuses in M7.
    6d passed the owner's test (MT-APP-14 run 1, 2026-10-08).
    **6e built** (ADR-0032; owner's M1 grey mute, K3 padlock and clamps,
    P1 pinned card): 0 mutes, the menu locks, Ctrl+R or F2 renames in the
    title bar, clip colours, the properties card (Alt+Enter;
    `app/clip_card.rs`), files dropped on a track or between tracks
    (`arrangement/drop.rs`). MT-APP-14 run 2 (owner) asked for three
    changes, all made: the right-click menu is for what was clicked (the
    owner's A, 11 rows on a clip instead of 33); the grid shows through
    clip bodies (G1, 55%); a click anywhere on the lanes moves the
    playhead. Run 3 passed except two findings, both
    fixed: the ✕ now sits level with the title's letters in every window
    (`widgets::title_row`), and drops between tracks work on Windows,
    where egui gets no pointer during a drag from Explorer (Lumen reads
    the system's cursor; ADR-0032). MT-APP-14 passed in run 4.
    **Slice 6 is done** (2026-10-08).
  - **Slice 7 done** (ADR-0033; owner's P2 palette with P3's values, K5
    area-light keycaps, E2 keyboard map with E3 rebinding in the palette,
    Shift+Tab by focus):
    - 7a: every key is a `Command` in one table (`commands/`: id, name,
      area, chords, about) run by one dispatcher (`app/commands.rs`);
      chords match modifiers exactly; a key acting on the selection is
      one command (Delete, Duplicate, Rename, Deselect);
    - 7b: the palette (Ctrl+K, the transport's magnifier;
      `app/palette.rs`);
    - 7c: the Keyboard shortcuts window and recording (`app/shortcuts.rs`),
      changed shortcuts and recent commands saved in the settings'
      `commands` section (an added field, no version change);
    - 7d: menus and tooltips from the keymap (`widgets::menu_item`,
      `menu_toggle`, `tip`), ruler and track-name menus, Z, Ctrl+A,
      Ctrl+L, Fit loop, F6 panes (`app/panes.rs`), Shift+Tab by focus.
    MT-APP-15 passed (2026-10-08).
  - **Slice 8 built** (2026-10-09): E2E `walkthrough` (the exit flow) and
    `access` (names and Tab order); the smooth playhead (ADR-0028
    amendment); the review (`docs/audits/2026-10-08-m3.md`) and its fixes
    (ADR-0033 amendment): Import audio… (Ctrl+Shift+I; tests answer file
    dialogs, `testing::answer_next_dialog`), the arrows move between clips
    when a clip is selected (the owner's choice), the time signature
    through `Document::set_time_signature` (clamped to 64 beats),
    `transport/`, `widgets/keys.rs` and `lumen-project` `edit/clipboard.rs`
    split out, files dropped together kept in order. 433 tests, 88 E2E,
    6 smoke tests pass; frame times in the M3 report. MT-APP-16 passed.
  - Esc after typing belongs to the text field (`App::typing`): egui
    drops a field's focus on Esc before the app's keys run, so key
    handlers must check `self.typing` as well as `text_edit_focused()`
    (ADR-0032). Never use `egui_wants_keyboard_input()` for that: it is
    true for any focused widget, and stopped every shortcut once a button
    was reached with Tab (ADR-0033).
  - New keys are commands: a row in `commands::ALL` (in its area's place)
    plus arms in `availability` and `run`; menus and tooltips get their
    keycaps from the keymap, never from written text.
  - Moved, trimmed or deleted clips crossfade over 5 ms while playing
    (owner: dragging a playing clip crackled); E2E scenarios can listen
    to the test device (`app.heard()`).
  - The plan and the follow-ups are in `docs/milestones/M3.md`.
- **Skills** (`.claude/skills/`), to load before working:
  - `lumen-branching` at the start of a session and before creating,
    renaming or pushing a branch;
  - `lumen-architecture` before writing or changing code;
  - `lumen-design` before any UI work;
  - `lumen-review` for the full repository review **at the end of every
    milestone** (owner decision), before the milestone report.
- Where to look:
  - `docs/PROGRESS.md`: every requirement's status, slice tables and the
    milestone reports;
  - `docs/DECISIONS.md`: ADR-0001 to ADR-0059, plus the table of owner
    decisions;
  - `docs/MANUAL_TESTS.md`: hardware tests with every run's results;
  - `docs/KNOWN_ISSUES.md`: problems found and not fixed yet, and where
    each is addressed;
  - `docs/ARCHITECTURE.md`: crates and threads;
  - `docs/DESIGN.md`: the design system;
  - `docs/DEVELOPMENT.md`: setup, test flags, WSL mirror, performance gate.

**How the owner works**
- Tests by hand on Windows with a Focusrite USB interface (ASIO and
  WASAPI). Give commands in PowerShell, starting Lumen with
  `cargo run --release -p lumen-app` from the repository root, never an
  `.exe` path.
- Checks CI on GitHub themselves: push, don't watch or poll it.
- **macOS CI is back** (owner, 2026-10-10; paused from 2026-10-07 when
  Actions minutes ran out). CI runs Windows, macOS and Linux. Still check
  macOS-specific code with the cross-target clippy in DEVELOPMENT.md
  before pushing.
- Wants variants pitched before a slice is built, for behaviour (what is
  heard, how controls act) as well as visuals, with a recommendation
  (owner, M4 slice 1).
- Judges visuals by eye, and wants personality rather than generic
  choices. Show visual options (typefaces, icons, colours) as rendered
  specimens before downloading assets or building. Ask before downloading
  files.
- Pause for approval at the end of every milestone. Each slice closes with
  a manual test (MT-*); the owner's findings are recorded as runs, fixed,
  and re-run.

**Engineering conventions learned so far**
- **One small branch per slice** (owner, 2026-10-15), named after the
  milestone and slice (`m6-s1-plugin-scan`); unrelated work and bug fixes go
  on one-off branches (`fix/…`, `chore/…`, `docs/…`) cut from `main`. A
  session that starts on a generated `claude/…` branch renames it first. The
  rules are in the `lumen-branching` skill.
- **Build location:** build in the repository's `target/` folder; never
  set `CARGO_TARGET_DIR`. A running `lumen.exe` blocks a rebuild: for
  screenshots, use the debug build (`target/debug/lumen.exe --screenshot`),
  never kill the owner's Lumen.
- **Before every commit:** `cargo fmt`, workspace clippy with
  `-D warnings`, and tests.
- **Pull requests never link the session:** the title, body, comments and
  commit messages of a PR contain no link to the Claude session (no
  `claude.ai/code/session_…` URL, no `Claude-Session:` trailer). The owner's
  instruction overrides the default attribution footer for these lines. Check
  a PR body for the URL before creating or editing it.
- **At the end of every slice** (owner rule, ADR-0029): the E2E suite
  (`cargo test -p lumen-app --test e2e`) and the real-binary smoke tests
  (`cargo test -p lumen-app --test smoke -- --ignored`) pass, and the
  slice adds E2E scenarios for what it delivered. Run Lumen by hand for
  screenshots only with `--audio test:fake --settings ... --autosave-dir
  ... --crash-dir ...`, never with the owner's settings, autosaves, crash
  reports or device. Crash reports (`crash.rs`) say what failed and where.
- **Before pushing platform-specific code:** run the WSL Linux mirror
  (`clippy.sh`, and `perf.sh` when engine or project code changes) and the
  macOS cross-target clippy. See DEVELOPMENT.md.
- **Performance gate:** fails CI at +2% instructions. An intended change
  updates `perf/baseline.json` from WSL (`perf.sh --update`), with the
  reason recorded.
- **Persisted changes:** every change to what is saved bumps the `.lumen`
  format version, with a migration and a golden fixture. Owner settings
  live in `%APPDATA%\Lumen\settings.json` (versioned).
- **Edits:** every edit is a named `Document` method that applies through
  the history (one undo step; drags merge into one step). `Document::apply`
  is test-only since the M3 review: UI code cannot edit in a closure.
- **Draw only what is in view.** Deep in, a clip can be hundreds of
  millions of points wide: anything sized by a clip's or range's width
  (dashed lines, repeats, labels) is cut to the visible lanes first. A
  dashed line across a long clip once asked for 64 GiB and crashed
  (owner, after slice 6c; E2E `hovering_a_long_clip_at_the_deepest_zoom_stays_quick`).
- **Design:** UI colours, sizes and durations come from `theme.rs`,
  `motion.rs` and `fonts.rs`, never inline. Windows draw their own title
  row; long text wraps or truncates and never widens a window.
- **Editing files from scripts:** rustfmt reflows code, so exact-match
  replacements often miss. Prefer the Edit tool, and write longer scripts
  to files rather than heredocs (Unicode and escapes break them).

**Open items outside the code**
- Owner actions:
  - sign Steinberg's ASIO SDK agreement before any distribution;
  - the Narrator accessibility check was deferred.
- If a push is rejected with "Internal Server Error", it is GitHub's side
  (seen on 2026-10-07 for about an hour). Retry later; the commits are safe
  locally.
