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

## Typefaces: Inter and Unbounded

- **Files:** `crates/lumen-app/assets/fonts/Inter.ttf` (Inter, by Rasmus
  Andersson) and `Unbounded.ttf` (Unbounded, by the Unbounded Project
  Authors), variable fonts embedded in the application.
- **Licence:** SIL Open Font License 1.1. The full texts are next to the
  files (`Inter-OFL.txt`, `Unbounded-OFL.txt`).
- **Source:** the Google Fonts repository,
  <https://github.com/google/fonts> (`ofl/inter`, `ofl/unbounded`).
- **How Lumen complies:** the fonts are bundled unmodified. The OFL allows
  bundling with any software, including proprietary software, as long as
  the licence text ships with them and the fonts are not sold on their own.
  The installer (M10) must include both licence texts.

## Pending owner actions

- **ASIO:** the Steinberg SDK's GPLv3 option may now apply, in place of the
  signed proprietary agreement (ADR-0006, ADR-0039). The owner must confirm
  the SDK's current licence text before relying on it, and the ASIO trademark
  notice is still required. The interface definitions in
  `crates/lumen-audio/src/asio/ffi.rs` must be checked against the SDK
  before the source is published.
- **VST:** the VST trademark notice and VST3 SDK licence compliance are
  needed before release (spec §7, M10).
