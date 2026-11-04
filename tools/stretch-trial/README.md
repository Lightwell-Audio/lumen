# Stretch trial (M9.5 S0)

A bench program for choice T in `docs/milestones/M9.5.md`: which time-stretch
library Lumen uses. It is never shipped. It is its own Cargo workspace, so the
candidates (one GPL, one MPL) never enter Lumen's `Cargo.lock` or the
`cargo-deny` check. The reasoning and the results are ADR-0119.

**Listen before you look.** `src/main.rs` (`letter`) and `results/results.json`
say which candidate each letter is.

## Run

Needs a C++ compiler and CMake 3.30 or newer (one candidate's build uses it;
`pip install cmake` gives a new enough one).

```bash
cargo build --release --manifest-path tools/stretch-trial/Cargo.toml
```

```bash
tools/stretch-trial/target/release/stretch-trial <sources> <out>
```

`<sources>` holds 48 kHz stereo WAV files. The trial writes the listening pack
to `<out>/pack/<source>/<transform>/<letter>.wav`, the original beside it, and
the measurements to `<out>/results.json`. Two checks read the pack:

```bash
python3 -I tools/stretch-trial/check.py <out>
```

```bash
python3 -I tools/stretch-trial/seams.py <out>
```

`check.py` fails if a render is more than 10 ms or 0.5 dB off its original
(the pack is loudness-matched). `seams.py` compares the prepared-ahead renders (plan E A:
85 ms pieces, each from a 340 ms lead-in, joined with 3 ms crossfades) with
the straight ones.

The trial also writes `<out>/sustained.json`: the cost of plan E A, every
piece rendered from its own lead-in, at 85 ms and at 2 s pieces.
`summarise.py <out>` writes `<out>/summary.json` by letter, and
`page/make_page.py <out>`, run in a folder holding the pack as FLAC in MP4
(`media/`), writes the listening page.

## What it measures

For each source, transform and candidate:

- CPU time per second of stereo 48 kHz audio, the best of three renders;
- the work to prepare one 85 ms piece from a 340 ms lead-in (the click-to-sound
  cost under E A);
- the candidate's own latency;
- allocations during steady processing (a counting allocator over `process`
  and `retrieve`; the `rubberband` wrapper's own one Vec per call is taken off);
- whether a render repeats bit for bit;
- whether 64-frame and 4,096-frame blocks give the same bits.

## The sources used on 2026-11-04

Fetched from the Internet Archive, not committed:

- drums: "108 bpm breakbeat" by EatMe, CC0,
  <https://archive.org/details/108_bpm_breakbeat_to_use>, `Sample00`, whole;
- voice: "Public Domain Hymns 01", LibriVox, public domain,
  <https://archive.org/details/publicdomainhymns_01_1110_librivox>, track 4,
  48 to 56 s;
- full mix: "Liberty Bell March", U.S. Army Field Band, public domain,
  <https://archive.org/details/LibertyBellMarch_747>, 30 to 38 s.

Each was converted with `ffmpeg -af aresample=48000:resampler=soxr -ar 48000
-ac 2 -c:a pcm_f32le`.
