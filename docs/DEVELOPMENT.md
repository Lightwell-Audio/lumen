# Developing Lumen

## Toolchain

The compiler version is pinned in `rust-toolchain.toml`; rustup installs it
automatically on first build. Install rustup from <https://rustup.rs>.

### Windows — recommended (production target, MSVC; in use on the dev machine since 2026-10-06)

Build output on the dev machine goes to `%USERPROFILE%\lumen-target` (set `CARGO_TARGET_DIR`), outside `AppData` and off the nearly full D: drive.

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
the UI prototype's GPU stack. If the drive holding the repository is short of
space, put the build output elsewhere:

```bash
export CARGO_TARGET_DIR="$(cygpath -w "$USERPROFILE")\lumen-target"
```

(Not `%LOCALAPPDATA%`: see the `AppData` warning above.)

## Checking macOS code from Windows

Platform-specific code (the cpal audio backend, for example) can be
type-checked for macOS without a Mac:

```bash
rustup target add aarch64-apple-darwin
cargo clippy -p lumen-audio --all-targets --target aarch64-apple-darwin -- -D warnings
```

This checks types and lints only; nothing is linked or run. Linux can't be
checked this way because ALSA's build script needs the Linux system
headers, so CI is the Linux check.

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

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | Code intended for production |
| `prototypes/` | Milestone 1 measurement prototypes; deleted or rewritten once answered |
| `docs/` | Specification companions: progress, decisions, plans, manual tests |
