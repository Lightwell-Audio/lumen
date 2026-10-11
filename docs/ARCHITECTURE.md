# Lumen — Architecture

**As built:** M4 slice 1 (2026-10-09). The decision records in
`docs/DECISIONS.md` hold the reasoning and the evidence. This page shows
how the parts fit.

## Processes and threads

```text
┌──────────────────────────── lumen (main process) ─────────────────────────────┐
│                                                                               │
│  UI thread ── egui/eframe (wgpu) ── Document: History of shared snapshots     │
│     │  every edit is one undo step (ADR-0024)                                 │
│     │                                                                         │
│     ├── background threads: media load/convert · project save/open ·          │
│     │   autosave · file dialogs · ASIO driver checks (child process)          │
│     │                                                                         │
│     ├── MIDI worker (lumen-midi): lists and opens ports, hot-plug             │
│     └── MIDI sender (starts with the first track that names a port): times    │
│         the engine's hardware events against the audio clock (ADR-0045)       │
│     ▼  commands, timelines, clip lists, MIDI track lists (lock-free queues;   │
│        retired data comes back to be freed off the audio thread)              │
│  audio control thread (supervisor): open/close/reset devices, recovery,       │
│     run-time driver switching (hands the engine to a new thread)              │
│     ▼                                                                         │
│  audio driver thread (RT): ASIO / WASAPI / CoreAudio / ALSA callback          │
│     runs the engine directly, no extra buffering                              │
│     (M6: per block, signal plugin hosts and wait until the deadline)          │
└───────────────────────────────────────────────────────────────────────────────┘
   lumen --probe-asio <clsid>: short-lived helper that loads one ASIO driver
   (M6) lumen-plugin-host processes and scanning processes (prototyped in M1)
```

| Concern | Design | Decisions |
|---|---|---|
| UI | egui host for chrome, widgets, text and accessibility; `lumen-render` (wgpu + Vello) for dense content (the arrangement grid now, clips and waveforms from M3 slice 4) | ADR-0002 |
| Design system | Semantic colour tokens (dark and light), type, spacing and motion, in `theme.rs`, `motion.rs` and `fonts.rs`; spec in `docs/DESIGN.md` | ADR-0023 |
| Real-time engine | Allocation-free, block-size-invariant; transport, tempo and meter maps, declick crossfades, clip player, MIDI player and the built-in instruments, the mixer graph (a plan compiled on the UI side), metronome | ADR-0003, 0017, 0019, 0020, 0035, 0045 |
| Audio devices | Lumen's own backend layer: native ASIO over COM, WASAPI shared and exclusive, cpal for macOS and Linux; a supervisor that recovers from loss, resets and sleep | ADR-0006, 0016, 0021 |
| Projects | Folder with versioned JSON (format v7), migrations over untyped JSON, atomic saves, rolling backups, relative media paths | ADR-0004 |
| Undo | Shared snapshots: tracks and clips in `Arc`, every step a cheap copy | ADR-0024 |
| Time | Integer ticks at 1,411,200 PPQ, with one rounding rule to samples | ADR-0009 |
| Plugins (M6) | `vst3` crate bindings, sandboxed host processes, out-of-process scanning | ADR-0005 |
| Real-time safety | Allocation guard in tests; RealtimeSanitizer in Linux CI with live canaries | ADR-0010 |
| Performance | divan wall-clock benchmarks, plus instruction-count gates under Callgrind in CI (+2%) | ADR-0011, 0022 |

## Crates

