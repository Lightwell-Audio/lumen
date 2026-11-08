# Third-party notices

Lumen is free software under GPL-3.0-or-later (ADR-0039; see `LICENSE`). It
is built on open-source components; most use permissive licences (MIT,
Apache-2.0, BSD, ISC, Zlib and similar, ADR-0013). Before the first release, a complete generated list of every
component and its licence text will ship with the installer (M10).

This file records the components whose licences need specific action.

## symphonia (audio decoding)

- **Crates:** `symphonia`, `symphonia-core`, `symphonia-common`,
  `symphonia-metadata`, `symphonia-format-riff`, `symphonia-format-ogg`,
  `symphonia-codec-pcm`, `symphonia-codec-vorbis`, `symphonia-bundle-flac`,
  `symphonia-bundle-mp3` (version 0.6).
- **Licence:** Mozilla Public License 2.0 (<https://mozilla.org/MPL/2.0/>).
- **Source code:** <https://github.com/pdeljanov/Symphonia>, and on crates.io.
- **How Lumen complies:** the crates are used unmodified, as published.
  Under the MPL, Lumen's own code stays proprietary; any change to these
  crates' files would have to be published under the MPL.
- **Recorded:** owner decision 2026-10-06 (`docs/DECISIONS.md`), with a
  `cargo-deny` exception in `deny.toml`.

## flacenc (FLAC export)

- **Crate:** `flacenc` 0.5.1, used unmodified with default features disabled.
- **Licence:** Apache License 2.0. Source and license:
  <https://github.com/yotarok/flacenc-rs>,
  <https://docs.rs/crate/flacenc/0.5.1/source/LICENSE>.
- **Use:** worker-thread encoding, one FLAC frame at a time. No native
  encoder or additional SDK is bundled.
- **Compliance:** retain its license and attribution in the generated
  distribution notices. Apache-2.0 is already allowed by `deny.toml`.
- **Recorded:** M9 R5, ADR-0107; approved X2 A in the milestone plan.

## midly (Standard MIDI Files)

- **Crate:** `midly` (version 0.5), used by `lumen-project` to read and
  write `.mid` files.
- **Licence:** the Unlicense (<https://unlicense.org/>), a dedication to the
  public domain with a permissive fallback. It sets no conditions.
- **Source code:** <https://github.com/negamartin/midly>, and on crates.io.
- **Recorded:** M5 slice 7 (ADR-0062), with a `cargo-deny` exception in
  `deny.toml`, as the Unlicense is not on the general allow list.

## Time-stretch and pitch-shift library, and its FFT (vendored C++ source)

- **Components:** a header-only time-stretch and pitch-shift library
  (version 1.3.2, upstream tag 1.4.0), and the header-only FFT and STFT
  library it uses. Both are vendored as source in
  `crates/lumen-media/vendor/`, compiled by `crates/lumen-media/build.rs`
  through a thin binding of our own (`src/stretch/shim.cpp`). The Rust wrapper
  crate is not used.
- **Licence:** MIT, for each. Copyright (c) 2022 Geraint Luff / Signalsmith
  Audio Ltd. for the stretch library, and Copyright (c) 2025 Signalsmith Audio
  for the FFT library. Each licence text is kept beside its source
  (`vendor/signalsmith-stretch/LICENSE.txt`,
  `vendor/signalsmith-linear/LICENSE.txt`).
- **Source code:** <https://github.com/Signalsmith-Audio/signalsmith-stretch>
  and <https://github.com/Signalsmith-Audio/linear>. The pinned versions and
  any change made here (one patched line, with its patch file) are listed in
  `crates/lumen-media/vendor/VENDORED.md`.
- **Compliance:** the licence notices ship with the distribution notices
  (M10). The licence sets no other condition. Not checked: patents, and any
  terms on the authors' names.
- **Recorded:** M9.5 S4c, ADR-0132; the owner chose it (T = C, 2026-11-08,
  ADR-0120).

## Typefaces: Inter, Unbounded and Martian Mono

- **Files:** `crates/lumen-app/assets/fonts/Inter.ttf` (Inter, by Rasmus
  Andersson), `Unbounded.ttf` (Unbounded, by the Unbounded Project
  Authors) and `MartianMono.ttf` (Martian Mono, by the Martian Mono Project
  Authors, Evil Martians; added in M4.5), variable fonts embedded in the
  application.
- **Licence:** SIL Open Font License 1.1. The full texts are next to the
  files (`Inter-OFL.txt`, `Unbounded-OFL.txt`, `MartianMono-OFL.txt`).
- **Source:** the Google Fonts repository,
  <https://github.com/google/fonts> (`ofl/inter`, `ofl/unbounded`,
  `ofl/martianmono`).
- **How Lumen complies:** the fonts are bundled unmodified. The OFL allows
  bundling with any software, including proprietary software, as long as
  the licence text ships with them and the fonts are not sold on their own.
  The installer (M10) must include the three licence texts.

## Pending owner actions

- **ASIO:** the Steinberg SDK's GPLv3 option may now apply, in place of the
  signed proprietary agreement (ADR-0006, ADR-0039). The owner must confirm
  the SDK's current licence text before relying on it, and the ASIO trademark
  notice is still required. The interface definitions in
  `crates/lumen-audio/src/asio/ffi.rs` must be checked against the SDK
  before the source is published.
- **VST:** the VST trademark notice and VST3 SDK licence compliance are
  needed before release (spec §7, M10).
