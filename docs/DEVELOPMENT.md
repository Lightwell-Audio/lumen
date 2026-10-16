# Developing Lumen

## Toolchain

The compiler version is pinned in `rust-toolchain.toml`; rustup installs it
automatically on first build. Install rustup from <https://rustup.rs>.

### Windows — recommended (production target, MSVC; in use on the dev machine since 2026-10-06)

Build output goes to the repository's own `target` folder (Cargo's default). Leave `CARGO_TARGET_DIR` unset, so everyone who builds on the machine (people and agents) shares one build. From 2026-10-05 to 2026-10-06 the agent built into `%USERPROFILE%\lumen-target` while the owner's shell built into `target`. Instructions started one copy while builds updated the other, which looked like features not working (MT-APP-3). That old folder can be deleted.

1. Install **Visual Studio 2022 Build Tools** with the *Desktop development
   with C++* workload (needs admin rights and acceptance of Microsoft's
   licence).
2. Make MSVC the default host and build:
   ```powershell
   rustup set default-host x86_64-pc-windows-msvc
   cargo test --workspace --all-targets
   ```

### Windows — temporary setup without admin rights (GNU)

This is the setup used on the development machine during Milestone 1 (see
ADR-0007). rustup's self-contained MinGW has `dlltool` but not the assembler it
needs, so crates using `raw-dylib` (`windows-sys` 0.61+) fail to build. The
fix is MSYS2's `binutils` plus its runtime DLLs, from
<https://repo.msys2.org/mingw/mingw64/>:

| Package | SHA-256 (first 16 hex digits) |
|---|---|
| `mingw-w64-x86_64-binutils-2.47-3-any.pkg.tar.zst` | `827363748CE33206` |
| `mingw-w64-x86_64-zlib-1.3.2-2-any.pkg.tar.zst` | `9E75842A070BA648` |
| `mingw-w64-x86_64-zstd-1.5.7-2-any.pkg.tar.zst` | `1ADD6705B344664F` |
| `mingw-w64-x86_64-gettext-runtime-1.0-1-any.pkg.tar.zst` | `BE68D7F260633284` |
| `mingw-w64-x86_64-libiconv-1.19-1-any.pkg.tar.zst` | `21E334D0911F25DE` |
| `mingw-w64-x86_64-libwinpthread-14.0.0.r98.g19f5121a2-1-any.pkg.tar.zst` | `565A7AC155098633` |
| `mingw-w64-x86_64-crt-git-13.0.0.r91.gfc0b67305-1-any.pkg.tar.zst` (Windows import libraries such as `libshlwapi.a` and `libopengl32.a`, needed by the UI stack) | `C613A34E44ABF31C` |

Extract them all into `%LOCALAPPDATA%\lumen-devtools` with Windows' built-in
`tar -xf <file>`. Then put the tools on `PATH` before building:

```bash
export PATH="$HOME/.cargo/bin:$(cygpath -u "$LOCALAPPDATA")/lumen-devtools/mingw64/bin:$PATH"
export LIBRARY_PATH="$(cygpath -w "$LOCALAPPDATA")\lumen-devtools\mingw64\lib"
```

`LIBRARY_PATH` is read by the GCC linker driver, so changing it does not
force a rebuild.

To undo: delete `%LOCALAPPDATA%\lumen-devtools`. Rust itself is removed with
`rustup self uninstall`.

## Warning: `AppData` redirection when building from the Claude desktop app