| Crate | Role |
|---|---|
| `lumen-core` | Product identity (the name, defined once), time base, the MIDI message (`midi`: up to three bytes, and an event at a frame; shared by the engine, the devices and the app, ADR-0045), tempo and meter maps, the editing grid (`grid`: steps, snapping, lines), curves for fades and envelopes (`curve`), fades (`fade`), pan laws (`pan`) and loudness (`loudness`: K-weighting, 100 ms blocks, gated LUFS and 4× true peak, BS.1770-4, ADR-0040) |
| `lumen-rt` | Real-time safety tooling: allocation guard and `rt_section` (RTSan hooks) |
| `lumen-audio` | Device layer: `Backend` and `Stream` traits; ASIO, WASAPI and cpal backends; `AudioSystem` supervisor; `Unavailable` (a driver that could not start, with the reason); a fake backend for tests |
| `lumen-devices` | Built-in devices (ADR-0045): real-time-safe DSP on `lumen-core` only, which the engine runs and the app builds. The `Instrument` trait and the polyphonic synth (`synth.rs`: 16 voices, polyBLEP oscillators, a state-variable low-pass, ADSR, five presets); effects join in M8 |
| `lumen-engine` | Engine on the audio thread: transport (`snapshot.rs`: the transport state published to the UI as a seqlock over atomics), timeline, the mixer graph (`mixer/`: `spec.rs` the routing types (nodes, sends, sidechains and their taps, devices); `compile.rs` checks and orders a routing and works out delay compensation; `plan.rs` runs the compiled plan with a bus per node; `delay.rs` the delay lines that line paths up, ADR-0041; `smooth.rs`: gliding levels; `meter.rs`: every node's peak, RMS and clip in a lock-free bank, and the measured node's loudness and true peak, ADR-0040; `mod.rs`: plan swaps and crossfades, ADR-0035), clip player (plays each clip into its track's bus), MIDI (`midi/`, ADR-0045: `mod.rs` the clip, track and message types; `hold.rs` the note tracker, reading clips a block at a time, chase and releases; `player.rs` the track-list swap and the player that renders instruments into track buses and sends hardware events out; `live.rs` live input, `Listen` and the live note tracker, ADR-0046, with the piano roll's audition as its own source, ADR-0047; `Layered` plays it beside the clip player), metronome and test tone, declick, scrubbing (`scrub.rs`: loop and tape styles, while stopped), the playhead while rendering ahead of it (`playhead.rs`, ADR-0041); `testing.rs` (the routing tests, benchmarks and the performance gate share); `EngineHandle` and `MixerHandle` on the UI side |
| `lumen-midi` | MIDI device layer (ADR-0044): `Backend` trait, the midir backend (WinMM, CoreMIDI, ALSA), a fake for tests, and `MidiSystem`, a worker thread that lists ports, notices hot-plug, opens the ports the user switched on and carries timestamped messages in a queue; `activity.rs` counts messages per port for the lights, and a waker tells the UI (ADR-0046) |
| `lumen-media` | Audio files: decoding (symphonia), sample-rate conversion (rubato), waveform peaks (ADR-0027) |
| `lumen-project` | Document model, `.lumen` format and migrations, atomic save and backups, media locating, `History` (undo), `routing` (where a track's audio goes, and loops: one rule for validation, edits and the engine, ADR-0036), `groups` (a group's tracks follow it in the list: spans, nesting, order, ADR-0037), `solo` (who is heard: mute and solo across groups, returns and the master, the owner's rules, ADR-0038), `edit` (clip and time-range edits: cut, delete and insert time, trim, slip, and Ableton's overlap rule; `edit/clipboard.rs`: copy and paste) |
| `lumen-render` | Toolkit-independent renderer: GPU-resident instanced layers (rectangles; boxes with a per-frame row table, ADR-0026; waveforms drawn per pixel from a peak atlas, ADR-0027) plus Vello; GPU pipeline cache |
| `lumen-perf` | Performance-gate scenarios and the `perf-gate` runner |
| `lumen-shader-check` | Build-time shader checks: build scripts validate every `.wgsl` file with naga and generate each shader's source and uniform size |
| `lumen-app` | The `lumen` application (modules below) |

`lumen-app` modules:

