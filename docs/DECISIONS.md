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
| [0001](#adr-0001--system-architecture) | System architecture: processes, threads, data flow | **Accepted** 2026-10-05 | — |
| [0002](#adr-0002--ui-framework) | UI: egui host + Lumen-owned renderer (`lumen-render`: wgpu + Vello) | **Accepted** 2026-10-06 | — |
| [0003](#adr-0003--real-time-audio-engine) | Real-time audio engine | **Accepted** 2026-10-05 | — |
| [0004](#adr-0004--project-file-format) | Project file format | **Accepted** 2026-10-05 | — |
| [0005](#adr-0005--vst3-hosting-and-plugin-isolation) | VST3 hosting and plugin isolation | **Accepted** 2026-10-05 (M6 must fix wake-up latency) | — |
| [0006](#adr-0006--audio-device-backends-and-asio) | Audio device backends and ASIO | **Accepted** 2026-10-05: sign Steinberg's ASIO SDK agreement (**owner action, before distribution**) | — |
| [0007](#adr-0007--toolchain-and-build-targets) | Toolchain and build targets | Accepted; **owner installing VS 2022 Build Tools**, then local builds switch to MSVC | — |
| [0008](#adr-0008--product-name-defined-once) | Product name defined once | Accepted | No |
| [0009](#adr-0009--time-base) | Time base: integer ticks at 1,411,200 PPQ | **Accepted** 2026-10-05 | — |
| [0010](#adr-0010--real-time-safety-is-enforced-by-tests) | Real-time safety enforced by tests | Accepted | No |
| [0011](#adr-0011--benchmarks-and-performance-gates) | Benchmarks and performance gates | Accepted | No |
| [0012](#adr-0012--lumens-own-license) | Lumen's own license | **Accepted** 2026-10-05: closed source (proprietary) | — |
| [0013](#adr-0013--dependency-license-policy) | Dependency license policy | Accepted | No |
| [0015](#adr-0015--shifttab-conflict) | Shift+Tab: detail panel versus backward focus | **Open** (decide with the shortcut system, M3) | Yes |
| [0016](#adr-0016--wasapi-coreaudio-and-linux-backends) | WASAPI through the `wasapi` crate; CoreAudio and ALSA through cpal | Accepted 2026-10-06 | No |
| [0017](#adr-0017--engine-transport-and-timeline) | Engine transport and timeline: Ableton-style stop, positions in ticks, live tempo and rate changes | Accepted 2026-10-06 | No |
| [0018](#adr-0018--transport-bar-icons-and-app-audio) | Transport bar: vector icons on egui's painter; the app opens the system default output until the device selector | Accepted 2026-10-06 | No |
| [0019](#adr-0019--audio-files-and-clip-playback) | Audio files: symphonia + rubato into memory; clip player with edge and jump fades; one lane per file until M3 | Accepted 2026-10-06 (owner: memory now, streaming in M9) | No |
| [0020](#adr-0020--metronome-test-tone-and-audio-health) | Metronome and test tone in the engine; dropouts from driver positions and delivery against real time | Accepted 2026-10-06 | No |
| [0021](#adr-0021--device-selector) | Device selector: saved settings, ASIO probing in a helper process, run-time driver switching by handing the engine to a new thread | Accepted 2026-10-06 | No |
| [0022](#adr-0022--performance-gates-by-instruction-count) | Performance gates: instruction counts under Callgrind in a pinned Linux container, compared with a committed baseline | Accepted 2026-10-07 | No |
| [0023](#adr-0023--design-system-and-motion) | Design system and motion: semantic tokens with dark and light palettes, egui style built from them, our own window title rows, motion tokens with Reduce motion | Accepted 2026-10-07 | No |
| [0024](#adr-0024--undo-by-shared-snapshots) | Undo by shared snapshots: every step is a cheap copy of the project that shares untouched tracks and clips | Accepted 2026-10-07 | No |
| [0025](#adr-0025--tracks-folding-and-interim-volume-and-pan) | Tracks: owner-chosen header and clip design, Ableton-style folding and resizing, interim volume and pan until the mixer | Accepted 2026-10-07 | No |
| [0026](#adr-0026--clips-on-the-gpu) | Clips on the GPU: a box layer with a per-frame row table; names in egui; measured first with a frame-time benchmark | Accepted 2026-10-07 | No |
| [0027](#adr-0027--waveforms) | Waveforms: 8-bit peak pyramids built at load; drawn per pixel on the GPU from an atlas; disk cache in M9 | Accepted 2026-10-07 | No |
| [0028](#adr-0028--navigating-the-arrangement) | Navigation: f64 view with split beats on the GPU, zoom to single samples, Ableton keys, Follow (page, on), drag to reorder, sample-level waveforms | Accepted 2026-10-07 | No |
| [0029](#adr-0029--end-to-end-smoke-tests) | End-to-end smoke tests: the whole app driven by egui_kittest in CI, plus real-binary smoke tests at the end of every slice; isolated settings, autosaves and a silent test device | Accepted 2026-10-08 | No |
| [0030](#adr-0030--grid-rulers-loop-locators-and-scrubbing) | Grid, rulers, loop, locators and scrubbing (M3 slice 5): a grid model in core, the loop and punch as document edits, punch at the loop's edges, locator keys, both scrub styles in an engine scrub mode | Accepted 2026-10-08 | No |
| [0014](#adr-0014--repository-layout-and-ci) | Repository layout and CI | Accepted: **private GitHub repository** (owner creates it) | — |

---

## Decisions made by the owner (2026-10-05 onwards)

| Question | Decision |
|---|---|
| Lumen's licence (ADR-0012) | Closed source (proprietary). Can be relicensed later by the owner. |
| ASIO licensing (ADR-0006) | Sign Steinberg's free proprietary ASIO SDK agreement. Owner action; required before any distribution. |
| UI (ADR-0002) | Lumen owns its rendering: a toolkit-independent `lumen-render` (wgpu instanced pipelines for dense content, Vello for vector graphics). The host (egui or a custom winit shell) is chosen after P-RENDER measures both. **Slint dropped** (2026-10-06): it tessellates shapes much as egui does, is harder to combine with custom GPU drawing, and needs a licence notice. Lyon rejected: CPU tessellation is the measured bottleneck. |
| Engine, plugins, project format (ADR-0001, 0003, 0004, 0005, 0009) | Approved as recommended. |
| Toolchain (ADR-0007) | Owner installs VS 2022 Build Tools; then switch local builds to MSVC. |
| Hosting (ADR-0014) | Private GitHub repository; owner creates it, then push and first CI run. |
| Remaining M1 work | Approved: P-RENDER (replaces Slint), P-HOST, then the app shell after the host decision. |
| Device-loss recovery | Deferred to M2 (found by MT-ASIO-5). |
| Audio decoder (M2) | symphonia (MPL-2.0), used unmodified with its licence notice shipped; a recorded exception to the permissive-only policy (ADR-0013). |
| Clip audio in memory or streamed (M2 slice 6, 2026-10-06) | **In memory for M2; disk streaming in M9**, built together with the recording writer. Streaming from disk with read-ahead (plus an optional per-clip RAM mode) is the industry-standard design and the target; the in-memory path becomes that RAM mode. |
| AAC / M4A import (2026-10-06) | Deferred: possible codec patent licensing for a commercial product needs checking first. WAV, AIFF, FLAC, MP3 and Ogg Vorbis ship. |
| Audio input outside ASIO (M2 slice 8, 2026-10-06) | **WASAPI and macOS/Linux input deferred to M9**, built with recording, monitoring and input meters, where it can be tested end to end. ASIO input already works and is selectable in M2. Drift between separate input and output devices is handled with rubato's `Slip`. macOS and Linux input stays compile-only until that hardware is available. |
| VST3 scanning and hosting on macOS and Linux | Stretch goal, unscheduled (owner, 2026-10-06). Windows is first-class; the CI isolation self-test stays Windows-only until then. |
| Playheads (M2 slice 7, 2026-10-06) | Two, as in Ableton: a grey start marker on the ruler's bottom edge (where Play starts) and the blue playhead. Space plays from the marker and stops in place. |
| Motion (2026-10-06) | Fluid, Apple-like motion: the motion system and a retrofit of the M2 UI in M3 (done in slice 2), and a polish pass in M10. |
| Metronome extras (2026-10-06) | Count-in, pre-roll and record-only with recording (M9); output routing with the mixer (M4). |
| M2 close and M3 plan (2026-10-07) | M2 approved; M3 approved as proposed, unsplit, with the M2 performance gates as its slice 1. |
| Typefaces (M3 slice 2, 2026-10-07) | Inter for the interface and Unbounded for titles, chosen from rendered specimens. The owner wants personality, not generic choices; visual options are shown as specimens before anything is built (ADR-0023). |
| Track design (M3 slice 4, 2026-10-07) | Option A headers with option B's dark clips; type icons; Ableton-style folding and resizing in 22 pt steps; sliders with values inside, with pan saturating toward the edges; record arm that glows and breathes; a "+ Add track ▾" menu button with a double-click hint (DESIGN §7a, ADR-0025). |
| macOS CI (2026-10-07) | **Paused**: the GitHub Actions minutes ran out, so CI runs Windows and Linux only. macOS code is checked with the cross-target clippy (DEVELOPMENT.md) before pushing; macOS CI comes back when there is budget. |
| Reviews (2026-10-07) | A full repository review at the end of every milestone, against CLAUDE.md, the architecture and the design system (the `lumen-architecture` and `lumen-design` skills), before the milestone report. |
| Navigation (M3 slice 4d, 2026-10-07) | **Zoom to single samples**, as Ableton does; waveforms draw individual samples past the stored detail. **Follow playhead: page by default, on**; continuous is the second mode. Ableton-style keys: `+`/`-` zoom, X back, W fit width, H fit height, Alt+wheel a track's height. |
| Rulers and scrubbing (M3 slice 5, 2026-10-08) | **Ruler layout A, Ableton split** (chosen from rendered specimens): locators and the loop brace in a strip above the bar ruler, a time ruler (minutes and seconds, samples or timecode) along the bottom of the arrangement. **Both scrub styles:** dragging in the scrub strip plays short loops of the grid step under the pointer at normal pitch (Ableton-style, for songwriting and arranging); holding Alt scrubs tape-style, audio following the hand's speed and pitch (for precise editing). |
| Waveform peak cache (M3 slice 4c, 2026-10-07) | **Deferred to M9**, built with disk streaming: until then audio is decoded into memory at load and peaks take about 21 ms per 5 minutes (ADR-0027). |
| App and file icons (2026-10-07) | An app icon, a `.lumen` file icon and per-user file association in M9, shown as specimens first. Project folders keep the standard folder icon. |

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

### Evidence — P-RENDER (`crates/lumen-render`, `prototypes/render-shell`, egui host in `prototypes/ui-stress`), 2026-10-06
`lumen-render` keeps clips and notes on the GPU as instanced rectangles
(uploaded once, updated on edit) and composites Vello vector graphics. The
same scene (20,000 clips, 100,000 notes, Vello overlay of 8 × 2,000-point
curves, 64 knobs, 64 meters) was measured in two hosts. MSVC release build,
RTX 4070 / Vulkan, 3400×1300, uncapped presentation, CPU time per frame in ms.

| Scenario | Bare winit, p99 | Inside egui, p99 | Earlier: egui shapes | Earlier: egui + GPU notes |
|---|---|---|---|---|
| Worst case: all clips + all notes + overlay | **0.84** | **2.22** | 20.0 (notes only) | 5.7 (notes only) |
| Same, at 200% | — | 1.59 | 11.4 | 5.1 |
| Editing views + overlay | 0.78 | 1.45 | 4.8–6.1 | — |
| Editing views + overlay + Parley labels on every visible clip | 1.39 | — | — | — |
| Rectangles only (120k), no overlay | 0.26 | — | — | — |

| Other measures | Bare winit | Inside egui |
|---|---|---|
| First frame on screen | 854 ms cold, **438 ms** with the Vulkan pipeline cache warm | 1,007 ms (cache not yet wired into egui's device) |
| Idle CPU, 10 s | 0.00–0.47% | 1.25% interactive, 4.53% after the benchmark: **open issue** |
| Accessibility | AccessKit tree, built by hand (≈60 lines), fully visible to UI Automation (20 named buttons and checkboxes) | egui's AccessKit integration (58 elements) |
| Text | Parley + Vello (≈100 lines, cached layouts): +0.6 ms for all visible labels | egui text |

Conclusions:
1. **The renderer removes the bottleneck in either host.** The worst case is
   2.2 ms inside egui and 0.8 ms bare, against the 8.3 ms budget for 120 fps
   (and about 25× faster than egui shapes). The canvas cost no longer
   depends on the toolkit.
2. **egui adds about 0.7–1.4 ms per frame** for its widgets and panels: real,
   but small.
3. **A custom shell is feasible but its cost is elsewhere.** Text and an
   accessibility tree were cheap to prototype; what was not prototyped (focus
   and keyboard navigation, text input and IME, layout, widget library,
   generating and updating the accessibility tree from live widgets) is the
   bulk of a toolkit.
4. Open items for either host: wire the pipeline cache into egui's device;
   explain the egui host's idle CPU; Linux and macOS behaviour of Parley and
   Vello is untested beyond CI compilation.

**Follow-up, same day:** the Vulkan pipeline cache was wired into egui's
device (through egui-wgpu's `device_descriptor` hook, adding
`PIPELINE_CACHE`). egui host first frame: **542 ms cold, 411 ms warm** (was
1,007 ms). Idle CPU: 0.16% and 0.00% over 10 s; the earlier 1.25–4.53% did
not reproduce, most likely Vello compiling shaders in the background. Frame
times unchanged (worst case p99 2.4–2.5 ms).

**Decision (owner, 2026-10-06): egui is the host**, conditional on the
shader-cache fix, which is now met. egui provides windows, panels, widgets,
text input, focus and accessibility. Every dense canvas is drawn by
`lumen-render` (wgpu instanced layers + Vello) through egui paint callbacks.
Because the renderer is toolkit-independent, a later move to a custom shell
would only replace the chrome. Production must store the pipeline cache in
Lumen's cache folder, not the temp folder.

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

**M2 slice 3 (2026-10-06):** `lumen-engine` takes over from the prototype
with the transport, loops, timeline swaps and denormal flushing (ADR-0017,
ADR-0010). Disk streaming arrives in slice 6; the worker pool when the
graph has parallel work (M4).

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

**Re-run, same day, second session (machine idle, Balanced power plan):**
functional results identical (0 misses, mismatches and overruns; crash and
hang survived). **Tail latency was much worse and reproduced 3 times:**
1-host p99 232–242 µs (against 19.5 µs earlier), 8-host p99 44–253 µs.
Spinning stayed fast (p99 9.8 µs), so the cost is waking a sleeping thread.
That is likely CPU power states or scheduling, but not yet diagnosed. The
transport still fits easily within a 2.67 ms period, but the p99 ≤ 50 µs
criterion is **not robustly met with pure event signalling**. M6 must
evaluate spin-then-wait signalling and how MMCSS and power states interact,
and measure on more than one machine state.

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

### Evidence — P-HOST (`prototypes/plugin-host`), 2026-10-06
A real third-party instrument hosted end to end, first in-process and then
isolated. Built on the `vst3` crate with Lumen's own host-side COM objects
(host application, event list, parameter changes, memory stream).
u-he TyrellN6, 48 kHz, 128-sample blocks, MSVC release build.

**In-process (step 1):** loads and activates in 21–370 ms (cold disk cache
first). Middle C renders at peak −13.4 dBFS / RMS −21.9 dBFS, all samples
finite. `process()` takes 37 µs median per block (1.4% of budget). State is
2,783 bytes; a fresh instance restored from it re-saves an identical state
and renders at identical levels.

**Isolated (step 2):** the plugin runs in a `plugin-host child` process
driven over the P-ISO shared-memory transport, paced in real time with a
1.33 ms deadline per block.

| Measure | Result |
|---|---|
| Host process up, plugin loaded and active | 30 ms |
| Round trip per block, including the plugin's processing | p50 66 µs, p99 115 µs, max 418 µs |
| State snapshot ("last known state") | 2,783 bytes in 0.52 ms |
| Injected crash (access violation in the host process) | detected the next block, exit code `0xC0000005` |
| Supervisor restart with restored state (off the audio loop) | 33 ms; restored state re-saves identical |
| Audio | −13.4 dBFS before the crash and after recovery; 15 silent blocks (40 ms) in between; 0 loop overruns |
| Orphaned processes afterwards | 0 (kill-on-close Job object) |

This closes the M1 exit criterion "VST3/isolation approach demonstrated".
Simplifications to fix in M6: the snapshot takes one block slot (production
uses a separate non-real-time channel); the supervisor channel allocates
(P-ISO already proved an allocation-free one); notes held at the crash are
re-sent by the test rather than tracked; no controller, editor or
parameters yet; the transport code is duplicated from P-ISO and must become
one shared crate.

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
CoreAudio and Linux implementations are chosen in M2 (ADR-0016).

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

Verified by the owner later the same day:
- **Reset requests** from the driver panel are handled: buffer 128 → 256 →
  128 within one stream gave three clean segments with 0 discontinuities.
  Each reset costs about 0.65 s of silence.
- **Hardware removal** is survived without a crash. Recreating buffers then
  fails with `ASE_NotPresent`, so M2 must keep the session and retry when the
  device comes back.
- The Focusrite driver is **multi-client** (it streamed alongside Ableton),
  so busy-driver error handling remains untested.

---

## ADR-0007 — Toolchain and build targets

**Status:** Accepted (local workaround pending VS Build Tools) · **Date:** 2026-10-05

### Decision
- Rust stable, pinned in `rust-toolchain.toml` (1.99.0), edition 2024.
- **Production Windows target: `x86_64-pc-windows-msvc`.** Reasons: PDB
  debugging and crash dumps, the same ABI ecosystem as plugins and drivers,
  and GitHub's Windows runners use it by default.
- **Update 2026-10-06:** Visual Studio 2022 Build Tools installed; rustup's default host is now `x86_64-pc-windows-msvc`. The workspace builds, lints clean and passes all tests with MSVC locally. The GNU workaround below is retired.
- **Previously, local development on this machine: `x86_64-pc-windows-gnu`.** No
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
- RealtimeSanitizer (`rtsan-standalone` 0.3, Apache-2.0) also catches locks
  and blocking system calls. `rt_section` marks an RTSan real-time context,
  so every guarded test is also a sanitizer test. The calls compile to
  nothing unless `RTSAN_ENABLE` is set at build time. The CI `rtsan` job
  (Linux) sets it, runs the real-time tests, and runs three canaries (a
  file open, a wait on a held lock, and an allocation inside a section).
  Each must abort with an RTSan report, which proves the sanitizer is live.
  **Coverage limits** (LLVM 20.1 runtime, probed under WSL on 2026-10-06):
  RTSan catches file opens, writes, contended locks and allocation, but not
  `std::thread::sleep` (Rust calls `clock_nanosleep`), file metadata
  (`statx`) or an uncontended `std::sync::Mutex` (one atomic operation, no
  system call). The first CI runs failed for exactly those reasons: their
  canaries were undetectable, while the sanitizer itself worked. Code
  review still has to keep sleeps and metadata calls off the audio thread.
  With `RTSAN_ENABLE`, the crate's build script downloads a prebuilt
  runtime from the project's GitHub releases. That happens in that CI job
  only, never in normal or release builds. (Added M2 slice 3.)
- `DenormalGuard` sets flush-to-zero (MXCSR FTZ and DAZ on x86-64, FPCR FZ on
  AArch64) for the length of each engine block and restores the previous
  setting after. (Added M2 slice 3.)
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

**Status:** Accepted 2026-10-05 by the owner: closed source (proprietary); can be relicensed later. The options below are kept as context.

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
  Hosted on a private GitHub repository (`andrewl913/lumen`). The first CI
  run (2026-10-06) passed every job, including the MSVC build and the VST3
  self-test. Follow-up: bump `actions/checkout` off Node 20 (deprecation
  warning).

---

## ADR-0015 — Shift+Tab conflict

**Status:** Open · **Date:** 2026-10-06

### Context
The spec (§5) assigns Shift+Tab to "toggle detail panel", as in Ableton.
Shift+Tab is also the standard way to move keyboard focus backwards, and the
spec requires keyboard-only core workflows. In the M1 shell, egui handled
both at once: focus moved into the detail panel as the panel was hidden, and
AccessKit panicked on the vanished focus target. That crash was fixed on
2026-10-06: Lumen now takes Shift+Tab before egui sees it, and releases focus
whenever a panel is hidden.

### Options
1. Keep Shift+Tab for the detail panel (Ableton users expect it) and give
   backward focus another key, such as Ctrl+Shift+Tab or F6 / Shift+F6
   between panes.
2. Keep standard Shift+Tab focus navigation and move the detail-panel toggle
   elsewhere in the default keymap.
3. Make it context-dependent: Shift+Tab moves focus while a text field or
   dialog has focus, and toggles the panel otherwise.

### Recommendation
Option 3, with F6 / Shift+F6 always cycling between panes. To be decided
with the remappable shortcut system and its conflict detection (M3).

---

## ADR-0016 — WASAPI, CoreAudio and Linux backends

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 2)

### Context
ADR-0006 left the non-ASIO backends to M2. Windows is first-class, so WASAPI
must be solid: shared mode for everyday use, exclusive mode (a should-have)
for low latency without an ASIO driver. macOS and Linux follow from the same
codebase but are not tested on hardware yet.

### Options considered
| Option | Notes |
|---|---|
| Raw COM over `windows-sys` (like the ASIO host) | Full control, including IAudioClient3 low-latency shared mode. About 1,000 lines of unsafe COM plumbing for what crates already do well. |
| **`wasapi` crate 0.25** (MIT, HEnquist; used by CamillaDSP) | Safe wrappers for endpoints, shared and exclusive, event-driven timing, format negotiation with driver quirks, alignment helpers. Does not expose IAudioClient3. Pulls in the `windows` crate (MIT/Apache). |
| cpal for WASAPI | No exclusive mode, no control over the period, and it hides the device's format. |
| **cpal 0.18** for macOS and Linux | Apache-2.0. CoreAudio and ALSA hosts (JACK and PipeWire behind features). Good enough to start; the API gaps from ADR-0006 (channel names, latency) matter less there for now. |
| Native CoreAudio and ALSA backends | More control, much more code; premature before macOS and Linux are tested on hardware. |

### Decision
- **Windows:** `lumen-audio::wasapi` on the `wasapi` crate. Each stream gets a
  render thread with the "Pro Audio" MMCSS class and fixed-size blocks.
  - Shared mode runs at Windows' rate and period (10 ms). Lumen never
    resamples silently: asking for another rate fails with a message that
    names the fix.
  - Exclusive mode opens at the requested rate and buffer size, with the
    first accepted format from f32, i32, 24-in-32, packed 24 and i16.
  - Exclusive mode starts event-driven, then checks the device clock for
    250 ms of silence. If the clock is more than 10% off real time, the
    device is reopened in polling mode with at most two blocks queued.
    (MT-AUDIO-4: the Focusrite USB driver signalled exclusive-mode events
    every 18 ms for a 10 ms buffer, so its clock ran at 60% of real time.)
- **macOS and Linux:** `lumen-audio::cpal_host` on cpal's default host
  (CoreAudio, ALSA). The callback lives in a slot the audio thread takes with
  `try_lock`, which never blocks.
- Both backends report device loss to the supervisor the same way ASIO does:
  a reset request (the reopen then fails, giving `Lost`) or a stall.

### Consequences
- **Follow-ups:**
  - **Input** for WASAPI and cpal arrives with the device selector (M2
    slice 8). Until then these backends reject input channels with a clear
    message.
  - **IAudioClient3** low-latency shared mode (about 3 ms instead of 10 ms on
    supporting drivers) is a candidate for M12 tuning, through the `windows`
    crate the `wasapi` crate already depends on.
  - **JACK and PipeWire** are revisited when Linux is tested on hardware.
- macOS is type-checked locally (`--target aarch64-apple-darwin`); Linux is
  checked in CI. Neither has run against real devices.

### Evidence (2026-10-06, this machine)
| Check | Result |
|---|---|
| Enumeration, both modes | Focusrite USB, NVIDIA HDMI and DualSense endpoints listed with correct rates; shared period 441/480 frames, exclusive minimum 132–144 frames |
| Shared, 48 kHz | 480-frame blocks, 99 callbacks/s (100 expected), 0 overloads |
| Exclusive, 48 kHz / 144 | 144-frame blocks (3 ms), about 335 callbacks/s (333 expected) |
| Exclusive, 44.1 kHz default | Aligned to 448 frames by the driver; ran cleanly |
| Shared, wrong rate | `ConfigFailed(Unsupported("44100 Hz: Windows runs … at 48000 Hz in shared mode …"))` |
| Licences | All 42 third-party crates on Windows, macOS and Linux are permissive |
| Focusrite USB, exclusive, events (before the fix) | Device clock 26,623 frames/s at 44.1 kHz (60%); about 61 callbacks/s instead of 100 at every buffer size |
| Focusrite USB, exclusive, after the fix | Calibration switches it to polling: 441 frames → 101/s, 256 → 174/s (100 and 172 expected); open takes about 1.1 s |
| NVIDIA HDMI, exclusive 144, after the fix | Calibration keeps events: 331/s (333 expected) |

---

## ADR-0017 — Engine transport and timeline

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 3)

### Context
The transport must be sample-accurate, survive tempo and time-signature
changes and device rate changes (MT-AUDIO-6 showed Windows can switch rate
under a running stream), and behave the way producers coming from Ableton
expect. The spec's Ableton-compatible shortcut set implies Ableton's
transport semantics.

### Decision
- **State:** the engine keeps the play position in samples (what the device
  consumes) and every marker (play marker, loop, punch) in ticks (what the
  music means). Sample positions of the markers are cached and recomputed
  when the tempo map or sample rate changes.
- **Tempo, time-signature and rate changes** carry the position through ticks
  (`ticks_at_sample` on the old mapping, `sample_at` on the new one), so the
  bar and beat stay put. Because the position lives on whole samples, a
  change can move it by up to half a sample.
- **Semantics (as in Ableton Live; revised 2026-10-06 at the owner's
  request, MT-APP-4):** two positions, both drawn. The **start marker** is
  a neutral grey arrow at the ruler's bottom edge, on the ruler only, and is
  where Play starts. The **playhead** is the blue arrow at the top edge with
  a line down the arrangement, and is where playback is.
  - Play (Space) starts from the start marker.
  - Continue (Shift+Space) starts from the playhead.
  - Stop and Pause leave the playhead where playback stopped.
  - Stop while stopped returns both to the start.
  - Locate (ruler click) moves both.

  The first version moved the playhead back to the marker on Stop and
  started Play from the playhead.
- **Loop:** while the loop is enabled and playback is before its end,
  playback wraps from the end sample to the start sample exactly, splitting
  the block. Playback that starts after the loop end plays on. Disabled or
  empty regions do nothing.
- **Punch:** the engine reports whether playback is inside an enabled punch
  region; recording through it arrives in M9.
- **Timeline:** `Timeline` holds the tempo map and a new `MeterMap`
  (`lumen-core`). Time-signature changes must sit on bar lines. A timeline
  is immutable on the audio thread: the UI builds a new one and swaps it in
  through a lock-free queue, and the old one is freed on the UI side (the
  P-ENGINE pattern).
- **Commands** take effect at the start of the next block. The UI reads the
  transport through a seqlock snapshot that never makes the audio thread
  wait.
- **Sources** (`Source` trait) render parts of a block that never cross a
  loop wrap or locate, and get a `locate` call on every discontinuity so
  streaming sources (slice 6) can reposition.

### Consequences
- The project file still has a single time signature; meter changes in
  projects arrive with the time-signature lane (M3).
- Commands at block boundaries are not sample-accurate in live use (at most
  one block late, about 3 ms at 128 frames). Offline renders apply them at
  fixed block boundaries, so renders stay repeatable.

### Evidence (2026-10-06)
`crates/lumen-engine`: 13 transport tests (loop wraps on the exact sample at
block sizes 1, 3, 64, 128, 333, 4096 and 10,000; odd tempo at 44.1 kHz with no
gaps or repeats; stop, pause and locate; tempo and rate changes keep the bar
and beat; loop and punch follow), a seqlock torn-read test, and 5,000 blocks
of mixed commands, timeline swaps and a rate change with zero heap
operations. `transport_demo` on WASAPI: tempo 120 → 90 kept bar 3.1
(sample 192,000 → 256,000); 7/8 bars land at the right samples.

---

## ADR-0018 — Transport bar icons and app audio

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 4)

### Context
The owner asked for real icons on the transport instead of text. The M2 plan
said "Vello paths". The app also needs audio now, before the device selector
(slice 8) exists.

### Decision
- **Icons** are geometry on a 16-unit grid (`lumen-app/src/icons.rs`),
  scaled to their box and drawn with egui's painter (anti-aliased paths).
  They are vectors, sharp at every scale (checked at 100%, 150% and 200%).
  Drawing them through Vello would need a GPU paint callback per button for
  no visible gain at 16–24 px; Vello stays for dense canvases (ADR-0002).
  The same geometry can move to Vello later if the design system wants it.
- **Colour tokens** live in `theme.rs`: one accent (a blue chosen to stay
  distinct for common colour-vision deficiencies), with red for Record as the
  only semantic exception. The full design system and the light theme
  arrive in M3.
- **Audio in the app** opens the system's default output, through WASAPI
  shared mode on Windows. Shared mode is the safe default: it never takes a
  device away from other applications or loads ASIO drivers at start-up.
  `--audio <driver>:<name>` (for example `asio:"Focusrite USB ASIO"`)
  picks another device until the device selector and its saved settings
  arrive (slice 8).
- **Transport bar:** go to start, stop, play/pause (one button that shows
  what a click does), record (disabled until M9), loop, metronome (disabled
  until slice 7), tempo, time signature, an editable position (click and type
  bars.beats.sixteenths or m:ss) and an audio status dot with text.
  Shortcuts: Space play/stop, Shift+Space pause/continue, Home go to start,
  ignored while typing in a field.
- **Layout:** the panel toggles are laid out first, from the right, so they
  survive any width or scale; the status text shortens to fit.
- **Repaint:** continuous while playing; once a second when idle, to pick up
  device events, which keeps idle CPU near zero.

### Consequences
- Tempo and time-signature edits are not undoable yet; undo arrives with the
  command system in M3.
- The loop range is fixed to bars 1–4 until the ruler can set it (M3).

---

## ADR-0019 — Audio files and clip playback

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 6). Owner decisions:
audio in memory for M2, disk streaming in M9; AAC/M4A deferred.

### Decision
- **`lumen-media`:** symphonia decodes WAV, AIFF, FLAC, MP3 and Ogg Vorbis to
  `f32`. rubato's FFT resampler converts to the device rate (rubato's own
  recommendation for fixed-ratio file conversion). Loads run on background
  threads with progress and cancel.
- **In memory:** converted audio is shared (`Arc`) by the clips that use
  it. The audio thread only reads memory. Disk streaming arrives in M9
  (ROADMAP.md, PROGRESS.md); this path becomes the per-clip RAM option.
- **Rate changes:** when the device rate changes, files are converted again
  in the background. Clips whose audio is not at the device rate stay
  silent rather than play at the wrong speed.
- **Clip player** (`lumen-engine`, a `Source`):
  - Clips start at a musical position and play unwarped.
  - Start samples are recomputed every block (a binary search per clip).
    Caching them by timeline address was rejected: a freed timeline's
    address can be reused.
  - 2 ms fades at clip edges.
  - **Every transport jump is click-free, in the engine, for every
    source** (revised after MT-MEDIA-1 run 1, where the owner heard a pop
    on loop wraps and stops). Locates and loop wraps get a 2 ms linear
    crossfade: the engine renders the 2 ms that would have followed the old
    position (flagged `tail` in `RenderContext`) and fades it out while the
    new audio fades in. Play fades in, and stop and pause fade out. Loops
    still wrap on their exact sample. The first design faded in only after
    jumps and skipped loop wraps to keep loops seamless, which left a hard
    cut whenever the audio at the loop end did not line up with the loop
    start.
  - Clip lists are swapped whole through lock-free queues, and old lists are
    freed on the UI side.
- **The simple timeline:** each imported file gets its own lane with one
  clip at the drop position, snapped. The pointer position is unknown while
  Windows drags files (winit 0.30 reports no position), so a drop may land at
  the playhead. Lanes become project tracks with headers, editing, undo and
  save/load in M3.

### Evidence (2026-10-06)
| Check | Result |
|---|---|
| Decoding | WAV 16/24/float and AIFF decode to their exact samples (self-written files) |
| Conversion | 44.1↔48 and 96→48 kHz keep length (±2 frames), pitch (440 Hz) and level |
| Golden renders | clips exact on every sample at block sizes 1–4096; seamless loop wraps; fades as specified |
| Real-time safety | zero heap operations over 3,000 blocks with 64 clips, list swaps and locates |
| Cost | 64 stereo clips at 128 frames: 25 µs median (0.9% of the 2.67 ms budget) |
| Licences | cargo-deny: advisories, bans, licences and sources ok, with MPL-2.0 exceptions for the 10 symphonia crates (THIRD_PARTY_NOTICES.md) |
| App | `--open-audio` of a 5 s file: ready in 38 ms; converted again to 44.1 kHz in 6 ms when the Focusrite started |

---

## ADR-0020 — Metronome, test tone and audio health

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 7)

### Decision
- **Metronome** (`lumen-engine`): synthesized clicks (Beep, Wood), so there
  are no samples or licences to manage.
  - Each click starts on the exact sample of its beat, with the downbeat
    accented: higher, and louder (1.0 against 0.6).
  - It follows the tempo and time-signature maps.
  - It is mixed after the jump crossfade, so a click on a loop start keeps
    its attack.
  - A ringing click carries on through wraps, locates and stops; only new
    beats start clicks.
  - Count-in, pre-roll and record-only belong to recording (M9). Count-in
    applies to recording in Ableton too.
- **Test tone:** 1 kHz at −20 dBFS on every output, whether playing or not,
  with 5 ms ramps.
- **Panic:** stops at once and silences playback, the metronome, the test
  tone and any fade in progress. MIDI all-notes-off joins it in M5.
- **Dropouts** (`lumen-audio` supervisor), counted two ways:
  1. **Skipped buffers:** where the driver reports a sample position
     (ASIO), a jump past the expected position counts the skipped buffers.
  2. **Starvation:** every second, audio delivered is compared with real
     time. A shortfall beyond 3% counts as dropouts, in buffers. The first
     window after any (re)open is ignored. This is what catches a driver
     that runs but starves the device (MT-AUDIO-4), which the CPU meter
     cannot see.
- **CPU** shows the peak load over the last second. Overloads count buffers
  that took longer to process than they last.
- **UI:**
  - The metronome button on the transport (right-click for volume and
    sound).
  - "CPU n%" and, when non-zero, a dropout warning on the bar.
  - Clicking the status dot opens the **audio panel**: device, Audio On/Off,
    test tone, CPU, overloads, dropouts (with reset), clip memory, and
    panic. Slice 8 grows this into the device selector.

### Evidence (2026-10-06)
- 8 metronome, tone and panic tests: exact onsets at block sizes 1–4096;
  accents, including in 7/8; volume; mid-bar loop wraps; ringing through
  stop; tone pitch and level.
- Zero heap operations with clips, metronome and tone running.
- 3 dropout tests with the fake backend: a starving driver counts, a healthy
  one counts zero, and skipped positions count exactly.
- On hardware: 0 dropouts on the NVIDIA HDMI output in WASAPI shared and
  exclusive.

### Update 2026-10-07: clicks fade out
Each click was cut off at a fixed 50 ms while the beep still had about
0.2% of its peak, a small tick after every click (owner: "a little static
on the metronome"). The last 15 ms of every click now fade to exactly zero
(smoothstep); the beep is 26 dB down where the fade starts, so the sound
is unchanged. Cost: `engine_transport` +2.4% instructions (about 136 per
128-frame block; the scenario is mostly click synthesis), an intended
change; the baseline was updated. Lengthening clicks to 80 ms instead
cost +57% and was rejected.

---

## ADR-0021 — Device selector

**Status:** Accepted · **Date:** 2026-10-06 (M2 slice 8). Owner decision:
WASAPI and macOS/Linux input deferred to M9.

### Decision
- **Saved settings** (`settings.rs`):
  - Versioned JSON in the user's settings folder, holding the audio device
    and the metronome.
  - Written atomically, and only once a setup has actually started.
  - A damaged file is kept aside as `settings.json.bad`; a newer format is
    left untouched.
  - At start-up the saved device is found by id, then by name. If it is
    missing, Lumen uses the default output with a notice. `--audio` is a
    one-off override.
- **ASIO probing in a helper process** (`probe.rs`, risk R6): `lumen
  --probe-asio <clsid>` loads one driver, prints JSON and exits, with a 5 s
  timeout. A driver that hangs or crashes only takes the helper down; its
  error text is shown.
  - Measured: 115–206 ms per probe.
  - Probing a driver another process is streaming on does not interrupt
    that stream.
  - WASAPI and the system hosts are listed in-process, which is safe.
- **Run-time driver switching:** `AudioSystem::switch` hands the running
  callback (the engine, with transport, clips and metronome state) to a
  fresh control thread for the new driver model. ASIO and WASAPI need
  differently set-up COM apartments, which cannot change once set.
  Switching device or settings within one driver model reconfigures in
  place, with the existing fallback to the last working setup.
- **What the window offers:**
  - ASIO buffer sizes are shown read-only, with the driver's **Control
    panel** button. Most ASIO drivers (Focusrite included) take the buffer
    size from their own panel, as in Ableton. A running ASIO driver reports
    only its current rate, so the window offers the standard rates and
    reverts if the driver refuses one.
  - WASAPI shared shows the Windows rate and period read-only; exclusive
    and the system hosts offer their real ranges.
  - One output pair and, for ASIO, one input pair at a time, by name (a
    driver with blank names gets "Out n" / "In n"). Wider routing comes
    with the mixer (M4).
- **Problems are banners with a way forward:** lost ("reconnects when it
  is back"), busy ("in use by another application", Try again), and failed
  to start (Try again). Each has an Audio settings button.
- **A failed switch keeps the previous setup** (MT-APP-6 run 1). The
  window says "{device} could not start (…). Still using {previous}", and
  the status returns to the previous device. A failed driver switch hands
  the engine back to the previous driver, so nothing spins forever.
- **Slow driver operations never block the UI.** The control panel opens
  on the audio thread and reports back on a channel; the Focusrite panel
  launches Focusrite Control, which takes seconds. A rate change shows
  "Changing to … kHz… the interface is re-locking its clock".

### Evidence (2026-10-06)
- 5 settings tests, 1 probe round-trip test, 1 driver-switch test.
- Probes on this machine: Focusrite USB ASIO reports 4 inputs, 2 outputs,
  six rates and buffers of 16–1024 in steps of 16. Focusrite Thunderbolt
  (no hardware) reports "Cannot open … 0x54f". FlexASIO reports its missing
  Realtek device.
- Screenshots of the window on WASAPI shared and on ASIO.
- Rate change on the Focusrite USB (`examples/rate_switch.rs`): about
  1.3 s in total. Lumen's own part is about 20 ms (stop, dispose, load,
  buffers). The driver's `set_sample_rate` takes about 900 ms and its first
  start afterwards about 390 ms, while the interface re-locks its clock.
  Other hosts face the same cost; Lumen shows progress instead.

---

## ADR-0022 — Performance gates by instruction count

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 1). Implements the
gates ADR-0011 planned for M2.

### Context
CI runs every divan benchmark once, but nothing fails when code gets slower.
Wall-clock times on shared runners vary by tens of percent, too much to
gate on.

### Options
1. Wall-clock benchmarks with a wide tolerance: flaky, and they miss
   regressions under the tolerance.
2. A benchmarking crate built on Callgrind (gungraun, formerly
   iai-callgrind): needs a runner binary whose version must match the
   crate, plus a harness of its own.
3. **Our own small harness on Callgrind** (chosen): scenario functions
   named `measured_<name>`, counted with `--toggle-collect`, so setup is
   excluded.

### Decision
- `crates/lumen-perf`:
  - `perf-scenario <name>` runs one deterministic scenario (fixed seeds and
    sizes, no I/O or threads in the measured part);
  - `perf-gate` runs them all under Valgrind and compares the counts with
    `perf/baseline.json`;
  - it fails above **+2%** and suggests updating the baseline when a count
    drops below −2%;
  - it prints a table, also written to the GitHub job summary, and the
    counts as JSON.
- First scenarios:
  - engine render with 64 clips;
  - transport with loop wraps, locates and the metronome;
  - tempo-map conversions;
  - quality-bar project serialize;
  - project parse.
  Each later milestone adds its hot paths (the arrangement model in M3,
  mixer in M4, MIDI in M5).
- CI runs the gate in a **pinned `ubuntu:26.04` container**. Counts depend
  on glibc, and the container matches the WSL mirror where the baseline is
  made, so a GitHub runner image update cannot move them.
- **Updating the baseline:** when a change is intended, or the toolchain or
  container changes, run `perf-gate --update` on Linux (`perf.sh --update`
  in WSL) and commit `perf/baseline.json` with the reason. Every CI run also
  prints the counts as JSON.

### Consequences
- Instruction counts measure work, not time. Cache misses, SIMD width and
  thread contention do not show. The wall-clock divan benchmarks and the
  quality-bar measurements remain for those.
- Linux only (Valgrind). That is enough for platform-independent code;
  platform code is measured on hardware.

### Evidence (2026-10-07, WSL Ubuntu 26.04, Valgrind 3.26)
- Three runs: the engine and tempo scenarios gave identical counts. The
  project scenarios varied by at most 0.002% (hash-table seeds).
- Injected regression: 4 redundant operations per sample in the clip
  player's edge fade gave **+7.24%** on `engine_clips_64`, and the gate
  exited with code 1.
- The whole gate takes about 14 s under Valgrind.

---

## ADR-0023 — Design system and motion

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 2). Spec:
[`docs/DESIGN.md`](DESIGN.md).

### Context
`CLAUDE.md` §5 asks for a design system before the main UI is built. The
owner asked for fluid, Apple-like motion (2026-10-06). The M2 UI used a
handful of dark-only constants and egui's default style.

### Decision
- **Semantic tokens** (`theme.rs`): one `Palette` per theme (dark default,
  light, and "match system"), plus type, spacing and radius scales. Painting
  code asks for `theme::p().text_secondary`, never a hex value. egui's own
  style (widgets, windows, popups, text sizes) is built from the same tokens
  in `theme::apply`.
- **Windows draw their own title row** (title, close button, Esc) instead of
  egui's title bar. egui tints the active window's title bar with the
  open-combo fill, over a rect that falls short of fixed-width windows. Our
  own row keeps one flat surface.
- **Motion** (`motion.rs`):
  - tokens: `FAST` 100 ms, `BASE` 180 ms, `GLIDE` 220 ms;
  - curves: cubic ease-out, and a critically damped spring for movement
    (no overshoot);
  - helpers that request frames only while something moves;
  - egui's built-in fades and panel slides take `BASE` through
    `style.animation_time`;
  - **Reduce motion** sets every duration to zero.
- The **app menu** (click "Lumen" in the transport bar) holds the theme,
  interface size (100–200%) and Reduce motion. These are saved with the
  settings; `--appearance` and `--scale` are one-off overrides that are not
  saved.
- **Typefaces** (owner's choice from rendered specimens, 2026-10-07):
  - **Inter** for the interface, and **Unbounded** as the companion face
    with personality (wordmark, window and panel titles);
  - both are variable fonts, registered once per weight through egui's
    `wght` variation coordinates; egui's bundled fonts remain as symbol
    fallbacks and the monospace (Hack);
  - licence: SIL OFL 1.1, bundled unmodified, with the licence texts
    (`THIRD_PARTY_NOTICES.md`);
  - candidates the owner rejected:
    - Space Grotesk, Sora and Bricolage Grotesque (too generic);
    - Syne (its G);
    - Instrument Serif, Fraunces, Chakra Petch, Martian Mono, Michroma,
      Krona One, Red Hat Display, Rubik, Archivo Expanded, Big Shoulders,
      Oxanium and Gabarito (shown, not chosen).
  - cost: about 1.6 MB of font data in the binary.

### Consequences
- Every new screen is built on the tokens. A colour written inline is a
  review finding.
- The GPU grid takes its colours per frame and rebuilds its instances when
  the theme changes.
- egui widgets keep their own shapes; only their colours, sizes and radii
  follow the tokens. Custom widgets (transport buttons, the close button)
  are drawn by us.

### Evidence (2026-10-07)
- Tests:
  - contrast (WCAG) of text, secondary text, accent and warning on every
    background in both themes;
  - white clip names on all 12 clip colours;
  - motion curves are monotonic and land exactly on 1;
  - a glide follows exactly when not animating and lands at once with
    Reduce motion;
  - settings round trip with the appearance.
- Screenshots: dark and light at 100%, dark at 200%, light at 150% with a
  clip.
- Idle CPU with the motion system: 0.2% of one core over 10 s (debug build,
  ASIO running).

---

## ADR-0024 — Undo by shared snapshots

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 3).

### Context
`CLAUDE.md` requires that everything is undoable as one coherent step, and
that "undo restores the exact previous state". The quality-bar project has
200 tracks, 20,000 clips and 100,000 notes. Undo must cover every later
edit too: MIDI, automation and plugin state.

### Options
1. **Command objects with inverses:** each edit knows how to undo itself.
   This is compact, but every new edit needs a correct inverse forever, and
   a wrong one corrupts the project silently.
2. **Full snapshots:** a deep copy per step. Exact by construction, but at
   quality-bar size each step costs tens of megabytes.
3. **Shared snapshots** (chosen): the project holds tracks as
   `Arc<Track>`, and each track holds its clips as `Arc<Clip>`. A step is a
   copy of the project's top level (about 200 pointers); an edit clones
   only what it changes (`Arc::make_mut`). Exact by construction and cheap:
   editing one note copies one clip.

### Decision
- `lumen_project::History`:
  - `current` and two stacks of labelled snapshots;
  - `apply(label, merge, edit)` records one step;
  - edits sharing a merge key while a gesture is open (a drag) become one
    step, and `seal()` ends the gesture;
  - `undo`, `redo`, `jump_to` and a version counter for "unsaved
    changes";
  - a step limit (10,000 by default) drops the oldest steps.
- Edits that change nothing record nothing.
- Transport and view state (loop region, zoom, panels) are saved with the
  project but are not undo steps, as in Ableton.
- The `.lumen` format is unchanged by the `Arc`s (serde serialises them
  transparently).

### Consequences
- Edit code mutates through `Arc::make_mut`: a little noise, but the
  compiler catches every place.
- Memory per step is proportional to what the step changed.
- Property tests check the guarantee: random edit sequences, then undo
  all, give the initial project; redo all gives the final one.
- Loading costs one allocation per track and per clip for the `Arc`s:
  parsing the quality-bar project rose by 3.2% in instructions
  (`project_parse`, ADR-0022 gate). The baseline was updated for this
  intended change; saving was unaffected (+0.01%).

---

## ADR-0025 — Tracks, folding and interim volume and pan

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 4a).

### Context
Slice 4 brings real tracks. The owner chose the look from rendered options
(option A's headers with option B's clips, a round record button, and
hints for armed and soloed tracks). During the slice they also asked for
Ableton-style folding and vertical resizing.

### Decision
- **Design:** `docs/DESIGN.md` §7a, drawn by `arrangement.rs` with the
  theme tokens (new: `header`, `clip_body`).
- **Folding:**
  - a folded track is a compact row;
  - an unfolded track is taller, with volume and pan in its header (M7
    adds its automation lanes);
  - dragging the bottom edge resizes, folding or unfolding at a threshold;
  - fold state, heights and the vertical scroll are saved in the
    project's view (format v3).
  - v3 was extended with `expanded` the day it was introduced, before any
    v3 file existed outside the test fixture, which was regenerated once
    for it.
- **Interim volume and pan:** the controls in unfolded headers really
  work, rather than being shown for show:
  - volume multiplies into the clip gain;
  - pan is a balance law in the clip player (unity at the centre, the far
    side fading out), added as `Clip::pan`.
  - The mixer in M4 replaces both with per-track processing, pan laws,
    smoothing and meters.
  - Clicks fixed 2026-10-07 (owner: arming popped; volume and pan drags
    crackled; the first play after opening popped):
    - Every document change resent the timeline, and every resend cut the
      audio and faded it back in. Now the app sends a timeline only when
      tempo or meter change; the engine leaves the audio untouched when a
      new timeline does not move the playhead, and crossfades when it does.
    - Clip levels glide. Clips carry their project id; the player carries
      each clip's level into the next list (a merge over id-sorted lists,
      real-time safe) and moves it at full scale per 20 ms. Muted and
      unheard clips stay in the list at gain 0, so mute and solo fade. A
      clip first heard partway through (its audio finished loading during
      playback) fades in; one heard from its first sample keeps its attack.
    - Cost: `engine_clips_64` +1.95% instructions (about 200 per clip per
      block), an intended change; the baseline was updated.
    - Still abrupt: deleting a clip or track that is playing (rare; it
      leaves the list at once). The M4 mixer moves gain, pan and mute to
      tracks, with smoothing there.
- **Mute, solo and arm are undo steps** (arm included, for consistency).
  Solo is exclusive on a plain click and added with Ctrl+click.
  Buttons act on the whole selection when the clicked track is part of it.
- **Keyboard:**
  - Ctrl+T adds an audio track and Ctrl+Shift+T a MIDI track;
  - Ctrl+D duplicates; Delete removes; F2 renames;
  - Alt+Up and Alt+Down move the selected tracks;
  - all of these are also in the right-click menu.
- **Scrolling:** the mouse wheel scrolls tracks vertically; Shift+wheel or
  a sideways swipe scrolls the timeline; Ctrl+wheel zooms.

### Refinements after MT-APP-9 run 1 (owner, 2026-10-07)
- **Type icons:** a type icon in every header (MIDI tracks had no cue
  besides their name), and more room after the fold arrow. The header
  widened from 184 to 204 pt so names still fit.
- **Unfolded header:** volume and pan became sliders with the value
  inside, instead of labelled number fields. Pan fills from the centre and
  saturates toward the accent as it nears an edge (owner request).
  Typing, double-click reset, Shift for fine and arrow keys all remain.
- **Record arm, "glow and breathe"** (chosen from two rendered options):
  - a red rail that glows into the header;
  - a red lane gradient with thin edges;
  - a ring that breathes every 2.4 s, concentric with the dot.
  - It is the one deliberate loop in the motion system, documented in
    DESIGN §6; it requests frames only while a track is armed, and Reduce
    motion holds it still.
- **Height steps:** heights snap to 22 pt steps (folded 2, unfolded 4 to
  14), so future height-dependent elements can rely on whole steps. Saved
  heights snap when a project opens.
- **Resize handle:** the edge glows with the accent across the header and
  the lane while hovered or dragged.
- **Adding tracks:** a "+ Add track ▾" menu button (chosen over split
  buttons) lists every type with its shortcut, with the hint "Or
  double-click empty space" beneath it. Double-click and the right-click
  menu remain.

### Consequences
- `lanes.rs` no longer draws: it loads audio and builds the engine's clip
  list. `arrangement.rs` owns the view.
- Group folding (hiding child tracks) arrives with group tracks in M4.

### Evidence (2026-10-07)
- Tests:
  - track operations, solo and mute audibility, folding heights, view
    state round trip, selection (click, Ctrl, Shift), keyboard moves;
  - the balance law in the engine; the golden renders unchanged;
  - v3 migration and fixture.
- Screenshots of a five-track project (muted, soloed, armed and unfolded
  tracks): dark at 100%, light at 100% and 150%.

---

## ADR-0026 — Clips on the GPU

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 4b)

### Context
The quality bar asks for at least 60 fps, with a 120 target, while
scrolling and zooming 200 tracks and 20,000 clips, on a mid-range machine.
M3 risk R1 planned clip bodies on the GPU from the start. Before building
it, slice 4b added a frame-time benchmark and measured the egui clips it
would replace.

### Measurement first
`lumen --stress --bench-frames out.json` opens the generated quality-bar
project and drives the view with vsync off: a warm-up, then 600 frames
each of scrolling in time, scrolling tracks and zooming. It reports the
time between frames (p50, p95, p99, worst), then 3 s idle with its
repaints and their causes. It exits with status 3 below 60 fps at p99.
`--bench-size WxH` sets the window. CI has no GPU, so it runs on the
development machine (ADR-0011).

Ryzen 7 7700X with an RTX 4070, p99 in ms (scroll time / scroll tracks /
zoom):

| Window | egui clips | GPU clips |
|---|---|---|
| 1600 × 900 | 1.6 / 2.5 / 6.7 | 1.5 / 1.6 / 1.1 |
| 3440 × 1380 (full width) | 7.5 / 8.2 / 13.4, worst 16.8 | 2.2 / 2.3 / 2.1, worst 4.3 |

egui met the floor on this fast machine at a normal window size. At full
width, zooming out puts thousands of clips on screen: egui then missed the
120 fps target, before any waveforms, so a mid-range machine would miss
60. The cost is tessellating every visible clip's shapes on the CPU each
frame.

Idle: 3 repaints in 3 s, all from the once-a-second device check.

### Decision
- **A box layer in `lumen-render`.**
  - Each box is a time range on a row slot, with body, title-bar, border
    and loading-fill colours and a loading progress (32 bytes).
  - Boxes are uploaded only when the project, the media or the theme
    change, or while a clip fades in or loads.
  - Where rows are goes up separately each frame: top, height and opacity
    per row, in a small float texture read in the vertex shader, so
    scrolling, folding, resizing and their animations upload a few hundred
    rows, never every clip. A texture rather than a storage buffer, so
    every backend can read it in the vertex stage.
  - The fragment shader draws the rounded body, title bar, border and
    loading fill with a signed distance for antialiasing.
  - Edges snap to whole pixels, and the callback's sub-pixel offset is
    passed in (computed as egui-wgpu rounds its viewport), so boxes land
    on the same pixels as egui's shapes: crisp borders, names aligned.
- **Names stay egui text**, drawn only for visible clips at least 24 pt
  wide: narrower, a name shows a letter or two, and at wide zooms
  thousands of names cost more than the clips.
- **Clips loading for import** (a few at a time, sized in points before
  they have a length) stay egui shapes.
- **Every shader is parsed and validated by naga**, so a shader error
  fails CI rather than the app at start-up. Since 2026-10-07 (owner
  request) this happens at build time: shaders are `.wgsl` files checked
  by `build.rs` (`lumen-shader-check`), and each uniform struct has a
  compile-time size assertion against its shader (DEVELOPMENT.md,
  "Shaders").

### Consequences
- Waveforms (slice 4c) and later dense content (notes, automation) can
  use the same row table.
- The GPU and egui looks must match: `clip_box` and `draw_body` take
  their colours from the same `ClipLook`, and screenshots are compared
  pixel by pixel when either changes.
- Frame times are re-measured before a slice that touches the arrangement
  closes (DEVELOPMENT.md, "Frame-time benchmark").

---

## ADR-0027 — Waveforms

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 4c) · The disk cache
is deferred to M9 (owner approved, 2026-10-07)

### Context
Clips need waveforms that stay crisp and fast at every zoom (§6.2 [M]),
in the owner-chosen look: bright waveforms on dark clips (DESIGN §7a).
M3 risk R3: peaks for long files must not stall loading or use too much
memory. Clips are already on the GPU with a per-frame row table
(ADR-0026).

### Decision
- **Peaks** (`lumen-media::Peaks`): minimum and maximum per channel in
  buckets of 16 frames, at the device rate the audio is held at; seven
  levels, each covering four buckets of the one below (up to 65,536
  frames). Stored as signed 8-bit fractions of full scale (minimum
  rounded down, maximum up, so quiet audio shows): about 5 MB per 5
  minutes of stereo. Built on the file's load thread after conversion,
  with its cancel flag.
- **Drawing** (`lumen-render`, in the box layer, on the same row table):
  - peaks go into one atlas texture (4096 texels wide, growing to 128 MB),
    with a small table of where each slot's levels start; files with the
    same peaks share a slot;
  - a wave is a time range on a row showing a slot's audio from a frame at
    a number of frames per beat. A clip spanning tempo changes is one wave
    per constant tempo, since audio plays at its own speed while beats
    stretch;
  - for each pixel, the fragment shader turns the pixel's width into a
    range of frames, picks the finest level whose buckets are no wider,
    and takes the extent of the few buckets underneath (at most six). So
    the waveform is exact at every zoom, with no CPU work per frame.
  - Stereo is left over right when the clip is at least 40 pt tall,
    otherwise one combined waveform.
  - **Pixel-anchored** (fixed after MT-APP-11 run 1: the waveform
    shimmered while scrolling). Columns are counted from the clip's
    pixel-snapped left edge, and the wave area's top and bottom are
    snapped too, so column *k* always shows the same frames at a given
    zoom. Scrolling then moves the waveform by whole pixels with its clip,
    as in Ableton. Measured: a third of a pixel of scroll changed 3.5% of
    the waveform's pixels before (8% at two thirds); after, the image is
    the same shifted by one pixel (21 of 138,000 pixels differ).
  - **Four or more buckets per pixel** (MT-APP-11 run 2: it shimmered
    while zooming). The shader reads the level with at least four buckets
    under each pixel (up to 17 reads), so the buckets straddling a pixel's
    edges decide little and detail no longer flickers as the zoom slides.
  - **Atlas growth is submitted at once** (MT-APP-11 run 2: the waveform
    vanished at the finest zoom). Queue writes run before the command
    buffers of the next submission, so the copy into a bigger atlas,
    recorded in the frame's encoder, landed after the new peaks and
    overwrote them with the old atlas's empty rows; the finest level,
    written first, became a flat line near the start of long files. The
    copy is now its own submission before the writes. A GPU test (skipped
    where there is no GPU) reads the atlas back after it grows; it fails
    with the old ordering.
  - Colour: the track colour, brightened on the dark theme's dark clip
    body (`theme::wave`).
- **Tests:** peaks (bucket extents, rounding, levels, mono and extra
  channels, cancel, memory), the wave mapping (offsets at the device
  rate, tempo splits), atlas span splitting, naga validation of the
  shader, and each shader's uniform size against its Rust struct (a
  mismatch had caused a GPU validation error at the first draw).

### Measurements (Ryzen 7 7700X, RTX 4070)
- Frame times with every clip of the stress project showing a waveform
  (`--stress` now gives its files generated audio), p99 (scroll time /
  scroll tracks / zoom): 1600 × 900 1.2 / 1.4 / 1.2 ms; 3440 × 1380
  1.8–2.0 / 2.1 / 1.9–2.3 ms. Within noise of the same runs without
  waveforms.
- Loading a 5-minute stereo WAV: 100–140 ms in all, of which peaks 21 ms.
- After the run 2 fixes (up to 17 reads per pixel): 3440 × 1380 p99
  2.7 / 2.2 / 1.9 ms, 1600 × 900 1.1 / 1.3 / 1.0 ms; unchanged.

### No disk cache until M9 (owner approved, 2026-10-07)
The plan said peaks would be cached on disk. Until M9 every file is
decoded into memory when it loads (owner decision, 2026-10-06), so peaks
are built from audio already in memory in about 21 ms per 5 minutes, a
small part of decoding (smaller still for compressed formats). A cache
would save that and add files to keep in step. It pays off with disk
streaming in M9, when audio is no longer decoded up front; it is built
then.

### Consequences
- Replaced peaks (after a device-rate change) are not reclaimed in the
  atlas until the app restarts; the atlas holds about 130 five-minute
  stereo files. Compaction comes with streaming (M9).
- The finest level is 16 frames per bucket. Today's deepest zoom is about
  30 frames per pixel at 200%; a deeper zoom (slice 4d or later editing)
  needs sample-level drawing.

---

## ADR-0028 — Navigating the arrangement

**Status:** Accepted · **Date:** 2026-10-07 (M3 slice 4d)

### Context
§6.1 asks for anchored zoom, zoom to fit, follow-playhead modes, pan,
edge auto-scroll, and reordering tracks by drag. The owner chose zoom down
to single samples (as Ableton does) and Follow by page, on by default
(owner-decisions table).

### Decision
- **One horizontal view, `TimeView`** (`timeview.rs`): the beat at the
  left edge and points per beat, in f64. Zooming about an anchor,
  scrolling, fitting, the limits and glides are pure functions, tested.
- **Limits, as in Ableton** (after MT-APP-12 run 1): the song sets how
  far out the view goes and how far right it scrolls. The furthest beat
  in use (the last clip's end, the playhead, the start marker, an enabled
  loop; at least 32 bars), with 10% of room after it, fills the width at
  the widest zoom, and scrolling stops when the view's right edge reaches
  it. A view already past it (clips deleted) stays and can scroll back.
  In, the view goes to 8 points per sample at the tempo in view.
- **Grid lines cover the view wherever it is**, a few screens' worth
  rebuilt only when the view leaves them. Lines stay at least 12 points
  apart (the finest snap step): beat lines go first, then bar lines
  thin to every 2nd, 4th, 8th... bar, as the ruler's numbers do. (The
  first grid was a fixed 512 bars of every beat: past bar 512 it was
  empty, and zoomed out it blurred into stripes.) The adaptive and fixed
  grid with subdivisions is slice 5.
- **Precision at sample-level zoom:** positions go to the GPU as whole
  and fractional beats (`lumen_render::Beats`), and the view's left edge
  likewise; shaders subtract the parts separately, so boxes, grid lines
  and waveform edges stay exact an hour into a song at 8 points per
  sample. The playhead and start marker use the exact position once
  their glide settles.
- **Keys, as in Ableton:** Ctrl+wheel or a pinch zooms at the pointer
  (one gesture is one step back); `+` and `-` glide around the pointer,
  else the playhead, else the middle; X steps back; W fits the song's
  width; H fits every track's height in whole 22 pt steps, and H again
  restores; Alt+wheel steps the height of the track under the pointer.
- **Follow** (`app/navigate.rs`): page turns the view when the playhead
  passes 95% of the width and puts it 2% in; continuous keeps it 40% in.
  Scrolling or zooming by hand, fitting, stepping back and clicking the
  ruler pause it; Play or the button resumes it. A Follow button (a
  playhead line with an arrow) toggles it, right-click chooses the mode,
  Ctrl+Shift+F as in Ableton. Saved in settings.
- **Reordering:** drag a header (the selection, or just that track); the
  dragged headers dim, an accent line across headers and lanes shows
  where they land, the list scrolls near its top and bottom edges, the
  drop is one undo step, Esc cancels. The drag is followed from the
  arrangement's state, so it survives its header scrolling out of view.
- **Sample-level waveforms:** below 16 frames per pixel, each visible
  wave's visible samples (with a screen's width to spare) go to a sample
  atlas; a per-wave table holds the strip and, computed in f64, where its
  first sample falls in viewport pixels. The shader draws the line
  through the samples under each pixel. Strips are reused while
  scrolling within their margin.
- **Ruler:** when bars are wider than 240 points it labels beats and
  sixteenths (bar.beat.sixteenth). Other formats are slice 5.

### Measurements (Ryzen 7 7700X, RTX 4070, `--bench-frames`)
At 3440 × 1380, p99 (scroll time / scroll tracks / zoom): 2.3–3.1 /
2.8–2.9 / 2.2–2.6 ms across three runs; medians 1.4–1.6 / 1.9–2.0 /
0.9 ms. Compared with 4c, about 0.1–0.15 ms more per frame at the median
(the navigation, follow and strip bookkeeping); well inside the 8.3 ms
of 120 fps. Idle: 3 repaints in 3 s.

### Consequences
- At the deepest zooms the ruler labels only sixteenths (KNOWN_ISSUES;
  slice 5 adds time and sample formats).
- Group folding and dragging tracks into groups come with group tracks
  (M4).

---

## ADR-0029 — End-to-end smoke tests

**Status:** Accepted · **Date:** 2026-10-08 (M3, between slices 4 and 5)

### Context
The owner asked for automated end-to-end smoke tests, so that new changes
cannot quietly break what already works, and for them to run at the end
of every slice. Until now the app was covered by unit tests of its logic,
GPU tests in `lumen-render`, and manual tests (MT-*). Nothing drove the
whole app the way a user does.

### Options
1. **egui's own test harness (`egui_kittest`)**, in-process: it runs the
   real `App` frame by frame, finds controls through the AccessKit tree by
   their accessible names, and clicks, types and presses keys. No window
   or GPU is needed, so it runs in CI on every platform.
2. **Driving the real binary** with OS automation (UI Automation on
   Windows): a window and a desktop in CI, slow and flaky.
3. **The binary's own test flags only** (`--smoke-test`, `--screenshot`,
   `--save-as`): true end to end with the GPU, but they cannot click or
   type.

### Decision
Both 1 and 3, in two layers:
- **The E2E suite** (`crates/lumen-app/tests/e2e/`): the whole app with
  egui_kittest (MIT/Apache-2.0, a dev-dependency only, kept at eframe's
  version). Scenarios cover tracks (add from the menu and the keyboard,
  mute, solo, arm, rename, move, delete, undo, redo, keyboard-only
  selection), the transport (Space, the buttons, metronome, loop,
  Follow), navigation (zoom keys, fit, back, the song's limits, track
  heights, Follow turning the page), and files (dropping audio, saving
  and opening, recovering after a crash). It runs in about 3 seconds and
  in CI on Windows and Linux, with the other tests.
- **Real-binary smoke tests** (`crates/lumen-app/tests/smoke.rs`): start
  `lumen` with its window and GPU, draw, import, save, reopen and
  screenshot (checked to be not blank), the quality-bar project, and the
  light theme at 200%. They need a desktop and a GPU, so they are
  `#[ignore]`d in CI and run locally at the end of every slice (a debug
  build, so they never clash with a release Lumen that is running).
- **Isolation:** every run uses its own settings file (new `--settings`
  flag), its own autosave folder, and a **silent test audio device**
  (`--audio test:fake`, `lumen_audio::fake`), which is not listed in the
  device selector. A run never touches the user's settings, autosaves or
  sound card (ASIO devices are often exclusive).
- **Start-up is a value:** `launch::Launch` holds what start-up needs,
  from the command line (`Launch::from_args`) or a test
  (`Launch::for_test`); `App::new` builds the app from it.
- **Where the tests live** (owner review, 2026-10-08): Rust's convention
  puts end-to-end and integration tests in the crate's `tests/` folder,
  where each file is a separate crate that sees only the public API, and
  keeps a binary's `main.rs` thin over a library so they can reach it
  (The Rust Book, ch. 11.3). So `lumen-app` is a library (`lib.rs`, with
  `run()`) and a three-line `main.rs`; the suite is `tests/e2e/` beside
  `tests/smoke.rs`. The tests drive the app through its controls and a
  small public API, `lumen_app::testing` (a clean start, and read-only
  looks at the project, playhead, view and background work), as
  `lumen_project::testing` does for projects; the app's fields stay
  private. Unit tests of private logic stay inline (`#[cfg(test)] mod
  tests`), also per the convention.
- **The rule** (owner): the E2E suite and the real-binary smoke tests run
  at the end of every slice, and each slice adds scenarios for the
  workflows it delivers (`lumen-architecture` skill, DEVELOPMENT.md).

### Found on the way
- **Alt+Up and Alt+Down selected the track above instead of moving the
  selection, and Shift+Up and Shift+Down did not extend it**, whenever
  no control had keyboard focus (keyboard-only use). egui matches
  shortcuts ignoring extra Shift and Alt, so the plain arrow keys, checked
  first, took them. Fixed by checking the most specific shortcuts first;
  covered by `the_keyboard_alone_selects_and_moves_tracks`.
- The "+ Add track" menu items without a shortcut had a trailing space in
  their accessible names; fixed.

### Consequences
- Accessible names are now part of the tested interface: renaming a
  control's accessible name breaks the tests that use it, on purpose
  (CLAUDE.md §5, item 10).
- GPU drawing is not in the CI suite; `lumen-render`'s GPU tests, the
  real-binary smoke tests and the frame-time benchmark cover it locally.
- The test device runs one block per 10 ms poll (about a quarter of real
  time), so tests that play wait on positions, never on wall-clock time.

## ADR-0030 — Grid, rulers, loop, locators and scrubbing

**Status:** Accepted · **Date:** 2026-10-08 (M3 slice 5)

### Context
Slice 5 gives the arrangement its time tools: the editing grid and
snapping, rulers in several formats, the loop brace and punch, named
locators, and Ableton-style scrubbing (an owner request raised from S to
M). The owner chose ruler layout A and both scrub styles from rendered
specimens (owner-decisions table, "Rulers and scrubbing"). This records
the choices made inside those.

### Decisions
- **The grid is a model in `lumen-core`** (`grid::Grid`): adaptive in five
  widths (6 to 64 points per line) or fixed from 8 bars to 1/64, with
  triplet and dotted feels and snap on or off. `snap`, `cell` (the step a
  time falls in) and `lines` are pure and tested, so clip editing (slice 6)
  and the piano roll (M5) share one implementation. Ableton's keys:
  Ctrl+1 to Ctrl+5. Alt bypasses snapping everywhere. The grid is saved
  with the view (format v4).
- **The loop and punch are document edits** (format v4 makes them part of
  undo), as in Ableton: drawing, moving, resizing and toggling the loop
  are undo steps, one per drag. The transport plays whatever the document
  says (`Transport::follow_loop`, every frame).
- **Punch in and out sit at the loop's edges**, as in Ableton, instead of
  a second region: two switches in the loop's menus. Fewer things on the
  strip, and the loop already marks the take. Recording uses them in M9.
- **Locators** are `Project.markers` (format v4), kept in time order. Keys
  (Ableton-compatible where Ableton has one): **M** sets a locator at the
  playhead or removes the one there (Ableton's "Set" button behaviour);
  **Ctrl+Left / Ctrl+Right** jump to the previous or next one (Ableton
  has no default; these are free in Lumen and match common DAWs);
  **F2** renames and **Delete** removes the selected one, taking
  precedence over the track keys while a locator is selected. Default
  names are "Locator N". Marker ids share the project's id space.
- **Scrubbing is an engine mode** (`Command::Scrub { at, style }`,
  `Command::EndScrub`; `lumen-engine/src/scrub.rs`), only while stopped;
  while playing, a drag on the scrub strip jumps playback from step to
  step instead, as Ableton does.
  - **Loop** (default): the grid cell under the pointer (`Grid::cell`,
    whether or not snapping is on), at least 50 ms, loops at normal speed;
    every pass and every change is enveloped over 2 ms.
  - **Tape** (Alt): a read head follows the hand with 30 ms of lag and
    10 ms of inertia, up to 4× speed either way, read from the source in
    256-frame chunks and resampled linearly; it fades to silence below a
    quarter of normal speed, so holding still is silent.
  - A scrub moves the play marker and the playhead to where it is, so
    letting go leaves the playhead there.
  - Real-time safe: the buffers are allocated with the engine; the
    real-time test scrubs both styles under the allocation guard.
- **The strip** (24 pt) holds the scrub strip and locator flags in its top
  14 pt and the loop brace in its bottom 10 pt; flags take precedence over
  scrubbing, and the brace has its own band (DESIGN §7d).

### Consequences
- `Source::render` is now also called at arbitrary positions while
  stopped. The clip player already rendered by position; disk streaming
  (M9) must keep random access cheap or serve scrubs from a cache.
- Loop and punch changes are undo steps: Ctrl+Z after drawing a loop
  removes it, which matches Ableton.
- The performance gate covers the engine with the scrub mode idle: one
  branch per block.

### Evidence
- Unit tests: `lumen_core::grid` (10), the scrubber (loop passes, short
  steps, crossfaded changes, tape speed, backwards, the speed cap, silence
  when still), locator order and undo, previous and next.
- Engine tests: `scrubbing_while_stopped_plays_the_step_and_moves_the_markers`,
  `scrubbing_never_touches_the_heap`.
- E2E: grid, rulers, looping, locators and scrubbing scenarios
  (`crates/lumen-app/tests/e2e/`).
- By ear: MT-APP-13.

