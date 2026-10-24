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
it first in a new session, and update it at the end of every slice. Keep it
short: the history of every slice is in `docs/HANDOFF_ARCHIVE.md`
(CLAUDE.md as it was on 2026-10-23), the slice reports in `docs/PROGRESS.md`
and the reasoning in `docs/DECISIONS.md`. Add one short paragraph to "Now"
per slice and move what is no longer current out of it.

**Now (2026-10-24)**
- **Branch `m7b-s3-render-consolidate`: M7b E3 (render core and consolidate)
  is built** (ADR-0095; **no format change**, still v28; not pushed). New:
  `lumen_media::wav` (the WAV writer), `lumen_engine::render_clips` (the
  device-free clip render: the live `ClipPlayer` run block by block, with
  bare outer ends so a baked clip is not faded twice),
  `lumen_project::edit::consolidate` (the planner, the swap and the MIDI
  merge), `lumen-app::consolidate` (the worker), `app/clip_consolidate.rs`
  (the command, progress and Esc in the toast), `Document::consolidate_clips`
  (one undo step for audio and MIDI together). Command `clip.consolidate`
  (last in `Command` and `ALL`, no default key), the clip menu's Edit row,
  and Ctrl+J now consolidates audio that does not join seamlessly. Rendered
  files are 32-bit float WAVs at the **device rate** (the media library holds
  audio at that rate), written to a `Renders` folder beside the autosaves and
  copied into `Samples/Rendered` on Save. Gates and what was not run: see
  PROGRESS. **Waiting for the owner: MT-EDIT-4** (`docs/manual-tests/M7b.md`).
- **M7b E2 (crossfades, ADR-0094, format v28) is merged (PR #54;
  MT-EDIT-3 passed).** A crossfade is a true overlap: drag the left clip's
  fade-out handle over the right clip.
- **M7b E1 is merged (PR #52)** (ADR-0093; format **v27**): join, crop,
  unloop, nudge, reverse, polarity, normalize and the select variants.
- **Next:** E4 (bounce; it starts with the R2 plugin-offline spike), E5
  (freeze), E6 (close M7b). Plan, owner choices and the slice list:
  `docs/milestones/M7b.md`.
- **Done:** M1 to M4.5 approved; M5 (MIDI), M6 (VST3 hosting) and M7
  (automation and envelopes) are built, with reports in PROGRESS. M7b E0
  (tools and split, format v26) is merged.
- **Open with the owner:** the manual tests of M5 and M6 that are not signed
  off (MT-MIDI-7 to 9, MT-VST-3 to 7 and the ADR-0069 callback numbers) and
  the M7 approval. Problems found and not fixed are in `docs/KNOWN_ISSUES.md`
  (check it before working in an area).
- **Later milestones:** M8 (built-in devices) and M9 (audio input and recording, export, single instance, the peak disk cache) per `docs/PROGRESS.md`; M7b closes with E6.

**Where to look**
- `docs/PROGRESS.md`: every requirement's status, slice tables and reports.
- `docs/DECISIONS.md`: ADR-0001 to ADR-0093 and the table of owner decisions.
- `docs/MANUAL_TESTS.md` and `docs/manual-tests/`: hardware and by-hand
  tests with every run's results.
- `docs/KNOWN_ISSUES.md`, `docs/ARCHITECTURE.md` (crates, threads, module
  tables), `docs/DESIGN.md` (the design system), `docs/DEVELOPMENT.md`
  (setup, flags, WSL mirror, performance gate), `docs/milestones/` (plans).

**Skills** (`.claude/skills/`), to load before working:
- `lumen-branching` at the start of a session and before creating, renaming
  or pushing a branch;
- `lumen-architecture` before writing or changing code;
- `lumen-design` before any UI work;
- `lumen-review` for the full repository review **at the end of every
  milestone** (owner decision), before the milestone report.
- Bounded work may be delegated under `.claude/skills/references/delegation.md`
  (the parent specifies, integrates and verifies).

**Gotchas that cost time before** (each is enforced by a test or a compiler
error where it can be)
- **Commands:** a new command goes at the END of `Command` and `commands::ALL`
  (`ALL[command as usize]`; the order test's last variant must move), with
  arms in `availability` and `run`. Menus and tooltips take keycaps from the
  keymap, never from written text. Every menu row is a `menu_item` or
  `menu_toggle`; use `widgets::submenu` for submenus.
- **Model fields:** a new persisted field bumps the format (migration plus a
  frozen golden fixture) and goes in `history::unchanged` (the compiler
  insists). A new `Clip` field must be added to every `Clip` literal.
- **Esc and typing:** key handlers must check `App::typing` as well as
  `text_edit_focused()`; never use `egui_wants_keyboard_input()` for that.
- **Draw only what is in view** (see below), and never size anything by a
  clip's width.
- **Notifications are read in full** (owner rule, 2026-10-23): a toast,
  notice, banner or error message wraps onto more lines and never ends in
  "…". Only labels (names, paths, values) may truncate. See DESIGN.md §9.
- **Menus list what can apply:** a command that cannot act on what was
  clicked is left out of its context menu rather than greyed (audio-only
  clip commands are not listed for a MIDI-only selection).
- **E2E rules:** wait for what the engine answers (`Lumen::wait_for`,
  `listen`), never a count of frames; write chords through `keys()`; let
  rows settle (`frames(30)`) after adding, moving or folding tracks; never
  flip process-wide settings (theme, Reduce motion). Timing-sensitive
  scenarios can fail under a loaded full run: rerun alone before chasing.
  A macOS-only `walkthrough_plugins` flake is known and ignored (owner).
- **Shots:** `$env:SHOTS = "<existing folder>"; cargo test -p lumen-app --test
  shots <name> -- --ignored --test-threads=1` (the theme is process-wide).
  Look at them in both themes at 100, 150 and 200%.
- **Plugin host:** `cargo build --release -p lumen-plugin-host` before
  `cargo run` (scanning and hosting need it beside the executable); close
  Lumen and rebuild it after a protocol version change.
- **Performance gate:** the WSL scripts test the pushed branch
  (`env LUMEN_REF=<branch>`); `perf.sh --update` writes the baseline inside
  WSL, so copy `perf/baseline.json` back and record the reason in the ADR.
- **Cloud container:** `apt-get update`, then `libasound2-dev libudev-dev
  pkg-config mesa-vulkan-drivers xvfb libxkbcommon-x11-0`; smoke tests run
  under `xvfb-run -a`.
- **Disk:** the D: drive fills (LNK1318 means full); `target/debug/incremental`
  is safe to delete, and `CARGO_INCREMENTAL=0` helps.
- **Shell tooling:** in the bash tool a heredoc with apostrophes or
  backslashes can fail to parse and `python` may wait on stdin; write files
  with the Write tool, and small edits with the Edit tool.
- **Naming:** the owner's product-neutral rule (top of this file) is enforced
  by `tests/third_party_names.rs` and `tests/product_name.rs`; anonymous
  labels (RP-n) stand for real plugins in tests and notes.

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
