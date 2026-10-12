# Build "Lumen": a production-grade DAW in Rust

> **How to use this file**
> - Save it in the repo root as `CLAUDE.md` (Claude Code loads it every session), or paste it as your first message.
> - This document defines **what** to build and **how good it must be**. You own the **how**: architecture, libraries, structure and algorithms are your decisions.
> - "Lumen" is the product name. Keep it defined in one place in the code and docs so it is easy to change if needed.
> - Tier legend: **[M]** must ship in 1.0 · **[S]** should ship in 1.0 · **[L]** later / post-1.0.

---

## 1. Mission

Build **Lumen**, a professional-grade desktop digital audio workstation written in **Rust**.

It combines **Ableton Live's arrangement-first workflow** with a **cleaner, more polished interface in the spirit of FL Studio**. There is no Session view and no clip launcher: the arrangement is the whole product. It must host **VST3** instruments and effects through a robust, Ableton-style device system, support **ASIO** with a proper device selector, work with **MIDI devices and audio instruments**, and provide a full MIDI editor with **quantize**, **Cubase-style drum maps**, **group tracks**, **automation and envelopes**, and the standard editing actions (split, consolidate, fades and more).

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

**Look and feel.** Minimalist and clean, dark by default with a light theme. Strong contrast on content (clips, notes, waveforms), quiet chrome, crisp vector icons, a single accent color, a muted track-color palette, restrained and purposeful motion. The clarity of FL Studio without its density. Before building the UI, produce a short **design system and key-flow spec** (layout, color, type, spacing, core widgets, the main workflows) and use it consistently everywhere.

**Interaction principles**
- Direct manipulation first: drag to move, trim edges, drag fade handles, draw notes and envelopes, double-click to open editors, right-click for context menus.
- A simple tool model (pointer, draw, erase, split, mute, zoom) with single-key switching and temporary modifier overrides.
- Identical snapping, selection, zoom and drag behavior in every editor.
- Immediate feedback: hover states, cursor changes, live value tooltips, ghost previews, visible snap guides.
- Designed empty, error and warning states (missing media with relink, plugin crashed with reload, audio device lost with recovery).
- Double-click resets a control to default; type-in values; fine adjustment with a modifier; mouse wheel adjustment; right-click a parameter for automation and MIDI mapping.
- Searchable **command palette** (Ctrl/Cmd+K) covering every command; tooltips show shortcuts; fully customizable shortcuts with conflict detection.

**Starting shortcuts** (all remappable; Ableton-compatible where sensible): Space play/stop · Ctrl/Cmd+Z / Shift+Z undo/redo · Ctrl/Cmd+D duplicate · Ctrl/Cmd+E split · Ctrl/Cmd+J consolidate · Ctrl/Cmd+G group / Shift ungroup · Q quantize, Shift+Q options · B draw mode · A automation view · Z / X zoom to selection / back · Ctrl/Cmd+L loop selection · Ctrl/Cmd+K command palette · Ctrl/Cmd+S save · Shift+Tab toggle detail panel · F9 record · 0 mute clip.

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

### 6.4 Drum maps (Cubase-style)
- A **drum map** assigns each sound: name, input note, output note, MIDI channel, output routing (the instrument or a specific instrument output), mute, solo, per-sound quantize, length override, color, and a choke group (for example open and closed hi-hat). [M]
- Clips store the performance in terms of the drum map's input notes, so attaching, swapping or editing a map **never destroys the notes**. A separate destructive "convert to another map" command exists. [M]
- Presets for General MIDI, GM2 and a common 808-style layout; user-created maps; import and export; the piano roll shows drum names instead of piano keys when a map is attached. Cubase drum map import [L]. [M]
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
- **Device chain** (Ableton-style) [M]: horizontal chain per track in the detail panel; each device is a collapsible card with power/bypass, name, preset menu and "open editor"; a compact parameter strip where the user chooses which parameters to show; drag to reorder; copy between tracks; instruments first, then MIDI and audio effects.
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
- [ ] [S] MIDI clock/MTC out · [L] clock/MTC in, Ableton Link

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
- [ ] [M] Shortcut editor with conflict detection and keymap presets (Ableton-compatible default) · [L] FL-style preset
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

