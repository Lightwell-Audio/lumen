# Lumen — Architecture

**As built:** M3 slice 6e (2026-10-08). The decision records in
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
│     ▼  commands, timelines, clip lists (lock-free queues; retired data        │
│        comes back to be freed off the audio thread)                           │
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
| Real-time engine | Allocation-free, block-size-invariant; transport, tempo and meter maps, declick crossfades, clip player, metronome | ADR-0003, 0017, 0019, 0020 |
| Audio devices | Lumen's own backend layer: native ASIO over COM, WASAPI shared and exclusive, cpal for macOS and Linux; a supervisor that recovers from loss, resets and sleep | ADR-0006, 0016, 0021 |
| Projects | Folder with versioned JSON (format v6), migrations over untyped JSON, atomic saves, rolling backups, relative media paths | ADR-0004 |
| Undo | Shared snapshots: tracks and clips in `Arc`, every step a cheap copy | ADR-0024 |
| Time | Integer ticks at 1,411,200 PPQ, with one rounding rule to samples | ADR-0009 |
| Plugins (M6) | `vst3` crate bindings, sandboxed host processes, out-of-process scanning | ADR-0005 |
| Real-time safety | Allocation guard in tests; RealtimeSanitizer in Linux CI with live canaries | ADR-0010 |
| Performance | divan wall-clock benchmarks, plus instruction-count gates under Callgrind in CI (+2%) | ADR-0011, 0022 |

## Crates

| Crate | Role |
|---|---|
| `lumen-core` | Product identity (the name, defined once), time base, tempo and meter maps, the editing grid (`grid`: steps, snapping, lines), curves for fades and envelopes (`curve`) and fades (`fade`) |
| `lumen-rt` | Real-time safety tooling: allocation guard and `rt_section` (RTSan hooks) |
| `lumen-audio` | Device layer: `Backend` and `Stream` traits; ASIO, WASAPI and cpal backends; `AudioSystem` supervisor; `Unavailable` (a driver that could not start, with the reason); a fake backend for tests |
| `lumen-engine` | Engine on the audio thread: transport, timeline, clip player, metronome and test tone, declick, scrubbing (`scrub.rs`: loop and tape styles, while stopped); `EngineHandle` on the UI side |
| `lumen-media` | Audio files: decoding (symphonia), sample-rate conversion (rubato), waveform peaks (ADR-0027) |
| `lumen-project` | Document model, `.lumen` format and migrations, atomic save and backups, media locating, `History` (undo), `edit` (clip and time-range edits: cut, paste, delete and insert time, and Ableton's overlap rule) |
| `lumen-render` | Toolkit-independent renderer: GPU-resident instanced layers (rectangles; boxes with a per-frame row table, ADR-0026; waveforms drawn per pixel from a peak atlas, ADR-0027) plus Vello; GPU pipeline cache |
| `lumen-perf` | Performance-gate scenarios and the `perf-gate` runner |
| `lumen-shader-check` | Build-time shader checks: build scripts validate every `.wgsl` file with naga and generate each shader's source and uniform size |
| `lumen-app` | The `lumen` application (modules below) |

`lumen-app` modules:

| Area | Modules |
|---|---|
| Projects | `document/` (`mod.rs`: the open project, its history and track edits; `clips.rs`: clip edits, fades, gain and clip properties; `import.rs`: loaded files onto new tracks or the track they were dropped on), `files.rs` (project folders, save, open, recent), `autosave.rs` (autosave and recovery) |
| Arrangement | `arrangement/` (`mod.rs`: rows, heights, folding, selection, keyboard; `headers.rs`: track headers and menus; `clips.rs`: lanes and clips; `select.rs`: hit-testing and drag gestures; `handles.rs`: fade and gain handles, where they are and how they are drawn; `waves.rs`: which audio each clip's waveform shows; `samples.rs`: samples for deep zoom; `reorder.rs`: dragging tracks; `drop.rs`: where dropped files go, a track or between tracks, and how that shows), `timeview.rs` (the horizontal view, f64), `lanes.rs` (the clip library and the engine's clip list), `media.rs` (loading audio in the background), `ruler.rs`, `position.rs`, `canvas.rs` (GPU grid and clip bodies) |
| Transport and audio | `transport.rs` (transport bar, audio panel, shortcuts), `audio.rs` (device choice and status), `audio_settings.rs` (settings window), `probe.rs` (ASIO helper process) |
| Design system | `theme.rs`, `motion.rs`, `fonts.rs`, `panels.rs` (sliding panels), `icons.rs` (vector icons, track-type icons, close button), `widgets.rs` (dialogs and their title row, value slider, fold arrow, gradients, glow line), `owner.rs` (dialogs owned by the main window; the system's cursor during a file drag) |
| App | A library with a thin binary (`main.rs` calls `lumen_app::run`), so tests in `tests/` start the real app. `lib.rs` (the `App` state, the frame loop and start-up, `App::new`); `launch.rs` (what start-up needs, from the command line or a test); `testing.rs` (the small public API the end-to-end tests use, ADR-0029; the tests themselves are in `tests/e2e/` and `tests/smoke.rs`); `app/` holds its behaviour by concern: `projects.rs` (new, open, save, the unsaved-changes prompt), `recovery.rs` (autosave and the recovery banner), `history.rs` (undo, redo, History window, toasts), `preferences.rs` (app menu, appearance, settings saves), `view.rs` (transport bar, arrangement composition, ruler, playhead, device banner), `navigate.rs` (zoom, fit, back, Follow), `grid.rs` (snapping, grid keys, menu and button), `rulers.rs` (the locator strip, bar ruler and time ruler, layout A), `regions.rs` (the loop brace and punch), `locators.rs` (locator flags, Set ◂ ▸, their keys), `scrub.rs` (the scrub strip), `editing.rs` (clip selection, drags and editing commands), `handles.rs` (dragging fade and gain handles), `clip_card.rs` (clip mute, lock, rename and colour, and the pinned properties card); `timefmt.rs` (time formats: minutes and seconds, samples, timecode) |
| Commands | `commands/` (`mod.rs`: every command once, with its id, name, area and default chords; `keys.rs`: chords matched exactly and the keymap, ADR-0033); `app/commands.rs` (whether a command can run now and why not, running it, and the one keyboard dispatcher); `app/palette.rs` (the command palette: rows, fuzzy search, typed values, context labels, drawing) |
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
- Audio-thread code never allocates, locks, blocks or panics. Tests enforce
  it: the allocation guard, plus RealtimeSanitizer in CI with canaries that
  prove the sanitizer is active.
- Third-party native code (plugins, drivers during probing) runs only where
  a crash cannot take Lumen down.
- Lumen never writes a file it cannot read back. Every persisted change
  bumps the format version, with a migration and a frozen fixture.
- Every edit goes through `Document::apply` and is one undo step.
- Disk work runs off the UI thread: the interface and the audio never wait
  on files.
- Colours, sizes and durations come from the design-system modules, never
  inline.
- User-visible strings come from `lumen_core::product` where they name the
  product.