The Claude desktop app on Windows is a packaged (MSIX) app. Anything its
processes write under `%LOCALAPPDATA%` is redirected to
`%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\`, and other
programs, including your own terminal, cannot see it at the normal path.
During M1, `lumen-devtools` and `lumen-target` ended up in that redirected
location. Use folders outside `AppData` (for example `%USERPROFILE%\...`)
for anything that both the agent and you need to use. Prototype binaries for
manual testing are in `%USERPROFILE%\lumen-bin`.

## Build output size

A full debug plus release build of the workspace takes about 4.6 GB, mostly
the UI prototype's GPU stack. Only if the drive holding the repository is
short of space, move the build output, and then set the variable the same
way for every shell and agent on the machine (never `%LOCALAPPDATA%`; see
the `AppData` warning above):

```bash
export CARGO_TARGET_DIR="$(cygpath -w "$USERPROFILE")\lumen-target"
```

## Checking macOS code from Windows

CI tests macOS (Apple silicon) on every pull request. A CI cycle is slow,
so run this before pushing any code that is compiled differently on macOS.
Platform-specific code (the cpal audio backend, for example) can be
type-checked for macOS without a Mac:

```bash
rustup target add aarch64-apple-darwin
cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
```

This checks types and lints only; nothing is linked or run. Linux can't be
checked this way because ALSA's build script needs the Linux system
headers. Use the WSL mirror instead (next section).

## Checking Windows code from Linux or macOS

Code that depends on the width of a foreign type can compile on one system
and not on another: the VST3 bindings' enum constants are `i32` on Windows
and `u32` elsewhere, which broke the Windows CI job once (M6 slice 0). A
crate with no system build scripts can be type-checked for Windows from any
system:

```bash
rustup target add x86_64-pc-windows-msvc
cargo clippy -p lumen-test-plugin --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

This checks types and lints only; nothing is linked or run. It was verified
for `lumen-test-plugin` alone, not for the whole workspace.

## Checking Linux in WSL

