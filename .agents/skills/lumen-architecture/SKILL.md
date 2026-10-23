---
name: lumen-architecture
description: Lumen's engineering rules. Use before writing, changing or reviewing Rust code in any Lumen crate (engine, audio, project, app, render), when deciding where code belongs, how an edit is made undoable, what touches the disk or the audio thread, or how a change is tested and gated.
---

# Lumen architecture

Before planning substantial work, read and follow the shared
[subagent delegation policy](../references/delegation.md). Prefer bounded
subagent tasks for investigation, implementation and tests; the parent owns
integration and final verification.

The sources of truth are `docs/ARCHITECTURE.md` (crates, threads, module
map), `docs/DECISIONS.md` (ADRs and owner decisions) and `AGENTS.md` §12
(where the project stands). Read the parts you touch. This skill is the
short form: the rules, and the checks that keep them.

## Hard rules (a violation is a High finding)

1. **The audio thread never allocates, locks, blocks or panics.** Anything
   the engine or a device callback runs is real-time code. Data reaches it
   through lock-free queues (`rtrb`); replaced data goes back to be freed
   off the audio thread. Tests enforce it: run new engine paths inside
   `rt_section` under the allocation guard (RTSan runs them in Linux CI).
2. **Never lose work.** Every persisted change bumps the `.lumen` format
   version, with a migration (`v<n>_to_v<n+1>` over untyped JSON) and a
   frozen golden fixture. Saves are atomic, with backups. Settings files are
   versioned too.
3. **Every edit is one undo step.** Project changes go through a named,
   tested `Document` method (`set_volume`, `move_tracks`, ...) that calls
   `self.history.apply(label, merge, edit)`. Drags pass a merge key so a
   whole gesture is one step. Never edit the project from UI code with an
   inline closure (`Document::apply` is test-only, for setting up a
   project).
4. **Disk work runs off the UI thread**: saves, opens, autosave, settings,
   media decoding, file dialogs. The UI polls a channel each frame.
5. **Errors are values, not panics.** `expect` and `unwrap` are only for
   real invariants (with the reason in the message). A failure the user can
   meet (a driver that won't start, a missing file) becomes a message with
   a way forward, such as `lumen_audio::Unavailable`.
6. **Third-party native code (plugins, drivers being probed) runs where its
   crash cannot take Lumen down**: a child process.
7. **Layering:** `lumen-core` at the bottom; `lumen-engine` depends only on
   core, rt and `lumen-devices` (real-time DSP on core alone, ADR-0045); `lumen-project` only on core; `lumen-audio`, `lumen-media`
   and `lumen-render` stand alone; only `lumen-app` joins them. No cycles,
   and no UI types below the app.
8. **The product name lives in `lumen_core::product`.** User-visible text
   that names it uses `PRODUCT_NAME`. Library crates use neutral wording.

## Structure

- **Modules stay small and single-purpose.** Past about 800 lines, or once
  a file mixes concerns, split it before adding more. In the app,
  `main.rs` only calls `lumen_app::run()`; `lib.rs` holds `App`, the frame
  loop and start-up; its behaviour lives in `app/` by concern (projects, recovery, history, preferences,
  view). The arrangement is `arrangement/{mod, headers, clips}`.
- **Shared controls live in the design system**, not in the feature that
  first needed them (`widgets.rs`, `icons.rs`); see the `lumen-design`
  skill.
- **Logic is a pure function with tests**, wrapped by the method that
  feeds it state (for example `lanes::engine_clips`). Translating the model
  into what the engine plays must be tested.
- **No long parameter lists:** use a small struct (`ClipLook`,
  `metronome::Window`) instead of `#[allow(clippy::too_many_arguments)]`.
- **No stale suppressions:** an `allow` carries a reason that is still
  true; dead code is deleted, not allowed.
- **Visibility is as narrow as works:** `pub(super)`, then `pub(crate)`.
- **Platform code** (`cfg(windows)` and others) must compile clean on every
  platform: code that is dead elsewhere fails `-D warnings` there.
- **Shaders are `.wgsl` files in `src/shaders/`**, checked at build time
  by `lumen-shader-check` from `build.rs`; never WGSL in a Rust string.
  Every uniform struct gets
  `const _: () = assert!(size_of::<T>() == shaders::NAME_UNIFORM);`.

## Gates

Before every commit:
- `cargo fmt --all`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace` (includes the E2E suite).

Before pushing:
- **Platform-specific code:** the WSL Linux mirror (`clippy.sh`), and the
  macOS cross-check, which catches macOS errors before a CI cycle:
  `cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings`.
- **Engine or project code:** the performance gate in WSL (`perf.sh`). It
  fails at +2% instructions. If a change is intended, update the baseline
  with `perf.sh --update` and record the reason in the ADR.
- Commands are in `docs/DEVELOPMENT.md`. Build in the repository's
  `target/`; never set `CARGO_TARGET_DIR`.

**At the end of every slice** (owner rule, ADR-0029), before the slice's
report and manual test:
- run the E2E suite, `cargo test -p lumen-app --test e2e`, and the real-binary
  smoke tests, `cargo test -p lumen-app --test smoke -- --ignored`. Both
  must pass; report the results.
- add E2E scenarios (`crates/lumen-app/tests/e2e/`) for every workflow the
  slice delivers, driven as a user would: by accessible names, keys and
  dropped files. A bug the owner finds gets a scenario that fails without
  the fix.
- tests live where Rust's convention puts them: unit tests of private
  logic inline (`#[cfg(test)] mod tests`); end-to-end and integration
  tests in the crate's `tests/` folder, using only the public API. If a
  test needs to see more, add a narrow, documented accessor to
  `lumen_app::testing`, never `pub` fields.
- run Lumen by hand only in isolation: `--audio test:fake --settings
  <dir>/settings.json --autosave-dir <dir>/autosave` (never the owner's
  settings, autosaves or audio device).

Tests that need hardware or a human (devices, plugins, look and feel) are
manual tests in `docs/MANUAL_TESTS.md` (MT-*). Add steps there, and record
each run's findings.

## Scope and records

- **Build only what the current slice plans.** If a request belongs to a
  later part or milestone, say where it is scheduled, and ask before
  building it.
- Significant choices get an ADR in `docs/DECISIONS.md`. Owner decisions
  go in its table.
- Every requirement's status is tracked in `docs/PROGRESS.md`.
- **Problems found and not fixed now go in `docs/KNOWN_ISSUES.md`**: what
  happens, its effect, and where it will be addressed. Check it before
  working in an area, so a known problem is not rediscovered or made
  worse. When one is fixed, remove it and record the fix in the relevant
  ADR or manual test run.
- At the end of each slice, keep `AGENTS.md` §12 and the
  `ARCHITECTURE.md` module tables current.
