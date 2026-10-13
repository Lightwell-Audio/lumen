# Lumen — 10-Step Implementation Roadmap

## How to use this roadmap

Each milestone must end with:

- A working, runnable build
- Automated tests for the new functionality
- Manual verification where hardware or third-party plugins are involved
- `docs/PROGRESS.md` updated
- `docs/DECISIONS.md` updated when architectural decisions change
- A git commit representing a known-good state
- A short milestone report listing what shipped, what was deferred, measurements, and risks

Do not begin the next milestone until the current milestone is stable.

---

## 1. Foundations & Architecture

**Goal:** Remove the highest-risk architectural uncertainty before building the DAW.

Build the repository, CI, test infrastructure and benchmark framework.

Prototype and settle:

- UI framework and rendering architecture
- Real-time audio engine architecture
- Windows ASIO strategy
- Cross-platform audio abstraction
- Versioned project format
- VST3 hosting architecture
- Plugin process-isolation strategy

Create:

- `docs/PROGRESS.md`
- `docs/DECISIONS.md`
- `docs/ROADMAP.md`
- Architecture documentation
- Initial benchmark and test harnesses

### Exit criteria

A minimal application launches, the chosen UI approach is validated, the audio architecture is proven with a prototype, the project format can save/load a minimal project, and the VST3/isolation approach has been demonstrated.

**Do not build the full DAW in this milestone.**

---

## 2. Audio Engine, Devices & Transport

**Goal:** Make Lumen reliably play and record audio.

Implement:

- Audio engine and real-time processing graph
- ASIO device selection on Windows
- WASAPI/CoreAudio/Linux backend abstraction
- Sample rate and buffer configuration
- Input/output channel selection
- Test tone
- CPU and dropout monitoring
- Audio on/off
- Panic handling
- Device disconnect/recovery
- Transport
- Tempo and time signature
- Metronome
- Looping
- Punch in/out
- Position display

Build the simplest timeline capable of playing audio.

### Exit criteria

Lumen can open an audio device, play audio at low latency, stop safely, recover from device changes, and maintain stable transport behavior.

---

## 3. Arrangement & Project System

**Goal:** Turn the audio engine into an actual DAW workflow.

Implement:

- Arrangement timeline
- Track model
- Audio/MIDI/group/return/master track types
- Track headers
- Track selection and multi-selection
- Clip model
- Clip rendering
- Waveforms
- Grid and snapping
- Horizontal/vertical zoom
- Scrolling and panning
- Markers and locators
- Loop regions
- Clip move/copy/trim
- Clip gain
- Fades
- Mute
- Rename/recolor
- Time-range selection
- Undo/redo
- Command palette
- Core keyboard shortcuts
- **Motion system** (owner request, 2026-10-06: fluid, Apple-like feel),
  part of the design system:
  - motion tokens (a few durations and spring-like easing curves);
  - rules for what animates (panels opening and closing, hover and press
    states, popovers, the playhead easing on jumps, clips settling after a
    drag, zoom and scroll easing);
  - a "reduce motion" setting.

  Hard rules: animation never delays input, and nothing redraws when nothing
  moves (near-zero idle CPU). **Retrofit the M2 UI with it:** the transport
  bar, the audio panel and settings window, clip lanes, the start marker and
  playhead.

Implement project saving/loading alongside the arrangement rather than postponing it.

### Exit criteria

A user can:

`Create → add track → import audio → arrange clips → edit → save → close → reopen`

and receive the same project state.

---

## 4. Mixer, Routing & Groups

**Goal:** Make the arrangement capable of real production workflows.

Implement:

- Channel strips
- Faders
- Pan
- Mute/solo
- Input/output routing
- Sends
- Return tracks
- Master track
- Sidechain routing
- Group tracks
- Nested groups
- Inline meters
- Mixer panel
- Cue/preview routing
- Automatic plugin delay compensation foundation

Add the routing graph as a proper part of the engine rather than UI-only behavior.

### Exit criteria

A multi-track project can be routed through groups, returns and the master bus with predictable solo/mute behavior.

---

## 5. MIDI & Piano Roll

**Goal:** Make Lumen a usable MIDI production environment.

Implement:

- MIDI device discovery
- MIDI input/output
- MIDI recording
- MIDI thru
- Monitoring
- Sustain
- Pitch bend
- Aftertouch
- Program/bank change
- Computer keyboard MIDI input
- Panic behavior
- MIDI clip model
- Piano roll
- Note drawing/editing
- Note selection
- Velocity lane
- Controller lanes
- Note audition
- Quantize
- Humanize
- Legato
- Fixed length
- Transpose
- Standard MIDI File import/export
- MIDI clock out