An Ubuntu 26.04 WSL distribution on the dev machine mirrors the Linux CI,
with the same system packages. Its clone (`/root/lumen`) pulls from this
repository, so **commit locally first**. The scripts live in
`C:\Users\<you>\lumen-wsl\` (see its README):

| Script | Runs |
|---|---|
| `clippy.sh` | CI clippy and tests on Linux |
| `perf.sh [--update]` | The performance gate under Valgrind (ADR-0022) |
| `deny.sh` | `cargo-deny`: advisories, bans, licences, sources |
| `ci-rtsan.sh`, `canary.sh` | RealtimeSanitizer tests and canaries |

```powershell
$env:WSL_UTF8 = "1"; wsl.exe -d Ubuntu -u root -- bash "/mnt/c/Users/<you>/lumen-wsl/clippy.sh"
```

The scripts reset the clone to `origin/main`. To check a branch, commit
it first (the clone fetches this repository), then name it with `env LUMEN_REF=<branch>`; without it
they test `main` and pass whatever the branch holds:

```powershell
$env:WSL_UTF8 = "1"; wsl.exe -d Ubuntu -u root -- env LUMEN_REF=m4-review bash "/mnt/c/Users/<you>/lumen-wsl/perf.sh"
```

Run `clippy.sh` before pushing any platform-specific (`cfg(windows)`)
code: Windows-only code is dead code elsewhere, and `-D warnings` fails on
it.

## Close Lumen before rebuilding it

`cargo build` cannot replace `lumen.exe` while the app is running. It
prints `failed to remove file …\lumen.exe` and leaves the old build in
place, which is easy to miss. During MT-APP-2 that made a fixed bug look
unfixed, and again in MT-APP-3. Close the app first. Better, build and
start in one step with `cargo run`, which stops on a failed build instead of
starting the old one:

```bash
cargo run --release -p lumen-app -- --audio asio:"Focusrite USB ASIO"
```

## Measuring UI latency

Set `LUMEN_TRACE=1` to log timestamped frames, mouse and key input, and
transport commands to stderr. `--autoplay-ms N` toggles play and stop
every N ms, as Space would, for timing the engine and redraw path without a
mouse:

```bash
LUMEN_TRACE=1 lumen --autoplay-ms 3000
```

On this machine (2026-10-06), every play and stop reaches the screen 14–19
ms after the command (28 toggles each on WASAPI and on ASIO).

## Trying delay compensation (M4 slice 6)

Nothing adds latency until devices arrive (M6). `--test-latency <ms>`
puts the engine's synthetic latent device (a pure delay) on the first
track, so delay compensation and its readout can be tried by hand
(MT-MIX-6). Tests do the same with `testing::set_test_latency`.

```powershell
cargo run --release -p lumen-app -- --test-latency 50
```

## Branches

One small branch per slice, named `m<milestone>-s<slice>-<topic>`
(`m6-s1-plugin-scan`); unrelated fixes and chores use `fix/<topic>`,
`chore/<topic>` or `docs/<topic>`, cut from `main`. The full rule is the
`lumen-branching` skill (`.claude/skills/lumen-branching/SKILL.md`).

## Everyday commands

```bash
cargo test --workspace --all-targets     # tests, plus each benchmark run once
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo bench -p lumen-rt                  # real benchmark run (release profile)
```

### Running the tests

CI runs the tests with `cargo nextest` (`cargo install cargo-nextest --locked`,
version pinned in `.github/workflows/ci.yml`); `cargo test` runs the same
tests and stays supported. The profiles are in `.config/nextest.toml`:

```bash
# Everything except the end-to-end suite, as many tests at once as cores:
cargo nextest run --workspace --all-targets --profile ci -E 'not binary(e2e)'
# The end-to-end suite, 12 tests at once:
cargo nextest run --workspace --all-targets --profile e2e -E 'binary(e2e)'
```

Why two steps: the end-to-end suite (about 340 tests) takes 200 s or more on a
CI runner and spends most of that waiting on the paced test audio device
(ADR-0063), not on the CPU, so it runs more tests at once than there are
cores. Measured on 4 cores: 157 s at 4 tests at once, 108 s at 8, 93 s at 16.
The other tests include timing checks (the MIDI sender's lateness bounds, the
real-time and record tests) that a busy machine can disturb, so they keep one
test per core. nextest also runs each test in its own process, which is
stricter than `cargo test` (a test cannot depend on state another test left
in the process). It does not run doc tests; no crate has any, so CI has no
doc-test step. Add `cargo test --workspace --doc --locked` back to the
workflow when one is written.

## The test plugins (M6, ADR-0065)

`crates/lumen-test-plugin` builds a VST3 module with a deterministic
instrument and effect. `cargo test -p lumen-test-plugin` runs everything,
including the crash and hang tests, which re-run the test binary as a child
and check how it ended. To make a host or the scanner meet a fault, set
`LUMEN_TEST_PLUGIN_FAULT` in the environment of the process that loads the
module, for example `process_crash@100` or `process_slow=3000,state_fail`.
The list of faults is at the top of `crates/lumen-test-plugin/src/faults.rs`.
A malformed value injects nothing and prints a message on stderr.

## The plugin host (M6, ADR-0066)

`cargo test -p lumen-plugin-host` builds the test plugin's shared library
(it runs `cargo build -p lumen-test-plugin` once, with the same profile),
then loads it through the operating system's loader, in process and in real
`lumen-plugin-host` child processes, with each fault in
`LUMEN_TEST_PLUGIN_FAULT`. The orphan test (Linux) kills a stand-in for
Lumen and checks that a host hung inside a plugin dies with it.
Type-check for the other systems with
`cargo clippy -p lumen-plugin-host --all-targets --target x86_64-pc-windows-gnu`
and `--target aarch64-apple-darwin` (`rustup target add` first); those
targets are not run here.

## Plugin discovery (M6 slice 1, ADR-0068)

Plugins are found by `lumen-plugin-host scan <path>`, one process per module,
which Lumen looks for **beside its own executable**. `cargo run -p lumen-app`
builds only the app, so build the scan program too, or the browser says it
was not found:

```powershell
cargo build --release -p lumen-plugin-host
cargo run --release -p lumen-app
```

The scan's answers are kept in `plugin-cache.json` next to `settings.json`.
Delete it to see a cold scan. Flags for runs and screenshots that must not
touch the machine's plugins: `--plugins off`, or `--plugin-roots <folder;folder>`
with `--plugin-host <program>` and `--plugin-cache <file>`. A screenshot with
a generated list needs only a cache file and `scan_at_startup: false` in the
settings file.

`cargo test -p lumen-plugin-catalog` copies the test plugin into folders as
bundles named for the fault each should have (a copy named `Crash` crashes,
`Hang` hangs) and scans them with the real host program. The end-to-end
scenarios in `tests/e2e/plugins.rs` do the same through the app.

## Linux cloud containers

A Linux cloud container needs system packages before the first build. The app
needs `libasound2-dev` and `libudev-dev` to build; install them with
`pkg-config`:

```bash
sudo apt-get install libasound2-dev libudev-dev pkg-config
```

The GPU tests and the screenshot tests also need a software Vulkan device,
which is `mesa-vulkan-drivers` (lavapipe; CI's Linux job installs it too).
The smoke tests need `xvfb` and `libxkbcommon-x11-0` as well, and run under
`xvfb-run -a`. The first `cargo test -p lumen-app --test e2e` takes minutes to
build.

## End-to-end tests (ADR-0029)

Run both **at the end of every slice**, and add scenarios for the
workflows the slice delivers.

```powershell
cargo test -p lumen-app --test e2e                            # the E2E suite (2 to 4 minutes; also in CI, see "Running the tests")
cargo test -p lumen-app --test smoke -- --ignored             # the real binary, with its window and GPU
```

- **The E2E suite** (`crates/lumen-app/tests/e2e/`) starts the whole app
  with egui's test harness, without a window, and drives it by accessible
  names, keys and dropped files. Use the `Lumen` helper: `start`, `click`,
  `key`, `drop_files`, `wait_for`, `shows`, `is_on`. Wait on state (a
  position, a flag), never on time; `wait_for` gives up after 30 s and
  says what it waited for. A control a test needs must have an accessible
  name (CLAUDE.md §5, item 10).
- **Listening:** the test device keeps what it plays. `app.heard()`
  returns the frames played since the last call (the fake device runs at
  about a quarter of real time), so a scenario can check for clicks, as
  `dragging_a_playing_clip_does_not_click` does. To hear a level, use
  `lumen.listen(settle, frames)`: it waits for the frames, not for a
  time (a busy CI machine played 768 frames in 150 ms).
- **The smoke tests** (`crates/lumen-app/tests/smoke.rs`) run `lumen`
  itself: start and draw, import, save, reopen with a screenshot that must
  not be blank, the quality-bar project, and the light theme at 200%. A
  window flashes up for each. They build the debug binary, so a release
  Lumen can stay open.
- **Isolation:** both use the silent test device, their own settings file
  and their own autosave folder. Do the same when running Lumen by hand
  for screenshots:
  `target/debug/lumen.exe --audio test:fake --settings <dir>/settings.json --autosave-dir <dir>/autosave --crash-dir <dir>/crashes ...`
- **Crash reports:** a panic writes `crash-<time>.txt` (what, where,
  backtrace, version, system, thread) to `%LOCALAPPDATA%\Lumen\Crashes`
  (or `--crash-dir`), and the next start offers it. A failed allocation is
  not a panic: the binary's allocator (`lumen_rt::ReportingAllocator`)
  reports it first. `--crash-test` (a panic) and `--crash-test-memory` (a
  failed allocation) check both paths; the smoke tests do.
- **Hovers, drags and selections by eye** (`crates/lumen-app/tests/shots.rs`):
  states a `--screenshot` cannot reach, rendered headless with the GPU
  (clips, waveforms and grid included) to PNG files. Ignored in CI; add a
  function for each new state worth checking.
  `$env:SHOTS = "<folder>"; cargo test -p lumen-app --test shots -- --ignored --test-threads=1`
  (one at a time: the theme is process-wide, so parallel shots mix themes)

## Test flags for files and recovery (M3 slice 3)

| Flag | What it does |
|---|---|
| `lumen <file>.lumen` | Opens a project |
| `--save-as <path>` | Saves once imports have loaded (as Save As would), then closes |
| `--autosave-dir <dir>` | Keeps autosaves in `<dir>` instead of the app data folder |
| `--autosave-after-ms <n>` | Autosaves after `n` ms instead of a minute |
| `--recover-latest` | Recovers the newest leftover autosave at start, as the banner's Recover button would |
| `--settings <file>` | Reads and saves settings in `<file>` instead of the user's (tests) |
| `--audio test:fake` | The silent test device: runs the engine without sound hardware (tests) |

Example: import, save into a project folder, then reopen it.

```powershell
cargo run -p lumen-app -- --open-audio "C:\Windows\Media\Windows Notify System Generic.wav" --save-as "$env:TEMP\Song.lumen"
cargo run -p lumen-app -- "$env:TEMP\Song\Song.lumen"
```

## Performance gate (ADR-0022)

CI fails when a scenario in `crates/lumen-perf` executes more than 2% more
instructions than `perf/baseline.json`. To run it before pushing, or to
update the baseline after an intended change, use the WSL mirror (Linux and
Valgrind):

```powershell
$env:WSL_UTF8 = "1"; wsl.exe -d Ubuntu -u root -- bash "/mnt/c/Users/Claire Li/lumen-wsl/perf.sh"
$env:WSL_UTF8 = "1"; wsl.exe -d Ubuntu -u root -- bash "/mnt/c/Users/Claire Li/lumen-wsl/perf.sh" --update
```

The script runs the latest local commit. After `--update`, copy
`/root/lumen/perf/baseline.json` into the repository and commit it with the
reason. A new scenario is a `measured_<name>` function in
`crates/lumen-perf/src/lib.rs`, marked `#[unsafe(no_mangle)]` and
`#[inline(never)]`, plus an entry in `SCENARIOS`.

