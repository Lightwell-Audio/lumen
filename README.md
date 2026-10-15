# Lumen

[![CI](https://github.com/andrewl913/lumen/actions/workflows/ci.yml/badge.svg)](https://github.com/andrewl913/lumen/actions/workflows/ci.yml)
[![Licence: GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/brand/lumen-mascot-anime-v1.png" width="180" alt="Lumi, Lumen's amber light-spirit mascot, waving in blue headphones">
</p>

> **Lumi says:** Welcome to Lumen! I'll show you [what works today](#what-works-today),
> help you [get started](#getting-started), and point you toward
> [ways to contribute](#contributing).

Lumen is a desktop digital audio workstation written in Rust. It is built
around one timeline: the arrangement is the whole product, and the browser,
device chain, clip editors and mixer exist to support it. The aim is
an arrangement-first workflow with a clean, polished interface.

> **Status: early development.** Lumen can play, record and edit MIDI, work
> with audio clips, mix and save projects. Audio recording, VST3 hosting and
> drum editing are still to come. See [Roadmap](#roadmap).

## What works today

- **Audio devices:** ASIO (with a device selector and driver control panel),
  WASAPI shared and exclusive on Windows, and CoreAudio or Linux audio
  through cpal. Lumen recovers when a device is lost, and switches device,
  sample rate and buffer size without restarting.
- **Transport:** a sample-accurate transport with loop, punch region,
  metronome, test tone, scrubbing and locators.
- **Arrangement:** tracks with folding and resizing, audio clips with move,
  trim, slip, loop-extend, fades along free curves, clip gain and colour,
  time-range selection, and the clip overlap rule.
- **Editing:** unlimited undo and redo, where each edit is one step; a
  command palette (Ctrl+K); a rebindable keyboard map with conflict checks.
- **MIDI:** live input and monitoring, MIDI recording, a piano roll with
  velocity and controller lanes, quantize and editing tools, Standard MIDI
  File import and export, and MIDI clock out. MIDI learn and drum editing
  are planned for later milestones.
- **Grid and zoom:** an editing grid with snapping, zoom down to single
  samples, and waveforms that stay crisp at every zoom level.
- **Mixer:** group tracks (nested up to three deep), sends and returns,
  a master track, output routing, sidechain inputs, automatic delay
  compensation, and a cue bus. Metering includes peak, RMS and, on the
  master, LUFS and true peak.
- **Projects:** a versioned, human-readable `.lumen` folder format with
  relative media paths, atomic saves, rolling backups, autosave and crash
  recovery. Older project versions open through migrations.
- **Interface:** dark and light themes, 100 to 200% UI scale, keyboard
  navigation, and a design system written down in
  [docs/DESIGN.md](docs/DESIGN.md).

Audio files can be imported by drag and drop or from the menu. WAV, AIFF,
FLAC, MP3 and Ogg Vorbis are supported.

## Roadmap

<p>
  <img src="docs/brand/lumi-curious.png" width="100" alt="Lumi looking curious with a hand on his chin">
</p>

> **Lumi says:** Curious about what's next? Check the status column below,
> then explore the milestone plans for the details.

Lumen is built in milestones. Each one ends with a working build, automated
tests, manual tests on real hardware where needed, and a written report.

| Milestone | Scope | Status |
|---|---|---|
| 1. Foundations | Repository, CI, benchmarks, and the prototypes that settled the architecture | Complete |
| 2. Audio engine and devices | Device selector, ASIO, transport, playback | Complete |
| 3. Arrangement and project system | Tracks, clips, editing, undo, save and open, autosave | Complete |
| 4. Mixer, routing and groups | Mixer graph, groups, sends, returns, metering, delay compensation | Complete |
| 4.5. Polish pass | Interface audit fixes: controls, motion, light layer | Built; manual tests pending |
| 5. MIDI and piano roll | MIDI devices, recording, piano roll, quantize, Standard MIDI Files and clock out | In progress — slices 0–7 built; slice 8 close underway |
| 6. VST3 hosting | Plugin scanning, instruments and effects, sandboxed plugin processes | Planned |
| 7. Automation and advanced editing | Automation lanes, clip envelopes, consolidate, bounce, freeze | Planned |
| 8. Drum maps and built-in devices | Drum maps, drum editor, step sequencer, EQ, compressor, synth, sampler | Planned |
| 9. Recording, media and export | Multi-channel recording, disk streaming, stem export | Planned |
| 10. Hardening and release | Plugin isolation, performance tuning, installers | Planned |

The full requirements, with the status of each, are in
[docs/PROGRESS.md](docs/PROGRESS.md). The product specification is in
[CLAUDE.md](CLAUDE.md).

## Getting started

<p>
  <img src="docs/brand/lumi-focused.png" width="100" alt="Lumi adjusting his headphones, ready to guide setup">
</p>

> **Lumi says:** Let's get set up. Start with the requirements for your platform,
> then build and run. If you don't have an audio interface handy, try the
> test-device command below.

### Requirements

- **Rust:** the exact compiler version is pinned in `rust-toolchain.toml`.
  Install [rustup](https://rustup.rs); it fetches the pinned version on the
  first build.
- **Windows** (the first-class platform): Visual Studio 2022 Build Tools with
  the *Desktop development with C++* workload. ASIO drivers are optional but
  recommended for low latency.
- **macOS:** Xcode command line tools.
- **Linux:** the system libraries used in CI:

  ```bash
  sudo apt-get install -y libxkbcommon-dev libwayland-dev libx11-dev \
    libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev libasound2-dev \
    libfontconfig1-dev mesa-vulkan-drivers
  ```

Windows build notes, including a setup without admin rights, are in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

### Build and run

```bash
git clone https://github.com/andrewl913/lumen.git
cd lumen
cargo run --release -p lumen-app
```

To run without an audio device, or on a machine without an interface, start
Lumen with the test device:

```bash
cargo run --release -p lumen-app -- --audio test:fake
```

### Test

```bash
cargo test --workspace --all-targets            # unit, integration and benchmark smoke tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test -p lumen-app --test e2e             # headless end-to-end suite
```

The real-binary smoke tests need a GPU and a window, so they are opt-in:
`cargo test -p lumen-app --test smoke -- --ignored`.

Performance is gated in CI by instruction counts under Callgrind, against a
committed baseline in `perf/baseline.json`. See
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for how to run and update it.

## Project layout

| Path | Contents |
|---|---|
| `crates/lumen-app` | The `lumen` application: interface, commands, settings, end-to-end tests |
| `crates/lumen-engine` | Real-time engine: transport, clip player, mixer graph, metronome |
| `crates/lumen-audio` | Audio device layer: ASIO, WASAPI, cpal backends and the device supervisor |
| `crates/lumen-project` | Document model, `.lumen` format, migrations, undo history, editing rules |
| `crates/lumen-media` | Audio decoding, sample-rate conversion and waveform peaks |
| `crates/lumen-core` | Time base, tempo and meter maps, grid, curves, fades, pan laws, loudness |
| `crates/lumen-render` | GPU renderer for dense content (wgpu, Vello) |
| `crates/lumen-rt` | Real-time safety tools: allocation guard and sanitizer hooks |
| `crates/lumen-perf` | Performance-gate scenarios |
| `crates/lumen-shader-check` | Build-time checks for the WGSL shaders |
| `docs/` | Architecture, design, decisions, progress, milestone plans and reports |

## Documentation

| Document | Purpose |
|---|---|
| [CLAUDE.md](CLAUDE.md) | Product specification, quality bar and working agreement |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, threads and how the parts fit together |
| [docs/DESIGN.md](docs/DESIGN.md) | Design system: colour, type, spacing, motion, key flows |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records, with reasons and consequences |
| [docs/PROGRESS.md](docs/PROGRESS.md) | Status of every requirement and each milestone's report |
| [docs/ROADMAP.md](docs/ROADMAP.md) | The ten-milestone plan |
| [docs/milestones/](docs/milestones/) | Milestone plans |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Toolchain setup, everyday commands, test and benchmark tools |
| [docs/MANUAL_TESTS.md](docs/MANUAL_TESTS.md) | Tests that need a person or hardware, with each run's results |
| [docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md) | Problems found and not yet fixed |
| [docs/PLUGIN_COMPATIBILITY.md](docs/PLUGIN_COMPATIBILITY.md) | Compatibility matrix for real-world VST3 plugins |
| [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) | Third-party licences that need specific action |

## Contributing

<p>
  <img src="docs/brand/lumi-celebrating.png" width="100" alt="Lumi smiling with closed eyes and both arms raised in celebration">
</p>

> **Lumi says:** Thanks for helping Lumen grow! Feedback and bug reports are
> welcome. For a larger code change, open an issue first, then follow the
> checklist below before sending your pull request.

Lumen is developed in the open, and feedback is welcome. Before you start
on a larger change, please open an issue so we can agree on the approach.
Lumen follows a milestone plan, so a feature may already have a planned
place in the roadmap.

Before you open a pull request:

- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
  and the test suite. CI runs these on Windows, macOS and Linux.
- Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and the engineering rules
  in `.claude/skills/lumen-architecture/`. The key ones: nothing
  that can block runs on the audio thread, every edit goes through the
  document so it can be undone, and anything saved to a project bumps the
  format version with a migration and a golden fixture.
- Put UI colours, sizes and durations in the theme, motion and font modules,
  never inline (see [docs/DESIGN.md](docs/DESIGN.md)).
- Record a decision in [docs/DECISIONS.md](docs/DECISIONS.md) when you change
  the architecture, a dependency's licence, or the on-disk format.

Changes to audio behaviour or the interface should be checked by hand where
possible. Say in the pull request what you tested and what you could not test,
such as a particular interface or plugin.

## Licence

Lumen is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. It is distributed without any warranty. See [LICENSE](LICENSE).

Third-party components are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
Signed, packaged builds are sold separately; the source is free (ADR-0039).

## Trademarks

ASIO is a trademark and software of Steinberg Media Technologies GmbH. VST is
a trademark of Steinberg Media Technologies GmbH, registered in Europe and
other countries. Lumen uses these names only to say which standards it
supports. Lumen is not made by or endorsed by Steinberg.
