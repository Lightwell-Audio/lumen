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
| [0012](#adr-0012--lumens-own-license) | Lumen's own license | **Superseded** 2026-10-09 by ADR-0039 (was: closed source) | — |
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
| [0031](#adr-0031--curves-for-fades-and-envelopes-format-v6) | Curves for fades and envelopes (format v6): a cubic Bézier from silence to full, edited by one free dot with Alt for S-shape, played from tables, drawn on the GPU | Accepted 2026-10-08 | No |
| [0032](#adr-0032--clip-editing-m3-slice-6) | Clip editing (M3 slice 6): the split-clip model, drags redone from where they started, Ableton's overlap rule, clip properties and the pinned card, drops on a chosen track, Esc for the field being typed in | Accepted 2026-10-08 | No |
| [0033](#adr-0033--commands-and-the-keymap-m3-slice-7) | Commands and the keymap (M3 slice 7): one table of commands, chords matched exactly, one command per key whatever is selected, one dispatcher | Accepted 2026-10-08 (7a) | No |
| [0034](#adr-0034--the-info-line-m4-slice-0) | The info line (M4 slice 0): hover hints leave the pointer for one strip along the bottom of the window, with K5 keycaps and lit modifiers | Accepted 2026-10-09 | No |
| [0035](#adr-0035--the-mixer-graph-m4-slice-1) | The mixer graph (M4 slice 1): a node per track with a stereo bus, compiled on the UI side into a flat ordered plan, swapped at a block edge with levels carried by id; plans of a new shape crossfade over 5 ms | Accepted 2026-10-09 | No |
| [0036](#adr-0036--routing-in-the-model-format-v7) | Routing in the model (format v7): a master track kept apart from the tracks, outputs, sends, polarity, trim and the pan law; one routing rule shared by validation, edits and the engine | Accepted 2026-10-09 | No |
| [0037](#adr-0037--group-tracks-format-v8) | Group tracks (format v8): a group's tracks always follow it in the list; grouping, ungrouping, folding, nested rails, lane bars, arming and deleting a group | Accepted 2026-10-09 | No |
| [0038](#adr-0038--mute-solo-returns-master-and-cue-m4-slice-4-format-v10) | Mute, solo, returns, master and cue (format v10) | Accepted 2026-10-09 | No |
| [0039](#adr-0039--lumens-licence-gpl-30-or-later-supersedes-adr-0012) | Lumen's licence: GPL-3.0-or-later, sold packaged binaries (supersedes ADR-0012) | Accepted 2026-10-09 | Yes (owner items) |
| [0041](#adr-0041--sidechains-and-delay-compensation-m4-slice-6) | Sidechains and delay compensation: devices and taps in the graph, delay lines, read ahead, the D1 readout | Accepted 2026-10-10 | No |
| [0042](#adr-0042--the-m4-exit-walkthrough-and-known-issues-m4-slice-7) | The M4 exit walkthrough; edits taken while stopped; session locks for autosaves; submenu names; the pipeline cache off the UI thread | Accepted 2026-10-10 | No |
| [0014](#adr-0014--repository-layout-and-ci) | Repository layout and CI | Accepted: **private GitHub repository** (owner creates it) | — |

---

## Decisions made by the owner (2026-10-05 onwards)

| Question | Decision |
|---|---|
| Lumen's licence (ADR-0039) | GPL-3.0-or-later, open source; the signed packaged binary is sold. Supersedes ADR-0012 (closed source). |
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
| Drag hints (MT-APP-13 run 1, 2026-10-08) | Chosen from rendered specimens: **A1** the loop brace grows a bracket at the edge a drag will resize, lights when it will move, and a chip names the action; **B2** (on trial) the scrub strip washes the step a drag would play and shows a quiet hint ("drag to listen · Alt tape"), and a tape scrub shows its speed; **C1** the grid button always shows the size, with a magnet lit while snapping and struck through when not. |
| Clip drag model (M3 slice 6, 2026-10-08) | **Split clip**, as in Bitwig, after comparing it with Ableton's (no marquee) and the classic model (range behind a modifier): the top half of a clip moves it (Ctrl copies), the bottom half selects a time range across tracks, empty lanes draw a marquee. Cursors and hints show which half is under the pointer. |
| Selection look (M3 slice 6, 2026-10-08) | Chosen from rendered specimens: **S3 glow**, selected clips glow in the accent like the record arm, a time range is a band with accent brackets along its top, the marquee a soft rounded accent box ("some character"); **H1**, hovering a clip brightens the half under the pointer with a dashed line at the split. |
| Fade and gain handles (M3 slice 6d, 2026-10-08) | Chosen from rendered specimens: **F3 with the glow**, a fade is a glowing accent curve with a round handle at its top and a dot that bends it; **G1**, clip gain is a glowing level line across the clip, dragged up or down, with the waveform shrinking under it and the dB beside the pointer. |
| Free curves (M3 slice 6d, 2026-10-08) | The owner asked for curves that move freely, left and right as well as up and down, "as flexible as Ableton", to set the bar for automation later. Chosen from interactive specimens: **C3**, one free dot the curve passes through, with Alt-drag adding S-shape (over C1, the dot alone, and C2, two Bézier handles). Replaces the stepped bend (Slow, Linear, Fast). ADR-0031. |
| Clip properties (M3 slice 6e, 2026-10-08) | Chosen from rendered specimens: **M1**, a muted clip drains to grey as in Ableton (name and waveform stay readable); **K3**, a locked clip shows a padlock in its title bar and light clamp bars at both ends; **P1**, the properties view is the hover card pinned beside its clip and edited in place (Alt+Enter, or Properties in the right-click menu), after seeing it in context, over a strip under the arrangement (P2) and a toolbar above the clip (P3). ADR-0032. |
| Lanes menu and grid behind clips (MT-APP-14 run 2, 2026-10-08) | Chosen from rendered specimens: **A**, each right-click gets the menu for what it landed on, as in Ableton (a clip, a time range, or empty lanes; commands that cannot run are left out), over B (grouped submenus) and C (an action strip); **G1**, the grid shows through a see-through clip body, closest to Ableton (the owner: G1 unless there is a strong argument for G2; none held up), over G2 (lines drawn across clips) and G3 (bar lines only). The owner also asked for a click anywhere on the lanes to move the playhead, as in Ableton. ADR-0032. |
| Hints and tooltips (2026-10-08) | The owner noted that hints have piled up slice by slice: behaviour should come across without reading much (cursors, handle shapes, motion before text). **A dedicated UX pass**, scheduled when the next milestones are planned; until then new hints stay short. |
| Split and tools (2026-10-08) | The owner asked to select a clip and drag it to cut it. **Split** is planned in M7 with the rest of the split work, and the drag-to-cut gesture is chosen there from rendered specimens. The tool model (pointer, draw, erase, split, mute, zoom; CLAUDE.md §5) is proposed for M7 with it, for the owner to confirm. |
| Commands and shortcuts (M3 slice 7, 2026-10-08) | Chosen from interactive specimens: palette **P2 with P3's values** (context names such as "Duplicate 2 clips", commands that cannot run listed dimmed with the reason, a detail strip, typed values becoming actions); keycaps **K5 area light** after asking for more personality than the first round (plain modifiers, the main key outlined with a light in its area's colour); shortcut editor **E2 with E3** (a keyboard map lit by area over the list, plus rebinding from a palette result); **Shift+Tab depends on focus**, F6 and Shift+F6 between panes (ADR-0015). |
| Clips from the keyboard (M3 review H2, 2026-10-08) | **The arrows follow the selection**, as in Logic and Cubase: with a clip selected ←/→ select the previous or next clip on its track and ↑/↓ the nearest clip on the track above or below, Shift adds; with tracks selected the arrows act on tracks as before (over Alt+←/→ only, and [ and ]). ADR-0033. |
| M3 close (2026-10-09) | M3 approved after MT-APP-16 passed. The M4 plan (`docs/milestones/M4.md`) waits for approval and its three questions. |
| M4 plan (2026-10-09) | Approved. **Solo: Ableton's by default** (a click solos only that track, Ctrl+click adds). **The UX pass on hints runs first**, as M4 slice 0. Pan law −3 dB constant power by default (the proposal, recorded as an assumption). |
| Group tracks (M4 slice 3, 2026-10-09) | Chosen from rendered specimens and asked before building: **H1**, a group's tracks step in 12 pt a level with every enclosing group's colour rail running down beside them (over H2 tree lines and H3 a tinted block); **L1**, a group's lane shows a thin bar per track inside it in that track's colour (over L2 a summed band with a waveform and L3 ghost clips); **deleting a group deletes its tracks** (Ableton; Ungroup keeps them) over moving them up a level; **a group's arm button arms its tracks** (Cubase-style, half a ring when some are armed) over no arm on groups (Ableton). Also proposed and not objected to: the Ableton rules for Ctrl+G, Ctrl+Shift+G, the fold arrow, drops and groups holding no clips (ADR-0037). |
| Group tracks, MT-MIX-3 run 1 (2026-10-09) | The owner's findings: **groups go three deep at most** (Group › Group › Group › tracks; "needing more than that probably doesn't do much"), refused with a note; **the track header column is resizable**, its width **saved per project** (over app-wide), 204 to 480 pt; soloing a track in a group must not silence it (a bug, fixed). ADR-0037 amendment. |
| Routing in the model (M4 slice 2, 2026-10-09) | Asked before building: **a new send starts at −∞** (over 0 dB) and **post-fader** (over pre-fader); **deleting a return removes its sends** in the same undo step (over asking first); **the pan law is chosen from the mixer panel's menu** and the palette (over a project settings window, M9). |
| Dropouts (2026-10-09) | Chosen from mockups: **A, recent and in words**: no chip while audio is healthy, "Dropouts ×3" in amber while they happen, fading after 10 s of clean audio, details in the audio panel; over B (a labelled total until reset) and C (an amber status dot). Driver resets no longer count (ADR-0020 update). |
| Mixer behaviour (M4 slice 1, 2026-10-09) | Asked after the graph was built (the owner: variants first, before building): **centre −3 dB** (a hard-panned side at unity; projects from M3 play 3 dB quieter) over centre unity with +3 dB sides; **balance** panning of stereo tracks over a true stereo pan; **mute silences every send**, pre-fader ones too (Ableton) over pre-fader sends playing on (Pro Tools, Cubase); **post-fader sends after the pan** over before it. All four as built (ADR-0035). |
| Hints (M4 slice 0, 2026-10-09) | Chosen from interactive specimens: **an info line** along the bottom of the window (C, Ableton's Info View in spirit) over quiet chips (A) and rest-to-reveal (B), **with the K5 keycaps** the palette and menus use (the owner's addition, for consistency), **no chips for held keys** (the cursor and the lit action say it), full width (ADR-0034). |
| Mute, solo, returns, master and cue (M4 slice 4, 2026-10-09) | The solo rules agreed as a table of 14 cases before building (`docs/milestones/M4.md` §5): solo keeps a soloed track's path open, **mute always beats solo** along the path, returns are solo-safe, **a soloed return plays wet only** (sends into it stay on, their tracks' outputs go silent), the master, metronome and cue are never touched by solo. **D1 A** a group's solo solos the group (its tracks show it hollow); **D2 A** additive solo is a preference (Ctrl+click then replaces); **D3 A** a lit "S" chip in the transport while anything is soloed, a click clears; **D4 A1** returns then the master pinned at the bottom under a divider, a "Returns" caption that folds them, returns lettered A, B, C and never grouped; **D5 C1** Ableton's Solo/Cue switch on the master, cue **pre-fader** to a chosen pair, unavailable with a note when that pair is the main one; **D6** the metronome has its own output (Main, Cue or a pair), never through the master; **D7** palette commands Solo and Mute selected tracks and Clear all solos, no default keys. Chosen from interactive specimens: **sends as K1 "comet" knobs** (a trail of light in the return's colour, brightening to a glowing head; a dot marks pre-fader; hovering a send lights its return and the reverse) over lantern, droplet and plain knobs; **a track silenced by another's solo shows a hollow M and a dimmed name** (hollow S inside a soloed group) over dimming the whole row. Asked while building 4d: **O1**, the cue output chosen in the audio settings beside the main pair, over Ableton's per-pair output configuration (so the metronome's output is Main or Cue); the owner's interface has two outputs, so cue is checked on the four-output test device. MT-MIX-4 runs 1 and 2, from mock-ups: **C4** knobs step up on selected rows, **O1** a "+N ▾" button drawn as "+ Add track ▾", **R2** an unfolded return lists its senders, **L4** its four loudest then "+N more", **S1** silent sends leave that list when let go; **12 returns at most**; the standard +6 dB of an empty return kept. |
| Mixer panel and meters (M4 slice 5, 2026-10-09) | Chosen from interactive specimens: **S1 console** strips (fader and meter side by side, 76 pt) over S2 light rail and S3 broad; sends **O2**, two knobs then a "+N ▾" list (over a grid of six, or every knob in a taller strip); meter colours **M1 accent by default, M2 track colour as a setting** (over M3 classic green); loudness **L2**, a column beside the master; header meters as well, **H2** inside the volume slider (over H1 at the edge and H3 the colour rail, which already carries arm and nesting). Behaviour, all as recommended: **B1** post-fader after the pan; **B2** peaks fall 20 dB/s, a 1.5 s hold, clip latched until clicked; **B3** a latching wheel; **B4** one fold with the arrangement; **B5** integrated loudness resets on Play from stop; **B6** any output pair; **B7** sections fold away in a short panel; **B8** strips drag to reorder, returns included. ADR-0040. |
| Sidechains and delay compensation (M4 slice 6, 2026-10-10) | Behaviour asked before building: **read ahead** (what is heard lines up with the playhead; live input in M11 gets a reduced-latency monitoring option) over delaying everything and moving the drawn playhead; sidechain taps **Ableton's three** (Pre FX, Post FX by default, Post Mixer; FX taps ignore mute) over post-fader only; **sidechains saved with the first device** that uses them (format v11 then) over saving them now. The readout from rendered mock-ups: **D1**, only while there is compensation, at the end of the device line, the path in the info line, always in the audio panel (over D2 always shown and D3 folded into the latency). ADR-0041. |
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
2. Use the **GPLv3 option**. Only possible if Lumen itself is GPLv3 (ADR-0039, now true; the SDK's current terms still need confirming).
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

**Status:** Superseded 2026-10-09 by ADR-0039 (GPL-3.0-or-later). Was: accepted 2026-10-05 as closed source (proprietary). The options below are kept as context.

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

**Status:** Accepted (owner, 2026-10-08: option 3, built in M3 slice 7) ·
**Date:** 2026-10-06

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

### Update 2026-10-09: dropouts told in words, and resets not counted
The owner saw a bare amber number ("⚠ 72") climb while changing the
Focusrite's buffer size, and it meant nothing without context.
- **Resets were counted.** The Focusrite's ASIO driver keeps counting
  samples through a reset, and the meter counted the gap to the reopened
  stream's position as skipped buffers (a simulated reset counted 78,122).
  A stream that opens now sets `Status::resync`; the next block starts the
  position count afresh. Test `a_driver_reset_is_not_counted_as_dropouts`.
- **Told in words** (owner's A of three mockups, over a labelled total and
  an amber status dot): nothing while audio is healthy; "⚠ Dropout" or
  "⚠ Dropouts ×3" as an amber chip after the CPU readout while they
  happen, fading out after 10 s of clean audio; clicking it opens the
  audio panel, which says "3 dropouts in the last minute · 72 since audio
  started" (or "since you reset") with what they mean and that a larger
  buffer helps. The mockup's clock time ("since 14:02") became "since
  audio started": Lumen has no local-time formatting, and it was not worth
  a dependency. Episodes and the last-minute tally are pure logic in
  `transport/dropouts.rs`; the chip asks for one repaint when it is due
  to go, so idle stays idle. E2E `dropouts_show_in_words_and_the_panel_explains_them`
  (through `testing::cause_dropouts`); screenshots in `tests/shots.rs`
  (`dropouts`: dark 100%, light 150%, dark 200%).
- **Second run (owner):** the panel's warning ("344 dropouts since audio
  started") stayed long after the audio was clean. The panel now warns
  only while the chip shows ("3 dropouts just now · 344 since audio
  started"), and the total stays a plain number among the meters; the
  last-minute tally went. Its tooltip tells the two checks apart (buffers
  the driver skipped, audio slower than real time; `Status::slow_dropouts`).
  ASIO resync requests were acknowledged but ignored: a driver that
  resyncs jumps its sample position, which counted as skipped buffers (a
  simulated resync counted 19,533). The block after a resync now reports
  no position, and a block without one restarts the count. Test
  `a_driver_resync_is_not_counted_as_dropouts`.

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

### Amendment (2026-10-08, owner: the followed playhead was choppy)
The engine publishes its position once per audio block, and blocks do not
line up with frames (512 samples at 48 kHz is 10.7 ms, a frame 16.7 ms),
so the playhead, and the view Follow moves with it, stepped by one block
on one frame and two on the next. Now the drawn playhead runs on the frame
clock at the sample rate (`playhead.rs`) and leans towards the engine's
position with a 0.5 s time constant, so the block steps average out. It
never runs backwards or more than 0.1 s ahead (a stalled device). A jump
of more than 0.25 s (Play, a locate) starts it again at the engine's
position, and a loop wrap moves it back by exactly the loop. Stopped, it is
the engine's position exactly. Only drawing and Follow use it; commands
(locators, the position field) still read the engine.
Evidence: unit tests in `playhead` (even steps, which fail on the raw
position; jumps; loop wraps; stopping; a stalled device); E2E
`the_drawn_playhead_keeps_up_while_playing_and_lands_exactly_on_stop`.

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
- **Hints for drags** (MT-APP-13 run 1, the owner's choices A1, B2, C1):
  a hint names what a drag will do (`widgets::draw_hint` chips on the
  brace, quiet captions on the scrub strip), and the grid button shows a
  magnet for snapping. The brace's edge zone grew from 5 to 8 points (at
  most a third of the brace), and what a drag grabs is decided where the
  button went down, not where egui first reports the drag a few points
  later (that turned edge grabs into moves). Right-clicking the scrub
  strip adds a locator there without moving the playhead, as in Ableton.
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
- Performance gate (WSL, `b3ccb4b`): engine_transport +0.46%,
  engine_clips_64 +0.00%, project_parse +0.03%; all inside +2%.
- By ear: MT-APP-13.

---

## ADR-0031 — Curves for fades and envelopes (format v6)

**Status:** Accepted · **Date:** 2026-10-08 (M3 slice 6d)

### Context
Slice 6d first gave fades four named shapes, bent in steps. The owner
asked for curves that move freely ("as flexible as Ableton") and that set
the bar for envelopes in general, ahead of automation (M7). A fade's shape
must be stored, played on the audio thread, drawn on the GPU, and edited
with one handle (the owner's choice C3).

### Options
1. **More named shapes, or one bend amount** (Ableton's fade slope). Simple
   to store, but the bend cannot move left or right.
2. **A curve through a stored point** (a spline through the handle's dot).
   Natural to edit, but S-curves need a second parameter anyway, and
   curves through points can overshoot.
3. **A cubic Bézier from (0, 0) to (1, 1) with two inner control points**
   (as CSS easing curves). Any smooth monotone shape; the four named shapes
   are exact cases (linear, x², 1 − (1 − x)², smoothstep); a familiar
   model for automation segments too.

### Decision
Option 3, as `lumen_core::curve::Curve`, saved as four numbers.
- **Editing (C3):** one handle, a dot plus an S amount. The control points
  sit either side of a centre, a sixth apart across; the curve's middle
  point (t = ½) is the dot, so the curve passes through it; Alt-drag
  spreads the points up and down for S-shape without moving the dot. The
  dot stays in the middle three quarters, which keeps every control point
  in the unit square: the curve always rises from silence to full. The
  four presets are exactly reachable, and `Curve::handle` gives back the
  handle of any curve it made.
- **Format v6:** a fade's `shape` name becomes `curve` (written only when
  not straight). The migration maps each name to its exact curve, so old
  projects sound the same; a golden v6 fixture holds a freely bent curve.
  Version 5's migration now writes its own fade words literally, so it
  stays true when later versions change fades.
- **Playing:** the audio thread does no solving. The UI side builds a
  1,024-step table per distinct curve (cached and shared by clips and
  lists), and the clip player reads it with linear interpolation: within
  about 10⁻⁶ of the curve except in its steepest first step.
- **Drawing:** the waveform shader takes each fade's four numbers and
  finds the level by halving (14 steps per pixel inside a fade only); the
  arrangement draws the curve with `Curve::at`.

### Consequences
- Automation segments (M7) use the same `Curve` and the same handle, so
  envelopes bend alike everywhere.
- A curve built some other way (a future two-handle editor) is still a
  valid curve; the C3 handle then reshapes it into its own family on the
  first drag.
- Wave instances grow to 108 bytes (uploaded only when the project
  changes).

### Evidence
- Unit tests: `lumen_core::curve` (presets exact to 10⁻⁵, every handle
  curve rising, the dot on the curve, handles round-tripping, saving),
  `lumen_core::fade`, the v5 → v6 migration, golden v5 and v6 fixtures.
- Engine: `fades_sound_as_drawn_in_every_shape_and_free_curve` (within
  2·10⁻⁵ of the drawn curve at five block sizes); fades under the
  allocation guard in `realtime.rs`.
- E2E: `the_dot_bends_a_fade_freely_and_alt_adds_s_shape`, and the
  listening scenario `fades_and_gain_are_heard`.

## ADR-0032 — Clip editing (M3 slice 6)

**Status:** Accepted · **Date:** 2026-10-08 (M3 slice 6, parts 6a to 6e)

### Context
Slice 6 makes clips editable on the arrangement: selecting, moving,
copying, time ranges and their edits, trimming, looping, slipping, fades
and gain, and, in 6e, a clip's own properties (mute, lock, name, colour)
and dropping files onto a chosen track. Every edit must be one undo step,
work from the keyboard as well as the mouse, and never click while
playing. The owner chose each look from rendered specimens (owner-decisions
table: clip drag model, selection look, fade and gain handles, free
curves, clip properties). ADR-0031 covers the curves.

### Decision
- **Split-clip model** (owner): a clip's top half moves it (Ctrl copies,
  Ctrl+Alt slips), its bottom half selects a time range across tracks,
  empty lanes draw a marquee; sides trim (Shift loop-extends); top corners
  fade; a line sets gain.
- **A drag redoes its edit from the project as the drag found it**, every
  frame, merged into one undo step: passing over a clip leaves it whole,
  and the result does not depend on the frame rate.
- **Ableton's overlap rule:** a clip put down on others (moved, copied,
  trimmed, pasted, or dropped from Explorer onto a track) cuts away what
  it covers (`lumen_project::edit::overwrite`).
- **Moved clips crossfade while playing** (5 ms in the engine), so
  dragging a playing clip does not crackle.
- **Clip properties (6e):**
  - mute (0, the menu, the card): the clip's colour drains to
    `theme::CLIP_MUTED` (M1); the engine already plays muted clips at gain
    0;
  - lock (the menu, the card): a padlock and clamp bars (K3). A lock keeps
    a clip in place, so locked clips can still be muted, renamed and
    recoloured; moves, trims, slips, fades, gain and Delete leave them
    alone;
  - rename: Ctrl+R (Ableton) or F2 opens a field in the title bar; a clip
    out of view is renamed in its card instead;
  - colour: the twelve track colours as swatches, and "Track colour" to
    follow the track again. Clips now draw in their own colour (format v5
    already saved it);
  - **the card (P1)**: an `egui::Area` placed each frame beside the
    clip's part in view (right, else left, kept across the lanes and
    inside the window), so it follows scrolling. It holds name, colour,
    gain, both fades (length in milliseconds through the tempo map, and
    shape), mute and lock, and the file's details. Tab walks its fields,
    so gain and fades no longer need the mouse. It follows the selection:
    another clip moves it there, none closes it, as do Esc and ✕. Gain
    and fades are greyed out on a locked clip, with the reason.
  - No format change: the fields existed since format v5.
- **Drops on a chosen track (6e):** over an audio track's lane, dropped
  files land on that track at the pointer (snapped), one after another
  when several; within 7 points of the line between two tracks, below the
  last, or over a track of another kind, they make new tracks there.
  `DropInto` travels with each load; files dropped together onto a track
  import together once all have loaded, as one undo step. While files are
  dragged over, the lane lights with a ghost clip, or an accent line marks
  where new tracks go.
- **Where the pointer is during a drag from the system** (MT-APP-14
  run 3: the owner was never offered a place between tracks). On Windows
  the window gets no pointer moves while Explorer drags files over it,
  and winit 0.30 drops the position the system reports with each drag
  update (`IDropTarget::DragOver`), so egui had no pointer and every drop
  went to new tracks at the end. While files hover, Lumen reads the
  system's cursor itself (`GetCursorPos`, `ScreenToClient`;
  `Owner::cursor`) and repaints each frame, so the lit lane or the line
  follows the pointer. Other systems keep egui's pointer (see
  KNOWN_ISSUES).
- **One title row for every floating window** (`widgets::title_row`, used
  by `Dialog` and the card). The ✕ is centred on the title's first
  letter rather than on its line, whose lower part is room for
  descenders: centred on the line, it sat 5.5 points above the letters in
  the card and in every dialog (owner, MT-APP-14 run 3).
- **Menus for what was clicked** (the owner's A, after MT-APP-14 run 2):
  a right-click inside the time range opens the range's menu, on a clip
  the clip menu (11 rows; both fade shapes in one Fades submenu), on
  empty lanes Paste, the grid (one submenu) and Snap to grid. Commands
  that cannot run are left out rather than greyed. An open menu takes
  Esc before the clip keys do, and drag hints hide under menus.
- **The grid behind clips** (G1): clip bodies are drawn at 55% opacity
  (`BODY_OPACITY`), title bars and waveforms solid. The specimen's 82%
  showed the grid by about one level of 255 in the dark theme, where the
  body and the lanes are close in colour; 55% shows it at about half its
  strength outside clips.
- **A click on the lanes moves the playhead** (owner, as in Ableton):
  snapped, Alt places freely, Follow pauses as for a ruler click; on a
  clip it also selects it; Ctrl and Shift clicks only build a selection.
- **Esc belongs to the field being typed in.** egui takes a text field's
  focus away on Esc before the app's keys run, so the clip keys used to
  see no field and take the Esc to clear the selection, and the field's
  loss of focus then applied what was typed. The app remembers whether a
  field had the keyboard at the end of the last frame (`App::typing`) and
  leaves that Esc to it. Found while testing 6e's rename.

### Consequences
- Every clip edit is a named `Document` method with tests; drags pass a
  merge key.
- The card is the keyboard way into a clip; the detail panel's clip tab
  (M5) can take its fields later without changing the model.
- New tracks made by a drop between tracks show as loading rows at the
  bottom until their files load, then join at the chosen place.

### Evidence
- Unit tests: `document::clips` (moves, copies, fades, gain, and
  `clip_properties_are_single_steps_and_work_on_locked_clips`),
  `files_land_on_the_track_or_between_the_tracks_they_were_dropped_on`,
  `arrangement::drop`, `app::clip_card` (fade milliseconds, card place).
- E2E (61 scenarios): `clips`, `editing`, `fades`, `looping`, and
  `properties` (0 mutes, the menu locks, Ctrl+R and F2 rename with Esc
  cancelling, the card from the keyboard, drops onto and between tracks
  with egui given no pointer during the drag, as on Windows, and
  `the_cards_close_button_is_level_with_its_title`),
  `each_right_click_gets_the_menu_for_what_it_landed_on`,
  `a_click_on_the_lanes_moves_the_playhead`.
- Screenshots (`tests/shots.rs`: `clip_properties` in dark at 100% and
  light at 150%, `file_drops`, `lanes_menus`).

### Update 2026-10-08: dropped files keep their order
Files dropped together onto new tracks joined the project in the order
their loads finished, so dropping kick.wav and snare.wav could make the
snare's track first (found in the M3 review: the E2E fixtures that drop
two files failed now and then under load). Ableton keeps the dropped
order. Files for new tracks now join from the front of their queue
(`Lanes::join_loaded`): each once every file of its own drop and of
earlier drops has loaded or failed, so a drop is one undo step and a fast
file waits for a slower one dropped ahead of it, shown as a loading row
in its place. Files dropped onto a track already waited for their whole
drop. Unit test
`files_dropped_together_join_in_drop_order_whatever_order_they_load_in`
(loads finishing out of order, across two drops); E2E
`files_dropped_together_keep_their_order`, and the fixtures
`with_two_clips` assert the dropped order.

---

## ADR-0033 — Commands and the keymap (M3 slice 7)

**Status:** Accepted · **Date:** 2026-10-08 (7a to 7d)

### Context
Slice 7 brings the command palette, remappable shortcuts with clash
detection, and tooltips and menus that show shortcuts. Until now each
feature read its own keys: eleven functions across the app, each with
`consume_key` checks. egui matches a plain key with Shift or Alt held, so
each function had to order its checks (Alt+Up before Up), and three keys
meant different things in different places (Ctrl+D, Delete, F2, Esc),
decided by which function ran first. Shortcut text in menus and tooltips
was written by hand next to the code that handled the key.

### Options
1. Keep the handlers and add a list of shortcuts for display. The list
   and the handlers drift apart, and nothing can be remapped.
2. A command per context ("Delete clips", "Delete tracks"), each with its
   own keys. The same chord is bound twice, so "no clashes" cannot be a
   rule.
3. **One table of commands; a command that acts on what is selected;
   one dispatcher.**

### Decision
Option 3.
- `commands/mod.rs` holds every command once: a stable id
  (`edit.duplicate`, for saved keymaps), a name, an area (for grouping and
  the K5 keycap light) and its default chords, Ableton's where it has one.
  A test keeps the table in the enum's order.
- `commands/keys.rs`: a `Chord` is Ctrl (Cmd on macOS), Shift, Alt and a
  key, matched **exactly**: Alt+Up and Up are different chords, and
  Shift+Delete is no longer Delete. The `Keymap` maps chords to commands
  and reports clashes; the defaults have none (a test).
- A key that acts on the selection is one command: Delete deletes the
  selected clips or time range, the locator, or the tracks; Duplicate,
  Rename (Ctrl+R and F2, now for tracks and locators too) and Deselect
  (Esc) likewise. Selections are one kind at a time
  (`App::sync_selection`), so the meaning is never ambiguous.
- `app/commands.rs`: `availability` says whether a command can run now,
  and why not in a few words (for the palette's dimmed rows); `run` runs
  it, for keys, the palette and menus alike; `command_keys` is the one
  dispatcher. It skips while a text field has the keyboard, leaves a
  focused control the keys it uses itself (`Chord::used_by_controls`:
  Space, Enter, Tab, the arrows, Home, End and the page keys, without Ctrl
  or Alt; 7d), runs the command bound to each
  chord pressed when it can run, and takes that key from the input so no
  widget also sees it. A key whose command cannot run is left for others.
- View commands need the lanes' size and the pointer, so they queue a
  `ViewStep` that the arrangement runs when it draws.
- The detail panel's chord is read from the keymap in the raw-input hook,
  before egui moves focus with it (ADR-0015).
- Menus take names and shortcut text from the table and the keymap.
- **The palette (7b, the owner's P2 with P3's values).** Each command
  has a one-sentence `about` in the table for the palette's strip. The
  rows are built by `App::palette_rows`, which the drawing and the tests
  share: typed values first (a tempo and a bar for a number), then
  commands and track or locator names by fuzzy score (subsequence,
  consecutive letters and starts of words count more; the table's order
  on ties); with nothing typed, recent commands first. A command's label
  says what it will act on (`App::command_label`: "Duplicate 2 clips",
  "Undo: Move clip"), and its search still matches the plain name, so
  "clip prop" finds "Properties of “kick”". Commands that cannot run stay
  listed with `availability`'s reason; Enter on one keeps the palette
  open and shows the reason in the strip. Running a row closes the
  palette first, so a command that focuses a field (Rename) gets the
  keyboard. Tempo goes through a new `Document::set_tempo` (one step, a
  drag merged), also used by the transport's tempo field, which edited
  the project in an inline closure before. "Go to track" queues a
  `ViewStep::Reveal`, which selects the track and scrolls just enough to
  show it.
- **Keycaps (the owner's K5)** are `widgets::keycaps`: the light under
  the key is the command's area colour (`Area::light`, one of the track
  colours).
- **Remapping (7c, the owner's E2 with E3).** `Keymap::set` refuses a
  chord another command has and names that command; the window shows the
  clash on the row with Reassign (`Keymap::reassign`, which takes it) or
  Cancel. A command's chords are slots: clicking one records over it,
  "+" adds one, Remove takes one away, ↺ resets the command (taking its
  defaults back from whoever has them), Reset all restores everything.
  Recording is one state for the window and the palette
  (`App::rebind`): `capture_chord` runs first each frame and takes every
  key and letter typed, so a chord being recorded never also runs its
  command or types into a field; the dispatcher and the detail panel's
  raw-input hook stand aside while it lasts, so even Shift+Tab can be
  recorded. Esc cancels, and with a clash waiting Enter reassigns. In the
  palette, Ctrl+Enter on a row (or a click on its keycaps) records there.
- **The keyboard map** (`app/shortcuts.rs`) is drawn from a table of
  keys: a 15-key main block and the arrows with the keys above them.
  Keys bound on the layer shown (Ctrl, Shift and Alt chips, or held
  modifiers) are outlined with the command's name, ellipsized, and the
  area's light; clicking a key searches the list for its chord, and a
  free one says so.
- **Saving.** The settings gain a `commands` section: the shortcuts that
  differ from the defaults, by command id, as text ("Ctrl+Shift+Z", Ctrl
  standing for Cmd on macOS, egui's key names), and the palette's recent
  commands. It is an added field with a default, as the follow mode and
  the time format were, so older files read as before and no version
  change is needed. `Keymap::from_saved` skips ids and chords it does not
  know, and a saved chord wins over a default that clashes with it (a
  default a later version adds).
- **Presets.** One keymap ships, Ableton's, shown by name in the window;
  the FL-style preset is [L] (CLAUDE.md §7).
- **Menus and tooltips (7d)** read shortcuts from the keymap, never from
  text written next to them: `widgets::menu_item` (a label and K5
  keycaps; accessible as "Rename Ctrl+R", as egui's menu buttons are
  named), `menu_toggle` (on or off, and the menu stays open), and
  `widgets::tip` (the name, the keycaps, a hint line). `App::menu_command`
  is a menu row that runs its command, greyed when it cannot. A menu is
  sized in egui's first pass from what its rows need; only after it does a
  row fill the width, or every menu would stretch to the window. Menus
  that act on what was clicked rather than the selection (a track's
  header) keep their own actions and show the command's keys.
- **New commands (7d):** Zoom to selection (Z, a `ViewStep::Fit` of the
  selection's span), Fit loop, Select all (Ctrl+A), Loop selection
  (Ctrl+L, the spec's), Next pane and Previous pane (F6, Shift+F6). The
  bar ruler's right-click menu lists the view commands; the time ruler's
  the formats; a track's name opens the track's menu (it sat on top of the
  header and took the right-click).
- **Panes (7d, `app/panes.rs`).** Each pane has an anchor covering it,
  named for screen readers ("Arrangement", "Detail panel", "Browser"),
  that takes the focus and draws the 2 pt accent ring round the pane;
  Tab goes on into the pane. F6 visits the shown panes in order, starting
  at the arrangement.
- **Shift+Tab by focus (7d, ADR-0015, the owner's choice):** the raw-input
  hook leaves it to egui, which moves focus back, while a text field has
  the keyboard or a window is open with something in it focused;
  elsewhere it toggles the detail panel.
- **A text field, not any focus, stops shortcuts (7d).** egui's
  `egui_wants_keyboard_input` is true whenever any widget has focus, so
  once a button was reached with Tab every shortcut stopped. The
  dispatcher, `App::typing` and the card's Esc now use
  `text_edit_focused`.

### Consequences
- A new command is one row in the table plus its `availability` and
  `run` arms; the compiler finds missing arms.
- The keymap can be saved by command id and changed at run time (7c).
- Plain keys held with an extra modifier no longer run the plain command.
  That frees chords such as Shift+Delete for later commands.

### Evidence
- Unit tests: `commands` (table order, unique ids and names),
  `commands::keys` (no default clashes, exact matching, chord text).
- E2E: the existing 62 scenarios pass unchanged;
  `shortcuts_match_their_modifiers_exactly`,
  `ctrl_r_renames_the_selected_track_as_f2_does`; the palette's
  (`palette`): Ctrl+K and Enter, recent first, the transport button,
  `every_command_can_be_found_and_run_from_the_palette` (the slice's pass
  test: each command found first by its name, then run, or kept open
  with its reason; commands that open a file dialog or the save question
  are found only), context names and reasons, numbers and names.
- Unit tests: `palette` (fuzzy scoring, typed values, counts),
  `the_tempo_is_clamped_and_a_drag_is_one_step`, `commands::keys`
  (chords saved and read back, rebinding with clashes and reset, only
  changes saved and winning when read).
- E2E (`shortcuts`): a chord recorded in the window runs and is kept for
  the settings while the old one is freed; a clash cancelled with Esc,
  reassigned, then Reset all; Ctrl+Enter in the palette; saved shortcuts
  used from the start. 72 scenarios.
- Screenshots (`keyboard_shortcuts`: the window, the Ctrl layer, a clash,
  and recording in the palette; dark at 100% and light at 150%).
- 7d: unit tests `a_focused_control_keeps_its_own_keys`,
  `f6_cycles_the_shown_panes_and_starts_at_the_arrangement`; E2E (`keys`):
  Z and X, Ctrl+A with Ctrl+L and Fit loop from the ruler's menu, F6,
  Shift+Tab except while typing, menus showing a changed key,
  `shortcuts_still_run_with_a_control_focused` (fails with the old check),
  a right-click on a track's name. 79 scenarios; the smoke tests pass.
  Screenshots (`menus_and_tips`: a clip's, the ruler's and a track's
  menus, the Play tooltip).
- Screenshots (`tests/shots.rs`, `command_palette`: empty with a recent
  command, a search, an unavailable command, a value and a track, in
  dark at 100% and light at 150%).

### Amendment (2026-10-08, the end-of-milestone review: `docs/audits/2026-10-08-m3.md`)
- **Import audio… (H1).** Importing was mouse-drag only, so the exit flow
  could not be done from the keyboard. A File command, Ctrl+Shift+I (Pro
  Tools' and Logic's; Ableton has none, and Ctrl+I is Insert time), in the
  app menu, the palette, a track's menu and the empty arrangement. The
  system dialog runs on its own thread; the chosen files load as dropped
  files do, at the playhead, onto the selected audio track or the track of
  the selected clip, else onto new tracks right after the selection
  (`document::import_place`, which a track's menu reaches by selecting
  that track). Tests never open a real dialog: `Launch::for_test` answers
  them from `testing::answer_next_dialog`, or as cancelled.
- **The arrows follow the selection (H2, the owner's choice, as in Logic
  and Cubase).** With a clip selected, ←/→ select the clip before or
  after it on its track and ↑/↓ the nearest clip in time on the closest
  track above or below that has clips; Shift adds to the selection (two
  new commands, Shift+←/→). With tracks selected the arrows act on
  tracks as before. They stay one command per key, as Delete is: the
  commands keep their ids (saved keymaps hold) and are named for both
  meanings ("Previous clip or fold track"), and the palette's label says
  which applies now ("Previous clip on the track", "Fold 2 tracks"). The
  next arrow goes from the clip the keys last chose (`clip_cursor`) while
  it is selected, else the first selected clip; with nothing further that
  way it selects that clip alone, so Ctrl+A then an arrow reaches one
  clip. The view scrolls the least that shows it (`TimeView::showing`).
- **Palette ranking.** A name that starts with the query scores more, and
  one equal to it most: "cut" matched "Next clip or unfold track" by its
  word starts above "Cut".
- **Tab order (M3, recorded, not changed).** egui needs side panels
  declared before the central area and focus follows declaration, so Tab
  goes transport → browser → detail panel → arrangement. F6 visits the
  panes in reading order and each pane is in order inside, so the flows
  work from the keyboard; the order is revisited in the M10
  accessibility pass.

Evidence: unit tests `import_goes_onto_the_chosen_audio_track_or_new_tracks_after_it`,
`arrows_step_along_a_track_and_to_the_nearest_clip_above_or_below`,
`showing_a_span_scrolls_the_least`, `fuzzy_search_prefers_word_starts_and_runs`;
E2E `audio_imports_from_the_keyboard_onto_the_selected_track`,
`importing_with_nothing_selected_makes_tracks_and_cancelling_does_nothing`,
`arrows_move_between_clips_when_a_clip_is_selected`,
`the_view_follows_a_clip_chosen_with_the_arrows`; the palette's pass test
now checks the command each top row runs. Screenshots of the empty
arrangement (both themes, 100% and 200%) and the shortcuts window.

---

## ADR-0034 — The info line (M4 slice 0)

**Status:** Accepted · **Date:** 2026-10-09

### Context
The owner found that hints had piled up slice by slice (2026-10-08): every
clip, handle, the loop brace, the scrub strip and the time ruler showed a
chip by the pointer saying what a drag does ("drag to move · Ctrl
copies"), on every hover. The spec asks for status-bar hints (§7) and for
behaviour to read without much text (§2).

### Options (shown as an interactive specimen)
- A. Quiet: no hover text; a chip only while a held key changes a drag.
- B. Rest to reveal: today's chips after the pointer rests 0.8 s.
- C. An info line: one strip along the bottom names what is under the
  pointer and what can be done there.
- A and C together (recommended at first).

### Decision
**C, with the K5 keycaps** (the owner), and no held-key chips: the cursor
already changes with the keys (copy, slip, loop) and the info line lights
the drag they choose.
- `app/info.rs`: an `Info` is a subject and actions; an action is a mouse
  gesture (click, double-click, right-click, drag) with modifiers, or a
  command shown with its shortcut from the keymap (left out while it has
  none, so a remapped key shows its new chord). Hovers call
  `App::set_info`; the strip, laid out before the lanes, draws the last
  frame's info and asks for one more frame when it changes.
- Drawn full width under the detail panel, 32 pt (`theme::size::INFO_LINE`):
  the subject, then the actions separated by dots, stopping with "…" where
  the window ends. Gestures: modifiers as plain caps (`widgets::modifier_caps`),
  a mouse (`Icon::Mouse`), the gesture's word. Commands: K5 keycaps as a
  picture (`Keys::picture`: not a control, so nothing unnamed for screen
  readers). The drag whose modifiers are held exactly is lit in the
  accent. The strip is one label for screen readers ("Info: Clip “kick”:
  drag to move · Ctrl+drag to copy · …").
- "Show info line" (View) hides it; saved in the settings' appearance (an
  added field, on by default).
- Removed: every hover chip (clips, clip edges, fade, bend and gain
  handles, the loop brace, the scrub strip, the time ruler), the arrow
  marks on chips, and the "Or double-click empty space" caption. Kept by
  the pointer: live values while dragging and "new track here" during a
  file drag. The brace's brackets and the scrub strip's washed step stay:
  they show, rather than say.
- Added info where nothing was said: empty lanes, the bar ruler, the empty
  track area. Wordy tooltips and the browser and detail placeholders were
  shortened.

### Consequences
- One place to look for how to do something; new interactive things set an
  `Info` (DESIGN.md §2 and §9).
- The window loses 32 pt of height to the strip (hidden by the command).
- Tests read the info line as words (`testing::info_line`, `info_lit`).

### Evidence
Unit test `info_reads_as_words_and_lights_the_drag_the_keys_choose`; E2E
`info` (hover, a remapped key shown, the empty track area, Show info line),
and the clip, fade, loop, properties and scrubbing scenarios now reading
the info line; screenshots (`tests/shots.rs`, `info_line`: a clip, with
Ctrl held, a clip's end; dark 100%, light 150%, dark 200%).

## ADR-0035 — The mixer graph (M4 slice 1)

**Status:** Accepted · **Date:** 2026-10-09

### Context
Until M4 the engine played one list of clips into one stereo output, with
each track's volume, pan, mute and solo baked into its clips' levels
(ADR-0025). M4 needs groups, returns, sends, the master, device outputs,
and later meters, sidechains and delay compensation (M4 plan, risks R1
and R2). The graph must cost well under the quality bar's budget (half of
2.67 ms at 128 frames, 48 kHz, 64 tracks), change while playing without
clicks, and never allocate on the audio thread.

### Options
- **Clips keep their track levels; buses only for groups.** Little
  change, but sends, polarity and meters need a per-track signal anyway.
- **A node graph walked on the audio thread** (pull model, each node
  asking its inputs). Flexible, but recursion and visit marks on the audio
  thread, and hard to bound.
- **A flat plan compiled on the UI side** (chosen): nodes in an order
  where everything that feeds a node comes before it, every buffer
  allocated up front, run front to back.

### Decision
- `lumen_engine::mixer`: a [`Routing`] (nodes with id, trim, polarity,
  volume, pan, pan law, audible, output, sends) is compiled by
  `compile_routing` into a `Plan`: Kahn's order (ties keep the listed
  order, so a routing always compiles the same way), indices instead of
  ids, a stereo bus of 256 frames per node, and a *topology* key over
  the order, outputs and sends (not levels). Duplicates, unknown targets
  and cycles (through outputs or sends) are refused with a
  `RoutingError`; the playing plan stays.
- Signal path per node: input (its clips, the nodes routed into it, the
  sends into it) → trim × polarity × mute → pre-fader sends → volume ×
  pan → post-fader sends → output (another node, a device pair, or
  nothing). Groups and returns are ordinary nodes that others feed. A
  device pair the device does not have is silent; a mono device gets the
  left side.
- `Mixer<T: TrackSource>` is the engine's `Source`. Each part of a block
  (at most 256 frames): clear the buses, let the track source (the clip
  player) add into its tracks' buses by id, run the plan. The clip player
  no longer pans; a clip whose track is not in the routing is not heard,
  and fades in when its track appears.
- Plans arrive through a lock-free queue (the newest wins; skipped ones go
  straight back) and old ones return to be freed, as clip lists do. A new
  plan takes over the levels of the nodes and sends it shares with the old
  one by id; levels glide linearly over 20 ms (`mixer/smooth.rs`, counted
  in frames, so block size does not matter). A plan of a new shape
  crossfades over 5 ms: the old plan, fed the same track audio, fades out
  as the new fades in, and new plans wait until it ends. A transport jump
  (already crossfaded by the engine) settles every level.
- Pan laws in `lumen_core::pan` (shared with the interface): −3 dB
  constant power by default, −4.5 dB, −6 dB linear and 0 dB balance. The
  hard side plays at unity; panning acts as a balance on stereo material.
  The master node uses the balance law, so it is not 3 dB down at its
  centre.
- The app sends the routing after the clip list and never ahead of it
  (`lanes.rs`): a deleted track's clips fade out while its node is still
  there, and a new track's clips wait for their node, then fade in.
  `routing.rs` derives the routing: every track into a master node (until
  slice 2 adds the master track, outputs and sends), a track inside a
  group into that group, mute and solo by the current rule (slice 4
  replaces it).
- Owner decisions on what is heard: centre −3 dB, balance panning, mute
  silences every send, post-fader sends after the pan.

### Consequences
- M3 projects play 3 dB quieter at the centre (the pan law).
- Meters (slice 5) read node buses; sidechains and delay compensation
  (slice 6) add edges and per-node delays to the same plan.
- `Engine<Mixer<ClipPlayer>>` everywhere; tests build routings with
  `NodeSpec::unity` and `lumen_engine::testing::routed` (the shape the
  benchmark and the performance gate measure).
- Performance gate: `engine_transport` rose 4.07% (about 240 instructions
  a block: the mixer's queue check and part loop over an empty plan) and
  `engine_clips_64` 0.78%; the baseline was updated in WSL with this
  reason, and `engine_routed_64` added.

### Evidence
- `tests/mixer.rs` (15 golden renders): a group equals the sum of its
  children (nested too); a group's level applies to its children; a muted
  track is silent with its sends; a send at 0 dB doubles into a return;
  pre- and post-fader sends; the −3 dB centre; polarity and trim; device
  pairs; a missing track; volume, pan, mute and polarity changes glide; a
  reroute crossfades with a constant sum; a joining track fades in; the
  same output at block sizes 1, 37, 128, 256, 300 and 1,024 across a glide
  and a crossfade; plans handed back; a cycle refused while the old
  routing plays on. The 18 clip golden renders pass unchanged through a
  one-node routing.
- `tests/realtime.rs`: 64 routed tracks with plans swapped as fast as a
  drag, under the allocation guard: no heap operations.
- Benchmark (`cargo bench -p lumen-engine --bench clips`, Windows, release):
  routed 16 / 64 / 200 tracks at 128 frames: 7.3 / 25.7 / 81 µs median,
  1.0% / 3.0% of the 2.67 ms block at 64 / 200 tracks; plain 64 clips
  15.1 µs.
- App: `routing.rs` unit tests; E2E `mixing` (pan and mute heard, mixer
  changes while playing never click); E2E `fades_and_gain_are_heard`
  updated for the −3 dB centre.

## ADR-0036 — Routing in the model (format v7)

**Status:** Accepted · **Date:** 2026-10-09

### Context
The mixer graph (ADR-0035) plays whatever routing the app derives, but the
project could only say "this track is in that group". M4 needs the master
track, outputs, sends, polarity, input trim and the pan law saved,
undoable and safe (risk R4: no edit may make the audio feed back).

### Decision
- **Format v7.** `Project::master` holds the one master track, apart from
  `tracks` (which never holds a master), so each view chooses where to show
  it (Ableton keeps it at the bottom; slice 4). `Project::pan_law`
  (`lumen_core::pan::PanLaw`, now serializable). Each track gains `output`
  (`auto`, `master`, `{"track": id}` a group or return, `{"device": n}` a
  pair of device outputs), `sends` (to a return, a `level` in dB written as
  a number or `"-inf"`, `pre_fader`), `invert` and `trim_db`. New fields are
  written only when set.
- **Migration v6 → v7:** a master already among the tracks (the golden
  fixtures have one) moves out to `master`, losing any `parent`; otherwise
  a master is made with the next free id (the highest `"id"` anywhere, plus
  one). Defaults are written on every track; the pan law is −3 dB constant
  power (owner, M4). Golden fixture `tests/fixtures/v7` (a return with a
  pre- and a post-fader send, one silent; a track straight to the master
  from inside its group; polarity; trim; the master at −2 dB out of the
  second pair; the linear law).
- **One routing rule:** `lumen_project::routing::destination` (automatic is
  the track's group when it is in one, otherwise the master; the master's
  automatic is the first device pair), `feeds` and `has_loop`. Validation
  rejects outputs that are not a group or return, sends that are not to a
  return, a second send to the same return, a master that is not a master,
  and any loop. The app's `routing.rs` builds the engine routing from the
  same rule; silent sends stay in it at zero gain, so turning one up glides
  instead of reshaping the routing.
- **Edits** (`document/mixer.rs`): `set_output`, `set_send` (made silent and
  post-fader, the owner's choice; up to +6 dB; a drag is one step),
  `set_send_pre_fader`, `set_invert`, `set_trim` (±24 dB), `set_pan_law`.
  Each tries the change on a copy first and is refused (`Refused::Loop`,
  `Refused::Invalid`) if it would feed back or the target cannot take
  audio. `set_volume` and `set_pan` reach the master. Deleting tracks sets
  outputs to them back to automatic and removes sends to them (owner: a
  deleted return takes its sends), in the same step. The master is never
  deleted or added. The edits wait for their controls (slices 4 and 5)
  under an `expect(dead_code)` that names them.
- **The history's change check** (`history::unchanged`) compared fields by
  hand and did not know the new ones, so routing edits recorded nothing. It
  now destructures `Project` and `Track`, so a new field fails to compile
  until it is compared.
- A new project's master has id 0; other ids count on from 1.

### Consequences
- The arrangement still shows only `tracks`; the master appears in slice 4.
- Opening an M3 project migrates it (3 dB quieter at the centre, as in
  slice 1); saving writes version 7.
- Performance gate: `project_parse` +0.82% (the loop check),
  `project_serialize` +0.04%; under the threshold, baseline unchanged.

### Evidence
Migration unit tests (a master made with the next free id, a master moved
out of the tracks); golden fixtures v1 to v7 load; validation test for
outputs, sends and the master; routing tests (automatic outputs, loops
through outputs, groups and sends); history test `routing_changes_are_edits`;
`document/mixer.rs` tests (outputs and undo, refused loops change nothing,
sends start post-fader and a drag is one step, deleting a return takes its
sends and undo brings them back, polarity, trim and the pan law); app
`routing.rs` tests (outputs, sends, polarity, trim and the law reach the
engine); E2E `a_project_from_before_the_mixer_opens_with_a_master_and_saves_as_v7`.

## ADR-0037 — Group tracks (format v8)

**Status:** Accepted · **Date:** 2026-10-09

### Context
M4 slice 3 makes group tracks real: the engine already sums a track into
its group (ADR-0035) and the model already had `parent`, but nothing let
a user make, nest, fold or leave a group, and the list order said nothing
about membership (a group's tracks could sit anywhere). The owner chose
the look and four behaviours from rendered specimens before building
(owner-decisions table).

### Decision
- **One order rule** (`lumen_project::groups`): a group's tracks follow it
  directly, nested ones inside theirs, so a group and everything in it is
  one run of the list (`span`). Validation rejects a list out of that
  order, and returns or the master inside a group (`can_be_grouped`, as in
  Ableton). Edits set parents and a rough order, then `normalize` puts
  every track back under its group, keeping the order of siblings. Helpers:
  `depth`, `ancestors` (stops on a cycle), `is_inside`, `with_contents`,
  `outermost`, `common_group`, `in_order`.
- **Format v8:** `ViewState::collapsed`, the groups folded to hide their
  tracks (view state, not an undo step, as track heights are). Migration
  v7 → v8 puts tracks in group order (the same preorder, written out in the
  migration so it never changes), takes returns and the master out of
  groups, and writes an empty `collapsed`. Golden fixture `v8` (a group
  nested in the drums holding a MIDI track; the drums folded).
- **Edits** (`document/groups.rs`; one undo step each, tried on a copy
  first and refused if the routing would feed back, `Refused::Loop`):
  `group_tracks` (Ctrl+G: the outermost of the selection, returns left out,
  into a new "Group N" where the topmost was, inside the innermost group
  that held them all); `ungroup` (Ctrl+Shift+G: a selected group goes, its
  tracks move up a level in place and become the selection, routing to it
  falls back to automatic; a selected track inside a group comes out to
  right after it, "Take track out of group"); `move_tracks` now takes a
  `Place` (an index among the tracks that stay, and a group) and moves a
  group with everything inside it. `delete_tracks` takes a group's tracks
  too (owner); `duplicate_tracks` copies a group with its tracks, fresh
  ids throughout, right after the original's run; `set_armed` on a group
  arms its audio and MIDI tracks (owner); `add_track` lands after the
  chosen track's run, in its group. Imports: `DropInto::NewTracks` gained
  `group`, so files dropped on a group's lane, or Import audio with a group
  chosen, make tracks at the end of that group.
- **Arrangement:** rows skip tracks hidden in a folded group; the GPU row
  table stays one slot per track (`RowSlot::HIDDEN` for hidden ones), since
  boxes and waves name their track's index. A group's arrow hides or shows
  its tracks (Alt+click: every group); Left and Right on a selected group
  do the same; Up and Down, Shift-click ranges and Fit to height use only
  the tracks shown; Go to track unfolds the groups around it. Alt+Up and
  Alt+Down move the selection among the tracks of its group, over a
  neighbouring group whole. Dragging headers (`reorder.rs`): between two
  tracks the moved ones join the group of the track below (so below a
  group's last track they come after it); the middle half of a group's
  header drops into it, lit; the line steps in to the level they land at.
- **Look** (`arrangement/nesting.rs`, the owner's H1 and L1): headers step
  in `INDENT` (12 pt) a level, capped at four levels so names keep room,
  with each enclosing group's rail at its own indent; a group's name is
  medium weight and, folded, followed by "· N"; its lane draws a bar per
  audio or MIDI track inside it where its clips are (bars thin as they
  crowd, then share rows), only what is in view. A group's arm button is
  lit when everything inside is armed and shows a lit dot with half a ring
  when only some is.
- **Commands:** Group tracks (Ctrl+G), Ungroup (Ctrl+Shift+G), Fold all
  groups and Unfold all groups (no keys), in the palette and the track
  menu (Ungroup reads "Take out of group" on a track inside one; a group's
  menu offers Hide or Show its tracks). `arrangement/menus.rs` now holds the
  track menus, split from `headers.rs`.

### Consequences
- Solo across groups is still the plain per-track solo; its rules are
  slice 4's (a table of cases for the owner first).
- Opening a v7 project may reorder tracks whose groups were scattered (no
  project made in the app had any: grouping did not exist).
- Two or more levels deep, long names are shortened sooner (204 pt
  headers; KNOWN_ISSUES).
- **Performance gate:** `project_parse` +1.71% and `project_serialize`
  +0.33% against the old baseline, of which slice 2 was +0.82% and +0.04%.
  A probe without the new order check measured +1.67%, so the check costs
  0.04%; the rest was not traced to more work per track or clip (most
  likely code generation around the new view field). The baseline was
  updated from WSL (`perf.sh --update`) on this commit, as an intended
  change.

### Evidence
`lumen_project::groups` unit tests (depth, spans, contents, outermost,
common group, order, normalize with missing parents and cycles);
migration test (scattered tracks put in order, a return taken out of its
group, `collapsed` written); golden fixtures v1 to v8 load; validation
tests (a track apart from its group, a return inside a group);
`document/groups.rs` tests (grouping in place and nesting, wrapping a
group, ungrouping a level up with routing falling back, taking tracks out
in order, moves that take groups whole and enter or leave them, a move
that would feed back is refused, deleting and duplicating groups, arming
a group); import tests (into a group at its end); drop and reorder place
tests; `nesting` tests (bar layout, arm state); E2E `groups` (7 scenarios:
keys and undo, nesting and taking out, folding saved with the project,
dragging in and out, arming and deleting, files dropped on a group's lane,
a tone heard through two groups and silenced by the outer mute).

### Amendment (2026-10-09, MT-MIX-3 run 1)
- **Solo inside groups** (bug: soloing a track in a group silenced it,
  because its group was not soloed and every unsoloed track was cut).
  `document::audible` now hears, while anything is soloed, the soloed
  tracks, what plays into them (a soloed group's tracks), and where their
  output leads (their groups, by `routing::destination`), so the path
  stays open; others in the same group stay silent. Returns, solo-safe and
  the rest are slice 4's table of cases. Tests: `routing.rs`
  `solo_keeps_the_path_through_groups_open`; E2E `groups` (a soloed track
  inside two groups is heard).
- **Groups go three deep at most** (owner): `groups::MAX_NESTING` and
  `groups::nesting`. Every group edit runs through one trial
  (`Document::trial`), refused with `Refused::TooDeep` when groups would go
  deeper than three and deeper than before (an older, deeper project still
  opens and edits; files are never rejected for it). `group_tracks` returns
  a `Result`; a refusal shows a note ("Groups go three deep at most",
  `Arrangement::note` turned into the toast). `check_move` lets a header
  drag grey its drop line and show a no-entry cursor where the drop would
  be refused (checked when the place changes, not every frame). Indents
  stop at three levels.
- **A resizable header column** (owner; format v9, `ViewState::header_width`,
  migration v8 → v9 writes 204, golden fixture `v9` at 296): drag the edge
  between the headers and the lanes (`app/view.rs` `header_divider`, on the
  headers' side of the edge so a drag at the lanes' left edge stays a clip
  or locator drag), 204 to 480 pt, double-click back to 204; the edge glows
  like a track's bottom edge (`widgets::draw_edge_glow_vertical`). Saved
  with the project's view, not an undo step, as panel sizes are. Header
  contents already followed the column's width. The known issue about
  short names is closed.
- Performance gate unchanged (`project_parse` −0.04%).
- **Deleting a track from its right-click menu crashed Lumen** (owner,
  after run 1). The header column lays out its rows at the start of a
  frame, and a header's menu deletes tracks partway through drawing them;
  later rows (and the dimming of dragged tracks) still indexed the old
  list. `headers.rs` now remembers the track each row was laid out for and
  skips rows whose track is no longer in its place for the rest of that
  frame, and a drag does not follow stale rows. E2E
  `deleting_any_kind_of_track_from_its_menu_or_by_key_never_crashes`
  (audio, MIDI, group with a track inside, and return; from the menu and
  by key; undo restores all) failed with the crash before the fix.
- **Tracks in groups played quieter** (owner, after run 1): groups and
  returns took the project's pan law (−3 dB at the centre by default), so
  each group level took another 3 dB off what was inside it (−6 dB two
  groups deep), and sends lost 3 dB at their return. Only the master was
  at unity (ADR-0035). Now every bus (group, return, master) pans with the
  balance law, unity when centred: the owner's −3 dB centre applies once,
  at the track that pans the sound. Tests: `routing.rs`
  `buses_are_unity_when_centred_and_tracks_follow_the_pan_law`; E2E
  `groups` (a tone through two groups plays as loud as the track alone;
  half as loud before the fix).

## ADR-0038 — Mute, solo, returns, master and cue (M4 slice 4, format v10)

**Status:** Accepted · **Date:** 2026-10-09

### Context
Slices 1 to 3 built the mixer graph, routing in the model and group
tracks, but solo only kept a soloed track's groups open, returns and the
master were invisible or mixed in with the tracks, sends had no control,
and there was no cue or metronome routing (deferred from M2). The plan
asked for the solo rules as a table of cases agreed with the owner before
building (risk R3). The owner agreed the 14 cases and chose D1 to D7, then
the A1 layout, K1 comet knobs and the hollow-M look from interactive
specimens, and O1 for the cue outputs (owner-decisions table).

### Decision
- **4a, the solo rules as one pure function** (`lumen_project::solo`):
  `hearing(project)` says per track whether its output and its sends are
  heard, whether it is silenced without being muted (hollow M) and whether
  it is heard through a soloed group (hollow S). A soloed path stays open
  (the soloed tracks, what plays into them and where they play out to);
  returns are solo-safe unless a return is soloed; a soloed return plays
  wet only (the sends into it stay on, the senders' outputs go silent);
  mute beats solo along the whole path, sends included; the master is
  never silenced by solo. The 14 cases are its tests. The engine gates a
  node's output apart from its sends (`NodeSpec::sends_audible`, a gliding
  output gate), so wet-only plays without clicks. Additive solo is a
  setting (`Mixing::additive_solo`, D2); the transport's S chip (count when
  more than one) and the palette's "Clear all solos" clear every solo in
  one step; palette commands "Solo selected tracks" and "Mute selected
  tracks" (a new Mixer area of commands, no default keys, D7).
- **4b, returns and the master pinned at the bottom** (`arrangement/
  pinned.rs`, the owner's A1): rows carry a zone (main, returns, master)
  and each zone clips its drawing and its pointer, so the tracks scroll
  under the pinned area. Returns keep any place in the project's list (no
  order rule in the file): the arrangement draws them pinned in list
  order, lettered A, B, C (then AA) in their colour, without an arm
  button, never dragged; a new return goes after the last. A "Returns"
  caption folds them; the master's row comes last (fold arrow, volume and
  pan). The divider resizes the area up to 60% of the lanes (the returns
  scroll inside it when it is smaller; double-click for its natural size,
  as the header column's edge). **Format v10:** `ViewState::returns_height`
  (absent at the natural size) and `returns_folded`; migration v9 → v10
  writes the fold; golden fixture `v10` (110 points, folded). H fits the
  tracks into the height above the pinned area.
- **4c, sends as comet knobs** (`widgets/knob.rs`, `arrangement/sends.rs`,
  the owner's K1): a knob per return on an unfolded audio, MIDI or group
  track, in the return's colour and letter; the level is a trail of light
  that brightens to a glowing head, dimmed when the send is not heard; a
  dot marks pre-fader. A 270° travel with 0 dB at three quarters and +6 dB
  at the top (`48·log10(p/0.75)`), silence below −60 dB. Drag (Shift
  fine), double-click for silence, click and type, arrow keys, and a menu
  for pre- or post-fader; a drag is one undo step. Hovering a send lights
  its return's header, and the reverse (one frame behind). Tracks with
  sends to show unfold a step taller (five steps), so the knobs have
  their own row under volume and pan.
- **4d, cue and the metronome's output** (the owner's C1 with O1): the
  audio settings choose a Cue output pair beside the main one
  (`AudioSettings::cue_channels`, an added field); the stream opens the
  main pair then the cue pair, so the engine always finds the cue on
  channels 3 and 4. The engine's cue bus takes each cued node after trim
  and polarity, before mute and the fader, whatever solo says, and writes
  it to that pair at the cue level (`Routing::cue`, `NodeSpec::cue`; the
  level glides; a device without the pair does not hear it). The master's
  Solo/Cue switch (greyed without a cue pair; a click says to choose one in
  Audio settings) turns the S buttons into headphone buttons (Ableton's
  click and Ctrl+click), and its unfolded row gains the cue level. What is
  cued is session state (`Arrangement::cued`); the switch and level are
  settings (`Mixing::cue_mode`, `cue_volume_db`). The metronome plays on
  Main or Cue (`MetronomeSettings::channel`, settings `metronome.output`),
  never through the master, falling back to the main pair. The test device
  has four outputs; the E2E tap records both pairs (`App::heard_all`).

### Consequences
- Track headers are taller when unfolded in projects with returns (110
  points instead of 88); saved heights are unchanged.
- A master output to a device pair other than the first would now land on
  the cue pair when one is chosen; no control offers such outputs until
  the mixer panel (slice 5), which defines output choices with the cue in
  mind.
- Known issues recorded: no wheel adjustment on sliders and knobs yet
  (slice 5), returns not reorderable by drag, "+N" for knobs past the
  header's width.
- Performance gate: `engine_routed_64` −5.6% (the gated output path
  compiled leaner), `engine_transport` +1.6% (the metronome's pair),
  others unchanged; baseline updated from WSL. Frame times on the stress
  project p99 1.5 to 1.8 ms; idle 3 repaints in 3 s, as before.

### Evidence
`solo.rs` tests (cases 1 to 13 and a group sending to a soloed return);
engine `mixer` tests (wet only with a gliding gate; a cued track before
its fader and mute, the cue level gliding, silence without the pair) and
`metronome` (Main, Cue, fallback); the allocation guard with a cue bus and
the metronome on the cue pair (`realtime`); `routing.rs` (wet only and
cue reach the engine); arrangement unit tests (returns after the tracks,
letters, the saved view); knob taper and typed levels; migration v9 →
v10 and the v10 golden fixture. E2E: `solo` (silenced tracks say so, the
chip clears, Ctrl+click adds, mute beats a group's solo, palette and
additive solo), `returns` (pinned under the divider, folding, the divider
and its saved size, the master's volume), `sends` (drag, silence,
pre-fader and undo; a typed level; a soloed return heard wet only), `cue`
(the switch explains itself, a cued track on the cue pair before its
fader and mute, the metronome on the cue pair).

### Amendment: MT-MIX-4 run 1 (2026-10-09)
- **The knob's bottom was not silence:** `knob::level` took the floor as
  position 0, so the bottom of the travel stored about −80 dB. The floor
  is now the position of −60 dB (`knob::floor`); E2E
  `a_send_at_the_bottom_is_silent_and_leaves_the_level_alone` stored
  −80.7 dB before the fix. A louder track with the send at the bottom was
  not reproduced (the same E2E measures the level before sending and at
  the bottom).
- **C4** (the owner, after two rounds of mock-ups): knobs take the row's
  selection; on a selected row the ring is a quarter of the way to the
  text colour (`SendKnob::selected`).
- **O1, drawn as "+ Add track ▾":** knobs past the header's width are
  reached from a "+N ▾" button opening every send of the track
  (`sends::more_sends`, an egui menu popup).
- **R2:** an unfolded return lists its senders with their knobs
  (`sends::senders_rows`), unfolding a step taller per sender
  (`Arrangement::senders`). Knob widgets are named by place (header, list,
  return) so the same send can show twice.
- E2E: the "+N" list sets the tenth return's send; a return lists its
  sender and sets it there. `tests/shots.rs` `sends` renders the selected
  row, the list and a return's senders in both themes and at 150%.

### Amendment 2: MT-MIX-4 run 2 (2026-10-09)
- **A project holds 12 return tracks at most** (the owner; letters A to
  L): `mixer::MAX_RETURNS`, `Document::returns_full`; the add menu greys
  the item with the reason, adding is refused with a note, and duplicating
  copies no return past the cap.
- **L4:** an unfolded return lists its four loudest senders
  (`sends::RETURN_ROWS`), then "+N more ▾" (the same button as on track
  headers, `sends::plus_button`) opening "Sends into …" with every sender.
  The order re-sorts only while no button is held, so rows never move
  under the pointer. List knobs are 18 pt without the hover ring (they
  touched before: 20 pt knobs with rings on 22 pt rows).
- **S1:** a silent send is not listed on its return, except while its
  knob there is being set (`Arrangement::held_send`); it goes when let
  go. The send stays on the track at −∞, pre- or post-fader as it was.
- **Kept:** an empty return plays an unchanged copy of what is sent, so a
  0 dB send adds +6 dB, as in every DAW (the owner chose the standard
  behaviour).
- E2E: four loudest then all from "+N more"; a send turned off leaves
  the list (held, it stays); twelve returns at most (unit test).

### Amendment 3: MT-MIX-4 run 3 (2026-10-09)
- A knob's live level (while dragged or typed) is a `draw_hint` chip
  above it on a tooltip layer (`SendKnob::live_value`), only on track
  headers: beside the knob it ran over the next one, and in lists it drew
  over the row's own level column.
- The "+N ▾" and "+N more ▾" lists scroll past ten rows (`LIST_ROWS`).

### Amendment 4: MT-MIX-4 run 4 (2026-10-09)
- A return's senders are ordered loudest first only when a sender joins
  or leaves; level changes never move a row (`sender_order`). Re-sorting
  after each change swapped two senders of the same name (a duplicated
  track), so one seemed to change the other. Identity was already by id
  throughout; E2E checks a duplicate's send in the project and by ear.

### Amendment 5: MT-MIX-4 run 5 (2026-10-09)
- A knob drag keeps its own position on the travel (widget memory) from
  the press point, instead of the current level plus each frame's
  movement: below the silence floor a slow drag was rounded back to −∞
  every frame and never added up. The few points of egui's drag threshold
  now count too.


---

## ADR-0039 — Lumen's licence: GPL-3.0-or-later (supersedes ADR-0012)

**Status:** Accepted · **Date:** 2026-10-09 · Owner decision. Supersedes ADR-0012.

**Context.** The owner wants Lumen to be open source, with the signed, packaged binary sold. ADR-0012 kept the licence open while the code was closed. ADR-0006 records that the Steinberg ASIO SDK's GPLv3 option is only possible if Lumen is GPLv3. ADR-0013 keeps dependencies permissive-only, with recorded exceptions (symphonia under MPL-2.0, OFL fonts). The VST3 SDK is MIT (ADR-0005). Every commit so far is by the owner, so the owner holds the copyright and can still offer another licence later.

**Options.**

| Licence | Can sell binaries | Forks must stay open | Notes |
|---|---|---|---|
| GPL-3.0-or-later | Yes | Yes | The GPL family is used this way by Blender and Ardour. Enables the ASIO GPLv3 route. Conflicts with Mac App Store terms. |
| MPL-2.0 | Yes | Changed files only | Competitors can keep most of their code closed. Weaker protection. |
| Apache-2.0 or MIT | Yes | No | Anyone can close the code and resell it. |
| FSL-1.1 (source-available) | Yes, except competing use | No | Not OSI open source. Converts to Apache-2.0 after two years. Closes the ASIO GPL route. |
| Dual licence (GPL plus a paid commercial licence) | Yes | Yes | Needs a contributor agreement before outside contributions. Kept open as an option. |

**Choice.** GPL-3.0-or-later. The text is in `LICENSE`. `license` is set once in the workspace (`Cargo.toml`), and every crate inherits it.

**Why.**
- Anyone who sells a modified Lumen must publish its source under the same licence, so improvements come back to the project.
- It is the licence the ASIO path needs (ADR-0006).
- Every current dependency is compatible: permissive licences, MPL-2.0 (symphonia, used unmodified), OFL fonts (bundled with their licence text), and the MIT VST3 SDK.
- GPLv3 rather than v2, because the Apache-2.0 dependencies are incompatible with GPLv2.

**Consequences.**
- The product that is sold is the signed installer, updates, support and the name. No licence stops a third party from selling a copy; a GPL copy must come with its source. The name stays protected only if it is registered as a trademark, and no trademark policy is written yet.
- Every distributed binary must have its source available: publish the tag for the exact commit, with the licence and third-party notices, which the installer ships (M10).
- Dependency policy (ADR-0013) is unchanged. A copyleft dependency still needs a recorded decision.
- macOS: GPL conflicts with the Mac App Store terms. Sell macOS builds directly or through Steam, not the Mac App Store, unless a legal review says otherwise.
- Outside contributions: before accepting one, add a Developer Certificate of Origin sign-off to the workflow. Ask for a contributor licence agreement only if dual licensing is needed later.
- Making the repository public is a separate owner action. The repository is private and nothing has been published, so there are no earlier grants to revoke.

**Open items (owner).**
1. Trademark clearance search for "Lumen" in software classes. Lumen Technologies is a large registrant.
2. Trademark policy for the name and logo: forks must use another name.
3. ASIO: confirm the SDK's current licence text allows the GPLv3 option, and follow its trademark rules. Check `crates/lumen-audio/src/asio/ffi.rs` against the SDK's headers before publishing.
4. VST: the VST trademark notice is needed. The MIT licence covers the SDK code, not the name.
5. One commit uses a second author email (`andrewl913@yahoo.com`). Decide whether to rewrite history before publishing. Not done.
6. Code-signing certificates and release secrets stay outside the repository.
7. Make the repository public and publish a release only after items 1 to 6. Not done.

## ADR-0040 — The mixer panel and meters (M4 slice 5)

**Status:** Accepted · **Date:** 2026-10-09

### Context
M4 slice 5 adds the Mixer tab, meters on strips and headers, loudness on
the master, one rule for value controls and output choices. The owner chose
the look and eight behaviours from interactive specimens before building
(owner-decisions table, "Mixer panel and meters"). Meters must read
sample-accurate peaks without the audio thread allocating, locking or
waiting on the UI, and must not raise idle CPU (risk R5).

### Decision
- **5a, meters in the engine** (`lumen_engine::mixer::meter`,
  `lumen_core::loudness`):
  - every node's output is measured after fader, pan and the solo gate
    (B1), each chunk, in the plan the listener hears (a plan fading out is
    not metered, nor a tail rendered ahead of a jump);
  - readings live in a `MeterBank` of atomics, one slot per node: the peak
    per side as `f32` bits raised with `fetch_max` and taken by the UI with
    `swap` (so a slow UI never misses a peak), the smoothed mean square
    (300 ms, `RMS_SECONDS`) stored by the audio thread and loaded by the UI,
    and a clip flag set over full scale and taken with `swap`. A muted
    node, or one whose output is gated while its sends play, records
    silence and its RMS falls;
  - `MixerHandle` gives each node id a slot when a routing is set; slots
    are never reused while a bank lasts (a new node never shows an old
    one's level), and when it fills a bank twice the routing's size takes
    over with only the nodes in use. Plans hold the bank in an `Arc`; old
    banks go back with retired plans and are freed on the UI side;
  - `Routing::measure` names the node whose loudness is measured (the
    app's master). Its output is K-weighted (BS.1770-4, coefficients
    derived for any rate as libebur128 does, and matching the standard's
    tables at 48 kHz) and summed into 100 ms blocks, sent through a
    512-block lock-free queue; the UI computes momentary (400 ms),
    short-term (3 s) and gated integrated loudness (absolute −70 LUFS,
    relative −10 LU) with `lumen_core::loudness::Loudness`. True peak
    oversamples four times with a 49-tap Kaiser-windowed sinc (13 taps a
    phase, every phase summing to one) and is taken like a peak;
  - RMS is the signal's true RMS: a full-scale sine reads −3.0 dB.
  - While stopped the engine renders nothing; it calls `Source::idle`, and
    the mixer lets every node's RMS fall as in silence (a load and a store
    per side), so meters come to rest.
- **5b, meters on the headers** (`meters.rs`, `arrangement/levels.rs`,
  `app/mixer.rs`; the owner's H2 and B2): each frame the app takes every
  track's level and moves its meter on (peaks fall 20 dB/s, a 1.5 s hold,
  clips latched until cleared), keeps the master's loudness windows and
  true peak, and resets integrated loudness and true peak when playback
  starts from stop (B5: `Transport::played_from_stop`, set by Space and
  the play button; Continue keeps it). `Loudness` only gates 400 ms blocks
  made wholly after a reset. While the device is not running, meters fall
  to rest. The frame loop keeps drawing while a meter moves and returns to
  its idle once-a-second poll when all rest. Commands: "Clear clip lights"
  and "Meters in track colours" (the M2 setting, `Mixing::
  track_colour_meters`, an added settings field), in the Mixer area.

- **5c, the Mixer tab** (`arrangement/strips.rs`, `strip.rs`, `buttons.rs`;
  `widgets/fader.rs`, `widgets/meter.rs`; the owner's S1, O2, L2, B4 and
  B7): the strips are another view of the arrangement (`impl Arrangement`),
  so selection, folding, cue, the return hover link and the send knobs are
  shared, not copied. The M and S buttons moved out of the header loop
  into `Arrangement::mute_solo`, used by both. The fader shares the send
  knob's taper and stops at −60 dB (a track's volume is saved as a number;
  `set_volume` clamps it). The output menu lists Master and the groups,
  leaving out any that would feed back (tried on a copy, `routing::
  has_loop`); device pairs come in 5e. A rename started on a strip edits on
  the strip (`renaming_in_strip`). The mixer is drawn after the
  arrangement, into the area the detail panel reserved, so Tab reaches it
  in reading order. The detail panel is never shorter than a strip with
  every section folded, nor so tall that the arrangement keeps less than
  180 pt.
- **Strips drag (the owner's B8):** a strip's background drags its track
  as a header does (`Arrangement::start_drag(.., in_mixer)`; the drag
  remembers where it started so headers and strips each follow their own),
  along x with the headers' drop rule (`reorder::drop_place`): between
  strips the moved ones join the group of the strip to the right, the
  middle half of a group's strip drops into it; returns move among returns
  only (their letters follow their place). Near the strips' edges the
  strips scroll.
- **The pan law** (the owner's slice 2 answer): the mixer's empty area has
  a right-click menu (the four laws, Clear clip lights), and the palette
  has a command per law; one undo step each (`set_pan_law`).
- **5d, the wheel on value controls** (`widgets/wheel.rs`, the owner's
  B3, closing the KNOWN_ISSUES row): value controls register where they
  are each frame; first thing next frame (before the arrangement or a
  scroll area reads the wheel) `wheel::route` gives the gesture to the
  control under the pointer if the gesture starts there, taking the scroll
  out of the input, or leaves it scrolling; a gesture is wheel events
  closer than 0.3 s, and a control's ends when the pointer leaves it.
  Controls take whole notches (40 points of scroll) as they draw:
  `value_slider` and `value_field` by their kind's `wheel_step` (1 dB, 5%
  pan, a hundredth of a length), the knob and the fader a dB (Shift a
  tenth; a send up from silence starts at −60 dB, set as a level since the
  knob's travel reads its floor as silence). The app does not end an undo
  gesture while the wheel holds a control (`wheel::adjusting`), so a
  wheel turn is one step, as a drag is.
- **5e, output pairs (the owner's B6, closing the KNOWN_ISSUES row on
  outputs landing on the cue):** `Output::Device(p)` now means the
  device's pair `p` (as the model always said), not the stream's. The
  stream opens the main pair, the cue pair, then every further pair the
  project plays to (`Audio::want_pairs`, reopening the stream only when
  that set changes; a set the device refuses falls back to the setup that
  worked and is not asked for again). `routing::Outputs` says where each
  device pair sits in the stream (main 0, cue 1, then the rest), and the
  routing sends a track to its pair there, or nowhere while that pair is
  not open; the master's Auto is the main pair, whichever pair that is. It
  rides in the routing's session data (`Cue::outputs`), so a reopened
  stream re-routes. The strips' output menus list the device's pairs with
  their channel names, looked up in the background only when a menu or the
  project needs them (`audio_settings::Window::output_names`, an ASIO
  driver in a helper process as the settings window does).
- **Found and fixed on the way:** saved panel sizes never came back.
  egui keeps a panel's size by its id and sizes it by what its contents
  take: the detail panel's few contents snapped it to its smallest after
  every change, and a project's saved size was overwritten by egui's
  memory. The detail panel's content area is now claimed (without
  interaction), and `panels::sliding` applies a size the app changes from
  outside (a project opening) once.

### Consequences
- Performance (divan, 128 frames at 48 kHz, this machine): 64 routed
  tracks 24 µs before, 31 µs with every node metered and the master's
  loudness measured (1.2% of the block); 200 tracks 89 µs before, 92 µs
  after. Peak and sum run in eight lanes so they vectorise. Performance
  gate (instructions, WSL): `engine_routed_64` +21.8% (every node metered
  and the master's loudness and true peak measured), `engine_transport`
  +2.6% (its one node metered), others unchanged; the baseline was
  updated from WSL (`perf.sh --update`) on this commit, as an intended
  change.
- The test routing (`testing::routed`) measures its master, so the
  allocation guard, RTSan and the performance gate cover the loudness tap.
- Frame times (`--stress --bench-frames`, a release-like build, the
  200-track project with the Mixer tab open): p99 2.4 ms zooming, 3.1 ms
  scrolling tracks, 3.2 ms scrolling time (1.5 to 1.8 ms before slice 5),
  well inside the 120 fps target; idle 3 repaints in 3 s, as before.
- The detail panel is now at least a strip tall (with every section
  folded) and at most the window less 180 pt of arrangement; the Mixer tab
  is its only working tab until Devices (M6) and Clip (M5).

### Evidence
`lumen_core::loudness` tests: EBU Tech 3341 cases 1 to 5 within 0.1 LU,
other rates (44.1 and 96 kHz), the K-weighting coefficients against
BS.1770-4's tables, true peak between samples (a quarter-rate sine at 45°
reads within −0.4 to +0.2 dB of its true peak) and of full-scale tones.
`mixer::meter` unit tests (peaks held until read, clips latched, slots
past the end ignored; RMS settles on a sine's −3 dB and falls 29 dB in two
seconds). Engine `tests/meters.rs`: a 0 dBFS sine reads 0.0 dB peak and
−3.01 dB RMS; levels after fader and pan; a muted node reads silence; a
soloed return's senders read silent while it reads the wet signal; an over
latches the clip until read; the master reads EBU case 1 at −23 LUFS
(momentary, short-term, integrated) and its true peak at −23 dB; a node
not routed has no level; banks grow past 64 nodes and keep reading;
stopped meters fall about 29 dB in two seconds without rendering, then to
silence. App `meters` unit tests (peaks fall 20 dB/s, the hold stays then
falls, clips stay until cleared, meters come to rest, an unrouted track
reads silence); `levels` (the meter shares the volume slider's scale).
E2E `meters`: a playing track's level shows after fader and pan law on it
and the master, and peaks and hold come to rest when stopped; clip lights
stay until clicked, one by one or all from the palette; integrated
loudness keeps counting through Pause and Continue and starts again on
Play from stop. `strip` (a short panel folds sends, then trim, then the
output), `fader` (the knobs' taper), `meter` (a bar's zones). E2E `mixer`:
strips follow the arrangement with returns after and the master pinned,
one fold for both views, a click selects; every strip control (fader,
typed volume, pan, polarity, trim, send, mute, solo, rename) sets its
track in one undo step; the output menu routes into a group and never
offers a group as its own output; the loudness column resets; a strip
dragged onto a group's strip goes in, returns reorder among themselves
and never move among tracks; the pan law from the mixer's menu and the
palette; the wheel sets a fader, pan and a send it starts on, one undo
step a gesture; a track sent to outputs 3/4 from its strip plays only
there on the four-output test device. `routing` tests: device pairs play
where the stream opened them (main, cue, then the rest; silent until
open; the master's Auto on the main pair). `wheel` unit tests: a gesture that starts on a control
adjusts it; one that started scrolling keeps scrolling over it, and after
a rest a new gesture on it adjusts it.

### Amendment (2026-10-10, owner's report on the mixer and header meters)

The owner found three layout faults after MT-MIX-5, and approved the fixes
from a rendered mock-up:

- **Strips are 96 pt, not 76 pt** (`strip::WIDTH`). At 76 pt a track name
  kept about seven characters; at 96 pt it keeps about eleven. Longer names
  still truncate with an ellipsis and keep their full name on hover.
- **The header volume's value sits 3 pt higher** (`headers::METER_LIFT`,
  through `widgets::value_slider_lifted`). The level bars along its bottom
  no longer touch the text. Other volume sliders (the cue) keep their
  centred text.
- **Peak readouts drop the decimal from 10 dB up** (`widgets::readout_db`,
  used by a strip's peak box and the master's true peak): "−12", where
  "−12.3" overflowed its box. Below 10 dB, one decimal, as before.

The strip's peak box is now wider too (half of the wider strip). The volume
field under the fader needed no change: a typed "−16.3" fits it at 96 pt and
150% (checked in the screenshot). Screenshot scenario:
`shots::mixer_strips_and_header_levels`.


## ADR-0041 — Sidechains and delay compensation (M4 slice 6)

**Status:** Accepted · **Date:** 2026-10-10

### Context
Spec §6.7 asks for automatic plugin delay compensation across the whole
project, with an indicator, and for sidechains from any track. Nothing in
Lumen adds latency until devices arrive (VST3 in M6, the built-in
compressor and limiter in M8), but the graph from slice 1 has to carry
both before then, or M6 rebuilds it (M4 risk R6). The owner chose the
behaviour before building (owner-decisions table, "Sidechains and delay
compensation").

### Decision
- **The graph** (`lumen_engine::mixer`; `spec.rs` holds the routing types,
  `compile.rs` compiles them, `plan.rs` runs the plan, `delay.rs` holds the
  delay lines):
  - a node has **devices** (`Device`) between polarity and mute, each
    reporting its latency. Until real devices exist the only one is the
    synthetic `Device::Delay { frames, listen }`: a pure delay that can add
    the node's sidechain input to what it delays, as a compressor's listen
    button does, so tests can hear a sidechain;
  - a node takes **sidechains** (`SidechainSpec { from, tap }`) into a
    second bus. The taps are Ableton's three (owner): **Pre FX** (after
    trim and polarity, before the devices), **Post FX** (the default, after
    the devices, before mute and the fader) and **Post Mixer** (after
    mute, fader and pan). Pre and Post FX ignore mute, so a muted ghost
    kick still keys; Post Mixer follows it. A node with devices or taps
    applies mute after them; others keep mute folded into the input gain,
    so routings without devices cost what they did;
  - sidechain edges join the compile order and the loop check: a group can
    listen to its own track, but a track cannot listen to its group.
- **Compensation** (`compile.rs`): each node's input arrives at the latest
  of everything feeding it (an output or send after its source's devices,
  a Pre FX sidechain before them). Every earlier path goes through a delay
  line as long as the difference: outputs, sends, sidechains, and what the
  tracks play into a node that waits for a late sidechain (an input line).
  All device outputs and the cue bus line up to the latest of them: the
  routing's `Compensation { frames, path }`, where `path` lists the
  latent nodes on the longest path. Lines are allocated when the plan is
  compiled. A new plan takes over the contents of the lines it shares
  with the old one, by key (`LineKey`), so a reroute while playing keeps
  what is in flight. A node whose paths pass through lines keeps running
  while silent, so its lines drain rather than stop with audio still in
  them.
- **Read ahead** (owner): `Source::latency` reports the compensation, and
  the engine renders the timeline that far ahead of the playhead
  (`Engine::ahead`). What is heard lines up with the playhead, the grid
  and the metronome. The playhead follows the render position that far
  behind, jumps included (`playhead.rs`): a loop wrap is rendered early
  and heard when the playhead wraps. Playback starts with a pre-roll the
  length of the compensation, during which the playhead shows where play
  started and the metronome stays silent.
- **Jumps while latent** (locates, loop wraps, tempo changes) are
  crossfaded by the mixer where the tracks play into their buses: 2 ms
  from what they would have played next. The engine's crossfade at the
  output would come before the jump is heard. Without latency the engine
  crossfades as before; nothing changes for projects without devices.
  Playback from stop empties the delay lines and fades the tracks in.
- **A latency change while playing** (a device added, removed or changing
  its latency) keeps the playhead going and moves the render position to
  stay ahead of it. The audio skips or repeats the difference: new delay
  lines start silent, and the input fades into them, so the gap comes and
  goes without a step. This is a correction to the pitch, which said "no
  gap". A gap of the difference cannot be avoided without rendering that
  stretch twice, and other hosts behave the same. It happens only when
  latency changes during playback.
- **The readout** (owner's D1, from rendered mock-ups): nothing while no
  device adds latency; then the audio status ends with "12.4 ms
  compensation", brighter than the rest of the device line. Hovering it
  puts "Delay compensation 12.4 ms · longest path: Kick → Master" in the
  info line. The audio panel always has a Latency group (output, round
  trip, delay compensation in ms and samples) and the longest path while
  there is one (`transport/compensation.rs`). The owner preferred D1 to a
  readout that is always shown (D2) and to folding compensation into the
  latency figure (D3).
- **Saving sidechains waits for the first device that uses them** (owner):
  no format change now. The project gets sidechain sources (format v11,
  with a migration) with the compressor or VST3.
- Until then the app adds the synthetic device for tests and trials only:
  `testing::set_test_latency` and `--test-latency <ms>` (on the first
  track), through `routing::Devices`.

### Consequences
- A latent node costs a ring-buffer swap per sample per line it uses. The
  perf gate's new `engine_latent_64` scenario (`testing::latent`: the routed
  mix with latent devices on every eighth track and the master, and a
  sidechain) measures it, and the allocation guard and RTSan check the
  same routing while latencies change, loops wrap, and playback locates,
  pauses and stops (`tests/realtime.rs`).
- Measured (performance gate, instructions, WSL): `engine_latent_64`
  1.33 G against 0.82 G for the same mix without latency (+61%: lines on
  most paths, since a latent master and eighth of the tracks put every
  other path and group input through a line). That is about 0.15 ms of a
  2.67 ms block on this machine. Projects without latency: `engine_routed_64`
  +0.55%, `engine_transport` +0.67%, `engine_clips_64` +0.02%. Two fixes
  kept them there: without latency the engine leaves the `Playhead`
  alone, and the plan holds its delay scratch rather than zeroing 2 KB
  on the stack every block (`engine_transport` was +3.16% before them).
  The baseline was updated from WSL for the new scenario, as an intended
  change.
- Plans whose shapes differ still crossfade over 5 ms. A plan's shape now
  includes its devices, sidechains and line lengths.
- Scrubbing renders through the delay lines, so with latency the scrub is
  heard that much later, and the loop-scrub's own envelope does not line
  up with the restarts it hides. Recorded in KNOWN_ISSUES for M6, when
  latency first exists outside tests.
- Meters show a node's level when it is processed, up to the compensation
  ahead of what is heard (milliseconds at most).

### Evidence
`mixer::delay` tests (a line delays by its length at any block size; a
new plan carries what is in flight, longer or shorter). `mixer::compile`
tests (sidechain sources come first; sidechains that loop, name a
missing node or feed themselves are refused; the same routing compiles the
same way, and devices and sidechains change its shape; no lines and no
compensation without latency; compensation follows the longest path, with
its latent nodes; a Pre FX sidechain leaves before the devices).
`playhead` tests. Engine `tests/compensation.rs`: what is heard lines up
with the playhead after a pre-roll that shows the start; parallel paths
(a latent track or a latent group against a straight one, upside down)
cancel exactly; sends meet at a return in line; a sidechain lines up
with the input it keys, Pre and Post FX; Pre and Post FX ignore mute and
Post Mixer follows it; loops stay in line with the playhead; a locate is
heard when the playhead jumps; a latency change while playing keeps the
playhead going without a step, and what is heard lines up again after
it. Without the lines the alignment tests fail; without read-ahead the
playhead tests fail (both checked by breaking them on purpose). App
`routing` (a latent device reaches the engine and is compensated),
`transport::compensation` (nothing until there is latency; the info line
names the path). E2E `compensation`: the readout appears only with a
latent track, its hover names the path, the panel shows ms, samples and
the path; two tracks of the same tone, one latent and one upside down,
cancel while playing and after the device is taken off (and do not
without compensation).

## ADR-0042 — The M4 exit walkthrough and known issues (M4 slice 7)

**Status:** Accepted 2026-10-10.

### Context
Slice 7 closes M4: the exit criterion as an E2E scenario and a manual
test, the end-of-milestone review and the report. The owner also asked
for the known issues to be fixed where they can be now.

### Decision
- **The exit walkthrough** (E2E `walkthrough_mix`): three recordings of
  one tone at three levels (kick, snare, vocal; in phase, so levels add
  and each source is told apart by its size); the drums grouped with
  Ctrl+G; a return carrying the synthetic latent device (480 frames)
  standing in for the reverb (a plugin in M6, the built-in one in M8);
  the group's send set from its mixer strip and the vocal's from its
  header, both at 0 dB. Heard at every step, as levels once settled: the
  whole mix (dry plus as much again through the return, which only adds
  up exactly while delay compensation lines the paths up); the drum group
  soloed (the return stays open, the vocal and its send go); the kick
  muted inside the soloed group (its send goes too); the vocal's solo
  added with Ctrl+click; the return soloed alone (wet only); solos cleared
  and the return muted (dry only). Saved and reopened: the same tracks,
  sends and master, and the same mix.
- **Taken while stopped (a bug the walkthrough found).** Stopped, the
  engine took nothing from its clip and routing queues (its idle path
  only let meters fall). After four edits the clip queue was full, the
  app held back every routing after it (routings follow clip lists), and
  Play started on stale mutes, solos and sends, which glided in after a
  frame; the compensation readout stayed stale meanwhile. `Mixer::idle`
  and `TrackSource::idle` (the clip player) now take what arrives while
  stopped; nothing plays, so nothing crossfades, and a latency change
  while stopped only moves the read-ahead (as before).
- **A running session's autosave is not offered to another Lumen**
  (KNOWN_ISSUES, M3 4c). A session holds an exclusive lock on
  `<session>.lock` in the autosave folder, taken on the autosave thread
  before its first autosave is written and released (and the file
  deleted) when the autosave is cleared or the session closes. Recovery
  skips sessions whose lock is held (a shared try-lock fails). The system
  frees the lock when a process ends however it ends, so a crashed
  session's autosave is offered as before; sessions from older versions
  have no lock file and are offered as before. Discard deletes the lock
  file with the rest. Single instance itself stays in M9.
- **Submenus are named without egui's arrow**: `widgets::submenu`
  replaces `ui.menu_button` inside menus (Colour, Fades, Grid, Open
  recent); the arrow is drawn, and screen readers hear "Fades".
- **The GPU pipeline cache never waits on the disk on the UI thread**
  (KNOWN_ISSUES, M3 review L3): `canvas::PipelineCache::prefetch` reads
  the file on its own thread from `run()`, before eframe opens the window
  and the GPU (which take far longer), and `install` writes it back on a
  thread. Devices without pipeline caching (DX12, Metal) do no disk work
  for it. `lumen-render` takes and gives bytes; it no longer touches
  files. Tests pass no cache, so they never touch the user's.
- **The M3 walkthrough's flake on Linux**: the two takes are always the
  same length; in some runs the Stop lands in the audio block before, so
  that block holds the stop's fade in one take and full sound in the
  other (worst difference: the tone's full peak, frames 33,832 onwards).
  The comparison stops 2,048 frames before the end of the shorter take.

### Consequences
- Edits made while stopped reach the engine as they are made, so Play
  starts on exactly what the project says.
- A lock file sits beside each autosave while a session runs.
- The known issues left are hardware checks, later features, or visual
  changes scheduled for M10 that want mock-ups first (KNOWN_ISSUES).

### Evidence
- Engine `tests/mixer.rs`
  `while_stopped_every_clip_list_and_routing_is_taken_and_play_starts_on_the_last`
  (fails at the fifth list without the fix); `tests/realtime.rs` adds a
  stopped stretch with lists and routings arriving, under the allocation
  guard.
- App `routing` test: a latent device on a return is compensated.
- `autosave` tests: a running session is not offered to another, and is
  once it ends; a normal close releases the lock and its file; a
  recovered session is locked again by its next autosave.
- E2E `fades` finds the submenu by its exact name "Fades".
- WSL mirror: the M3 walkthrough failed 5 of 14 runs before, passes 12 of
  12 after.
- E2E `walkthrough_mix` passes; without the idle fix it timed out
  waiting for the compensation.

### Found by the owner after the walkthrough (2026-10-10)
- **A clip copied to another track pasted back onto its own.** Paste went
  to the first selected track, or else where the clip was copied from;
  clicking a clip clears the track selection, and a click on another
  track's empty lane only moved the playhead. Now a plain click in the
  lanes (on a clip or empty) makes that track the insert track
  (`Arrangement::insert`) and clears the headers' selection, so Ctrl+V
  pastes there at the playhead (a header clicked afterwards wins again).
  The lanes' menu pastes where it was right-clicked, time and track
  (`Arrangement::menu_spot`), without moving the playhead. E2E
  `a_copied_clip_pastes_onto_the_track_clicked` (keys and menu).
- **"The loudness seems low."** The measurement is right: the EBU
  reference tone (stereo 1 kHz at −23 dBFS) reads −23.0 LUFS through the
  engine at unity, and −26.0 LUFS momentary and integrated, −26.0 dBTP,
  through the app, where a centred track takes the default −3 dB pan law
  (ADR-0035). So every file plays and measures 3 dB under its own level.
  E2E `the_loudness_column_reads_the_reference_tone_less_the_pan_law`.
  Whether stereo material should keep its level at the centre is the
  owner's choice (pending).
