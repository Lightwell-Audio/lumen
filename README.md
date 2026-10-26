# Lumen

[![CI](https://github.com/gummycookie/lumen/actions/workflows/ci.yml/badge.svg)](https://github.com/gummycookie/lumen/actions/workflows/ci.yml)
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
[gummycookie.github.io/lumen](https://gummycookie.github.io/lumen/).

> **Status: early development, not yet 1.0.** Lumen records and edits MIDI,
> plays and edits audio clips, hosts VST3 instruments and effects in separate
> processes, automates parameters, and mixes and saves projects. Audio
> recording is built and awaiting hardware tests. Export of audio masters and
> stems, drum maps and the built-in instruments and effects are still to come.
> See [Roadmap](#roadmap) and [What is not built yet](#what-is-not-built-yet).

## What works today

- **Audio devices:** ASIO with a device selector and the driver's control
  panel; WASAPI in shared and exclusive mode on Windows; CoreAudio on macOS and
  the cpal backend on Linux. Lumen recovers when a device is lost, and it
  switches device, sample rate and buffer size without restarting. Audio input
  is available from ASIO and WASAPI.
- **Transport:** sample-accurate playback with loop, punch region, metronome
  with count-in, test tone, scrubbing and locators.
- **Arrangement:** audio, MIDI, group, return and master tracks; folding and
  resizing; clips with move, trim, slip, loop-extend, fades, clip gain and
  colour; time-range selection; and crossfades, which are true overlaps.
- **Editing:** undo and redo, where each edit is one step. Join merges
  selected clips in place, and Join into new file renders a selection into one
  new file. There are also crop, unloop, nudge, reverse, polarity, normalize,
  bounce, and freeze, unfreeze and flatten for tracks. A command palette opens with Ctrl+K, and the keyboard map can be
  rebound, with conflict checks.
- **Recording:** MIDI recording with replace or overdub, count-in and loop
  passes. Audio recording writes one crash-safe WAV per armed track, with
  latency compensation and a manual offset, and is awaiting hardware tests.
- **MIDI:** device listing with hot-plug, a MIDI From and MIDI To setting per
  track, live input and monitoring, and the computer keyboard as a MIDI
  keyboard. The piano roll has velocity and controller lanes. Quantize has
  strength, swing and a sensitivity window, and there are humanize, legato,
  fixed length, transpose, scale constraint and reverse. Standard MIDI File
  import and export, MIDI clock out and panic are included.
- **Automation:** lanes for track controls, plugin parameters, sends and
  tempo, with Read, Write, Touch and Latch modes. Clip envelopes work on audio
  and MIDI clips. MIDI learn feeds a mapping list, and a manual move overrides
  automation until it is re-enabled.
- **Plugins (VST3):** scanning runs in the background, in a separate process,
  with a cache and a blocklist. Each plugin device runs in its own process, so
  a crashing plugin is isolated, flagged and can be reloaded from its last good
  state. Hosted plugins have a generic parameter UI and floating editors, and
  their state and presets are saved in the project. Safe mode starts Lumen
  without loading any plugin. Testing with real third-party plugins is still
  in progress; see [docs/PLUGIN_COMPATIBILITY.md](docs/PLUGIN_COMPATIBILITY.md).
- **Built-in instrument:** a polyphonic synth with five presets.
- **Mixer:** group tracks nested up to three deep, sends and returns, a master
  track, output routing, sidechain inputs, automatic delay compensation and a
  cue bus. Metering includes peak and RMS, and the master adds LUFS and true
  peak.
- **Projects:** a versioned `.lumen` project folder with relative media paths,
  atomic saves, rolling backups, autosave and crash recovery. Older project
  versions open through migrations.
- **Files:** WAV, AIFF, FLAC, MP3 and Ogg Vorbis import by drag and drop or
  from the menu, and Standard MIDI Files.
- **Interface:** dark and light themes, 100 to 200% UI scale, keyboard
  navigation, and a design system written down in
  [docs/DESIGN.md](docs/DESIGN.md).

### What is not built yet

- Export of the master and stems to audio files (WAV and FLAC), with
  sample-rate conversion, dither and loudness reporting.
- Takes, loop recording into takes, and Capture MIDI.
- Disk streaming for clips too large for memory.
- Drum maps, the drum editor and the step sequencer.
- Built-in EQ, compressor, limiter, delay, reverb and sampler.
- Warp and time stretch, ripple delete and insert silence.
- Installers, and VST3 hosting on macOS and Linux.

## Roadmap

<p>
  <img src="docs/brand/lumi-curious.png" width="100" alt="Lumi looking curious with a hand on his chin">
</p>

> **Lumi says:** Curious about what's next? Check the status column below,
> then explore the milestone plans for the details.

Lumen is built in milestones. Each one ends with a working build, automated
tests, manual tests on real hardware where needed, and a written report. The
owner reviews each milestone before the next one starts. Milestone 9 is being
built before milestone 8 (owner decision, 2026-10-23).

Status below is from [docs/PROGRESS.md](docs/PROGRESS.md), last updated
2026-10-25. Manual tests (MT-*) are listed in
[docs/MANUAL_TESTS.md](docs/MANUAL_TESTS.md).

| Milestone | Scope | Status |
|---|---|---|
| 1. Foundations | Repository, CI, benchmarks, and the prototypes that settled the architecture | Complete |
| 2. Audio engine and devices | Device selector, ASIO, transport, playback | Complete; approved 2026-10-07 |
| 3. Arrangement and project system | Tracks, clips, editing, undo, save and open, autosave | Complete; approved 2026-10-09 |
| 4. Mixer, routing and groups | Mixer graph, groups, sends, returns, metering, delay compensation | Complete, 2026-10-10 |
| 4.5. Polish pass | Interface audit fixes: controls, motion, light layer | Built; MT-POL-5 and MT-POL-6 pending |
| 5. MIDI and piano roll | MIDI devices, recording, piano roll, quantize, Standard MIDI Files, clock out | Slices 0 to 7 built; slice 8 close and MT-MIDI-7 to 9 pending |
| 6. VST3 hosting | Plugin scanning, instruments and effects, a separate process per plugin, editors, presets and state | Slices 0 to 6 built; closeout under way: MT-VST-7 on real plugins, the review's Medium and Low findings, and the report |
| 7. Automation and envelopes | Automation lanes, Read/Write/Touch/Latch, clip envelopes, MIDI learn | Built, 2026-10-22; manual tests passed; owner approval pending |
| 7b. Advanced editing and render | Join, crop, crossfades, consolidate, bounce, freeze and related editing | Slices E0 to E6 built; E6 closeout (MT-EDIT-7) and owner sign-off pending |
| 9. Recording, media and export | Audio input, audio recording, takes, Capture MIDI, export, disk streaming | In progress: render consistency, audio input and recording built; hardware tests MT-REC-1, MT-REC-2 and MT-REC-4 pending; takes and Capture MIDI are next, then export and disk streaming |
| 8. Drum maps and built-in devices | Drum maps, drum editor, step sequencer, EQ, compressor, limiter, delay, reverb, sampler | Planned, after milestone 9; a five-preset synth is already built |
| 10. Hardening and release | Plugin isolation hardening, performance tuning, installers | Planned |

Not scheduled: VST3 hosting on macOS and Linux. The owner approved native
macOS hosting on 2026-10-19; see [docs/milestones/macOS-VST3.md](docs/milestones/macOS-VST3.md).

The full requirements, with the status of each, are in
[docs/PROGRESS.md](docs/PROGRESS.md). The product specification is in
[CLAUDE.md](CLAUDE.md), and the plan for each milestone is in
[docs/milestones/](docs/milestones/).

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
git clone https://github.com/gummycookie/lumen.git
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
| [docs/PROGRESS.md](docs/PROGRESS.md) | Status of every requirement and each milestone's report |
| [docs/ROADMAP.md](docs/ROADMAP.md) | The ten-milestone plan |
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
