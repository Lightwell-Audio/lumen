# Lumen — Manual Test Scripts

Manual checks that need hardware, third-party software, or a person are grouped by milestone. Test IDs and run notes are preserved in their milestone files.

Run commands from the repository root (`D:\Projects\lumen`). Prefer `cargo run --release` so the current code is built before launch; starting an `.exe` by path can run a stale copy (MT-APP-3; see [Development](DEVELOPMENT.md)). Older M1 binaries remain in `%USERPROFILE%\lumen-bin`.

| Milestone | Manual tests |
| --- | --- |
| M1 · Foundations and prototypes | [M1.md](manual-tests/M1.md) |
| M2 · Sound and device selector | [M2.md](manual-tests/M2.md) |
| M3 · Arrangement UI | [M3.md](manual-tests/M3.md) |
| M4 · Mixer, routing and group tracks | [M4.md](manual-tests/M4.md) |
| M4.5 · Polish pass | [M4.5.md](manual-tests/M4.5.md) |
| M5 · MIDI | [M5.md](manual-tests/M5.md) |
| M6 · VST3 hosting and the device system | [M6.md](manual-tests/M6.md) |
| M7 · Automation and envelopes | [M7.md](manual-tests/M7.md) |
| M7b · Advanced editing and render | [M7b.md](manual-tests/M7b.md) |
| Native VST3 hosting on macOS | [macOS-VST3.md](manual-tests/macOS-VST3.md) |