### Exit criteria

A user can record a MIDI performance, edit it in the piano roll, quantize it, play it back and save/reopen it without changing its musical content.

---

## 6. VST3 Hosting & Device System

**Goal:** Make Lumen a real plugin-hosting DAW.

Implement:

- VST3 scanning
- Async/cached scanning
- Plugin paths
- Blocklist
- Plugin search
- Plugin loading
- Instruments
- Effects
- Plugin editors
- Generic parameter UI
- Plugin state save/restore
- Presets
- Parameter automation interface
- Sidechain
- Multi-output instruments
- Plugin latency reporting
- Plugin tails
- Bypass
- Floating plugin windows
- High-DPI handling

Then implement the **separate-process plugin host** and crash recovery.

### Exit criteria

Lumen can scan, load, play and save real third-party VST3 plugins while surviving a deliberately crashing test plugin.

---

## 7. Automation, Envelopes & Advanced Editing

**Goal:** Give users professional DAW editing and automation workflows.

Implement:

- Track automation
- Plugin automation
- Device automation
- Send automation
- Tempo automation
- Time-signature automation
- Read/Write/Touch/Latch
- Automation recording
- Automation override/re-enable
- MIDI learn with a mapping list (moved from M5, owner, 2026-10-13)
- Clip envelopes
- Point editing
- Curves
- Shape tools
- Freehand automation
- Point simplification

Then implement:

- Split
- Join
- Consolidate
- Ripple delete
- Insert silence
- Duplicate ranges
- Slip editing
- Crop
- Reverse
- Normalize
- Bounce in place
- Render selection
- Freeze/unfreeze/flatten
- Crossfades

Every operation should be undoable and testable.

### Exit criteria

The core editing operations behave predictably and survive save/load and undo/redo.

---

## 8. Drum Maps, Drum Editor & Built-In Devices

**Goal:** Complete the specialized MIDI workflow and provide enough built-in instruments/effects to produce without third-party plugins.

Implement:

### Drum system

- Cubase-style drum maps
- Drum map presets
- User-created maps
- Input/output notes
- Choke groups
- Per-sound routing
- Per-sound quantize/length
- Drum editor
- Per-hit velocity
- Lane mute/solo
- Lane reordering
- Step sequencer
- Clip content faces in the arrangement (owner, 2026-10-11): drum rows named by the map, chord names from chord detection, bass and melody drawn as a contour

### Built-in devices

- Utility
- Parametric EQ
- Compressor
- Limiter
- Delay
- Reverb
- Polyphonic synth
- Sampler

### Exit criteria

A complete drum workflow works end to end:

`Drum map → MIDI clip → drum editor/step sequencer → instrument → mixer`

and built-in devices can be automated and saved inside projects.

---

## 9. Recording, Media, Export & Production Workflow

**Goal:** Make Lumen practical for complete projects.

Implement:

