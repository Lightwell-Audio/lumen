# Architecture audit, 2026-10-07 (after M3 slice 4a)

Scope: every crate under `crates/` (about 21,000 lines, 187 tests). It was
checked against `CLAUDE.md` (§2–§5), the rules in `docs/ARCHITECTURE.md`,
`docs/DESIGN.md`, and the conventions in `CLAUDE.md` §12.

## What holds

| Standard | Result |
|---|---|
| Audio thread never allocates, locks, blocks or panics | ✅ The engine's `process()` (clips, metronome, timeline swaps, rate changes) runs in real-time sections in tests, checked by the allocation guard and by RealtimeSanitizer in Linux CI with live canaries. The ASIO, WASAPI and cpal callbacks use atomics only; their `format!`, `vec!` and `expect` calls are in device setup, not in the callbacks. |
| Layering | ✅ No cycles: `lumen-audio`, `lumen-media` and `lumen-render` stand alone; the engine depends only on core and rt; the project only on core; only the app joins them. |
| Every edit is one undo step | ✅ All project changes go through `Document::apply` / `History::apply`; the history is only read elsewhere. Property-tested (ADR-0024). |
| Lumen never writes a file it cannot read back | ✅ Format v1–v3 with migrations and frozen fixtures; saving validates first; atomic writes and backups. |
| `unsafe` is justified | ✅ Every block carries a SAFETY comment (clippy `undocumented_unsafe_blocks`, enforced by `-D warnings`). |
| CI gates | ✅ fmt, clippy `-D warnings` and tests on Windows, macOS and Linux; RTSan with canaries; cargo-deny; the instruction-count performance gate. |

## Findings

### High: breaks a stated rule

- **H1. Disk work on the UI thread.**
  - `settings::save` runs on the UI thread (`main.rs`,
    `save_settings_when_settled`).
  - `autosave.rs` writes its note file and deletes autosaves there too.
  - Both are small files, but a slow or synced disk (OneDrive, a network
    drive) stalls a frame. That breaks "disk work runs off the UI thread"
    (ARCHITECTURE) and the quality bar's "nothing blocks".
- **H2. Keyboard-only and accessibility gaps in the track headers.**
  - A track cannot be selected without the mouse, so Ctrl+D, Delete, F2
    and Alt+arrows cannot be reached from the keyboard alone.
  - The header background, the name, the resize handle and the empty area
    have no accessible names.
  - CLAUDE.md §5 checklist items 1 and 10, and §7 "keyboard-only core
    flows; accessible control names" [M].
- **H3. A panic where an error belongs.** `audio.rs` creates the ASIO
  backend with `expect("COM apartment for ASIO")`. If COM cannot start on
  the audio control thread, that thread panics and audio stops without a
  message, instead of showing "audio could not start" with a way forward.

### Medium: architecture and maintainability

- **M1. Two oversized modules.**
  - `main.rs` (1,764 lines) mixes the app state, the file workflow and
    prompts, the menus, banners, the history window and toast, the ruler
    and arrangement composition, and start-up.
  - `arrangement.rs` (1,637 lines) mixes headers, lanes, clip drawing,
    sliders and icons.
  - Slice 4b–4d add to both, so they should be split first.
- **M2. A general control lives in a feature module.** `value_slider` (drag,
  Shift for fine, double-click reset, type a value, arrow keys) is exactly
  the control behaviour CLAUDE.md §5 asks for everywhere. It belongs in a
  design-system widgets module, so the M4 mixer and later devices reuse it,
  rather than in `arrangement.rs`.
- **M3. Volume and pan edits bypass the document's named operations.** They
  are inline `doc.apply` closures in `arrangement.rs`, while every other
  track edit is a tested `Document` method.
- **M4. Untested model-to-engine translation.** `lanes::engine_clips` turns
  the project into what is heard: file offsets converted to the device
  rate, clip regions capped, solo and mute, track gain and pan. It has no
  unit tests.

### Low: consistency and polish

- **L1. The product name is hard-coded** in five user-visible strings
  (`lanes.rs`, `main.rs` twice, `lumen-media`, the ASIO probe window
  title), instead of `lumen_core::product`.
- **L2. Design tokens:**
  - three literal font sizes in `main.rs` (ruler numbers, the drop
    overlay);
  - one literal colour (text on lit buttons);
  - the toast's 1.4 s dwell is not a motion token;
  - `docs/DESIGN.md` §3 does not list the `lane`, `header`, `clip_body`
    and `shadow` tokens.
- **L3. Stale suppressions.** `#[allow(dead_code)] // for the device
  selector (M2 slice 8)` on `Driver::version`, `Driver::clock_sources` and
  `DriverEntry::description`: slice 8 is done and they are still unused.
- **L4. Long parameter lists.** Three `#[allow(clippy::too_many_arguments)]`
  (`draw_clip`, `Arrangement::name`, the metronome click). Small parameter
  structs would read better.

## Recommended order

1. Before slice 4b:
   - H1–H3 and M3–M4 (correctness and rules);
   - L1–L3 (quick);
   - M1–M2 (mechanical splits, so 4b–4d land in clean modules).
2. L4 alongside M1.
