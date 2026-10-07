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

**macOS CI is paused** (owner, 2026-10-07), so run this before pushing any
code that is compiled differently on macOS. Platform-specific code (the
cpal audio backend, for example) can be type-checked for macOS without a
Mac:

```bash
rustup target add aarch64-apple-darwin
cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
```

This checks types and lints only; nothing is linked or run. Linux can't be
checked this way because ALSA's build script needs the Linux system
headers. Use the WSL mirror instead (next section).

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

## Everyday commands

```bash
cargo test --workspace --all-targets     # tests, plus each benchmark run once
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo bench -p lumen-rt                  # real benchmark run (release profile)
```

## Test flags for files and recovery (M3 slice 3)

| Flag | What it does |
|---|---|
| `lumen <file>.lumen` | Opens a project |
| `--save-as <path>` | Saves once imports have loaded (as Save As would), then closes |
| `--autosave-dir <dir>` | Keeps autosaves in `<dir>` instead of the app data folder |
| `--autosave-after-ms <n>` | Autosaves after `n` ms instead of a minute |
| `--recover-latest` | Recovers the newest leftover autosave at start, as the banner's Recover button would |

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

CI has no GPU, so the arrangement's frame times are measured on the
development machine. Before closing a slice that changes the arrangement,
run:

```powershell
cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json"
cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json" --bench-size 3440x1380
```

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
