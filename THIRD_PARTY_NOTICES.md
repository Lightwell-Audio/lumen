# Third-party notices

Lumen is closed source (ADR-0012). It is built on open-source components;
most use permissive licences (MIT, Apache-2.0, BSD, ISC, Zlib and similar,
ADR-0013). Before the first release, a complete generated list of every
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

## Pending owner actions

- **ASIO:** shipping requires the signed Steinberg ASIO SDK licence agreement
  and the ASIO trademark notice (ADR-0006).
- **VST:** the VST trademark notice and VST3 SDK licence compliance are
  needed before release (spec §7, M10).