- Crash-safe audio recording
- **Disk streaming for clip playback** (owner decision, 2026-10-06): a read-ahead thread feeds the audio thread from disk, prefetching around loops and locates, so large sessions do not have to fit in RAM. Compressed files are decoded once into an uncompressed cache. M2 keeps clip audio in memory; that path becomes the per-clip RAM option. Built together with the recording writer, which needs the same disk machinery.
- **Audio input for WASAPI, CoreAudio and Linux** (owner decision, 2026-10-06): a capture stream behind the existing device interface. On WASAPI, input and output are usually separate devices with their own clocks, so a drift corrector (rubato's `Slip`) keeps them in step. Tested end to end with recording, monitoring and input meters. ASIO input works since M2.
- Multi-track recording
- Monitoring
- Input meters
- Recording latency compensation
- Manual recording offset
- MIDI recording polish
- Autosave, crash recovery and rolling backups: done early, in M3 slice 3
- **App icon and project file icon** (owner request, 2026-10-07):
  - Lumen's icon, and an icon for `.lumen` files in Explorer and file
    dialogs, drawn from the design system;
  - candidates shown to the owner as rendered specimens before one is
    built;
  - the `.lumen` file type registered per user (no administrator rights)
    so projects show their icon and open by double-click; the installer
    registers it system-wide in M10.
  - Project folders keep the standard folder icon. Custom folder icons
    need hidden `desktop.ini` files that sync tools and other machines
    handle poorly; the project file inside carries the identity instead.
- Collect and save
- Media pool
- Missing-media relinking
- Project templates
- Recent projects: done early, in M3 slice 3
- Revert
- Project settings
- Export master
- Export stems
- WAV
- FLAC
- Bit-depth selection
- Sample-rate conversion
- Dither
- Normalize
- Effect tails
- Offline rendering
- Real-time rendering
- Loudness report
- Preferences
- First-run audio setup

### Exit criteria

A complete song can be created, recorded, saved, recovered, reopened and exported without manual intervention outside Lumen.

---

## 10. Hardening, Performance & Release

**Goal:** Turn the working DAW into a production-grade 1.0 release.

### Reliability

- Long-session soak tests
- Crash recovery
- Plugin crash/hang testing
- Device hot-unplug testing
- Sleep/resume
- Sample-rate changes
- Buffer changes
- Safe mode
- Atomic saves
- Recovery testing

### Performance

Measure and optimize:

- Audio callback budget
- UI frame rate
- Project loading
- Startup time
- Plugin scanning
- Arrangement scrolling
- Zooming
- Large MIDI projects
- Large clip counts
- Memory usage
- Idle CPU

Validate against the project's defined quality bar.

### UX

Complete:

- Accessibility
- High-DPI
- 100/150/200% scaling
- Light theme
- Dark theme
- Keyboard navigation
- Command palette coverage
- Context menus
- Tooltips
- Empty/error states
- Layout persistence
- **Motion polish pass** (owner request): tune durations and easing across
  the whole app once every view exists; check reduce-motion everywhere

### Release

- Windows installer
- Portable build
- Third-party license listing
- VST/ASIO notices
- Documentation
- Manual test suite
- Final compatibility testing

### Exit criteria

The mandatory `[M]` requirements are complete or explicitly approved for deferral, the scripted end-to-end workflow succeeds, and measured results are recorded.

---

## Stretch goals (unscheduled)

**Goal:** Worthwhile work that is not on the path to 1.0. Items are pulled
into a numbered milestone only with the owner's approval.

### VST3 scanning and hosting on macOS and Linux
Added by the owner on 2026-10-06. The M1 scanner and host prototypes are
Windows-only, so the CI isolation self-test runs on Windows alone.

- Module loading: macOS bundles (`CFBundle`, `bundleEntry`/`bundleExit`);
  Linux shared objects (`dlopen`, `ModuleEntry`/`ModuleExit`)
- Discovery: `Contents/MacOS/` and `Contents/<arch>-linux/`; the standard
  search paths (`/Library/Audio/Plug-Ins/VST3`, `~/Library/Audio/Plug-Ins/VST3`,
  `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3`)
- Misbehaving test module as a macOS bundle and a Linux `.so`; crashes
  classified by signal (SIGSEGV, SIGBUS) instead of Windows exit codes
- Isolation transport on POSIX: `shm_open` shared memory; futex (Linux) or
  Mach semaphores / `os_sync_wait_on_address` (macOS) for signalling; process
  groups instead of Job objects
- macOS specifics: universal binaries (arm64 + x86_64), code signing,
  notarization and Gatekeeper quarantine of plugin bundles
- The scanner isolation self-test enabled in CI on all three platforms

### Exit criteria
The same scan, isolate and crash-recovery checks that pass on Windows pass on
macOS and Linux, in CI and on real hardware.

---

# Development Rule

Inside every milestone, work in small vertical slices.

For example, Milestone 3 should NOT be one task called "Build the arrangement."

Instead:

1. Track model
2. Track creation
3. Track deletion
4. Track selection
5. Track headers
6. Clip model
7. Clip rendering
8. Waveform display
9. Clip selection
10. Clip movement
11. Snapping
12. Trim
13. Fades
14. Time-range selection
15. Undo/redo
16. Save/load integration

Complete one slice, test it, commit it, then continue.

# Milestone Gate

At the end of each milestone report:

- What shipped
- What tests passed
- What was manually tested
- Performance measurements
- Known bugs
- Deferred work
- Architectural decisions
- Risks for the next milestone

Then stop and wait for approval.

# Priority

When choosing between adding another feature and strengthening an existing feature, use:

1. Audio stability
2. Data integrity / never losing work
3. UX responsiveness
4. Correct behavior
5. Performance
6. Feature breadth
