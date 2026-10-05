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
| [0001](#adr-0001--system-architecture) | System architecture: processes, threads, data flow | Proposed; evidence recorded | **Yes** |
| [0002](#adr-0002--ui-framework) | UI framework: egui/eframe + wgpu, hybrid rendering | Proposed; evidence recorded (P-UI) | **Yes** |
| [0003](#adr-0003--real-time-audio-engine) | Real-time audio engine | Proposed; evidence recorded (P-ENGINE) | **Yes** |
| [0004](#adr-0004--project-file-format) | Project file format | Proposed; evidence recorded (P-FORMAT) | **Yes** |
| [0005](#adr-0005--vst3-hosting-and-plugin-isolation) | VST3 hosting and plugin isolation | Proposed; evidence recorded (P-VST3, P-ISO) | **Yes** |
| [0006](#adr-0006--audio-device-backends-and-asio) | Audio device backends and ASIO | Proposed; evidence recorded (P-ASIO); **ASIO licensing needs human decision** | **Yes** |
| [0007](#adr-0007--toolchain-and-build-targets) | Toolchain and build targets | Accepted (local workaround pending VS Build Tools) | Install is your call |
| [0008](#adr-0008--product-name-defined-once) | Product name defined once | Accepted | No |
| [0009](#adr-0009--time-base) | Time base: integer ticks at 1,411,200 PPQ | Proposed (revised during P-FORMAT); evidence recorded | **Yes** |
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
- P-ENGINE (ADR-0003): an allocation-free, block-size-invariant core costs
  under 1% of the budget for 64 tracks.
- P-ISO (ADR-0005): the per-block cost of separate-process hosts is about
  9 µs for one host and about 35 µs p99 for 8 in parallel at 128 frames.
  Crashes and hangs are survived without stalling the audio loop.
- P-ASIO (ADR-0006): the driver callback can run the engine directly. Driver
  probing in a child process protects the main process.

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

### Decision (proposed, refined by P-UI)
**egui/eframe with the wgpu backend, with hybrid rendering**, under a strict
application structure:
- egui provides windows, panels, widgets, text, input, focus and
  accessibility.
- **Dense content (clip bodies, waveforms, notes, automation curves) is drawn
  by Lumen's own wgpu pipelines** inside egui paint callbacks. Geometry lives
  on the GPU and is updated on edit, not per frame; a frame only uploads the
  view transform. Plain egui painting remains fine at moderate density, with
  viewport culling and level-of-detail tiers (merged spans, plain rectangles,
  then full detail with labels).
- A framework-independent core (document model, commands, undo) keeps the UI
  layer replaceable.
- Lumen's own design system (tokens, widgets, icons) is painted on egui's
  primitives. egui's default look is not used.

Why not another framework: the measured bottleneck is CPU tessellation of tens
of thousands of vector shapes per frame. Every framework that tessellates on
the CPU (iced's canvas, Slint, egui alike) hits it at this density, and GPU
instancing is the general fix. egui's own cost was small (Lumen's UI code at
most 2.7 ms per frame), and its paint callbacks made the fix straightforward.

### Evidence — P-UI (`prototypes/ui-stress`), 2026-10-05
Setup: eframe/egui 0.36.2 on wgpu, which chose Vulkan on an RTX 4070 (driver
610.88). Window 3400×1300 px, presentation uncapped for measurement, 400
frames per scenario (40 warm-up). Synthetic session: 200 tracks × 100 clips
(20,000), waveforms on audio tracks from a peak pyramid, 100,000 notes. Track
headers are real egui widgets; clips and notes are painted. "CPU" is eframe's
per-frame CPU time: Lumen's UI code plus egui tessellation plus wgpu
rendering, excluding vsync waits.

| Scenario | Scale | CPU p50 | p95 | p99 | max | 120 fps (≤ 8.3 ms) | 60 fps (≤ 16.7 ms) |
|---|---|---|---|---|---|---|---|
| Arrangement, 48 px rows, pan + scroll (180 clips, 4,236 waveform columns) | 100% | 3.9 | 4.6 | 4.8 | 5.1 | ✅ | ✅ |
| Arrangement, fit all, 20 px rows (4,100 clips) | 100% | 11.0 | 11.9 | 12.2 | 12.8 | ❌ | ✅ |
| Arrangement, zoom sweep 2–400 px/beat | 100% | 3.8 | 8.6 | 9.1 | 9.6 | ❌ (tail) | ✅ |
| Piano roll, 120 px/beat, pan | 100% | 5.2 | 5.8 | 6.1 | 6.5 | ✅ | ✅ |
| Piano roll, all 100k notes, **egui shapes** (53,838 merged spans) | 100% | 18.3 | 19.8 | 20.0 | 20.9 | ❌ | ❌ |
| Piano roll, all 100k notes, **GPU instanced** | 100% | 4.8 | 5.4 | 5.7 | 6.0 | ✅ | ✅ |
| Arrangement, 48 px rows, pan + scroll | 200% | 0.9 | 1.3 | 1.5 | 1.6 | ✅ | ✅ |
| Arrangement, fit all, 20 px rows | 200% | 4.7 | 5.3 | 5.5 | 8.7 | ✅ | ✅ |
| Piano roll, all 100k notes, egui shapes | 200% | 10.2 | 11.0 | 11.4 | 12.1 | ❌ | ✅ |
| Piano roll, all 100k notes, GPU instanced | 200% | 4.3 | 4.9 | 5.1 | 5.5 | ✅ | ✅ |

The GPU-instanced scenarios also draw a dense static arrangement in the same
frame, which accounts for most of their remaining time. Notes themselves cost
the CPU almost nothing.

Other criteria:
- **Idle CPU:** 0.00% of a core over 10 s with the window open and untouched
  (0.31% in a first run). ✅
- **Start-up**, process creation to first frame: 545 ms warm; 1,007 ms on the
  first run after building (cold caches). ✅ warm, borderline cold. Real
  start-up adds project loading in the background.
- **Accessibility** (inspected from outside through Windows UI Automation,
  the API screen readers use): ✅ 58 elements with roles and names. Transport
  buttons are "Play", "Stop", "Record", "Loop". Track toggles are
  keyboard-focusable checkboxes with descriptive names ("Mute Track 1") via a
  name override. Findings for M3: the tempo field is an unnamed spinner (the
  label must be associated with `labelled_by`); only on-screen rows are
  exposed; painted content (clips, notes) needs explicit accessibility nodes
  and keyboard navigation.

Not covered: text crispness at 100% (needs a human look), IME, multiple
windows, plugin-window parenting, the light theme, and a mid-range machine.
This machine is high-end, so the 120 fps margins above must be re-measured on
mid-range hardware in M3.

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

### Evidence — P-FORMAT (`crates/lumen-project`), 2026-10-05
All pass criteria met. 22 tests, all passing:
- **Golden fixture** `tests/fixtures/v1/golden.lumen` (groups, audio and MIDI
  clips, notes, VST3 and built-in devices with state, media, tempo change)
  loads into an exact expected model. The fixture is frozen; future versions
  must keep loading it through migrations.
- **Round trips are exact** for 25 varied synthetic projects, in memory and
  through disk.
- **Damaged input never panics:** every truncation of a file is rejected
  (each prefix tested), and 5,000 random byte corruptions never panic (more
  than 4,000 are rejected; the rest are harmless value changes).
- **Newer files are refused** with "saved by a newer version of Lumen
  (format 999; this version reads up to format 1)". Non-project JSON is
  identified as such.
- **Inconsistent projects are rejected precisely** on load (note out of range,
  group cycle, missing media) **and are never written**: saving validates
  first. A NaN gain, which JSON would silently turn into `null`, is refused,
  and the old file stays byte-identical.
- **Crash safety:** a simulated failure after the temp file is written but
  before the commit leaves the old file intact and no temp file behind.
  Leftovers of a crashed save are ignored by load and cleaned by the next
  save. Backups rotate (`Backups/<name> 2026-10-05T183012.345Z.lumen`, keep N).
- **Relative media paths:** a project folder moved elsewhere still finds its
  `Samples/`. Missing media reports where it looked, ready for relinking.
- **Migration mechanism:** a synthetic three-version history is applied in
  order, stamps the version, reports which step failed, and refuses unknown
  versions. Production has no migrations yet: version 1 is the first.

**Scale** (release build, this machine, divan median over 10 runs) at quality-bar
size: 200 tracks, 19,900 clips, 100,000 notes, 199 plugin states of 4 KiB:

| Operation | Time |
|---|---|
| Serialize to readable JSON | 70 ms |
| Parse + migrate + validate | 87 ms |
| Atomic save to disk, including fsync | 83 ms |
| File size | 12.0 MB |

Notes on the format: each note is one line (`[start, length, pitch,
velocity]`), and ids are 16 hex digits so JavaScript-based tools can't
corrupt them. Unknown fields are ignored on load; every persisted change
bumps the version anyway. The size is dominated by indentation and clip
objects. Compression, or a smaller indent, can come later without changing
the data model.

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

### Evidence — P-ISO (`prototypes/isolation-ipc`), 2026-10-05
All pass criteria met. Setup: a parent "audio thread" paced in real time by a
high-resolution waitable timer, with MMCSS "Pro Audio" on both sides. Child
host processes apply a gain to stereo blocks in shared memory (double-buffered
by request parity). Signalling uses named auto-reset events. Deadlines are
half the period, enforced with a second high-resolution timer. Every block's
output is verified bit-exactly. The audio loop runs inside `rt_section`.
20 s per scenario.

| Scenario (128 frames @ 48 kHz unless noted) | Round trip p50 | p99 | p99.9 | max | Misses / mismatches / overruns |
|---|---|---|---|---|---|
| 1 host, events | 8.7 µs | 19.5 µs | 29.0 µs | 41.1 µs | 0 / 0 / 0 |
| 1 host, both sides spinning (lower bound) | 2.3 µs | 7.0 µs | 23.0 µs | 82.4 µs | 0 / 0 / 0 |
| 4 hosts, events | 16.3 µs | 30.3 µs | 61.3 µs | 144.3 µs | 0 / 0 / 0 |
| 8 hosts, events | 18.5 µs | 35.5 µs | 56.2 µs | 121.7 µs | 0 / 0 / 0 |
| 16 hosts, events | 27.1 µs | 53.3 µs | 93.3 µs | 253.8 µs | 0 / 0 / 0 |
| 8 hosts, events, 64 frames (1.33 ms period) | 18.6 µs | 34.4 µs | 62.3 µs | 143.0 µs | 0 / 0 / 0 |

Fault injection into host 0 of 2 at block 1,000:

| Fault | Detected | Classified | Replacement spawned | Back in service | Healthy host |
|---|---|---|---|---|---|
| Access violation (real crash, not a panic) | same block | exit code 0xC0000005 | 6.6 ms | 4 blocks (10.7 ms) | 0 misses, 0 mismatches |
| Hang (sleeps forever) | after 3 consecutive misses (5.3 ms) | still running, killed | 3.8 ms | 5 blocks (13.3 ms) | 0 misses, 0 mismatches |

The audio loop made 0 heap operations in every scenario and never overran its
period. Failed hosts are handed to a supervisor thread through a lock-free
queue, and killed, freed and respawned there.

Findings that shape M6:
1. **The transport is cheap enough to make separate-process the default.**
   Event signalling costs about 9 µs for one host and about 35 µs p99 for
   8 hosts in parallel, about 1.3% of a 128-frame period. Spinning saves
   about 6 µs but burns a core per host. Rejected as a default; possibly an
   option for one "low-latency" sandbox.
2. **Timer slop:** in the fault cases the wait overshot the 1.33 ms deadline
   by up to 0.32 ms (max 1.66 ms) because of timer wake-up latency. Production
   must arm the deadline with a margin, or spin for the last stretch, so the
   remaining budget is real.
3. **Hosts must suppress Windows Error Reporting UI**
   (`SetErrorMode(SEM_NOGPFAULTERRORBOX)`), or a crash parks the process
   behind a dialog and turns into a hang. They should write their own crash
   report instead.
4. A **kill-on-close Job object** guarantees no orphaned plugin hosts if
   Lumen dies.
5. Fan-out cost grows with the number of host processes (sequential
   signal/wait). Grouping (per plugin type or vendor) keeps the count modest;
   M6 measures this with real plugins.

Not covered yet: real plugins in the host (P-VST3 covers scanning only),
editor windows across processes, the POSIX transport (futex / Mach
semaphores), and parameter and event traffic beyond the audio buffers.

### Evidence — P-VST3 (`prototypes/vst3-scan`, `prototypes/vst3-test-module`), 2026-10-05
All pass criteria met.
- **The `vst3` crate 0.3 builds with no SDK, no libclang and no C++**
  (pre-generated bindings) on the GNU target. Its `ComPtr`, `cast` and
  generated `*Trait` methods were enough to enumerate every real factory.
- **Real plugins: 39 of 40 installed modules enumerated** in sandboxed child
  processes (`LoadLibraryEx` → `InitDll` → `GetPluginFactory` → class info
  → `ExitDll`): 10.4 s wall time with 4 parallel processes (20.5 s of
  per-module time). Kontakt 8, Omnisphere, FM8, u-he, 21 iZotope Ozone 12
  modules, Neural DSP, Toontrack, Melodyne, Spitfire and IK all succeeded.
  EastWest Opus failed in its own `DllMain` (`ERROR_DLL_INIT_FAILED`) after
  8.9 s. It was cleanly classified, and the scanner was unaffected. See
  `docs/PLUGIN_COMPATIBILITY.md`.
- **`moduleinfo.json` fast path:** present in 4 of 40 modules (one in the
  non-standard `Contents/` location). For all 4 it matches the factory exactly
  (class names and class ids), so it can replace loading the binary when
  present. Its practical value today is limited by low adoption.
- **Isolation self-test** (deliberately misbehaving test module, now in CI on
  Windows): a crash in `GetPluginFactory` was contained and classified
  (`0xC0000005`, 74 ms); a hang was killed at the timeout; a missing factory
  was reported.

Findings that shape M6:
1. Scanning cost is dominated by plugin `InitDll` and `DllMain` work
   (Kontakt 1.9 s, Opus 8.9 s), so the scanner must be parallel,
   cached and non-blocking, exactly as specified.
2. Most real plugins (35 of 40 here) still ship the legacy single-file
   layout. Discovery must keep supporting it.
3. Sub-category strings are messy (`Fx|Fx|Guitar`). The browser should
   normalise and de-duplicate them.

Not covered: instantiation, processing, state, presets, editors (M6).

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

### Evidence — P-ASIO (`prototypes/asio-probe`), 2026-10-05
All automated pass criteria met. The native host is about 1,500 lines of
Rust over COM, CLI included, with no SDK, no C++ and no bindgen.

**Sandboxed query of all 7 registered drivers** (each in a child process with
a 15 s timeout): the parent was never at risk.

| Driver | Result | Init | Details |
|---|---|---|---|
| Focusrite USB ASIO | OK | 1.7 ms | 4 in ("Input 1", "Input 2", "Loopback 1", "Loopback 2") / 2 out; Int32LSB; buffer fixed at 128 (granularity 16); 48 kHz only; clock "USB" |
| ASIO4ALL v2 | OK | 2.3 ms | 2 in / 2 out; Int32LSB; buffer 64–2048 (preferred 512, granularity 8); 44.1–192 kHz |
| Ableton Move / Ableton Push | Unavailable | — | "No device is connected to the PC." |
| Focusrite Thunderbolt ASIO | Unavailable | — | "Cannot open Focusrite Thunderbolt ASIO. (Error code: 0x54f)" |
| Alpha ASIO driver | Unavailable | — | "Failed to open ASIO input device" |
| FlexASIO | Unavailable | — | configured PortAudio device not found |

**Streaming** (2 in / 2 out, silent output, callback inside `rt_section`):

| Run | Callbacks | Discontinuities | Heap ops | Callback time p50 / p99 / max | Worst share of period |
|---|---|---|---|---|---|
| Focusrite, 128 @ 48 kHz, 30 s | 11,253 / 11,250 expected | 0 | 0 | 0.5 / 1.0 / 6.5 µs | 0.24% |
| Focusrite, same, with 64-track engine | 11,250 / 11,250 | 0 | 0 | 34.2 / 61.0 / 300.8 µs | 11.3% |
| ASIO4ALL, 64 @ 44.1 kHz, 15 s, with 64-track engine | 10,316 / ~10,335 (start-up) | 0 | 0 | 11.3 / 20.4 / 55.8 µs | 3.8% |

Findings that shape the M2 design:
1. **Callbacks are not evenly spaced.** The Focusrite USB driver aligns them to
   1 ms USB frames: median interval 3.0 ms against a 2.667 ms period, with
   every 9th callback arriving immediately after the previous one (minimum
   1.9 µs). The engine must not assume even spacing. Budgets are judged per
   callback against the period, with headroom for back-to-back pairs.
2. **In-callback cost exceeds the hot-cache benchmark** (34 µs versus 23 µs
   median for the same 64-track load), from cold caches between callbacks.
   Budget estimates should use in-situ measurements.
3. **Reported latency includes driver safety buffers:** Focusrite reports
   6.27 ms in and 8.27 ms out (14.5 ms round trip) at 128 frames. The device
   selector must show the driver's numbers, not buffer ÷ rate.
4. Both callback paths work: `bufferSwitchTimeInfo` (Focusrite) and plain
   `bufferSwitch` with `getSamplePosition` (ASIO4ALL).
5. ASIO's callbacks carry no context pointer, so the active stream must live
   in a process-wide slot. This confirms the design: one ASIO driver per
   process.
6. Some drivers return odd values (the Focusrite "version" is a large
   negative number) or truncate error text. The UI should treat driver
   strings and numbers defensively.

Not yet verified: reset requests from a driver panel (MT-ASIO-4), hardware
removal (MT-ASIO-5), busy driver (MT-ASIO-6). These need a person at the
machine.

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
3. **Integer ticks at a highly composite resolution: 1,411,200 PPQ**
   (2⁷·3²·5²·7²) in `i64`.

### Decision (proposed)
Option 3, implemented in `lumen_core::time`. One quarter note is 1,411,200
ticks, so these grid values are exact integers: straight notes down to 1/512,
and dotted, triplet, quintuplet, septuplet and nonuplet values down to 1/256.
At 999 BPM and 192 kHz one sample is about 122 ticks, so tick positions
resolve well below one sample. `i64` covers about 6.5·10¹² quarter notes.
Conversion to samples goes through the tempo map with one documented
rounding rule (nearest sample, ties away from zero), so a boundary shared by
two clips always maps to the same sample. Unwarped audio keeps its start in
ticks and its length in samples.

**Revision during P-FORMAT:** the first proposal was 705,600 PPQ (2⁶·…). The
grid test showed that dotted 1/256 is not an integer there (16,537.5 ticks).
Doubling the resolution fixed that at no cost. Nothing had been persisted, so
no migration was needed.

### Evidence — P-FORMAT time base, 2026-10-05
9 unit tests in `lumen-core`, all passing:
- every grid value listed above is exact; 1/1024 is documented as out of range;
- 120 BPM at 48 kHz: a quarter note is exactly 24,000 samples and an eighth
  triplet exactly 8,000;
- multi-segment tempo maps accumulate correctly, and seconds → ticks inverts
  exactly;
- **every sample is addressable at 999 BPM / 192 kHz**: for samples spread over
  2 million, tick → sample → tick returns the same sample;
- 10,000 random positions across a three-segment map round-trip
  ticks → seconds → ticks exactly;
- sample positions never go backwards; invalid maps (empty, not starting at
  0, unordered, BPM outside 20–999) are rejected.

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

Recorded exceptions (2026-10-05; `cargo-deny check` passes all four checks
locally with cargo-deny 0.20.2):
- **`epaint_default_fonts`** (egui's bundled fonts: OFL-1.1, Ubuntu Font
  Licence), allowed for that crate only. Used by the prototypes. Lumen's own
  fonts are chosen with the design system in M3. OFL is the standard licence
  for embeddable fonts and requires shipping its text.
- **RUSTSEC-2026-0192** (`ttf-parser` unmaintained; informational, not a
  vulnerability), ignored. It is reached only through winit's Linux Wayland
  decorations (`sctk-adwaita` → `ab_glyph`). Revisit when upstream moves.

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
