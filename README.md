# Lumen

A professional desktop digital audio workstation written in Rust: an
arrangement-first workflow with a clean, minimal interface, VST3 hosting with
crash isolation, and ASIO as a first-class driver on Windows.

**Status:** Milestone 3 of 10 (Arrangement & Project System), slices 1–3
done.
- **Works today:**
  - plays audio through ASIO or Windows audio, chosen in an audio settings
    window, with recovery when a device is lost;
  - a sample-accurate transport with loop, metronome and test tone;
  - audio files dropped onto the timeline;
  - undo and redo with a history;
  - new, open, save and save as, with project folders;
  - autosave and crash recovery;
  - dark and light themes.
- **Not yet:** track headers, clip editing, mixer, MIDI, plugins and
  recording. It is not yet usable as a DAW.

| Document | Purpose |
|---|---|
| [CLAUDE.md](CLAUDE.md) | Product specification and working agreement |
| [docs/ROADMAP.md](docs/ROADMAP.md) | The 10-milestone roadmap |
| [docs/PROGRESS.md](docs/PROGRESS.md) | Status of every requirement |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records |
| [docs/milestones/](docs/milestones/) | Milestone plans and results (M1, M2, M3) |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, threads and how the parts fit |
| [docs/DESIGN.md](docs/DESIGN.md) | Design system: colour, type, spacing, motion, key flows |
| [docs/MANUAL_TESTS.md](docs/MANUAL_TESTS.md) | Tests that need a person or hardware, with every run's results |
| [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) | Licences that need specific action |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Toolchain setup and everyday commands |

"Lumen" is a working name, defined once in `crates/lumen-core/src/product.rs`.

## Licence

Lumen is free software under the GNU General Public License, version 3 or
later (see [LICENSE](LICENSE)). Its third-party components are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Signed, packaged builds are
sold separately; the source is free (ADR-0039).