**Post-1.0 candidates:** CLAP hosting, MPE, device racks with macros, Ableton Link, control-surface support (Mackie/HUI), spectral editing, advanced comping, MP3/AAC export.

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

**Last updated:** 2026-10-12: **the owner's Lumen design system is
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
Pencil on the grid; plan `docs/milestones/M5-slice4.md`). **No format change**
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
clip and the lane open are not measured here** (see below); MT-MIDI-5 waits.
Built by **Haiku subagents** under the new delegation policy
(`.claude/skills/references/delegation.md`: the parent specifies, integrates
and verifies; Haiku for bounded work), the parent wrote the UI wiring. In a
Linux cloud container, install `libasound2-dev libudev-dev pkg-config` to
build, `mesa-vulkan-drivers` for GPU tests and shots, and `xvfb`,
`libxkbcommon-x11-0` to run the smoke tests under `xvfb-run -a`. Next: slice 5
(quantize and its tools), specimens first; the owner's MT-MIDI-5 (and 4).

Before it, 2026-10-11: **M5 slice 3 (MIDI clips and the piano
roll) is built** on `m5-midi` (ADR-0047; the owner's Look A studio keys,
Draw A click-adds-then-drag-length, Live A preview-then-commit, chosen from
the "Lumen Piano Roll Specimens" artifact; plan `docs/milestones/M5-slice3.md`).
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
MT-MIDI-4 waits for the owner.

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
(Ubuntu 24.04 glibc); trust CI's 26.04 container. MT-MIDI-3 waits for the
owner, after MT-MIDI-1 and 2. Next: slice 3 (MIDI clips and the piano roll),
specimens first; the "MIDI Clips & Editing" canvas holds earlier
explorations for it.

Before it, 2026-10-10: **M5 (MIDI and the piano roll) is approved and
under way** on `m5-midi` (plan `docs/milestones/M5.md`; owner choices: C1 a
small built-in synth plus MIDI out, C2 the detail panel's Clip tab *and* a
way to pop the piano roll out into its own window, C3 Ableton's keyboard
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
M4 is approved (Ableton's solo by default, −3 dB pan law). **Slice 0,
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
Ableton's three taps, saving sidechains with the first device that uses
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
tests). The owner's drag-to-cut request is recorded for M7 with split
(the M3 plan, out of scope). Editing from scripts: write Python edits to
a file with raw strings (`r'''…'''`); inline heredocs broke on `\u{…}`
escapes and quotes several times.

**Status**
- M1 Foundations ✅ and M2 Audio Engine, Devices & Transport ✅ (approved 2026-10-07).
- **M4.5 Polish pass: built 2026-10-10; slices 1–4 done, slices 5 and 6
  built (MT-POL-5 and MT-POL-6 wait).** M5 under way: slices 0 to 4 built (`docs/milestones/M5.md`).
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
    beats on the GPU, sample-level waveforms), Ableton keys (`+`/`-`, X,
    W, H, Alt+wheel), Follow (page by default, on), drag to reorder tracks.
    MT-APP-12 passed (run 2). **Slice 4 is done** (2026-10-08).
  - MT-APP-12 run 1: zoom-out now stops at the song (as in Ableton) and
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
    Ctrl+Left/Right, Set ◂ ▸); the scrub strip (Ableton-style loops of the
    grid step, Alt for tape), with an engine scrub mode.
    MT-APP-13 passed in run 2 (after drag hints, the snap magnet and
    locators added on the strip). **Slice 5 is done.**
  - **Slice 6 done** (clip editing; owner's split-clip drag model,
    S3 and H1 selection look, L3 handles, Ableton's overlap rule, F3 and
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
    playhead, as in Ableton. Run 3 passed except two findings, both
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
  - `lumen-architecture` before writing or changing code;
  - `lumen-design` before any UI work;
  - `lumen-review` for the full repository review **at the end of every
    milestone** (owner decision), before the milestone report.
- Where to look:
  - `docs/PROGRESS.md`: every requirement's status, slice tables and the
    milestone reports;
  - `docs/DECISIONS.md`: ADR-0001 to ADR-0033, plus the table of owner
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
