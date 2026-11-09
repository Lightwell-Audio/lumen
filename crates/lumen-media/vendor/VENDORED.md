# Vendored C++ sources

Header-only libraries compiled into `lumen-media` by `../build.rs`
(M9.5 S4c, ADR-0132). Both are MIT-licensed; each folder keeps its licence
text, and `THIRD_PARTY_NOTICES.md` at the repository root lists them.

| Folder | What | Version | Changes here |
|---|---|---|---|
| `signalsmith-stretch/` | time-stretch and pitch-shift | header version 1.3.2, upstream tag 1.4.0 | one line: `patches/0001-reanalyse-previous-block-off-by-one.patch` (upstream #35: the high band was lost at rates within about 0.1% of 1). Marked "LUMEN PATCH 0001" in the header; re-apply it after an update unless the new tag fixes it |
| `signalsmith-linear/` | FFT and STFT used by the stretch library | fetched with the tag above; its own tag was not recorded, so record it the next time the files are updated | none |

Our binding is `../src/stretch/shim.cpp` (a C interface over the library's
class, with every call guarded against C++ exceptions) and
`../src/stretch/mod.rs` (the safe Rust side). Nothing in `vendor/` is edited
without a patch file in `patches/` that says why, a marker on the changed
line, and a row in the table above.

## Build rules (`../build.rs`)

- C++14, optimisation level 2, no fast-math and no fused multiply-add
  contraction (`-fno-fast-math`, `-ffp-contract=off`, `/fp:precise`), to keep
  the arithmetic as written; the stretcher is seeded, so its random phases are
  the same every run. The tests prove identical bits for any page size, jump
  and block size within one build; identical bits between platforms (the
  maths library differs) are not claimed or checked.
- Compiler warnings are off for the vendored sources only.

## Updating

1. Fetch the new tag for both libraries into these folders.
2. Update the table above, with the tags, and the notices file if a licence
   text or a copyright line changed.
3. Run `cargo test -p lumen-media stretch` (alignment, level, pitch,
   determinism and seams) and `cargo bench -p lumen-media --bench warped`.