| Area | Modules |
|---|---|
| Projects | `document/` (`mod.rs`: the open project, its history and track edits; `instrument.rs`: a MIDI track's instrument and MIDI output, ADR-0045; `clips.rs`: clip edits, fades, gain and clip properties; `mixer.rs`: outputs, sends, polarity, trim and the pan law; `groups.rs`: grouping, ungrouping and moving tracks between groups; `import.rs`: loaded files onto new tracks or the track they were dropped on), `files.rs` (project folders, save, open, recent), `autosave.rs` (autosave and recovery) |
| Arrangement | `arrangement/` (`mod.rs`: the view state, constants and colours; `rows.rs`: rows and zones, heights, scrolling, revealing a track; `tracks.rs`: track selection and the commands on selected tracks; `headers.rs`: the header column and the empty area; `header_row.rs`: one track's header; `menus.rs`: the track and add-track menus; `nesting.rs`: how groups look (indents and rails, lane bars, a group's arm state); `clips.rs`: lanes and clips; `selection_look.rs`: how the clip selection, hover, time range and marquee are drawn; `select.rs`: hit-testing and drag gestures; `handles.rs`: fade and gain handles, where they are and how they are drawn; `waves.rs`: which audio each clip's waveform shows; `samples.rs`: samples for deep zoom; `reorder.rs`: dragging tracks; `drop.rs`: where dropped files go, a track or between tracks, and how that shows; `pinned.rs`: returns and the master pinned at the bottom, the zones, divider and caption, the master's header and Solo/Cue switch; `sends.rs`: comet send knobs on headers; `buttons.rs`: M and S, shared by headers and strips; `levels.rs`: meters inside the headers' volume sliders (H2); `strips.rs`: the Mixer tab, its loudness column and menu; `strip.rs`: one channel strip and its sections; `strip_output.rs`: a strip's output button and menu, ADR-0040), `meters.rs` (meter ballistics and the master's loudness, moved on each frame), `timeview.rs` (the horizontal view, f64), `lanes.rs` (the clip library, the engine's clip list, and sending the routing after it), `routing.rs` (the engine's routing from the project and the cue: a node per track, a master, the cue bus, the device's output pairs where the stream opened them), `media.rs` (loading audio in the background), `ruler.rs`, `position.rs`, `canvas.rs` (GPU grid and clip bodies) |
| Transport and audio | `transport/` (`mod.rs`: what drives the engine: play, stop, locate, scrub, Follow, the timeline it plays; `bar.rs`: the bar's controls, tempo, time signature, position field, the loop and punch menu; `status.rs`: CPU, the dropout chip, the device readout and its panel, the settings window; `compensation.rs`: the delay compensation readout, the owner's D1; `dropouts.rs`: dropouts as episodes and a last-minute tally), `playhead.rs` (the smooth playhead drawn and followed while playing), `audio.rs` (device choice and status), `audio_settings.rs` (settings window), `probe.rs` (ASIO helper process) |
| Design system | `theme.rs`, `motion.rs`, `fonts.rs`, `panels.rs` (sliding panels), `icons.rs` (vector icons, track-type icons, close button), `widgets/` (`mod.rs`: dialogs and their title row, value slider, fold arrow, gradients, glow line; `keys.rs`: shortcuts as K5 keycaps, menu rows and tooltips drawn from the keymap; `knob.rs`: the comet send knob, ADR-0038; `fader.rs`: the channel fader; `meter.rs`: a strip's stereo meter and scale, a level's zones; `wheel.rs`: the latching wheel on value controls, ADR-0040; `toggle.rs`: the switch, the segmented control, choice rows and menu captions, M4.5; `menu.rs`: every menu, closing gently and fading, M4.5; `empty.rs`: empty states; `signature.rs`: the time signature field; `scroll_bar.rs`: the fading scroll bar; `fade_out.rs`: closed windows fading; `mark.rs`: the Dawn mark, a meter of the master, ADR-0043), `light.rs` (the five levels of light and their helpers, ADR-0043), `brand.rs` (the window icon drawn as pixels from the mark), `owner.rs` (dialogs owned by the main window; the system's cursor during a file drag) |
| App | A library with a thin binary (`main.rs` calls `lumen_app::run`), so tests in `tests/` start the real app. `lib.rs` (the `App` state, the frame loop and start-up, `App::new`); `launch.rs` (what start-up needs, from the command line or a test); `testing.rs` (the small public API the end-to-end tests use, ADR-0029; the tests themselves are in `tests/e2e/` and `tests/smoke.rs`); `app/` holds its behaviour by concern: `projects.rs` (new, open, save, the unsaved-changes prompt, Import audio, file dialogs, answered by tests instead of shown), `recovery.rs` (autosave and the recovery banner), `history.rs` (undo, redo, History window, toasts), `preferences.rs` (app menu, appearance, settings saves), `view.rs` (transport bar, arrangement composition, ruler, playhead, device banner), `navigate.rs` (zoom, fit, back, Follow), `grid.rs` (snapping, grid keys, menu and button), `rulers.rs` (the locator strip, bar ruler and time ruler, layout A), `regions.rs` (the loop brace and punch), `locators.rs` (locator flags, Set ◂ ▸, their keys), `scrub.rs` (the scrub strip), `editing.rs` (clip selection, the arrow keys between clips, drags and editing commands), `handles.rs` (dragging fade and gain handles), `clip_card.rs` (clip mute, lock, rename and colour, and the pinned properties card), `info.rs` (the info line: what is under the pointer and what can be done there), `cue.rs` (the cue as the engine plays it, the master's switch and cue level lent to the headers), `mixer.rs` (meters moved on each frame, and the device's output pairs opened and lent to the strips), `test_modes.rs` (the command-line test modes around each frame: save as, autoplay, trace, screenshot, smoke test), `clipboard_keys.rs` (puts back the Copy, Cut and Paste key presses egui-winit turns into clipboard events, outside text fields); `timefmt.rs` (time formats: minutes and seconds, samples, timecode) |
| Commands | `commands/` (`mod.rs`: every command once, with its id, name, area and default chords; `keys.rs`: chords matched exactly and the keymap, ADR-0033); `app/commands.rs` (whether a command can run now and why not, running it, and the one keyboard dispatcher); `app/palette.rs` (the command palette: rows, fuzzy search, typed values, context labels, drawing); `app/shortcuts.rs` (recording chords for the window and the palette, the Keyboard shortcuts window and its keyboard map); `app/panes.rs` (F6 between panes, their focus anchors) |
| MIDI | `midi.rs` (the device layer with the user's switches), `midi_settings.rs` (the MIDI page of the settings window), `midi_play.rs` (the engine's MIDI tracks from the project: notes, controllers, loops, instruments, targets, the port table; pure and tested), `midi_out.rs` (MIDI to hardware: the stream clock, the frame-to-time map, the scheduler and the sender thread), `app/hardware.rs` (choosing instruments, opening the outputs tracks name, the outputs and inputs the menus offer, handing the live feed to the engine, Panic), `live.rs` (the live feed on the audio thread: driver stamps and the computer keyboard to device frames, ADR-0046), `app/keyboard.rs` (the computer keyboard mode), `transport/midi_chip.rs` (the MIDI chip and its popover), `transport/fit.rs` (what the transport bar shows in the room it has), `arrangement/midi_io.rs` (MIDI From and Monitor menus, shared by the track menu and the strips), `arrangement/listen_light.rs` (header lights) |
| Clip editor | `note_editor/` (the piano roll's state, gestures, overlay, input, note commands and the pop-out window, ADR-0047), `piano_roll/` (notes and keys on the GPU: `notes.rs` the instance builder, `draw.rs` lanes, grid, notes and keys per surface, `mini.rs` and `arrangement/mini_notes.rs` the notes on arrangement clips), `audition.rs` (one previewed note at a time and every way of letting go), `app/detail.rs` (the detail panel's tabs), `note_editor/lane/` (the velocity and controller lane under the roll, ADR-0050: `geometry.rs` the lane's kinds, its value and x mapping and its hit tests; `gesture.rs` the pure arithmetic of Pencil, Line, Move, Box and Bend; `paint.rs` the stems, heads, traces, previews and value scale; `ui.rs` the header, the pointer and the commit; `mod.rs` the state and the cache), `document/lanes.rs` (the lane's undoable edits, one step each), `lumen_project::edit::{events, velocity}` (the pure transforms behind them) |
| Other | `settings.rs` (user settings file and its background writer), `trace.rs` (`LUMEN_TRACE` logging), `bench.rs` (the frame-time benchmark, `--bench-frames`) |

## Prototypes (M1)

| Prototype | Status |
|---|---|
| `engine-core` | Answered; rewritten as `lumen-engine` (M2) |
| `asio-probe` | Answered; became the ASIO backend in `lumen-audio` (M2) |
| `isolation-ipc`, `plugin-host` | Seed the plugin host process (M6) |
| `vst3-scan`, `vst3-test-module` | Seed the scanner and the misbehaving test plugin (M6); CI self-test on Windows |
| `ui-stress`, `render-shell` | Reference for dense rendering in the arrangement (M3 slice 4) |

## Rules every crate follows
- Layering: `lumen-core` at the bottom; `lumen-engine` depends only on `lumen-core`, `lumen-rt` and `lumen-devices`; `lumen-devices` only on `lumen-core`; `lumen-project` only on `lumen-core`; `lumen-audio`, `lumen-media`, `lumen-midi` and `lumen-render` stand alone; only `lumen-app` joins them. No cycles, and no UI types below the app.
- Audio-thread code never allocates, locks, blocks or panics. Tests enforce
  it: the allocation guard, plus RealtimeSanitizer in CI with canaries that
  prove the sanitizer is active.
- Third-party native code (plugins, drivers during probing) runs only where
  a crash cannot take Lumen down.
- Lumen never writes a file it cannot read back. Every persisted change
  bumps the format version, with a migration and a frozen fixture.
- Every edit is a named `Document` method that goes through the history's `apply`, and is one undo step (UI code never edits the project in a closure; `Document::apply` is for tests).
- Disk work runs off the UI thread: the interface and the audio never wait
  on files.
- Colours, sizes and durations come from the design-system modules, never
  inline.
- User-visible strings come from `lumen_core::product` where they name the
  product.
