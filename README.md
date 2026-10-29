# Lumen

[![CI](https://github.com/Lightwell-Audio/lumen/actions/workflows/ci.yml/badge.svg)](https://github.com/Lightwell-Audio/lumen/actions/workflows/ci.yml)
[![Licence: GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue.svg)](LICENSE)

<p align="center">
  <img src="docs/brand/lumen-mascot-anime-v1.png" width="180" alt="Lumi, Lumen's amber light-spirit mascot, waving in blue headphones">
</p>

> **Lumi says:** Welcome to Lumen! I'll show you [what works today](#what-works-today),
> help you [get started](#getting-started), and point you toward
> [ways to contribute](#contributing).

Lumen is a desktop digital audio workstation written in Rust. It is built
around one timeline: the arrangement is the whole product, and the browser,
device chain, clip editors and mixer exist to support it. There is no session
view and no clip launcher.

Lumen is free software under GPL-3.0-or-later. A public project page is at
[lightwell-audio.github.io/lumen](https://lightwell-audio.github.io/lumen/).

> **Status: early development, not yet 1.0.** Lumen records and edits MIDI,
> plays and edits audio clips, hosts VST3 instruments and effects in separate
> processes, automates parameters, records audio, and mixes, saves and exports
> projects. Drum maps and the built-in effects are still to come.
> See [Roadmap](#roadmap) and [What is not built yet](#what-is-not-built-yet).

## What works today

The audio engine, transport and arrangement are built: ASIO, WASAPI and
CoreAudio devices that recover from loss or a changed rate/buffer; clips with
trim, slip, fades, gain and true crossfades across audio, MIDI, group, return
and master tracks; and editing commands (join, crop, reverse, normalize,
bounce, freeze) that are each one undoable step from a command palette and a
remappable keymap. MIDI recording, the piano roll, quantize, and automation
(Read/Write/Touch/Latch, clip envelopes, MIDI learn) are in place. VST3
instruments and effects host in a separate, crash-isolated process per
plugin, with a generic parameter UI, floating editors, and state and presets
saved in the project — real-plugin testing is still in progress, see
[docs/PLUGIN_COMPATIBILITY.md](docs/PLUGIN_COMPATIBILITY.md). The mixer has
groups, sends, returns, delay compensation and LUFS metering; projects save
to a versioned, autosaving `.lumen` folder with crash recovery and
migrations. Audio recording with takes, Capture MIDI, the media pool and
export of masters and stems are built; export awaits its hardware test.

See [docs/PROGRESS.md](docs/PROGRESS.md) for the full requirement-by-requirement
status.

### What is not built yet

Disk streaming for clips too large for memory, drum maps and the step
sequencer, the built-in EQ/compressor/limiter/delay/reverb/sampler, warp and
time-stretch, installers, and VST3 hosting on macOS (approved, in progress)
and Linux (not scheduled).

## Roadmap

<p>
  <img src="docs/brand/lumi-curious.png" width="100" alt="Lumi looking curious with a hand on his chin">
</p>

> **Lumi says:** Curious about what's next? Check the status table in
> PROGRESS.md, then explore the milestone plans for the details.

Lumen is built in ten milestones. Each one ends with a working build,
automated tests, manual tests on real hardware where needed, and a written
report; the owner reviews each milestone before the next one starts.
Milestone 9 is being built before milestone 8 (owner decision, 2026-10-23).

The milestone-by-milestone status is the table at
the top of [docs/PROGRESS.md](docs/PROGRESS.md), which also tracks every
requirement in [CLAUDE.md](CLAUDE.md) against its current status. The plan
for each milestone is in [docs/milestones/](docs/milestones/), and manual
tests (MT-*) are listed in [docs/MANUAL_TESTS.md](docs/MANUAL_TESTS.md).

VST3 hosting on macOS is approved (2026-10-19) and in progress; see
[docs/milestones/macOS-VST3.md](docs/milestones/macOS-VST3.md). Linux hosting
is not scheduled.

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
git clone https://github.com/Lightwell-Audio/lumen.git
cd lumen
cargo build --release -p lumen-plugin-host
cargo run --release -p lumen-app
```

Build the plugin host first, as above. It is a separate program that scanning
and hosting need beside the application, so after a change to its protocol,
close Lumen and rebuild it.

To run without an audio device, or on a machine without an interface, start
Lumen with the test device:

```bash
cargo run --release -p lumen-app -- --audio test:fake
```

Start with Shift held (Windows) or with `--safe-mode` to open Lumen without
loading any plugin, for example after a plugin has stopped Lumen from starting.

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
| `crates/lumen-engine` | Real-time engine: transport, clip player, mixer graph, MIDI and recording |
| `crates/lumen-audio` | Audio device layer: ASIO, WASAPI and cpal backends, and the device supervisor |
| `crates/lumen-midi` | MIDI device layer: ports listed, hot-plugged and opened off the UI thread |
| `crates/lumen-devices` | Built-in devices (the synth), with real-time-safe DSP |
| `crates/lumen-hosting` | Lumen's side of plugin hosting: one process per plugin device, loaded in the background |
| `crates/lumen-plugin-host` | The plugin host: loads a VST3 module in its own process over a versioned protocol |
| `crates/lumen-plugin-catalog` | Finding, scanning and caching plugins, without loading them into Lumen |
| `crates/lumen-test-plugin` | Deterministic VST3 test plugins, with injectable faults |
| `crates/lumen-project` | Document model, `.lumen` format, migrations, undo history, editing rules |
| `crates/lumen-media` | Audio decoding, sample-rate conversion and waveform peaks |
| `crates/lumen-core` | Time base, tempo and meter maps, grid, curves, fades, pan laws, loudness |
| `crates/lumen-render` | GPU renderer for dense content (wgpu, Vello) |
| `crates/lumen-rt` | Real-time safety tools: allocation guard and sanitizer hooks |
| `crates/lumen-perf` | Performance-gate scenarios |
| `crates/lumen-shader-check` | Build-time checks for the WGSL shaders |
| `prototypes/` | The early prototypes that settled the architecture, kept for reference |
| `site/` | The public project page, published with GitHub Pages |
| `docs/` | Architecture, design, decisions, progress, milestone plans, audits and manual tests |

## Documentation

| Document | Purpose |
|---|---|
| [CLAUDE.md](CLAUDE.md) | Product specification, quality bar and working agreement |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, threads and how the parts fit together |
| [docs/DESIGN.md](docs/DESIGN.md) | Design system: colour, type, spacing, motion, key flows |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records, with reasons and consequences |
| [docs/PROGRESS.md](docs/PROGRESS.md) | Status of every requirement, the milestone table, and each milestone's report |
| [docs/milestones/](docs/milestones/) | Milestone plans |
| [docs/audits/](docs/audits/) | Milestone reviews and their findings |
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
- Write documentation in product-neutral language. Name no third-party
  products, plugins or vendors as comparisons, references or examples; keep
  the technical names, platforms, formats and legal notices that accuracy
  requires.
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
