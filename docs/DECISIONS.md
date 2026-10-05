# Lumen — Architecture Decision Records

Every significant technical decision is recorded here with its context,
options considered, decision, consequences, evidence and status. An existing
decision is never silently reversed: a new record supersedes it.

**Statuses:** *Proposed* (recommended, awaiting evidence or approval) ·
*Accepted* · *Needs human decision* (licensing, legal, money, or anything else
irreversible or external) · *Superseded*.

## Index

| ADR | Title | Status | Approval needed |
|---|---|---|---|
| [0001](#adr-0001--system-architecture) | System architecture: processes, threads, data flow | Proposed | Yes, at end of M1 |
| [0002](#adr-0002--ui-framework) | UI framework | Proposed (pending P-UI) | Yes |
| [0003](#adr-0003--real-time-audio-engine) | Real-time audio engine | Proposed (pending P-ENGINE) | Yes |
| [0004](#adr-0004--project-file-format) | Project file format | Proposed (pending P-FORMAT) | Yes |
| [0005](#adr-0005--vst3-hosting-and-plugin-isolation) | VST3 hosting and plugin isolation | Proposed (pending P-VST3, P-ISO) | Yes |
| [0006](#adr-0006--audio-device-backends-and-asio) | Audio device backends and ASIO | Proposed; **ASIO licensing needs human decision** | Yes |
| [0007](#adr-0007--toolchain-and-build-targets) | Toolchain and build targets | Accepted (local workaround pending VS Build Tools) | Install is your call |
| [0008](#adr-0008--product-name-defined-once) | Product name defined once | Accepted | No |
| [0009](#adr-0009--time-base) | Time base: integer ticks | Proposed (pending P-FORMAT) | Yes |
| [0010](#adr-0010--real-time-safety-is-enforced-by-tests) | Real-time safety enforced by tests | Accepted | No |
| [0011](#adr-0011--benchmarks-and-performance-gates) | Benchmarks and performance gates | Accepted | No |
| [0012](#adr-0012--lumens-own-license) | Lumen's own license | **Needs human decision** | Yes |
| [0013](#adr-0013--dependency-license-policy) | Dependency license policy | Accepted | No |
| [0014](#adr-0014--repository-layout-and-ci) | Repository layout and CI | Accepted (assumption: GitHub) | No |

---

## ADR-0001 — System architecture

**Status:** Proposed · **Date:** 2026-10-05

### Context
Priorities, in order: never lose work and never interrupt audio; UX quality and
responsiveness; performance; completeness. Plugins are third-party native code
that may crash or hang; 1.0 requires separate-process isolation.

### Options considered
1. **Single process**, plugins in-process with exception guards. Simplest and
   lowest latency, but one bad plugin kills the session. Fails the 1.0
   requirement.
2. **Main process plus sandboxed plugin host processes** (as in Bitwig).
   Isolates plugins. Costs IPC per audio block and makes editor windows
   harder.
3. **Engine in its own process, separate from the UI.** Protects audio from UI
   stalls but doubles the IPC surface, and the UI needs low-latency access to
   meters and positions. Real-time-safe design inside one process gives the
   same protection more cheaply.

### Decision (proposed)
Option 2.
- **`lumen` (main process):** UI thread; non-real-time engine control threads
  (graph compiler, disk I/O, autosave, scanning coordinator); the audio driver
  thread, which runs the engine directly with no extra buffering; a real-time
  worker pool for parallel graph processing; disk streaming and recording
  writer threads.
- **`lumen-plugin-host` processes:** load plugins, run their processing on
  their own real-time thread, and own their editor windows. They talk to the
  main process over shared memory (audio, events, parameters) plus a control
  channel.
- **Scanning** always happens in short-lived child processes.
- **Data flow:** the document model (project) lives on the UI side as an
  immutable, structurally shared tree. Edits are commands that produce new
  versions, which makes undo and autosave snapshots cheap and thread-safe. The
  engine never reads the document. It receives compiled, flat,
  real-time-friendly structures through lock-free queues and reports back
  (meters, positions) through atomics and lock-free queues.

### Consequences
- The transport and fan-out cost of plugin IPC must be measured (P-ISO) before
  this is accepted.
- Plugin editor windows live in another process. Focus, z-order and DPI must
  be designed (M6, risk R12).

### Evidence
Pending P-ISO and P-ENGINE.

---

## ADR-0002 — UI framework

**Status:** Proposed (pending P-UI) · **Date:** 2026-10-05

### Context
The arrangement and piano roll must hold 60 fps minimum and 120 fps target with
200 tracks, 20,000 clips and 100,000 notes. Other requirements: near-zero idle
CPU; window usable within 2 s; high-DPI and 100–200% scaling; light and dark
themes; keyboard navigation; accessible names; multiple windows; drag and drop
from the OS; a polished custom look. Nearly every DAW-specific view (clips,
waveforms, notes, envelopes, meters, knobs) is custom-painted whatever the
framework, so the framework mostly supplies windowing, input, text, layout,
focus and accessibility.

### Options considered (state as of October 2026)
| Option | Strengths | Weaknesses |
|---|---|---|
| **egui / eframe 0.36** (wgpu, MIT/Apache) | AccessKit on by default (UI Automation on Windows); custom painting is first-class; repaints only on demand (idle ≈ 0); multiple native windows; mature, very widely used | Immediate mode needs disciplined culling and caching for dense views; default look must be fully replaced; single-pass layout quirks; frequent breaking releases (pin and upgrade deliberately); text quality at 100% scale needs a visual check |
| **iced** (MIT) | Elm architecture suits command-based undo; good text (cosmic-text); used by COSMIC | No shipped screen-reader support found; dense canvases re-tessellate on change; smaller ecosystem |
| **Slint** (GPLv3 / royalty-free with attribution / commercial) | Polished declarative UI; accessibility | Dense custom drawing (20,000 clips) is awkward; licence terms need a human decision |
| **GPUI** (Zed, Apache-2.0) | Built for 120 fps editor workloads; has a native action/keybinding system | Not supported as a general-purpose library (crates.io 0.2.2, October 2025); no documented screen-reader support |
| **Vizia** (MIT) | Built for audio UIs; AccessKit | Small community; performance on huge canvases unproven |
| **Xilem / Masonry** | Modern architecture, Vello renderer, AccessKit | Still alpha |
| **Web UI** (Tauri / WebView2) | Best text, accessibility and polish tooling | Two languages; marshalling cost for dense data and meters; WebView memory and startup; plugin windows still native |
| **Custom toolkit** (winit + wgpu + Vello + Parley + AccessKit) | Full control | Very high cost; reinvents focus, text input, IME and accessibility |

### Decision (proposed)
**egui/eframe with the wgpu backend**, under a strict application structure:
- A framework-independent core (document model, commands, undo) so the UI
  layer stays replaceable.
- Lumen's own design system (tokens, widgets, icons) painted on egui's
  primitives. egui's default look is not used.
- Dense views use viewport culling, level-of-detail aggregation and caching.
  The escape hatch, if measurements require it, is a custom wgpu renderer
  inside an egui paint callback.

### Evidence
Pending P-UI.

---

## ADR-0003 — Real-time audio engine

**Status:** Proposed (pending P-ENGINE) · **Date:** 2026-10-05

### Context
No dropouts at 64–128 samples. A 64-track project must stay under 50% of the
128-sample / 48 kHz budget (2.67 ms). Sample-accurate timing. Offline render
must equal real-time playback and be repeatable. Real-time correctness is
enforced by tests.

### Options considered
1. Process the whole graph on the driver thread, single-threaded. Simple, but
   leaves cores idle once plugins are heavy.
2. **Driver thread plus a real-time worker pool** processing independent graph
   nodes in parallel (MMCSS "Pro Audio" on Windows).
3. Anticipative pre-rendering of non-live tracks with extra buffering (as in
   Reaper). Big CPU wins later, but complex. The architecture should allow it,
   not require it.

### Decision (proposed)
Option 2, with room for option 3 later:
- Graphs are compiled on a non-real-time thread into flat schedules with
  preassigned buffers, handed to the audio thread through a lock-free queue,
  and returned to a non-real-time thread to be freed. The audio thread never
  allocates or frees.
- Commands flow from UI to engine through single-producer/single-consumer ring
  buffers with preallocated messages. Feedback flows back through atomics and
  ring buffers.
- Built-in devices are sample-accurate: blocks are split at event boundaries.
  VST3 receives sample offsets through its parameter-change and event lists.
- Determinism: the engine is block-size invariant, never reads the wall clock
  during rendering, and sets denormal flushing identically on real-time and
  offline threads. Offline render uses the same code path.

### Evidence — P-ENGINE (`prototypes/engine-core`), 2026-10-05
All pass criteria met.
- **No heap traffic:** 2,000 blocks of 64 tracks, including 20 graph swaps, a
  seek and 200 live commands, made 0 heap operations on the audio side
  (`lumen-rt` guard). Retired graphs are freed on the handle side.
- **Block-size invariance:** 3 s of 16 tracks with automation points falling
  mid-block rendered bit-identically at block sizes 1, 3, 64, 128, 333, 512,
  4096 and 10,000 (larger blocks are chunked at 4,096).
- **State-preserving swaps:** swapping in an unchanged graph every 37 blocks is
  bit-identical to never swapping.
- **Cost** (release build, Ryzen 7 7700X, single thread, divan median):

  | Tracks × frames | Median | Slowest | Share of budget (48 kHz) |
  |---|---|---|---|
  | 64 × 64 | 9.1 µs | 19.6 µs | 0.68% of 1.33 ms |
  | 64 × 128 | 22.7 µs | 38.7 µs | 0.85% of 2.67 ms |
  | 256 × 128 | 72.5 µs | 146.6 µs | 2.7% of 2.67 ms |

  Each track reads a source buffer and runs a biquad, gain smoothing and
  constant-power pan. This is the engine's own overhead; real projects add
  plugins and disk streaming on top.

Not covered: worker-pool parallelism, denormal flushing, disk streaming,
transport and loops. Those land in M2.

---

## ADR-0004 — Project file format

**Status:** Proposed (pending P-FORMAT) · **Date:** 2026-10-05

### Context
The format must be human-friendly and versioned, open older projects forever,
use relative paths, support autosave and recovery with safe atomic saves and
backups, and hold plugin state (opaque binary, sometimes megabytes).

### Options considered
| Option | Notes |
|---|---|
| **JSON** (serde_json) | Readable, diffable, universal tooling, fast. Binary blobs need base64. |
| RON | Rust-native and readable; little tooling outside Rust. |
| XML (gzipped, as in Ableton .als) | Verbose; awkward in Rust; gzip defeats "human-friendly". |
| SQLite | Transactional and incremental, but not human-readable. Schema migrations are well understood. |
| Binary (bincode, MessagePack) | Fast and compact; opaque; brittle to evolve. |

### Decision (proposed)
- A **project folder** (like Ableton's) containing `<name>.lumen`, a UTF-8,
  pretty-printed JSON document, plus `Samples/`, `Backups/` and
  `Recordings/`. Media paths are stored relative to the project folder, with
  the absolute path and a fingerprint kept as relink hints.
- An envelope `{ "format": "lumen-project", "version": N, ... }`. Loading
  parses into untyped JSON, runs the chain of pure migrations vN→vN+1, then
  deserializes the current typed model. Every historical version keeps a
  golden fixture that is never deleted. Files from a newer version are refused
  with a clear message.
- Atomic save: write to a temporary file in the same folder, flush and fsync,
  rotate backups, then replace the original by rename.
- Plugin state is stored inline as base64. Compression and sidecar files will
  be revisited if size becomes a problem.

### Evidence
Pending P-FORMAT.

---

## ADR-0005 — VST3 hosting and plugin isolation

**Status:** Proposed (pending P-VST3, P-ISO) · **Date:** 2026-10-05

### Context
VST3 instruments and effects with multi-out, sidechain, latency, tails,
state, presets, every parameter automatable, and editors in floating windows.
Crashing, hanging or misbehaving plugins must be isolated, flagged and
recoverable. Strong (separate-process) isolation is required by 1.0. Scanning
must never crash or block the app.

### Options considered — bindings
| Option | Notes |
|---|---|
| **`vst3` crate 0.3** (coupler-rs; MIT OR Apache-2.0) | Rust bindings generated from the official headers, plus COM helpers. Pure Rust. The SDK has been MIT-licensed since VST 3.8 (October 2025). |
| Official C++ SDK through a C++ shim (cxx or bindgen) | Provides the SDK's host helper classes, but needs MSVC and CMake in every build. |
| `vst3-sys` | Derived from the formerly GPLv3 SDK; stale. Rejected. |
| Hand-written bindings | No reason to now that the SDK is MIT. Rejected. |

### Options considered — isolation
| Option | Notes |
|---|---|
| In-process with crash guards | Fails the 1.0 requirement on its own; a corrupted heap cannot be recovered. |
| **Sandbox host processes**: shared-memory audio, OS-event signalling, per-block deadlines | Isolation holds; the cost is IPC per block per process (measured by P-ISO). |
| Grouping policy: one process per instance, per plugin type, per vendor, or one shared sandbox | Trades memory and process count against fault isolation. To be set from P-ISO numbers. |

### Decision (proposed)
- Bindings: the `vst3` crate, with Lumen's own host classes.
- Isolation: plugins run in `lumen-plugin-host` processes. The audio thread
  signals each host per block and waits only until a deadline. A host that
  misses its deadline or dies is replaced by silence, flagged in the UI, and
  restarted off the audio thread with its last known state.
- Scanning: always out of process, with a timeout. `moduleinfo.json` is a
  fast path where present. Results are cached by path, size and modification
  time, with a blocklist that records reasons.
- Editors: created by the plugin host as its own top-level windows. Child
  windows parented across processes are avoided, because they attach the two
  processes' input queues, so a hung plugin could freeze Lumen's UI.
  Validation in M6.

### Open licensing point
The MIT licence covers the SDK code, not the "VST" trademark or logo. Using
the VST name or logo must follow Steinberg's trademark rules (to be confirmed
before release).

### Evidence
Pending P-VST3 and P-ISO.

---

## ADR-0006 — Audio device backends and ASIO

**Status:** Proposed; **ASIO licensing needs human decision** · **Date:** 2026-10-05

### Context
The device selector needs: driver types (ASIO and WASAPI on Windows; CoreAudio
on macOS; PipeWire, JACK and ALSA on Linux); channel names; input, output and
round-trip latency; rates and buffer sizes; the driver control panel; reset
and rate-change handling; clear busy and missing-driver messages; and graceful
fallback.

### Options considered
| Option | Notes |
|---|---|
| **cpal 0.18** for everything | Cross-platform. Its ASIO support needs the Steinberg SDK at build time (the build script downloads it, sidestepping the licence step), LLVM/libclang for bindgen, and Visual Studio. Its API lacks channel names, latency reporting, the control panel and reset handling. |
| PortAudio or RtAudio through FFI | C/C++ builds; ASIO still needs the SDK; same API gaps. |
| **Lumen's own backend layer** | Lumen-defined traits sized to the device-selector spec. Windows: a native ASIO host over COM written in Rust (no C++), plus WASAPI. macOS and Linux: native backends, or cpal where it is enough. |

### Decision (proposed)
Lumen's own backend layer. On Windows, a native ASIO host in Rust. Driver
enumeration and opening run first in a sandboxed child process, because some
drivers block on dialogs or crash when hardware is missing. The WASAPI,
CoreAudio and Linux implementations are chosen in M2.

### ASIO licensing — needs human decision
Steinberg's ASIO SDK is dual-licensed: a **proprietary licence** (free, but an
agreement must be signed with Steinberg) or **GPLv3** (October 2025). Under
either, using the "ASIO" name or logo must follow Steinberg's trademark rules.
The choices for shipping:
1. **Sign the free proprietary ASIO SDK agreement** (recommended if Lumen is
   closed-source or permissively licensed). The native Rust host is written
   against the SDK's interface definitions under that agreement.
2. Use the **GPLv3 option**. Only possible if Lumen itself is GPLv3 (ADR-0012).
3. Ship **clean-room interface definitions without the SDK**. Legally plausible
   (interoperability) but needs legal review; trademark rules still apply.

The M1 prototype (`prototypes/asio-probe`) uses clean-room definitions, is not
distributed, and does not pre-empt this decision.

### Evidence
Pending P-ASIO.

---

## ADR-0007 — Toolchain and build targets

**Status:** Accepted (local workaround pending VS Build Tools) · **Date:** 2026-10-05

### Decision
- Rust stable, pinned in `rust-toolchain.toml` (1.99.0), edition 2024.
- **Production Windows target: `x86_64-pc-windows-msvc`.** Reasons: PDB
  debugging and crash dumps, the same ABI ecosystem as plugins and drivers,
  and GitHub's Windows runners use it by default.
- **Local development on this machine for now: `x86_64-pc-windows-gnu`.** No
  MSVC was installed, and installing Visual Studio Build Tools needs admin
  rights and acceptance of Microsoft's licence, which is the owner's call. The
  self-contained MinGW in rustup lacks the GNU assembler that `dlltool` needs
  for `raw-dylib` crates (`windows-sys` 0.61+). The workaround is MSYS2's
  `binutils` 2.47 and its runtime DLLs, unpacked to
  `%LOCALAPPDATA%\lumen-devtools\mingw64\bin` and put on `PATH`.
  See `docs/DEVELOPMENT.md`.

### Consequences
- Local builds and CI use different targets until VS Build Tools is
  installed. CI is authoritative.
- **Action for the owner:** install Visual Studio 2022 Build Tools with the
  "Desktop development with C++" workload, then switch the local default to
  MSVC (steps in `docs/DEVELOPMENT.md`).

---

## ADR-0008 — Product name defined once

**Status:** Accepted · **Date:** 2026-10-05

User-visible naming comes only from `lumen_core::product` (`PRODUCT_NAME`,
`PROJECT_EXTENSION`). Crate names (`lumen-*`) are internal identifiers. If the
product is renamed, old project extensions must still open.

---

## ADR-0009 — Time base

**Status:** Proposed (pending P-FORMAT) · **Date:** 2026-10-05

### Context
Positions must be sample-accurate. Grid values (whole note down to very fine
divisions, triplets, dotted notes) must be exact, and split followed by join
must return the original exactly. Tempo runs from 20 to 999 BPM, sample rates
up to 192 kHz.

### Options considered
1. Floating-point beats (`f64`), as in Ableton and Reaper. Precise enough, but
   not exact: tuplets like 1/3 are inexact and arithmetic is not associative.
2. Integer ticks at a conventional resolution (480 or 960 PPQ). Exact for the
   grid, but coarser than one sample at fast tempos and high rates.
3. **Integer ticks at a highly composite resolution: 705,600 PPQ**
   (2⁶·3²·5²·7²) in `i64`.

### Decision (proposed)
Option 3. One quarter note is 705,600 ticks, so every grid value down to 1/64
notes, with triplets, quintuplets, septuplets and dotted variants, is an exact
integer. At 999 BPM and 192 kHz one sample is about 61 ticks, so tick
positions resolve below one sample. `i64` covers about 1.3·10¹³ beats.
Conversion to samples goes through the tempo map with a single, documented
rounding rule, so a boundary shared by two clips always maps to the same
sample. Unwarped audio keeps its start in ticks and its length in samples.

### Evidence
Pending P-FORMAT time-base tests.

---

## ADR-0010 — Real-time safety is enforced by tests

**Status:** Accepted · **Date:** 2026-10-05

### Decision
- `lumen-rt` provides `GuardedAllocator`, a counting wrapper around the system
  allocator, and `rt_section`. Test binaries install the guard, run real-time
  code inside sections, and assert zero heap operations. A missing guard fails
  loudly (`guard_is_active`).
- Planned: RealtimeSanitizer (`rtsan-standalone`, Apache-2.0) on Linux and
  macOS CI to also catch locks and blocking system calls (M2).
- Code on the audio thread must not panic. Panics there are bugs, and
  unwinding never crosses an FFI boundary.

### Evidence
The guard catches allocations, deallocations and growth past capacity, nested
sections and other threads (8 tests). Overhead: 31.7 ns versus 23.3 ns per
64-byte alloc/free pair (divan, this machine), negligible for test and debug
builds.

---

## ADR-0011 — Benchmarks and performance gates

**Status:** Accepted · **Date:** 2026-10-05

- Wall-clock micro-benchmarks use **divan** (MIT/Apache). CI runs every
  benchmark once as a smoke test (`cargo test --all-targets`).
- Shared CI runners are too noisy for wall-clock gates. Regression gates will
  use instruction counts (a Callgrind-based harness on Linux) for hot DSP and
  model code, plus scheduled runs on a dedicated machine for frame-time and
  audio-budget scenarios. Gates are introduced in M2, when there is engine
  code worth gating.
- Quality-bar measurements (frame time, audio budget, startup, idle CPU) are
  recorded with machine details in `docs/PROGRESS.md` and the milestone
  reports.

---

## ADR-0012 — Lumen's own license

**Status:** Needs human decision · **Date:** 2026-10-05

`CLAUDE.md` does not state Lumen's licence. It decides:
- the ASIO path (ADR-0006): GPLv3 is only possible if Lumen is GPLv3;
- which dependency licences are acceptable (ADR-0013);
- distribution obligations (source availability, notices).

Until it is decided, crates carry no `license` field and the dependency
policy stays permissive-only, which keeps every option open.

---

## ADR-0013 — Dependency license policy

**Status:** Accepted · **Date:** 2026-10-05

Only permissive licences (MIT, Apache-2.0, BSD, ISC, Zlib, BSL-1.0, CC0,
Unicode), enforced in CI by `cargo-deny` (`deny.toml`). Weak copyleft (for
example MPL-2.0), font licences (OFL) and anything GPL need a recorded
decision before they are allowed. A third-party licence listing ships with
the installer (M10).

---

## ADR-0014 — Repository layout and CI

**Status:** Accepted (assumption: the repository will be hosted on GitHub) · **Date:** 2026-10-05

- A Cargo workspace. `crates/` holds code intended for production;
  `prototypes/` holds measurement code that is deleted or rewritten once its
  question is answered.
- `Cargo.lock` is committed; this is an application.
- GitHub Actions on Windows (MSVC), macOS and Linux: rustfmt, clippy with
  warnings denied, tests including benchmark smoke runs, and cargo-deny.
  **No remote is configured yet, so CI has not run.**