## Shaders

WGSL shaders live in `.wgsl` files under a crate's `src/shaders/`
(`lumen-render`, and the `ui-stress` prototype). They are checked when the
crate builds:

- `build.rs` calls `lumen_shader_check::check("src/shaders")`, which parses
  and validates each file with naga (the compiler wgpu uses). An error
  fails `cargo build` with naga's message and the file and line.
- It generates a `shaders` module: `BOXES` (the source of `boxes.wgsl`)
  and `BOXES_UNIFORM` (the size of its uniform block). Each uniform struct
  in Rust has a `const` assertion against that size, so a layout mismatch
  (a `vec3` aligns to 16 bytes) is a compile error.

To add a shader: put `name.wgsl` in `src/shaders/`, use `shaders::NAME`,
and assert the size of its uniform struct against `shaders::NAME_UNIFORM`.
`naga` is pinned to the version wgpu uses; upgrade them together.

## Frame-time benchmark (ADR-0026)

CI has no GPU (the Linux job has lavapipe, a software Vulkan device, for
GPU unit tests only), so the arrangement's frame times are measured on the
development machine. Before closing a slice that changes the arrangement,
run:

```powershell
cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json"
cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json" --bench-size 3440x1380
```

Add `--bench-editor` to measure with the stress project's 100,000-note clip
open in the clip editor (M5 slice 3): the editor follows the scroll and
zoom, and its pitches scroll with the tracks.

It opens the generated quality-bar project (200 tracks, 20,000 clips) with
vsync off, scrolls and zooms it, and reports p50, p95, p99 and the worst
frame per phase, then the repaints during 3 s idle and what caused them.
It exits with status 3 if any phase misses 60 fps at p99. Record the
results in the slice's ADR or report when they change.

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | Production code (see `docs/ARCHITECTURE.md`) |
| `crates/lumen-app/assets/fonts/` | Inter and Unbounded, with their licence texts |
| `perf/baseline.json` | Instruction-count baseline for the performance gate |
| `prototypes/` | Milestone 1 measurement prototypes; deleted or rewritten once answered |
| `docs/` | Specification companions: progress, decisions, plans, manual tests |
