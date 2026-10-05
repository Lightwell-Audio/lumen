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
- MIDI learn

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
- Multi-track recording
- Monitoring
- Input meters
- Recording latency compensation
- Manual recording offset
- MIDI recording polish
- Autosave
- Crash recovery
- Rolling backups
- Collect and save
- Media pool
- Missing-media relinking
- Project templates
- Recent projects
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
