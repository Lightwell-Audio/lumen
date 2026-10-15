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
| [0017](#adr-0017--engine-transport-and-timeline) | Engine transport and timeline: transport stop behavior, positions in ticks, live tempo and rate changes | Accepted 2026-10-06 | No |
| [0018](#adr-0018--transport-bar-icons-and-app-audio) | Transport bar: vector icons on egui's painter; the app opens the system default output until the device selector | Accepted 2026-10-06 | No |
| [0019](#adr-0019--audio-files-and-clip-playback) | Audio files: symphonia + rubato into memory; clip player with edge and jump fades; one lane per file until M3 | Accepted 2026-10-06 (owner: memory now, streaming in M9) | No |
| [0020](#adr-0020--metronome-test-tone-and-audio-health) | Metronome and test tone in the engine; dropouts from driver positions and delivery against real time | Accepted 2026-10-06 | No |
| [0021](#adr-0021--device-selector) | Device selector: saved settings, ASIO probing in a helper process, run-time driver switching by handing the engine to a new thread | Accepted 2026-10-06 | No |
| [0022](#adr-0022--performance-gates-by-instruction-count) | Performance gates: instruction counts under Callgrind in a pinned Linux container, compared with a committed baseline | Accepted 2026-10-07 | No |
| [0023](#adr-0023--design-system-and-motion) | Design system and motion: semantic tokens with dark and light palettes, egui style built from them, our own window title rows, motion tokens with Reduce motion | Accepted 2026-10-07 | No |
| [0024](#adr-0024--undo-by-shared-snapshots) | Undo by shared snapshots: every step is a cheap copy of the project that shares untouched tracks and clips | Accepted 2026-10-07 | No |
| [0025](#adr-0025--tracks-folding-and-interim-volume-and-pan) | Tracks: owner-chosen header and clip design, track folding and resizing, interim volume and pan until the mixer | Accepted 2026-10-07 | No |
| [0026](#adr-0026--clips-on-the-gpu) | Clips on the GPU: a box layer with a per-frame row table; names in egui; measured first with a frame-time benchmark | Accepted 2026-10-07 | No |
| [0027](#adr-0027--waveforms) | Waveforms: 8-bit peak pyramids built at load; drawn per pixel on the GPU from an atlas; disk cache in M9 | Accepted 2026-10-07 | No |
| [0028](#adr-0028--navigating-the-arrangement) | Navigation: f64 view with split beats on the GPU, zoom to single samples, the default key bindings, Follow (page, on), drag to reorder, sample-level waveforms | Accepted 2026-10-07 | No |
| [0029](#adr-0029--end-to-end-smoke-tests) | End-to-end smoke tests: the whole app driven by egui_kittest in CI, plus real-binary smoke tests at the end of every slice; isolated settings, autosaves and a silent test device | Accepted 2026-10-08 | No |
| [0030](#adr-0030--grid-rulers-loop-locators-and-scrubbing) | Grid, rulers, loop, locators and scrubbing (M3 slice 5): a grid model in core, the loop and punch as document edits, punch at the loop's edges, locator keys, both scrub styles in an engine scrub mode | Accepted 2026-10-08 | No |
| [0031](#adr-0031--curves-for-fades-and-envelopes-format-v6) | Curves for fades and envelopes (format v6): a cubic Bézier from silence to full, edited by one free dot with Alt for S-shape, played from tables, drawn on the GPU | Accepted 2026-10-08 | No |
| [0032](#adr-0032--clip-editing-m3-slice-6) | Clip editing (M3 slice 6): the split-clip model, drags redone from where they started, the clip overlap rule, clip properties and the pinned card, drops on a chosen track, Esc for the field being typed in | Accepted 2026-10-08 | No |
| [0033](#adr-0033--commands-and-the-keymap-m3-slice-7) | Commands and the keymap (M3 slice 7): one table of commands, chords matched exactly, one command per key whatever is selected, one dispatcher | Accepted 2026-10-08 (7a) | No |
| [0034](#adr-0034--the-info-line-m4-slice-0) | The info line (M4 slice 0): hover hints leave the pointer for one strip along the bottom of the window, with K5 keycaps and lit modifiers | Accepted 2026-10-09 | No |
| [0035](#adr-0035--the-mixer-graph-m4-slice-1) | The mixer graph (M4 slice 1): a node per track with a stereo bus, compiled on the UI side into a flat ordered plan, swapped at a block edge with levels carried by id; plans of a new shape crossfade over 5 ms | Accepted 2026-10-09 | No |
| [0036](#adr-0036--routing-in-the-model-format-v7) | Routing in the model (format v7): a master track kept apart from the tracks, outputs, sends, polarity, trim and the pan law; one routing rule shared by validation, edits and the engine | Accepted 2026-10-09 | No |
| [0037](#adr-0037--group-tracks-format-v8) | Group tracks (format v8): a group's tracks always follow it in the list; grouping, ungrouping, folding, nested rails, lane bars, arming and deleting a group | Accepted 2026-10-09 | No |
| [0038](#adr-0038--mute-solo-returns-master-and-cue-m4-slice-4-format-v10) | Mute, solo, returns, master and cue (format v10) | Accepted 2026-10-09 | No |
| [0039](#adr-0039--lumens-licence-gpl-30-or-later-supersedes-adr-0012) | Lumen's licence: GPL-3.0-or-later, sold packaged binaries (supersedes ADR-0012) | Accepted 2026-10-09 | Yes (owner items) |
| [0041](#adr-0041--sidechains-and-delay-compensation-m4-slice-6) | Sidechains and delay compensation: devices and taps in the graph, delay lines, read ahead, the D1 readout | Accepted 2026-10-10 | No |
| [0042](#adr-0042--the-m4-exit-walkthrough-and-known-issues-m4-slice-7) | The M4 exit walkthrough; edits taken while stopped; session locks for autosaves; submenu names; the pipeline cache off the UI thread | Accepted 2026-10-10 | No |
| [0043](#adr-0043--the-light-layer-and-the-mark) | The light layer (five levels of light) and the mark (Dawn in Sunrise): the playhead beam, meter heads, the mark as a master meter, the window icon | Accepted 2026-10-10 | No |
| [0044](#adr-0044--midi-devices-and-clip-events) | MIDI devices (`lumen-midi`, midir, a worker thread) and clip events (format v11) | Accepted 2026-10-10 | No |
| [0045](#adr-0045--hearing-midi-the-event-path-the-synth-and-midi-to-m5-slice-1-format-v12) | Hearing MIDI: the event path, the synth (`lumen-devices`) and MIDI To (format v12) | Accepted 2026-10-10 | No |
| [0046](#adr-0046--playing-live-midi-from-monitoring-the-computer-keyboard-and-the-midi-chip-m5-slice-2-format-v13) | Playing live: live input in the engine, MIDI From and Monitor (format v13), the computer keyboard, the MIDI chip | Accepted 2026-10-11 | No |
| [0047](#adr-0047--midi-clips-and-the-piano-roll-m5-slice-3-format-v14) | MIDI clips and the piano roll: note edits and mute (format v14), notes on the GPU, the clip editor and its pop-out, auditioning | Accepted 2026-10-11 | No |
| [0052](#adr-0052--the-design-system-replaces-adr-0051s-lit-look) | The design system's material and tokens replace ADR-0051's lit look | Accepted 2026-10-11 | No |
| [0055](#adr-0055--recording-midi-m5-slice-6-no-format-change) | Recording MIDI (M5 slice 6, no format change): the engine's Record with a count-in, live capture with its timeline sample, a pure take builder (Replace, Overdub, loops, punch) and one undo step per take | Accepted 2026-10-13 | No |
| [0058](#adr-0058--notes-carry-on-across-seams-and-overdub-shows-its-clip-at-once) | Notes carry on across seams (touching MIDI clips that hold the same note play it as one), and Overdub shows its clip at once | Accepted 2026-10-13 | No |
| [0059](#adr-0059--a-carried-note-belongs-to-its-clip-and-the-clip-editor-shows-hidden-notes) | A carried note belongs to the clip it began in (a chase there does not strike it), and the clip editor shows notes outside the window | Accepted 2026-10-13 | No |
| [0014](#adr-0014--repository-layout-and-ci) | Repository layout and CI | Accepted: **private GitHub repository** (owner creates it) | — |
| [0065](#adr-0065--deterministic-test-plugins-lumen-test-plugin) | Deterministic test plugins with injectable faults (`lumen-test-plugin`, M6 slice 0) | Accepted 2026-10-15 | No |
| [0066](#adr-0066--the-plugin-host-crate-and-its-protocol) | The plugin host crate (`lumen-plugin-host`): loading through the OS loader, a bounded versioned protocol, stale-block rejection and deadlines (M6 slice 0, part 2) | Accepted 2026-10-15 (the audio path and the process-grouping policy are not decided) | No |
| [0067](#adr-0067--the-shared-memory-audio-path-and-the-measurement-harness) | The shared-memory audio path (`lumen_plugin_host::shm`): one block in flight, untrusted-region rules, two-sided wake-up settings, Windows job object, and the measurement harness (M6 slice 0, part 3) | Accepted 2026-10-15 for the transport (the wake-up, the real-time wait contract and the process grouping wait for MT-VST-0) | No |

---

## Decisions made by the owner (2026-10-05 onwards)

| Question | Decision |
|---|---|
| Product-neutral documentation (2026-10-14) | Do not use third-party products or vendors as design references, comparisons, endorsements or examples; describe the behavior directly. Preserve technical identifiers, exact protocol requirements, hardware names and required legal attribution. |
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
| Playheads (M2 slice 7, 2026-10-06) | Two: a grey start marker on the ruler's bottom edge (where Play starts) and the blue playhead. Space plays from the marker and stops in place. |
| Motion (2026-10-06) | Fluid motion: the motion system and a retrofit of the M2 UI in M3 (done in slice 2), and a polish pass in M10. |
| Metronome extras (2026-10-06) | Count-in, pre-roll and record-only with recording (M9); output routing with the mixer (M4). |
| M2 close and M3 plan (2026-10-07) | M2 approved; M3 approved as proposed, unsplit, with the M2 performance gates as its slice 1. |
| Typefaces (M3 slice 2, 2026-10-07) | Inter for the interface and Unbounded for titles, chosen from rendered specimens. The owner wants personality, not generic choices; visual options are shown as specimens before anything is built (ADR-0023). |
| Track design (M3 slice 4, 2026-10-07) | Option A headers with option B's dark clips; type icons; track folding and resizing in 22 pt steps; sliders with values inside, with pan saturating toward the edges; record arm that glows and breathes; a "+ Add track ▾" menu button with a double-click hint (DESIGN §7a, ADR-0025). |
| macOS CI (2026-10-07, resumed 2026-10-10) | Paused on 2026-10-07 when the GitHub Actions minutes ran out. **Back on 2026-10-10** (owner): CI tests Windows, macOS (Apple silicon) and Linux again. A new push to a pull request cancels its superseded run to save minutes. The cross-target clippy (DEVELOPMENT.md) stays as a quick check before pushing macOS-specific code. |
| Reviews (2026-10-07) | A full repository review at the end of every milestone, against CLAUDE.md, the architecture and the design system (the `lumen-architecture` and `lumen-design` skills), before the milestone report. |
| Navigation (M3 slice 4d, 2026-10-07) | **Zoom to single samples**; waveforms draw individual samples past the stored detail. **Follow playhead: page by default, on**; continuous is the second mode. Keys: `+`/`-` zoom, X back, W fit width, H fit height, Alt+wheel a track's height. |
| Rulers and scrubbing (M3 slice 5, 2026-10-08) | **Ruler layout A** (chosen from rendered specimens): locators and the loop brace in a strip above the bar ruler, a time ruler (minutes and seconds, samples or timecode) along the bottom of the arrangement. **Both scrub styles:** dragging in the scrub strip plays short loops of the grid step under the pointer at normal pitch (for songwriting and arranging); holding Alt scrubs tape-style, audio following the hand's speed and pitch (for precise editing). |
| Drag hints (MT-APP-13 run 1, 2026-10-08) | Chosen from rendered specimens: **A1** the loop brace grows a bracket at the edge a drag will resize, lights when it will move, and a chip names the action; **B2** (on trial) the scrub strip washes the step a drag would play and shows a quiet hint ("drag to listen · Alt tape"), and a tape scrub shows its speed; **C1** the grid button always shows the size, with a magnet lit while snapping and struck through when not. |
| Clip drag model (M3 slice 6, 2026-10-08) | **Split clip** (chosen over a no-marquee variant and a classic model where a modifier draws a range): the top half of a clip moves it (Ctrl copies), the bottom half selects a time range across tracks, and empty lanes draw a marquee. Cursors and hints show which half is under the pointer. |
| Selection look (M3 slice 6, 2026-10-08) | Chosen from rendered specimens: **S3 glow**, selected clips glow in the accent like the record arm, a time range is a band with accent brackets along its top, the marquee a soft rounded accent box ("some character"); **H1**, hovering a clip brightens the half under the pointer with a dashed line at the split. |
| Fade and gain handles (M3 slice 6d, 2026-10-08) | Chosen from rendered specimens: **F3 with the glow**, a fade is a glowing accent curve with a round handle at its top and a dot that bends it; **G1**, clip gain is a glowing level line across the clip, dragged up or down, with the waveform shrinking under it and the dB beside the pointer. |
| Free curves (M3 slice 6d, 2026-10-08) | The owner asked for automation curves with points that move freely in both time and value. Chosen from interactive specimens: **C3**, one free dot the curve passes through, with Alt-drag adding S-shape (over C1, the dot alone, and C2, two Bézier handles). Replaces the stepped bend (Slow, Linear, Fast). ADR-0031. |
| Clip properties (M3 slice 6e, 2026-10-08) | Chosen from rendered specimens: **M1**, a muted clip drains to grey (name and waveform stay readable); **K3**, a locked clip shows a padlock in its title bar and light clamp bars at both ends; **P1**, the properties view is the hover card pinned beside its clip and edited in place (Alt+Enter, or Properties in the right-click menu), after seeing it in context, over a strip under the arrangement (P2) and a toolbar above the clip (P3). ADR-0032. |
| Lanes menu and grid behind clips (MT-APP-14 run 2, 2026-10-08) | Chosen from rendered specimens: **A**, each right-click gets the menu for what it landed on (a clip, a time range, or empty lanes; commands that cannot run are left out), over B (grouped submenus) and C (an action strip); **G1**, the grid shows through a see-through clip body, the owner chose G1 unless there was a strong argument for G2; none held up, over G2 (lines drawn across clips) and G3 (bar lines only). The owner also asked for a click anywhere on the lanes to move the playhead. ADR-0032. |
| Hints and tooltips (2026-10-08) | The owner noted that hints have piled up slice by slice: behaviour should come across without reading much (cursors, handle shapes, motion before text). **A dedicated UX pass**, scheduled when the next milestones are planned; until then new hints stay short. |
| Split and tools (2026-10-08) | The owner asked to select a clip and drag it to cut it. **Split** is planned in M7 with the rest of the split work, and the drag-to-cut gesture is chosen there from rendered specimens. The tool model (pointer, draw, erase, split, mute, zoom; CLAUDE.md §5) is proposed for M7 with it, for the owner to confirm. |
| Commands and shortcuts (M3 slice 7, 2026-10-08) | Chosen from interactive specimens: palette **P2 with P3's values** (context names such as "Duplicate 2 clips", commands that cannot run listed dimmed with the reason, a detail strip, typed values becoming actions); keycaps **K5 area light** after asking for more personality than the first round (plain modifiers, the main key outlined with a light in its area's colour); shortcut editor **E2 with E3** (a keyboard map lit by area over the list, plus rebinding from a palette result); **Shift+Tab depends on focus**, F6 and Shift+F6 between panes (ADR-0015). |
| Clips from the keyboard (M3 review H2, 2026-10-08) | **The arrows follow the selection**: with a clip selected ←/→ select the previous or next clip on its track and ↑/↓ the nearest clip on the track above or below, Shift adds; with tracks selected the arrows act on tracks as before (over Alt+←/→ only, and [ and ]). ADR-0033. |
| M3 close (2026-10-09) | M3 approved after MT-APP-16 passed. The M4 plan (`docs/milestones/M4.md`) waits for approval and its three questions. |
| M4 plan (2026-10-09) | Approved. **Solo behavior** (a click solos only that track, Ctrl+click adds). **The UX pass on hints runs first**, as M4 slice 0. Pan law −3 dB constant power by default (the proposal, recorded as an assumption). |
| Group tracks (M4 slice 3, 2026-10-09) | Chosen from rendered specimens and asked before building: **H1**, a group's tracks step in 12 pt a level with every enclosing group's colour rail running down beside them (over H2 tree lines and H3 a tinted block); **L1**, a group's lane shows a thin bar per track inside it in that track's colour (over L2 a summed band with a waveform and L3 ghost clips); **deleting a group deletes its tracks**; **a group's arm button arms its tracks** (half a ring when some are armed) over no arm on groups. Also proposed and not objected to: rules for Ctrl+G, Ctrl+Shift+G, the fold arrow, drops and groups holding no clips (ADR-0037). |
| Group tracks, MT-MIX-3 run 1 (2026-10-09) | The owner's findings: **groups go three deep at most** (Group › Group › Group › tracks; "needing more than that probably doesn't do much"), refused with a note; **the track header column is resizable**, its width **saved per project** (over app-wide), 204 to 480 pt; soloing a track in a group must not silence it (a bug, fixed). ADR-0037 amendment. |
| Routing in the model (M4 slice 2, 2026-10-09) | Asked before building: **a new send starts at −∞** (over 0 dB) and **post-fader** (over pre-fader); **deleting a return removes its sends** in the same undo step (over asking first); **the pan law is chosen from the mixer panel's menu** and the palette (over a project settings window, M9). |
| Dropouts (2026-10-09) | Chosen from mockups: **A, recent and in words**: no chip while audio is healthy, "Dropouts ×3" in amber while they happen, fading after 10 s of clean audio, details in the audio panel; over B (a labelled total until reset) and C (an amber status dot). Driver resets no longer count (ADR-0020 update). |
| Mixer behaviour (M4 slice 1, 2026-10-09) | Asked after the graph was built (the owner: variants first, before building): **centre −3 dB** (a hard-panned side at unity; projects from M3 play 3 dB quieter) over centre unity with +3 dB sides; **balance** panning of stereo tracks over a true stereo pan; **mute silences every send**, pre-fader ones too over pre-fader sends playing on; **post-fader sends after the pan** over before it. All four as built (ADR-0035). |
| Pan law for stereo (after M4 slice 7, 2026-10-10) | **B**: stereo material keeps its level at the centre (balance); the project's pan law (−3 dB by default) applies only to tracks whose clips are all mono files. Chosen over keeping −3 dB on every track (every file played and measured 3 dB under its own level) and a 0 dB default. Projects from before play stereo tracks 3 dB louder. ADR-0042. |
| Hints (M4 slice 0, 2026-10-09) | Chosen from interactive specimens: **an info line** along the bottom of the window (C, a persistent info line) over quiet chips (A) and rest-to-reveal (B), **with the K5 keycaps** the palette and menus use (the owner's addition, for consistency), **no chips for held keys** (the cursor and the lit action say it), full width (ADR-0034). |
| Mute, solo, returns, master and cue (M4 slice 4, 2026-10-09) | The solo rules agreed as a table of 14 cases before building (`docs/milestones/M4.md` §5): solo keeps a soloed track's path open, **mute always beats solo** along the path, returns are solo-safe, **a soloed return plays wet only** (sends into it stay on, their tracks' outputs go silent), the master, metronome and cue are never touched by solo. **D1 A** a group's solo solos the group (its tracks show it hollow); **D2 A** additive solo is a preference (Ctrl+click then replaces); **D3 A** a lit "S" chip in the transport while anything is soloed, a click clears; **D4 A1** returns then the master pinned at the bottom under a divider, a "Returns" caption that folds them, returns lettered A, B, C and never grouped; **D5 C1** a Solo/Cue switch on the master, cue **pre-fader** to a chosen pair, unavailable with a note when that pair is the main one; **D6** the metronome has its own output (Main, Cue or a pair), never through the master; **D7** palette commands Solo and Mute selected tracks and Clear all solos, no default keys. Chosen from interactive specimens: **sends as K1 "comet" knobs** (a trail of light in the return's colour, brightening to a glowing head; a dot marks pre-fader; hovering a send lights its return and the reverse) over lantern, droplet and plain knobs; **a track silenced by another's solo shows a hollow M and a dimmed name** (hollow S inside a soloed group) over dimming the whole row. Asked while building 4d: **O1**, the cue output chosen in the audio settings beside the main pair, over a separate cue output pair (so the metronome's output is Main or Cue); the owner's interface has two outputs, so cue is checked on the four-output test device. MT-MIX-4 runs 1 and 2, from mock-ups: **C4** knobs step up on selected rows, **O1** a "+N ▾" button drawn as "+ Add track ▾", **R2** an unfolded return lists its senders, **L4** its four loudest then "+N more", **S1** silent sends leave that list when let go; **12 returns at most**; the standard +6 dB of an empty return kept. |
| Mixer panel and meters (M4 slice 5, 2026-10-09) | Chosen from interactive specimens: **S1 console** strips (fader and meter side by side, 76 pt) over S2 light rail and S3 broad; sends **O2**, two knobs then a "+N ▾" list (over a grid of six, or every knob in a taller strip); meter colours **M1 accent by default, M2 track colour as a setting** (over M3 classic green); loudness **L2**, a column beside the master; header meters as well, **H2** inside the volume slider (over H1 at the edge and H3 the colour rail, which already carries arm and nesting). Behaviour, all as recommended: **B1** post-fader after the pan; **B2** peaks fall 20 dB/s, a 1.5 s hold, clip latched until clicked; **B3** a latching wheel; **B4** one fold with the arrangement; **B5** integrated loudness resets on Play from stop; **B6** any output pair; **B7** sections fold away in a short panel; **B8** strips drag to reorder, returns included. ADR-0040. |
| Sidechains and delay compensation (M4 slice 6, 2026-10-10) | Behaviour asked before building: **read ahead** (what is heard lines up with the playhead; live input in M11 gets a reduced-latency monitoring option) over delaying everything and moving the drawn playhead; three sidechain taps (Pre FX, Post FX by default, Post Mixer; FX taps ignore mute) over post-fader only; **sidechains saved with the first device** that uses them (format v11 then) over saving them now. The readout from rendered mock-ups: **D1**, only while there is compensation, at the end of the device line, the path in the info line, always in the audio panel (over D2 always shown and D3 folded into the latency). ADR-0041. |
| Interface audit (after M4, 2026-10-10) | Chosen from interactive mock-ups (`docs/audits/2026-10-10-design.md`): value fields take the surface's **control fill** (T1); a **`well` token** for grooves and meter wells (T2); a **switch and a segmented control** replace egui's checkboxes and radios (T3); **underline tabs**, toggles keep their fill when off (T4); the arm button **grey at rest**, red on hover (T5); numbers in **Martian Mono** at width 87.5 (S1, download asked when built); the slider handle **a solid line that opens into top and bottom ticks near its value** (S2, the owner's own variant C); fader scale labels thinned (S3); one size scale (S4); one empty-state widget with a ghost strip in the mixer (S5); swatches **2 × 6 with a "Track colour" pill**, paths shortened in the middle (S6); one time-signature field (S7); a **fading overlay scroll bar** (U2); undo shows an **accent wash and a glide** (M4); clips glide on release and undo, groups fold with a glide, rows glide, highlights slide (M1, M2, M3, M5). Built first, as **milestone 4.5**, recorded as a mid milestone (`docs/milestones/M4.5.md`). |
| MIDI interaction exploration (before M5, 2026-10-11) | From a Design canvas of six MIDI pain points, each with a familiar and an experimental option: **every familiar option for now** (mini piano roll on clips, the piano roll in the detail panel, Capture MIDI in M9, Q and Shift+Q, a velocity lane, linked copies with M7's editing). **Content faces and chord detection move to M8** with drum maps. Velocity drawn inside notes is ruled out: short notes are too small. Table in `docs/milestones/M5.md` §4. |
| Menus with settings (M4.5, after MT-POL-2, 2026-10-10) | Chosen from an interactive mock-up (X1): **C, fades when you leave**: a change leaves the menu open; once changed, the pointer leaving it starts a 0.4 s wait, then a fade; coming back cancels it; Esc, a click elsewhere and commands close it at once. Over A (stays until dismissed) and B (lingers 1.2 s, then fades). Every menu fades out instead of vanishing. |
| Hearing MIDI (M5 slice 1, 2026-10-10) | Asked before building, all as recommended: the built-in synth's default sound is **S1 warm poly** (two saws ±7 cents, 12 dB low-pass at 2.4 kHz, A 5 ms · D 300 ms · S 70 % · R 250 ms, 16 voices, oldest stolen) over clean keys and a square pluck; until the device chain, the sound changes through **E1 presets in a menu** (Poly, Keys, Bass, Pluck, Pad; track menu and palette) over a small knob card or a fixed patch; **T1 MIDI To has one destination at a time** (an instrument or an external port and channel), rather than layering both; **H1 chases notes and controllers** on Play and jumps, rather than chasing controllers only. Plan in `docs/milestones/M5.md`, slice 1. |
| Playing live (M5 slice 2, 2026-10-11) | Chosen from specimens (the "MIDI: Playing Live" design canvas), all as recommended: **L A the selected MIDI track listens** without being armed (a hollow arm ring), plus every armed track (selection-based auto-arm), over armed tracks only; **J A steady timing**, each live message placed at its driver stamp one buffer later, over playing at the next buffer; **C B a MIDI chip with in and out lights** and a popover whose rows show each port's traffic with Track and Remote pills, unplugged ports dimmed, over a word and one light; **K A a lit keycap button and a readout** (`C3–C4 · vel 100`) for the computer keyboard, over a mini keyboard in the bar; **H B a light by the name** of each listening MIDI track, over none; **R A MIDI From and Monitor in the track menu and on the mixer strip**, over an I/O block on tall headers. The computer keyboard toggles with **Ctrl+Shift+K** (M stays Set locator). Plan in `docs/milestones/M5.md`, slice 2. |
| Piano roll (M5 slice 3, 2026-10-11) | Chosen from live specimens (the "Lumen Piano Roll Specimens" artifact), all as recommended: **Look A, studio keys** (a piano-key column, darker lanes for black keys, notes in the track's colour shaded by velocity, an outline on selected notes, handles on hover) over a labelled pitch rail; **Draw A, a click adds one grid step and dragging sets its length** over drag-to-paint; **Live A, preview then commit**: notes follow the pointer and pitches are auditioned, the playing clip changes once on release, over changing it continuously. Built by Sonnet agents at medium effort in worktrees (the owner's request). Plan in `docs/milestones/M5.md`. |
| Clip editor's view (M5 slice 3, 2026-10-11) | Owner: the piano roll zooms and scrolls on its own, independent of the arrangement; it is fitted to each clip it opens; grid settings stay shared. ADR-0048. |
| Time selection in empty lanes (M5 amendment, 2026-10-11) | Owner (MT-MIDI-4): a drag in empty lanes selects time on the tracks it spans, snapped, as a clip's lower half does; the time ruler selects time on the selected tracks, or every track when none is selected, and keeps the selection. The clip marquee moved to Ctrl+Shift (the integrator's choice, for the owner to confirm). ADR-0049. |
| Velocity and controller lanes (M5 slice 4, 2026-10-11) | Chosen from live specimens (the "Lumen Controller Lane Specimens" artifact), all as recommended: **V A stems with round heads**, one per note start, over filled bars as wide as their notes; **L A one lane at a time with a tab switcher** (Velocity, Mod wheel, Pitch bend, Pressure, "+ CC") over a stack of lanes with Add and Remove; **T A Pencil, Line and Curve in the lane\'s own header** over modifier-only drawing; **R B Line, Curve and bend write an event every 1/64 note, dropping only repeated values, and the Pencil follows the grid when snapping is on** over every tool following the 1/16 grid. Plan in `docs/milestones/M5.md`. ADR-0050. |
| MT-MIDI-2 and MT-MIDI-3 follow-ups (2026-10-11) | **MA a monitor badge** on MIDI track headers, only when Monitor is not Auto (a speaker in accent for In, slashed grey for Off; click cycles), over the arm showing all three or an IN/OFF word ("MIDI: Playing Live" canvas, boards M). **Held notes glide to a new preset** at once (no retrigger), over keeping the old sound or retriggering. **The GS Wavetable latency is deferred**: a per-port output offset to be built later, recorded in KNOWN_ISSUES. |
| Design system over ADR-0051 (2026-10-11) | The owner's **Lumen design system** (a Design System artifact: tokens, a Material card and a card per component) is the reference for tokens, components and widgets. Asked where it and ADR-0051 differ, the owner chose **the design system on all four**: **on** is `accent_muted` with accent letters and a 38% accent hairline, no glow; **solo** (and the primary, the chosen tool) is filled accent (`accent_hi` to `accent_lo`, no halo; amber only warns); a **lit M** is the `mute` grey with `on_mute` letters; **raised** controls are `control_hi` to `control` with an edge light and a border, Ember on hover, no drop shadow, radius by height; the **arm** is round with a grey dot at rest. ADR-0052. |
| Recording MIDI (M5 slice 6, 2026-10-12) | Asked and chosen (the plan in `docs/milestones/M5.md`, and the "Lumen Recording Specimens" canvas): **Replace by default, with an Overdub switch** (C4, as planned); **loop passes layer** into one clip in both modes (separate takes are M9); **recording settings in the app settings file**, not the project, so no format change; **a clip runs exactly from record start to stop**, not to the bar line (the owner's choice over the recommended bar-line end); **1-A** the count-in on the position field, counting up through negative bars, and Record pulsing on each beat, with no new widget; **2-B** an Overdub button beside Record, and the count-in and "Metronome only while recording" in the metronome's menu; **3-A** a clip being recorded is record red with a "REC" tag and drops into its track colour on stop. Count-in and record-only for MIDI move from M9 (the 2026-10-06 row) to this slice; audio recording stays M9. ADR-0055. |
| Covered MIDI clips (M5, 2026-10-13, after MT-MIDI-7 run 3) | Asked, from two variants: **A, the clip window** (chosen, as recommended): a clip covered, trimmed or split keeps every note, hidden outside its edges, and shows them when an edge is dragged back; moving the top clip away leaves a gap. B (stacked clips that reappear by themselves) was set aside. Overdub across clips: the owner will reproduce the observed behavior before deciding whether to change it. Format v15. ADR-0056. |
| Recording edges and MIDI trims (M5, 2026-10-13, the owner's four points) | Asked for directly: MIDI clip edges that drag without jitter or offset; Overdub begun at or before a clip extends that clip and merges into it; recording begun in the middle of a clip makes a new clip from the playhead to the stop; and the clip recorded over redraws continuously, not at commits. Built as stated (no variants pitched: the owner gave the behaviour). ADR-0057. |
| Overdub at clip edges (M5, 2026-10-13, MT-MIDI-7 run 7) | Asked for directly, reversing part of the row above: Overdub begun before a clip and reaching its start, or begun exactly at its start, records a **new clip** and does not merge into or grow the clip there in the arrangement; the new clip carries the track's existing notes along with the take, and the old clip stays a clip of its own. Built as stated. ADR-0060. |
| MIDI learn moved to M7 (M5, 2026-10-13) | Asked, as recommended: **MIDI learn and its mapping list move to M7**, built with automation, where M6 and M7 define the parameters it maps and a mapped controller writes automation through the same path as Lumen's controls (Touch and Latch). M5 slice 7 keeps Standard MIDI Files and MIDI clock out (plan in `docs/milestones/M5.md`). Its choices, asked in chat, all as recommended: **F1 A** a multi-track file dropped on a track merges into one clip there, dropped between tracks or on empty lanes makes one MIDI track per file track (type 0 split by channel); **F2 A** notes are placed in beats and the project tempo is untouched, with a notice offering "Use them" for the file's tempo map and first time signature (one undo step); **F3 A** clock runs only while playing; **F4 A** a Sync delay per port, −20 to +50 ms. MTC out [S] is deferred, unscheduled. |
| Waveform peak cache (M3 slice 4c, 2026-10-07) | **Deferred to M9**, built with disk streaming: until then audio is decoded into memory at load and peaks take about 21 ms per 5 minutes (ADR-0027). |
| App and file icons (2026-10-07) | An app icon, a `.lumen` file icon and per-user file association in M9, shown as specimens first. Project folders keep the standard folder icon. |
| Logo and light (2026-10-10) | Chosen from rendered specimens on the identity canvas (a Design artifact): four marks, then six colourways. **D, Dawn, in Sunrise**: a sun rising over a horizon, its reflection breaking into a waveform; an amber sun (`#FFC46B`) over water in the accent. **The light layer holds** (five levels: Unlit, Ember, Lit, Beam, Flare, and five rules), and **the mark is a meter** (the sun rises with the master's level, red while recording), with every proposed use of light: the playhead beam, Play's beam, meter heads, the mark above an empty arrangement. ADR-0043. A guitarist-first direction was pitched and set aside by the owner. |

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
2. **Main process plus sandboxed plugin host processes**.
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
from the OS; a polished custom look. Nearly every arrangement and editor view (clips,
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
3. Anticipative pre-rendering of non-live tracks with extra buffering. This can
   reduce CPU use, but adds complexity. The architecture should allow it,
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
| XML (gzipped .als) | Verbose; awkward in Rust; gzip defeats "human-friendly". |
| SQLite | Transactional and incremental, but not human-readable. Schema migrations are well understood. |
| Binary (bincode, MessagePack) | Fast and compact; opaque; brittle to evolve. |

### Decision (proposed)
- A **project folder** containing `<name>.lumen`, a UTF-8,
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

**Status:** Accepted 2026-10-05 (the index records it; P-VST3 and P-ISO evidence is below). The open points M6 settles by measurement are the wake-up mechanism, the real-time wait contract and the process grouping (`docs/milestones/M6.md` §2) · **Date:** 2026-10-05

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
  per-module time). Test instruments D, E and B, a 21-module effect suite, Test effect A,
  and several other modules succeeded. Test instrument I failed in its own `DllMain` (`ERROR_DLL_INIT_FAILED`) after
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
   (Test instrument D 1.9 s, Test instrument I 8.9 s), so the scanner must be parallel,
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
Test instrument H, 48 kHz, 128-sample blocks, MSVC release build.

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
- The Focusrite driver is **multi-client** (it streamed alongside the driver and output device),
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
1. Floating-point beats (`f64`). Precise enough, but
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
The spec (§5) assigns Shift+Tab to "toggle detail panel".
Shift+Tab is also the standard way to move keyboard focus backwards, and the
spec requires keyboard-only core workflows. In the M1 shell, egui handled
both at once: focus moved into the detail panel as the panel was hidden, and
AccessKit panicked on the vanished focus target. That crash was fixed on
2026-10-06: Lumen now takes Shift+Tab before egui sees it, and releases focus
whenever a panel is hidden.

### Options
1. Keep Shift+Tab for the detail panel (as specified for the detail-panel toggle) and give
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
under a running stream), and behave the way producers expect. The spec's default shortcut set defines the transport semantics.

### Decision
- **State:** the engine keeps the play position in samples (what the device
  consumes) and every marker (play marker, loop, punch) in ticks (what the
  music means). Sample positions of the markers are cached and recomputed
  when the tempo map or sample rate changes.
- **Tempo, time-signature and rate changes** carry the position through ticks
  (`ticks_at_sample` on the old mapping, `sample_at` on the new one), so the
  bar and beat stay put. Because the position lives on whole samples, a
  change can move it by up to half a sample.
- **Semantics (revised 2026-10-06 at the owner's
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
    also applies to recording.
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
    size from their own panel. A running ASIO driver reports
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
owner asked for fluid motion (2026-10-06). The M2 UI used a
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
  project but are not undo steps.
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
track folding and vertical resizing.

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
   . Measured: a third of a pixel of scroll changed 3.5% of
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
to single samples (to single samples) and Follow by page, on by default
(owner-decisions table).

### Decision
- **One horizontal view, `TimeView`** (`timeview.rs`): the beat at the
  left edge and points per beat, in f64. Zooming about an anchor,
  scrolling, fitting, the limits and glides are pure functions, tested.
- **Limits** (after MT-APP-12 run 1): the song sets how
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
- **Keys:** Ctrl+wheel or a pinch zooms at the pointer
  (one gesture is one step back); `+` and `-` glide around the pointer,
  else the playhead, else the middle; X steps back; W fits the song's
  width; H fits every track's height in whole 22 pt steps, and H again
  restores; Alt+wheel steps the height of the track under the pointer.
- **Follow** (`app/navigate.rs`): page turns the view when the playhead
  passes 95% of the width and puts it 2% in; continuous keeps it 40% in.
  Scrolling or zooming by hand, fitting, stepping back and clicking the
  ruler pause it; Play or the button resumes it. A Follow button (a
  playhead line with an arrow) toggles it, right-click chooses the mode,
  Ctrl+Shift+F. Saved in settings.
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
locators, and arrangement scrubbing (an owner request raised from S to
M). The owner chose ruler layout A and both scrub styles from rendered
specimens (owner-decisions table, "Rulers and scrubbing"). This records
the choices made inside those.

### Decisions
- **The grid is a model in `lumen-core`** (`grid::Grid`): adaptive in five
  widths (6 to 64 points per line) or fixed from 8 bars to 1/64, with
  triplet and dotted feels and snap on or off. `snap`, `cell` (the step a
  time falls in) and `lines` are pure and tested, so clip editing (slice 6)
  and the piano roll (M5) share one implementation. the default keys:
  Ctrl+1 to Ctrl+5. Alt bypasses snapping everywhere. The grid is saved
  with the view (format v4).
- **The loop and punch are document edits** (format v4 makes them part of
  undo): drawing, moving, resizing and toggling the loop
  are undo steps, one per drag. The transport plays whatever the document
  says (`Transport::follow_loop`, every frame).
- **Punch in and out sit at the loop's edges**, instead of
  a second region: two switches in the loop's menus. Fewer things on the
  strip, and the loop already marks the take. Recording uses them in M9.
- **Locators** are `Project.markers` (format v4), kept in time order. Keys
  (default bindings): **M** sets a locator at the
  playhead or removes the one there (the Set action);
  **Ctrl+Left / Ctrl+Right** jump to the previous or next one; these keys are
  unbound by default and remain available for customization;
  **F2** renames and **Delete** removes the selected one, taking
  precedence over the track keys while a locator is selected. Default
  names are "Locator N". Marker ids share the project's id space.
- **Scrubbing is an engine mode** (`Command::Scrub { at, style }`,
  `Command::EndScrub`; `lumen-engine/src/scrub.rs`), only while stopped;
  while playing, a drag on the scrub strip jumps playback from step to
  step instead.
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
  strip adds a locator there without moving the playhead.
- **The strip** (24 pt) holds the scrub strip and locator flags in its top
  14 pt and the loop brace in its bottom 10 pt; flags take precedence over
  scrubbing, and the brace has its own band (DESIGN §7d).

### Consequences
- `Source::render` is now also called at arbitrary positions while
  stopped. The clip player already rendered by position; disk streaming
  (M9) must keep random access cheap or serve scrubs from a cache.
- Loop and punch changes are undo steps: Ctrl+Z after drawing a loop
  removes it, as an undoable edit.
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
asked for curves with points that move freely in both time and value, setting
the bar for envelopes in general ahead of automation (M7). A fade's shape
must be stored, played on the audio thread, drawn on the GPU, and edited
with one handle (the owner's choice C3).

### Options
1. **More named shapes, or one bend amount** a single fade-slope control. Simple
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
- **the clip overlap rule:** a clip put down on others (moved, copied,
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
  - rename: Ctrl+R or F2 opens a field in the title bar; a clip
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
- **A click on the lanes moves the playhead** (owner):
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
two files failed now and then under load). The drop order is now preserved. Files for new tracks now join from the front of their queue
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
  the K5 keycap light) and its default chords, where a default binding exists.
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
- **Presets.** One default keymap ships and is shown by name in the window;
  the alternative preset is [L] (CLAUDE.md §7).
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
  could not be done from the keyboard. A File command, Ctrl+Shift+I
  (there is no default binding; Ctrl+I is Insert time), in the
  app menu, the palette, a track's menu and the empty arrangement. The
  system dialog runs on its own thread; the chosen files load as dropped
  files do, at the playhead, onto the selected audio track or the track of
  the selected clip, else onto new tracks right after the selection
  (`document::import_place`, which a track's menu reaches by selecting
  that track). Tests never open a real dialog: `Launch::for_test` answers
  them from `testing::answer_next_dialog`, or as cancelled.
- **The arrows follow the selection (H2, the owner's choice
  ).** With a clip selected, ←/→ select the clip before or
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
  it (slice 4 places it at the bottom). `Project::pan_law`
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
  order, and returns or the master inside a group (`can_be_grouped`). Edits set parents and a rough order, then `normalize` puts
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
  Audio settings) turns the S buttons into headphone buttons (click and Ctrl+click), and its unfolded row gains the cue level. What is
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
  0 dB send adds +6 dB (the owner chose this behavior).
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
| GPL-3.0-or-later | Yes | Yes | The GPL family is used this way by other open-source applications. Enables the ASIO GPLv3 route. Conflicts with Mac App Store terms. |
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
    second bus. The owner chose three taps: **Pre FX** (after
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

### Amendment (2026-10-10, M4 review finding M3)
Every new plan copied all of the old plan's delay lines on the audio
thread, sample by sample, with a modulo per sample. A new plan arrives
with each step of a fader drag, so the cost grew with the total
compensation. A project heavy with lookahead plugins (M6) could have
dropped out while editing. Now, when the old plan will not play again (no
crossfade, so every plan of the same shape) and both lay out their lines
alike, the new plan swaps buffers with it (`Lines::take_over`). Lines that
change length are still copied, during the 5 ms crossfade of a new shape.

Measured by the new gate scenario `engine_latent_swaps_64` (the
compensated mix with a new plan every block): 1.777 G instructions before
the fix, 1.565 G after. A steady block costs 1.327 G per 1,000. What a
swap still costs is mostly the 20 ms level glides it starts, which is the
work a fader move asks for. Baseline updated for the new scenario; the
others moved by under 0.2%. Evidence: unit test
`a_plan_of_the_same_lines_takes_them_over_without_a_copy`; the
allocation-guard test `delay_compensation_never_touches_the_heap` swaps
plans of the same shape (its variants 0 and 6).

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
- **The pan law for stereo: the owner's B (2026-10-10).** Only mono
  material pans by the project's law: a track whose clips are all mono
  files (`routing::plays_mono`). Every other track (stereo, mixed, MIDI,
  empty) balances, so stereo keeps its level at the centre, as groups,
  returns and the master already did. Nothing saved changes: projects
  from before play their stereo tracks 3 dB louder. The pan law's menu
  caption reads "Pan law (mono tracks)" and its commands say so. The EBU
  reference tone now reads −23.0 LUFS and −23.0 dBTP through the app.
  Tests: `routing` (mono, stereo, mixed and empty tracks), E2E
  `a_mono_track_follows_the_pan_law_and_a_stereo_one_keeps_its_level`,
  `the_loudness_column_reads_the_reference_tone_at_minus_23`; the level
  expectations of the cue, mixing, groups, meters, fades and walkthrough
  scenarios updated for stereo at unity.
- **Ctrl+C, Ctrl+X and Ctrl+V did nothing in the real window.** egui-winit
  turns the clipboard chords into `Copy`, `Cut` and `Paste` events and
  drops the key press; Ctrl+V sends nothing at all when the system
  clipboard holds no text, and on Windows Shift+Delete chords become Cut,
  Ctrl+Shift+Delete (Delete time) included. Only releases arrive as keys.
  The E2E tests pressed keys the window never sends, so they passed.
  `app/clipboard_keys.rs` puts the presses back before anything reads
  the keys, outside text fields (which keep the events): Copy as C, Paste
  as V, Cut as X; a Cut with Shift waits for its key's release; a V
  released with no Paste seen is pressed then. `Lumen::key` now sends
  these chords as the window does (without the fix, the three clipboard
  scenarios fail).
- **A paste right after a click landed at the old playhead** (one run in
  four of the new test): the playhead is what the audio thread last
  reported. `Transport::position` counts a locate the engine has not
  played a block since; edits at the playhead use it.
- **"The meters clip around +1.1 dB" after the pan law change.** No gain
  is added: a full-scale 48 kHz file reads 0.0 dB on its track and the
  master. Files at another rate are converted to the device's on import,
  and the new samples land between the old ones, where a loud master's
  waveform goes over full scale (its true peak, commonly +0.5 to +1.5
  dBTP); MP3 and Ogg decode to floating point and keep their overs too.
  At −3 dB those overs were hidden; at unity the meter shows them. A 44.1 kHz tone with samples at 0 dBFS and its waveform
  at +3 dB reads +3.0 dB (E2E
  `full_scale_reads_zero_and_a_converted_file_shows_its_true_peak`).

---

## ADR-0043 — The light layer and the mark

**Status:** Accepted 2026-10-10.

### Context
The owner asked to cement the design system and to give Lumen a logo that
indulges the theme of light, in the interface too. Options were shown on
an identity canvas (a private Design artifact): four marks (Aperture,
Beam, Corner, Dawn), six colourways of Dawn, the light rules and levels,
the window with light applied, the tokens and the component library. The
owner chose Dawn in Sunrise and approved the light layer, the mark as a
meter and every proposed use of light. Lumen already glowed in places
(the S3 selection, edges, fade curves, the comet knobs, the armed ring),
each with its own values.

### Decision
- **Five levels of light** in `light.rs` (DESIGN.md §6a): Unlit; Ember
  (`EMBER_WASH`, a hover wash); `LIT` (the S3 glow: 1.5 pt line, halo
  1.5 pt at 28% and 3 pt at 12% past it); `BEAM` (2 pt accent line with a
  1 pt `core`, halo 3 pt at 22%, casting 56 pt at 9%); `FLARE` (record
  red, halo 2.5 pt at 45% and 6 pt at 18%). Helpers draw them along lines,
  rects, curves and as cast light. On the light theme halos are halved and
  the core is the accent. The existing glows (`draw_glow_rect`, the edge
  glows, fade curves, the clip-half wash) now use them; their look moved
  slightly to the shared values (edge halos 1.5 pt wider).
- **New uses:** the playhead while playing is a beam with a `core` cap that
  casts light on the lanes (stopped, it is unlit as before); Play while
  playing gets the beam's halo and a `core` icon; every RMS bar has a
  `BEAM` head in its zone's colour; clip lights flare.
- **The mark** (`widgets/mark.rs`, DESIGN.md §6b): Dawn on a 64 grid,
  painted with egui shapes (the sun's gradient as six discs). In the
  transport bar it is a meter of the master's RMS (−48 to 0 dB), resting
  with the sun peeking over the horizon; recording will turn it red in
  M9. Above an empty arrangement it is half risen, and fully risen while
  addable files are dragged over.
- **New tokens:** `core`, `sun` and `sun_core` in both palettes.
- **Window icon** (`brand.rs`): the mark on a dark rounded square, drawn
  as 128 px RGBA from the same geometry with 4×4 samples per pixel, so no
  image files or decoders are added. The installer's app and file icons
  stay in M9 (owner, 2026-10-07) and will use this drawing.

### Consequences
- Every new glow picks a level; a one-off glow is a design-review finding.
- The mark draws a frame only when the meters do, so idle stays idle.
- The cast is two gradient quads per frame while playing: no measurable
  frame cost.
- Tests: unit tests for the levels, the mark's rise and the icon's pixels;
  E2E `light::the_mark_rises_with_the_master_and_settles_when_stopped`;
  `shots::light_layer` renders the empty, playing and mixer states in both
  themes. MT-APP-18 is for the owner.

## ADR-0044 — MIDI devices and clip events

**Status:** Accepted 2026-10-10 (M5 slice 0).

### Context
M5 needs MIDI in and out, and the model has to hold more than notes
(controllers, pitch bend, pressure, program changes) before the piano
roll and recording are built on it.

### Decision
- **`lumen-midi`**, a stand-alone crate (like `lumen-audio`), on **midir**
  (MIT; WinMM on Windows, CoreMIDI, ALSA). A `Backend` trait with the real
  midir backend and a scriptable fake. `MidiSystem` runs a worker thread:
  it polls the driver every 500 ms, reports ports added and removed,
  opens an input when either of its switches (Track, Remote) is on and an
  output when Track is, drops connections for ports that vanish, and opens
  them again on replug. A port that will not open is marked failed and
  tried again only when the user switches it or the device is replugged,
  so a busy port is not retried in a loop. The UI sees a snapshot and an
  event channel and never waits on a driver (risk R5).
- Messages from every open input go into one queue (`rtrb`) as `Timed`
  values: the input's slot, the driver's microsecond stamp, up to three
  bytes. System exclusive is not carried yet.
- **Settings:** `midi.ports` in the settings file, found by direction and
  name, remembered while unplugged; an added field, no settings version
  change. The window is now "Audio and MIDI settings" with two tabs.
- **Format v11:** `ClipContent::Midi` gains `events`, a list of
  `ClipEvent { at, kind }` with kinds Controller, PitchBend, Pressure and
  Program (with an optional bank), written only when present. The
  migration changes nothing in a file; a frozen golden fixture holds all
  four kinds. Splits, trims, loop-extends and slips cut events as they cut
  notes. Validation refuses out-of-range values.

### Consequences
- Old projects open unchanged; notes keep their compact form.
- The device layer has ten unit tests on the fake, and E2E drives the
  settings page through it (hot-plug, switches, saved state, a busy port).
- Not built: virtual ports (WinMM cannot create them; a loopback driver
  such as loopMIDI is the Windows answer), system exclusive, and the
  engine side of MIDI (slice 1).
- midir is MIT, so no licensing decision is needed.


## ADR-0045 — Hearing MIDI: the event path, the synth and MIDI To (M5 slice 1, format v12)

**Status:** Accepted 2026-10-10 (M5 slice 1). Owner choices S1, E1, T1, H1
are in the owner-decisions table ("Hearing MIDI").

### Context
Until now a MIDI track could be added and held no sound. Slice 1 makes MIDI
audible, with nothing hanging, before recording and the piano roll are built
on it: MIDI events through the mixer graph with sample offsets, a built-in
instrument (VST3 arrives in M6), and a way to send a track to hardware.

### Decision
- **The message.** `lumen_core::midi::Msg`, three bytes, `Copy`, with the
  standard decoding (a note-on with velocity 0 is a note-off) and `Event`
  (a message at a frame in a block). Both sides share it, so the engine
  stays on core.
- **The event path** (`lumen-engine/src/midi/`). MIDI clips reach the engine
  as their own track list (`MidiTrack`: clips with notes and controller
  messages in ticks, loop period, target, instrument, a tracker), built on
  the UI side by the pure, tested `lumen-app/src/midi_play.rs`, swapped
  whole through a queue, old lists returned to be freed (as clip lists
  are). Clips that did not change keep their note lists (an `Arc` per clip,
  so the engine's comparison is a pointer check). `MidiPlayer` is a
  `TrackSource` played beside the audio clips (`Layered`): each block it
  works out which events fall in the block from ticks through the timeline
  (an event belongs to the block holding `sample_at(tick)`; the window of
  ticks is widened by two ticks so rounding cannot lose one), sorts them
  (note-offs, controllers, note-ons at one frame) and hands them to the
  instrument, whose audio is added to the track's bus. Volume, pan, sends,
  groups, solo, cue, meters and delay compensation therefore apply as they
  do to audio. Tempo changes move notes with the music because positions are
  recomputed from ticks every block.
- **Notes never hang** (risk R1). One tracker per track
  (`midi/hold.rs`) holds every sounding note with where it came from. Stop
  and pause, a jump, a loop wrap, an edit that removes or moves a sounding
  note, a clip mute, a change of instrument or output, a track leaving the
  list and Panic all release exactly what sounds. **Chase** (H1): Play and
  every jump start the notes already sounding at the new position and
  apply the latest controllers, pitch bend, pressure and program before it
  (a clip that has ended still leaves its controllers). The pedal and the
  bend are reset on release. After an edit the held notes are brought in
  line with the new data (`sync`) without retriggering what is unchanged.
  `Source` gains `stopped`, `panic` and `ringing` (default no-ops);
  `RenderContext` gains `mode` (Play, Scrub, Ring) and `stream`.
  While stopped and `ringing()` the engine renders with `Mode::Ring`, which
  only instruments answer: a release tail rings out instead of being cut,
  and note-offs reach their port. Scrubbing plays no MIDI.
- **Instruments.** `lumen-devices` (new crate: real-time-safe DSP on
  `lumen-core` only, grown by M8) holds the `Instrument` trait and the
  **synth**: 16 voices, oldest stolen with a 2 ms fade, two polyBLEP
  oscillators, a 12 dB state-variable low-pass, ADSR, sample-accurate
  sub-blocks between events, pitch bend of two semitones, CC 1 and channel
  pressure opening the filter by up to an octave and a half, sustain pedal,
  CC 120/121/123, program changes 0 to 4 for the presets. The five presets
  (E1) are Poly (S1, the default), Keys, Bass, Pluck and Pad. A note keeps
  the sound it began with when the preset changes. Voices carry across list
  swaps by track id (the instrument box and the tracker swap pointers), so
  an edit elsewhere never cuts a sounding note.
- **Layering amended.** `lumen-engine` depends on `lumen-core`, `lumen-rt`
  and `lumen-devices`; `lumen-devices` depends on `lumen-core` alone. The
  rule in the architecture skill and ARCHITECTURE.md says so.
- **Format v12.** The instrument is a built-in device on the track:
  `{"format": "builtin", "name": "synth"}` (the stable id; never the product
  name) with its preset's stable name as the device's state (`"poly"`,
  `"keys"`, `"bass"`, `"pluck"`, `"pad"`). A track may name a MIDI output:
  `midi_to: {port, channel}` (channel 0 to 15), written only when set and
  remembered while the port is unplugged. The migration changes nothing in
  a file (the fields are optional); a frozen golden fixture holds a synth
  and an output. Validation refuses a synth or output on a track that is not
  MIDI, a bad channel, an empty port name or an unreadable preset;
  `history::unchanged` compares `midi_to`. New MIDI tracks get the synth on
  Poly; MIDI tracks in older projects have none and stay silent until one
  is chosen (they could hold no notes before v11). Duplicating a track gives
  the copy its own device id.
- **Edits** (one undo step each): `Document::set_instrument(ids, preset)`
  ("Instrument: Pad", "Remove instrument") and `set_midi_to(ids, output)`
  ("MIDI To: Synth 3", "MIDI To: Instrument"). The track menu has
  **Instrument** (the five presets and None) and **MIDI To** (Instrument,
  then each output with Channel 1 to 16), built from `widgets::submenu` and
  `menu_toggle`. Palette commands: Instrument: Poly, Keys, Bass, Pluck, Pad;
  MIDI To: Instrument; Panic (no default key).
- **MIDI To** (T1: the instrument or a port, one or the other). Events for
  a port track leave the engine on a lock-free queue (`MidiOut::At`), each
  stamped with the **device frame at which it is heard**: the block's frame
  count, the frame within it and the delay compensation's read ahead.
  `lumen-app/src/midi_out.rs` turns frames into time on its own thread. The
  audio callback publishes `(frame, time)` each block (a sequence-numbered
  atomic record; a clock read and atomics, nothing else); callbacks only run
  late, so the earliest `time - frame/rate` over the last 256 blocks is the
  time of frame 0, and the device's reported output latency is added so
  hardware sounds with the audio of the same frame. A `Scheduler` holds
  messages in order and the notes it has sent on; the engine's `Flush`
  (stop, panic) drops what has not gone and lets go of what has, and a
  note-off for a note that is not sounding is not sent a second time (found
  by E2E). A port that is not open drops its messages and is forgotten. The
  sender thread starts with the first track that names a port. If the audio
  stream stands still for a second (the device was lost, Audio was switched
  off) it lets go of every note it sent on. Outputs that
  tracks name are switched on for tracks automatically and remembered in
  the settings (the user would otherwise hear nothing and see no reason).
  Port numbers are an append-only table of names, so a number keeps its port
  for the whole run.
- **Defaults taken** (standard behaviour): stop, a jump, a loop wrap, a clip
  mute, an edit that removes or moves a sounding note, a change of
  instrument or MIDI To and Panic release notes (the synth's tail rings
  out; Panic cuts it, and sends All Notes Off and All Sound Off, CC 123 and
  120, on every channel of every port in use, from the engine and again
  from the app in case the engine is not running). Track mute and solo act
  on the instrument's audio (the mixer): the synth keeps
  receiving notes, so unmuting mid-note is heard; a MIDI To track that is
  muted or silenced by solo sends nothing and lets go of what it sounded.
  Choosing an instrument on a track that plays to an output switches it
  back to the instrument (one undo step). The synth listens on every
  channel. Bank selects pass through to ports and are ignored by the synth.
  Limits per track: 128 held notes and 1,024 queued events per block, past
  which new ones are dropped and counted (`MidiPlayer::dropped`, zero in
  every test).

### Consequences
- Engine tests (`tests/midi.rs`, 14): note starts and ends to the sample at
  five tempo and rate pairs (20 to 999 BPM, 44.1 to 96 kHz, blocks of 64 to
  480); across a tempo change; loop wraps; stop; jump with chase; chase of
  controllers; edits; target changes; hardware stamps; panic; a randomised
  run of 400 commands in which every note-on has its off; the synth through
  the mixer from its exact sample; the release tail after stop; an edit
  elsewhere not cutting a sounding note. The allocation guard
  (`tests/realtime.rs`) runs 32 tracks with loops, jumps, stops, list swaps
  and Panic with zero heap operations, and RTSan passes the same test in
  WSL. Synth tests (10): exact frames, independence from how blocks are cut
  (bit-identical), envelopes, pedal, bend, stealing, presets, a random storm
  that stays finite. 11 E2E scenarios (`tests/e2e/midi_play.rs`) cover what
  is heard, the menus, the hardware port and each way a note could hang.
- **Cost.** `engine_synth_64` (64 MIDI tracks each playing a four-note chord
  that repeats every beat and rings past it, about 256 voices, through the
  64-track routed mix) runs a 128-frame block in about 0.36 ms in a release
  build on the development machine, 13.5 % of the 2.67 ms a block lasts at
  48 kHz; the first version took 0.86 ms before the pan gains were computed
  once per note. Its baseline in `perf/baseline.json` is 8,249,464,908
  instructions per 1,000 blocks (a new scenario, so no earlier number to
  regress from, added by hand rather than with `--update`, which would also
  have moved the other baselines by their under-1 % drift); the existing
  scenarios moved by under 1 %.
- **Timing.** The sender's scheduling is tested with injected times and
  against the fake backend: messages keep order and spacing, and arrive
  about 1.5 ms after plan at worst (asserted under 4 ms; the plan's goal is
  2 ms). Real hardware timing has not been measured: no MIDI interface is on
  the test machine (MT-MIDI-2 asks the owner to listen to Windows' GS
  Wavetable Synth).
- **Not built** (later slices or other milestones): MIDI From, monitoring
  and thru and the computer keyboard (slice 2); notes drawn and edited
  (slice 3), so the demo project is generated by code; knobs on the synth
  (the device chain, M6/M8); MIDI clock out (slice 7).
- A visual need was not met by existing widgets and is not built: the MIDI
  To menu lists sixteen channels per output as rows, which is long; a
  compact grid would be nicer and needs a mock-up first. A track whose
  output is unplugged shows nothing on screen, only "(not connected)" in its
  menu.


## ADR-0046 — Playing live: MIDI From, monitoring, the computer keyboard and the MIDI chip (M5 slice 2, format v13)

**Status:** Accepted 2026-10-11 (M5 slice 2). Owner choices L A, J A, C B,
K A, H B, R A and Ctrl+Shift+K are in the owner-decisions table ("Playing
live"), chosen from the "MIDI: Playing Live" design canvas.

### Context
Slice 1 made MIDI clips audible. A MIDI track also has to play what a
player plays now, from a controller or the computer keyboard, before
recording (slice 6) can keep it. The owner asked for a MIDI activity chip
with a quick way to switch ports.

### Decision
- **Live input in the engine** (`lumen-engine/src/midi/live.rs`). The app
  hands the MIDI player a `LiveInput` (through a queue, the old one handed
  back to be freed); each part, the player pulls the messages to be heard
  in it, stamped with the device frame (`RenderContext::device`, the
  callback's frame count without the read ahead). A track plays what its
  `Listen` takes: a bit per source (an input's slot, 0 to 62, and the
  computer keyboard, 63) and an optional channel. Live messages go the way
  clip events go: into the instrument, or out to the MIDI To port
  rechannelled to the track's channel. Live notes have their own tracker,
  apart from the clips', so a key held while a clip plays the same note
  ends only its own note. A track that stops listening, listens to
  something else, changes instrument or port, or leaves the list lets go
  of its live notes, pedal and bend; Panic forgets them; **stop and jumps
  leave them alone** (a held key keeps sounding). A note-off for a note
  the track is not holding is dropped. Live input waiting makes the engine
  render while stopped (`ringing`). While scrubbing, live input waits for
  the scrub to end.
- **Timing (J A, "steady").** `lumen-app/src/live.rs`'s `LiveFeed` runs on
  the audio thread. A message's play time is its driver stamp plus the
  input's offset to the clock (the smallest `arrived - stamp` lately seen,
  creeping up by 1/256 of a later gap so a slow driver clock is followed;
  a stamp more than 20 ms off starts the offset again, so drivers with
  useless stamps fall back to arrival time). Frame 0's time is the
  earliest `time - frame` over the last 256 callbacks (`StreamClock`, now
  with the block length). A message is heard one block after it was
  played, never before the part being rendered and never more than two
  blocks after the block that takes it. A note-on more than 100 ms old
  (the audio stood still) is dropped; its note-off still goes. The
  computer keyboard is stamped with the clock when the UI reads the key.
  Recording (slice 6) will use the same times.
- **The model, format v13.** `Track::midi_from` (`All` inputs switched on
  for tracks and the computer keyboard, `Keyboard`, or a `Port` by name,
  remembered while unplugged; and a channel or all) and `Track::monitor`
  (Auto, In, Off; stored on every track kind, used by audio tracks from
  M9). Both are written only when not the default. No-op migration, golden
  fixture `v13`, validation, `history::unchanged`.
  `Document::set_midi_from` and `set_monitor`, one undo step each.
- **Who listens (L A, selection-based auto-arm).** Pure and tested in
  `midi_play::listening`: Monitor In always; Auto when armed **or
  selected**; Off never; a MIDI To track that is muted or silenced by solo
  never. The arm of a track that listens only because it is selected shows
  a hollow record-red ring. The engine list is resent when the selection,
  arms, monitors, MIDI From or the ports change.
- **Where it is set (R A).** The track menu's MIDI From (All inputs,
  Computer keyboard, each input with its channels, ports not connected
  dimmed) and a Monitor row (In · Auto · Off); palette commands; From and
  To rows on MIDI tracks' mixer strips.
- **The computer keyboard (K A).** A command (Ctrl+Shift+K, area MIDI)
  toggles a mode that is not saved. While on, A to K, W E T Y U, O and L
  play notes, Z X move the octave (C-2 to C8, from C3), C V step the
  velocity (1, 20 … 120, 127, from 100); repeats are ignored; each key ends
  the note it started. Only plain letters are taken, and not while a text
  field, a rebind or a window has the keys. Esc, turning it off, Panic and
  the window losing focus let go of what it holds. The transport has a lit
  keycap button and, where there is room, a readout (`C3–C4 · vel 100`);
  in the narrowest window the readout leads the info line instead.
- **The MIDI chip (C B).** A DIN glyph with in and out lights in the
  transport bar's right cluster. Activity is counted with atomics where it
  happens: per input slot in the driver callback (channel messages only,
  so clock and active sensing never flash), per output in the device
  layer's worker when a send succeeds, and in `LiveKeys`. The UI flashes a
  light when a count changes (`light::flash`, the BEAM level, fading over
  `motion::NOTE`). Its popover lists every port with its own light and
  Track and Remote pills that act through the settings page's own path,
  unplugged ports dimmed with "not connected", and "MIDI settings…".
- **Header lights (H B).** A light right after the name of each MIDI track
  that listens flashes (BEAM, `light::flash`) when any source its MIDI From
  takes shows traffic. A listening track keeps a fixed 9 pt for it, so a
  name never shifts with a flash; tracks that do not listen keep no room
  and never light. The channel filter is ignored for the light.
- **No polling for lights.** The driver callback wakes the UI
  (`MidiSystem::start_with_waker`, `ctx.request_repaint`) after counting a
  channel message; lights repaint only while they fade, so idle stays idle
  with inputs switched on (E2E `live_lights`).
- **The transport bar fits 960 × 540** (`transport/fit.rs`): the bar
  measures its essentials each frame and optional pieces take what is left
  in a set order, first to go first: the device text, the keyboard readout
  (to the info line), the CPU readout, the compensation readout, the
  dropout chip (the dot turns amber), then a compact layout (4 pt spacing,
  the tempo without "BPM", no divider before the dot). The position field,
  the audio dot and the MIDI chip always show (E2E `barfit`). The compact
  layout was not mocked up first; MT-MIDI-3 step 10 asks the owner to
  judge it.

### Consequences
- Engine tests (`tests/live.rs`, 9) and the allocation guard over live
  input with list swaps and a feed replaced mid-run; feed tests for the
  steady spacing under late callbacks, the bounds, stale notes and bad
  stamps; E2E for listening, MIDI From, Monitor, the keyboard, the chip.
- Built by Sonnet agents in separate worktrees (2b, 2c and 2d in
  parallel, then 2e, then two fix-ups), each reviewed and merged with the
  full suite run before every push.
- Perf gate: slice 2b adds +0.70% to `project_parse` (two optional fields;
  callgrind shows the cost in serde_json's map building, not in Lumen's
  code), under the gate. Measured in an Ubuntu 24.04 sandbox, every
  scenario reads high against the baseline recorded on 26.04 (+5.3% for
  `project_parse` at the baseline's own commit, from glibc's allocator), so
  the CI container is the authority; the baseline was not changed.
- Unverified: timing with a real controller (MT-MIDI-3).

## ADR-0047 — MIDI clips and the piano roll (M5 slice 3, format v14)

**Status:** Accepted 2026-10-11 (M5 slice 3). Owner choices Look A, Draw A
and Live A are in the owner-decisions table ("Piano roll"), chosen from
live specimens. Built in four parts (3a to 3d) by Sonnet agents in
worktrees, merged and checked by the integrator.

### Context
Slices 1 and 2 made MIDI audible and playable, but notes could only come
from the demo project. The requirements ask for a piano roll that draws,
selects, moves, resizes, splits, glues, mutes, duplicates, nudges,
transposes and makes notes legato, with audition and zoom synchronized
with the arrangement; the quality bar asks for 100,000 notes at 60 fps or
better. C2 (M5 plan) put the editor in the detail panel's Clip tab with a
way to pop it out into its own window.

### Decision
- **Note mute, format v14.** `Note(start, length, pitch, velocity, muted)`;
  `Note::new` makes an unmuted note. An unmuted note saves as before
  (`[start, length, pitch, velocity]`), a muted one adds `true`; loading
  takes both. The v13 to v14 migration changes no data. Muted notes stay in
  the model and are skipped where clips become engine events
  (`midi_play.rs`).
- **Note edits are pure transforms** in `lumen_project::edit::notes` (add,
  delete, move, copy, nudge, transpose, resize either end, split, glue,
  mute, duplicate, legato) on a clip's notes and a selection of indices;
  each returns the indices of the notes it leaves picked. Groups are
  clamped as a whole (start ≥ 0, end ≤ the content's length, pitch 0–127).
  Notes stay sorted by start, then pitch. Controller and other events are
  never touched by note edits. Legato only lengthens (to the next onset of
  any pitch, or the content's end), so overlaps stay. Glue joins picked
  notes of one pitch that touch or overlap, with the first note's
  velocity. Positions: `content_length`, `to_clip_time`, `to_arrangement`
  (`edit/note_time.rs`); MIDI clips have no hidden offset, since trim and
  slip move the notes themselves.
- **Selection by index, checked by pointer.** `NoteSelection` holds the
  clip's `Arc` with the indices. `refresh` keeps them while the clip is the
  same or its notes are equal (a rename or recolour), and clears them
  otherwise (undo, redo, the clip gone).
- **Edits through `Document`** (`document/notes.rs`, `midi_clips.rs`), each
  one undo step, refused on locked, missing or audio clips.
  `create_midi_clip(track, NewClip::Range | Bar)` follows the clips'
  overlap rule and refuses to cut a locked clip.
- **Notes on the GPU** (`piano_roll/`). The roll reuses `lumen-render`'s
  instanced boxes with its own layers and drawing targets per surface
  (`Surface::Docked`, `Surface::Window`), so the docked editor and a
  pop-out never rebuild each other. Instances are rebuilt only when the
  clip, the picks or the colour change; pan and zoom move the view, and a
  new row height uploads a 128-slot row table. Boxes are in absolute
  quarter notes, sharing the arrangement's `TimeView` exactly. Loop repeats
  play as the engine plays them (notes that start inside a repeat, cut at
  its end) and are drawn at half strength, never pickable. Out-of-view
  boxes collapse in the vertex shader rather than being culled in the
  builder, which would rebuild on every scroll. New tokens: `key_white`,
  `key_black`, `key_text`, `lane_black`, `lane_split`.
- **Mini notes** on MIDI clips in the arrangement: a sibling box layer
  drawn after the clip bodies, rebuilt with them. The renderer places
  boxes by row slot, so each MIDI track gets 32 thin bands and a clip's
  pitch range maps onto them (centred under 16 semitones): notes read as
  lines, not semitone-high boxes.
- **The clip editor** (`note_editor/`): the state (clip, tool, picks,
  gesture, pitch scroll and row height, the box cache), `gesture.rs` (pure
  arithmetic for move, copy, resize, marquee and draw), `paint.rs` (the egui
  overlay over the GPU roll: dimming outside the clip, hover handles, the
  moved notes' preview, marquee, lit key, playhead, ruler), `ui.rs` (input),
  `ops.rs` (note commands and the audition hook), `window.rs` (the pop-out
  viewport). `app/detail.rs` holds the detail tabs, out of `lib.rs`.
  **Live A:** a gesture previews over the committed notes and changes the
  document once, on release, as one undo step; Esc cancels with no change;
  the GPU boxes are not rebuilt during a drag. **Draw A:** with Draw (B) a
  click adds one grid step at the cell start, dragging sets the end, a
  click on a note removes it. With the pointer: click picks without moving
  the playhead, Shift adds, Ctrl-drag copies, the ends resize, a box picks,
  Alt ignores the grid; the ruler moves the playhead. Double-clicking a
  MIDI clip opens it; double-clicking an empty MIDI lane makes a snapped
  one-bar clip and opens it.
- **Commands** (area MIDI): Create MIDI clip (Ctrl+Shift+M; the time range,
  else a bar at the playhead on the selected MIDI track), Edit MIDI clip
  (Enter), Pop out clip editor (Ctrl+Shift+E), Draw tool (B), Split notes
  (Ctrl+E), Glue notes (Ctrl+J), Legato (L), nudge finely (Alt+Left/Right),
  and palette-only transpose and nudge. **Keys by context:** while the
  editor holds the keyboard (the pointer over it, the Detail pane focused,
  or the pop-out focused), Delete, Ctrl+D, 0, Ctrl+A, Esc, Up/Down,
  Shift+Up/Down and Left/Right act on notes (delete, duplicate, mute, pick
  all, cancel then unpick, transpose a semitone or an octave, nudge a grid
  step); elsewhere they keep their meaning. No default chord clashes
  (`no_two_default_shortcuts_clash`).
- **Auditioning is a live source** (`AUDITION = SOURCES - 2` in
  `lumen-engine/src/midi/live.rs`). `LiveIn` carries an optional track; an
  audition message sounds only on that track, whether or not it listens,
  through its instrument or MIDI To port. The source has its own tracker,
  so a preview and a clip note or a held key on the same pitch each let go
  of their own. `audition.rs` plays one preview at a time for 250 ms, and
  lets go when the gesture ends or is cancelled, the editor changes clip
  or closes, the transport stops, the window loses focus, the setting is
  turned off, or Panic (which forgets it). The setting is
  `settings.midi.audition`, on by default (an added field). The editor
  passes the clip to the hook: its state is lifted out of `App` during a
  gesture, so reading the open clip from `App` found none (caught by E2E
  `pressing_a_note_auditions_it_and_escape_lets_go`).
- **Two hanging-note fixes found on the way.** In the engine, events on one
  frame sort note-offs first, so a note played and released on the same
  frame stayed on; the off now lands a frame after its on
  (`apart_from_its_on`). In `midi_out.rs`, the scheduler kept one
  "sounding" flag per key, so a clip note and a preview on the same key
  sent two ons and one off; it now counts ons per key and answers each.
- **Benchmark:** `--stress` adds a 201st track, "Dense", with one 400-beat
  clip of 100,000 notes over six octaves; `--bench-editor` opens it in the
  clip editor for the run, its pitches scrolled with the tracks.

### Consequences
- Every edit to notes is one named, undoable `Document` method; the editor
  never edits in a closure and never changes the document per frame.
- The GPU roll costs nothing per frame beyond the view while panning and
  zooming; a gesture costs at most one rebuild at its start and end.
- Index-based picks are simple and fast, but any change to a clip's notes
  from elsewhere clears them (deliberate: stale indices would pick the
  wrong notes).
- Hit-testing is a linear scan of the clip's notes per pointer event,
  about 0.1 ms at 100,000 notes.
- Not built (KNOWN_ISSUES): the pop-out's size and place are not saved; no
  auto-scroll at the editor's edges while dragging; notes can be created
  only with the mouse (every other note edit is keyboard-reachable); no UI
  for the audition setting yet; a preview on a muted hardware track still
  sends.
- Measured (release, stress project plus the 100,000-note track, p99):
  with the dense clip open in the editor 1.20 to 1.82 ms at 1600 × 900 and
  2.10 to 2.22 ms at 3440 × 1380, inside the 120 fps target; idle 3
  repaints in 3 s. Details in the slice 3 row of PROGRESS.md.
- Unverified: the real second window and the pop-out's DPI and
  multi-monitor behaviour, and previews on real hardware (MT-MIDI-4).

### PR 14 follow-up (2026-10-14): computer keyboard in the native editor

The native clip editor now calls `computer_keyboard_keys` before
`command_keys`, matching the main window. Previously its own key events
could run edits such as Legato instead of playing MIDI notes. The fix is
one dispatch call; the embedded editor already uses the main window's
handler. Regression `notes::the_native_editor_plays_computer_keyboard_notes_before_editing_shortcuts`
drives a separate immediate viewport callback with its own input: L sends
note-on/off without editing while MIDI mode is on, then runs Legato with
the mode off. It fails without the fix and passes with it. Native OS focus
and physical playback remain a manual check in MT-MIDI-9 step 5.
Verification on Windows: formatting, workspace clippy with `-D warnings`,
all 1,454 workspace tests (335 E2E), and all six real-binary smoke tests
pass. No visual, persisted-format or audio-engine changes.

### Amendment (2026-10-11): previews on hardware tracks

Owner decision. On a hardware track (a MIDI output), a piano-roll preview
is skipped for a key the track's own clip is sounding now. A port sees
only plain MIDI, so the preview's off would end the clip's note too. The
app decides "sounding" from the project, with the engine's rules
(`midi_play::sounding_keys` after `hold.rs`): a note sounds from its start
to its end within its repeat of a looping clip; muted notes and clips
sound nothing; a stopped transport and a silenced output (a mute, or
another track's solo) count as not sounding. The skipped tone gets no
note-on and no off; the rest of a chord plays. Built-in synth tracks keep
their previews on channel 15 unchanged. Known limits are in KNOWN_ISSUES:
a clip note that starts during a preview of its key is not handled, and
the playhead the check reads can differ from the port's timing by the
output latency.

### Amendment (2026-10-12): double-click draws with the pointer

Owner bug report: a double-click on empty space in the roll, with the
Pointer tool, should draw a note. A second press on empty space, within
egui's double-click delay and click distance of the first
(`input_options.max_double_click_delay` and `max_click_dist`; the first press
is remembered in `NoteEditor::last_empty_press`), starts the Draw tool's
press: the same grid cell, length by dragging, audition and one undo step
(`start_draw`). The note is picked on release. Shift keeps the marquee. A
press on a note, with the Draw tool, or any drag forgets the first press, so
a double-click on a note keeps its meaning and two slow or distant clicks add
nothing. The info line lists "double-click: add a note" after the marquee.

## ADR-0048 — The clip editor's own zoom and scroll (M5 slice 3 amendment)

**Status:** Accepted 2026-10-11, owner decision. It amends the M5 slice 3
plan (`docs/milestones/M5.md`, which kept zoom synchronized with the
arrangement) and ADR-0047's "zoom synchronized with the arrangement".

### Context
ADR-0047 gave the piano roll the arrangement's `TimeView`, so zooming or
scrolling the arrangement moved the roll and the reverse. The owner asked
for the roll to zoom and scroll on its own: a clip is worked on at its own
scale, without the arrangement's zoom deciding it.

### Decision
- **Its own view.** `NoteEditor::time` is a `TimeView` the roll owns. It
  uses the same beats as the arrangement, so a clip sits at the same place
  in both views. Drawing, hit testing, the ruler, the notes and the playhead
  use it. The editor never writes the arrangement's view, and the arrangement
  never writes the roll's.
- **Zoom and scroll.** Ctrl+wheel over the roll zooms it, anchored on the
  pointer (`TimeView::zoomed`). Shift+wheel or a sideways swipe scrolls it in
  time (`TimeView::scrolled`; egui turns Shift into a horizontal scroll).
  Both take their limits from the roll's own view (`App::zoom_limits_at`, the
  tempo where the roll is). Wheel and keys in the roll no longer pause the
  arrangement's Follow.
- **The zoom keys.** While the editor has the keyboard (the same condition that
  makes the arrows its notes' keys), + and - zoom the roll, anchored on the
  pointer over the roll, else on the playhead when it is in view, else on the
  middle of the roll. Otherwise they zoom the arrangement as before. This keeps
  roll zoom reachable from the keyboard. W, Z and X stay with the arrangement.
- **Fit on open.** The first draw of a clip fits its span to the roll's width
  with the small margin of `TimeView::fit` (`view::fit_clip`) and centres its
  middle pitch. Opening the same clip again keeps the view; a different clip is
  framed again. The view is not remembered per clip (the simplest correct
  behaviour the owner allowed).
- **Grid and snap.** Grid settings (width, fixed size, triplet, dotted, snap)
  stay shared. The roll snaps, and names its grid in the toolbar and the menu,
  at its own zoom, so the grid it snaps to is the grid it draws.
- **Benchmark.** `--bench-editor` drives the roll to the arrangement's driven
  zoom and scroll, so the run still measures the roll zoomed and scrolled.

### Consequences
- Two views over one set of beats: no note is converted through the
  arrangement's zoom, and a clip's notes stay where the roll put them.
- Keyboard-only users keep roll zoom through + and -.
- Not built: a fit-to-clip key for the roll (W, Z and X fit the arrangement),
  and Follow-playhead in the roll (not required by the owner's decision).
- Tests: `view::fit_clip` unit tests; E2E `notes::opening_a_clip_frames_it_in_the_roll`
  and `notes::the_roll_zooms_and_scrolls_on_its_own` (the arrangement's zoom
  leaves the roll's note positions alone, and the roll's zoom leaves the
  arrangement alone).
- Unverified: the feel of Ctrl+wheel and Shift+wheel on a real mouse and
  trackpad (MT-MIDI-4 to be re-run).

## ADR-0049 — Time selection in empty lanes (M5 amendment)

**Status:** Accepted 2026-10-11, from the owner's request in MT-MIDI-4 step 7.
The owner asked for a time selection anywhere on the timeline and for the
ruler not to clear the track selection. The modifier and the selection rules
below are the integrator's choices, not the owner's words; they are listed
for the owner to confirm or change.

### Context
A time range could only start on a clip's lower half, so an empty stretch of
a track could not be selected by dragging in its lane. A drag in empty lanes
drew a clip marquee instead. The time ruler selected a range on every track
and cleared the track selection, so Create MIDI clip (Ctrl+Shift+M) could not
be aimed at a chosen track.

### Decision
- **Empty lanes make a time range.** A drag from empty lane space selects time
  on the tracks it spans: from the track under the press down to the track
  under the pointer (a horizontal drag keeps one track). It snaps to the grid,
  and Alt places it freely. It uses the same range as a clip's lower half
  (`Gesture::Range`, `TimeRange`).
- **The clip marquee is Ctrl+Shift (Cmd+Shift on macOS)** (assumed). It keeps
  the clips already selected, as Shift or Ctrl did. Shift or Ctrl alone no
  longer draw a marquee: they make a range, so no modifier means two things.
  Alt stays the snap bypass, and Alt-drag moves windows on several desktops,
  so it was not used. Decided by `select::empty_lane_drag`.
- **A range and the track selection go together** (assumed). A range's tracks
  are the selected tracks: a lane or lower-half drag selects the tracks it
  spans, and the ruler's range covers the selected tracks (every track when
  none is selected) and keeps the selection. A range stays only while it
  covers exactly the selected tracks (`TimeRange::covers_exactly`, checked in
  `sync_selection` each frame). Selecting other tracks drops it, as before.
- **Create MIDI clip picks its track in this order** (assumed): the first
  selected MIDI track, then the first MIDI track of the time range, then the
  track last clicked on the lanes.
- **Pasting a copied range clears only the tracks it was copied from**
  (`Clipboard::rows`, `edit::landing_tracks`). A range copied from a selection
  with gaps no longer clears the tracks between. The pasted range is selected
  on the tracks it landed on.

### Consequences
- Track headers show the tracks a range spans, since the range's tracks are
  the selection. A clip's lower-half drag with a track selected keeps its
  range now, where the old frame-by-frame sync used to clear it.
- Hints: empty lanes say "select time", and Ctrl+Shift "select clips"; the
  time ruler says "select time on the selected tracks, or all".
- Tests: unit tests in `arrangement/select.rs` (`over`, `covers_exactly`, the
  marquee modifier), the clipboard test for skipped tracks in `lumen-project`;
  E2E in `tests/e2e/clips.rs` (empty-lane range, horizontal drag, ruler on the
  selected track, the marquee still selects clips) and `notes.rs` (Ctrl+Shift+M
  after an empty-lane drag puts the clip on that track).
- Unverified: Cmd+Shift marquee on macOS, and the feel of choosing tracks by
  header before a ruler drag. Both are in MT-MIDI-4 step 7.

## ADR-0050 — Velocity and controller lanes (M5 slice 4, no format change)

**Status:** Accepted 2026-10-11 (M5 slice 4). Owner choices V A, L A, T A and
R B are in the owner-decisions table ("Velocity and controller lanes"), chosen
from live specimens. Built in parts by delegated agents, integrated and checked
by the integrator.

### Context
Slice 3 gave the piano roll notes, but no lane for their velocities, and no way
to see or draw a clip's controller events (mod wheel, other CCs, pitch bend,
channel pressure). The events have been in the model since format v11
(`ClipEvent`, `EventKind`). The requirements ask for a velocity lane (drag a
stem, draw across stems, box-select) and lanes for CC, pitch bend and pressure
with pencil, line and curve tools. Curves reuse the fades' free curves
(`lumen_core::curve`, ADR-0031).

### Decision
- **No format change.** The lanes read and write the events already in clips;
  the format stays at v14.
- **Pure edits in `lumen_project::edit`.** `velocity.rs`: set, set_each,
  offset, scale, ramp, at_start and stems. `events.rs`: a `Lane`
  (`Controller(n)`, `PitchBend`, `Pressure`), the steps `FINE` (1/64 note) and
  `GRID` (1/16 note), and in_lane, set_point, delete_points, move_points,
  in_box, ramp and replace_span.
- **A ramp keeps the user's shape.** A ramp drops only repeated values, so it
  never becomes a staircase the user did not draw. MIDI has no ramps: a drawn
  line is stored as plain events, one every 1/64 note (R B).
- **Undoable `Document` methods** in `document/lanes.rs`: set_velocities_each,
  ramp_velocities, offset_velocities, move_lane_points, delete_lane_points,
  draw_lane_span and draw_lane_points (the Pencil). A drawn line is capped at
  `MAX_SPAN_EVENTS` (4000) events. Each gesture is one undo step, and the
  document changes once, on release (Live A, as for notes). Small structs
  carry a gesture: `Drag`, `Line`, `Shift`.
- **The lane** (`note_editor/lane/`) sits under the roll and shares its
  horizontal zoom, scroll and grid. Its 26 pt header holds the tabs (Velocity,
  Mod wheel, Pitch bend, Pressure, and a "+ CC" menu of ten common controllers)
  and, at the right, the tools Pencil, Line and Curve. The plot is 28% of the
  editor's height, clamped to 48–150 pt. The lane is left out when the roll
  above would be under 140 pt; a "Lane" button in the editor's toolbar hides it.
- **Velocity lane.** The Pencil sets every stem its stroke passes: chords share
  a stem, and a fast stroke does not skip stems. Line and Curve ramp the
  velocities of the notes between their ends.
- **Controller lanes.** A press on a point moves it (value and time, snapped).
  Shift-drag draws a box that picks points; in the velocity lane it picks the
  notes of the stems inside it. Delete removes the picked points when the lane
  was the last place pressed. Esc cancels a gesture. After Curve, a dot at the
  curve's middle bends it until anything else changes; the curve is the fades'
  free curve (`lumen_core::curve::Curve::from_handle`).
- **Commands** (area MIDI, in the palette, rebindable; the three tools have
  the keys 1, 2 and 3, the rest no default keys):
  `midi.toggle_lane`, `midi.lane_velocity`, `midi.lane_mod_wheel`,
  `midi.lane_pitch_bend`, `midi.lane_pressure`, `midi.lane_pencil`,
  `midi.lane_line`, `midi.lane_curve`, and `midi.raise_velocity` and
  `midi.lower_velocity` (the picked notes' velocity by 8, one undo step).
- **Changed from the plan:** the keys 1, 2 and 3 for Pencil, Line and Curve
  are plain keys that work whenever a clip is open (not only with the lane
  focused). Shift with the Pencil does **not** draw a line: in the lane Shift
  starts a box, as it adds to a selection everywhere else, and the Line tool
  (key 2) draws lines. Owner to confirm.

### Consequences
- Every lane edit is one named, undoable `Document` method over a pure
  transform; the lane never edits in a closure.
- `velocity::set`, `velocity::scale` and `events::set_point` are tested, but
  nothing calls them yet. Slice 5 (humanize and quantize) is their first user;
  they have no `Document` wrappers yet.
- Limits, all recorded in `docs/KNOWN_ISSUES.md` (2026-10-11, M5 slice 4): the
  frame times are unmeasured; the mouse wheel over the lane does not scroll or
  zoom; the first stem at the clip's left edge shows half its head; "+ CC"
  offers ten common controllers only, and the tabs, the chosen lane, the tool
  and whether the lane is shown are not kept between sessions; there is still
  no auto-scroll at the editor's edges, lane drags included; the lane has no
  info-line hints yet.
- Quantize, humanize and the rest of the M5 list are later slices and are not
  in this record.

### Tests
- `edit/velocity.rs`: 16 unit tests. `edit/events.rs`: 30. `document/lanes.rs`:
  16. `note_editor/lane/`: 63 unit tests.
- E2E `tests/e2e/lanes.rs`: 18 scenarios: the lane under the roll, hide and
  show, the tabs, the "+ CC" menu, a velocity stroke as one undo step, Live A,
  Esc, the Pencil, a line, the curve's bend, the pitch bend lane, moving a
  point, a box with Delete as one step, what the lane draws reaching the fake
  MIDI port, the palette's lane and tool commands, raise and lower velocity,
  and a box in the velocity lane.
- `crates/lumen-engine/tests/midi.rs`: drawn controllers and bends land on their
  exact sample, in order, with their values, at five combinations of tempo,
  sample rate and block size. This is the golden check for controller and bend
  timing.
- Screenshot test `clip_lane` (`tests/shots.rs`): velocity and curve, dark at
  100% and 200%, light at 100%, checked by eye.
- Four older note-editor E2E tests and the `note_editor` shot had their clicks
  moved off a row the lane now covers.
- Results: the full workspace tests pass (lumen-app lib 417, E2E 272, the other
  crates as before), and `cargo clippy --workspace --all-targets -- -D warnings`
  is clean.

### Measurements
Frame times are not measured. `--stress --bench-frames --bench-editor` was run
once in a cloud container under Xvfb on a software Vulkan device (lavapipe, no
GPU): about 7 fps and p99 227 to 243 ms, which says nothing about real
hardware and is not comparable with the earlier runs. One thing it does show:
idle was 3 repaints in 3 s, the same as before the lane, so the lane adds no
repaints when nothing moves. It was not checked whether the lane was open in
that run's editor. Stems are cached per clip version and drawing is limited to the
visible part, which should keep the lane cheap, but that is unproven. MT-MIDI-5
step 12 asks the owner to run `--stress --bench-frames --bench-editor` at
1600 × 900 and 3440 × 1380; the p99 per phase and the idle repaints are added
to this record when they are in.

**Unverified:** the timing and audibility of drawn bends and controllers on real
hardware and on the built-in synth (MT-MIDI-5).

### Amendment (2026-10-12, owner bug reports): a head for every note, and group drags

Two reports: velocities of several notes could not be changed together, and
the velocity of one note could not be changed when notes overlapped.
- **A head per note.** A stem is still one per start, but every note of it
  draws its own head at its own velocity (a chord at 100 and 60 shows two
  heads). Heads are drawn in note order, so the higher index is on top; each
  is lit when picked, greyed when muted, and larger when hovered. The stem
  runs to the loudest head, lit up to the loudest picked one.
- **The head-grab rule.** `LaneView::head_at` answers which note a press
  grabs: among the notes of the stems within `HIT` of the pointer in x, those
  whose own head is within `HIT` of it. A picked note whose head covers the
  pointer (within `HEAD_RADIUS`) comes first, then the nearest head, then the
  higher note index. Hover uses the same rule (`LaneHover::Head`), so the head
  that lights is the one that would be grabbed.
- **Group drag.** A press on a head starts `Kind::Heads` instead of the
  Pencil. Its group is the picked notes when the grabbed note is one of them,
  else the grabbed note alone, which is then picked. Dragging moves each
  note's own velocity by the pointer's travel in values from the press
  (relative, so a grab does not jump), each clamped to 1..=127; the document
  changes once, on release, as one undo step through `set_velocities_each`, and
  the picks are kept. Notes the clamp leaves unchanged are not part of the
  edit. The chip shows the grabbed note's new value. A press away from any
  head, and Shift, behave as before (the Pencil strokes and the box).
- **Tests.** Unit tests for `head_at` (a chord, equal heads, a picked note
  that covers, the nearest, nothing beyond `HIT`, stems close together) and
  for `heads` (shared delta, a single unpicked note, relative grab, no travel,
  clamping). E2E in `tests/e2e/lanes.rs`: group drag with its undo, clamping at
  1 and 127, an unpicked head, a relative grab, a chord, coincident heads and
  a near start.

## ADR-0051 — Premium control look (approved from canvas mock-ups 2026-10-11)

**Status:** Accepted 2026-10-11 (owner, from interactive mock-ups on the
design canvas: button states and the clip tab).

### Context
Controls were flat fills; "on" was a flat `accent_muted` with accent text.
The owner wanted a more finished, tactile interface that every surface
(transport, headers, mixer strips, note editor, dialogs) inherits.

### Decision
- Raised controls: fill with a slight top-to-bottom lift, a 1 px light top
  edge, `border`, soft drop shadow; hover steps to `control_hover`;
  pressed is sunken (inset shadow) and 1 px lower.
- On is lit: accent gradient with `text_on` letters; solo in the warning
  colour, arm and record in the record colour; mute on is recessed with hint
  letters. State keeps its label, icon or shape, never colour alone.
- Segmented controls sit sunken in a `well` track with a raised thumb that
  still glides; they take an optional icon (centred on the label's line,
  snapped to pixels). Tabs keep the sliding underline and gain a glow.
- The shared layer is `widgets/raised.rs` (`Face`, `paint`, `paint_sunken`,
  `button`, `icon_button`), `theme::{Tone, lit, lift, edge_light, thumb}`.

### Consequences
- Glow levels used: `light::LIT` (accent, solo, tab underline) and
  `light::FLARE` (record); no new level. `light::glow` fades a halo by `t`.
- Changed now: `toggle_button`, `segmented`, `tabs`, the M, S and cue
  buttons. Not yet: egui's own buttons and menu buttons (flat, from
  `theme::apply`), the arm button's round face, switches, the note editor.
  Call sites adopt `raised::button`/`icon_button` as they migrate.
- Gradients are a small mesh per control; Reduce motion only removes the
  cross-fades. Unverified on a GPU window.

## ADR-0052 — The design system replaces ADR-0051's lit look

**Status:** Accepted 2026-10-11 (owner, choosing between the design system
and ADR-0051 point by point; supersedes ADR-0051's Decision and
Consequences where they differ).

### Context
The owner's Lumen design system (a Design System artifact: `tokens.json`,
a README, a Material card, a card per component and a `porting.md`) was
made from the code beside ADR-0051's premium control look, built the same
day. They agree on the shape (raised controls, sunk wells, a raised thumb)
and differ on states. The owner asked for the tokens, components and
widgets to be made consistent with the design system.

### Options (per difference)
1. On, solo: ADR-0051's lit accent gradient with a glow and amber solo, or
   the design system's `accent_muted` with a hairline and a filled accent
   solo without a halo.
2. Mute: recessed with hint letters, or the lit `mute` grey.
3. Raised: a computed lift, a drop shadow and a `control_hover` step, or
   the `control_hi` token, Ember and no shadow.
4. Arm: a raised square lit red, or the round button.

### Decision
The design system on all four (the owner-decisions table). In code:
- New palette tokens: `ground`, `control_hi`, `thumb_hi`, `thumb`,
  `edge_light`, `shade`, `ember`, `accent_hi`, `accent_lo`, `on_accent`,
  `mute`, `on_mute`; `radius::{XL, FULL, for_height}`;
  `stroke::{HAIRLINE, LIT, FOCUS}`; `size::{CONTROL_LARGE, TRANSPORT_ICON,
  TRANSPORT_BAR, TRACK_STEP, HEADER_COLUMN, MONO_SMALL, KEYCAP}` (space XL,
  XXL and the display style arrive with the first-run screens);
  `motion::{WASH, BREATH}`.
- `theme::Tone` is `On`, `Accent` (filled), `Mute` and `Record` (the one
  lit state that glows); `Warning` and `Recessed` are gone. `lift`,
  `thumb()` and `control_shadow` are replaced by tokens and the `shade` lip.
- `widgets/raised.rs` paints to the Material card: the gradient, the
  border (`border_strong` on hover), the edge light, Ember on hover; pressed
  is flat `control_press` with the lip and the content 1 pt down; disabled
  is flat; `paint_sunken` is `well` and the lip; `paint_field` adds the
  border; the focus ring sits 2 pt outside; `primary_button` is filled.
- egui's style: 6 pt radii on its 24 pt widgets, windows 12, menus 8,
  Ember hover, a border on raised controls, wells for text fields.

Kept, as the design system does: K5 keycaps, the arm ring's breath, the
comet knobs, the header slider's level bars and the handle that opens
around its value. Kept as earlier owner decisions the design system does
not overrule: the grid through clips (G1), the half-clip hover (H1), the
piano roll's Look A note colours and velocity bars, Beam meter heads
(ADR-0043), the palette's 520 pt width and 30 pt rows, the pan fill moving
to the accent.

### Consequences
- Visible across the app: every on state turns quieter (no glows but the
  recording one), solo turns blue, mute grey, buttons lose their shadows,
  24 pt controls round to 6 and windows to 12.
- ADR-0051's tests for amber solo and recessed mute are replaced by tests
  for the tokens' contrast and the tones.
- The fixtures, screenshots and E2E names are unchanged; the owner checks
  the look in MT-DS-1.

## ADR-0053 — Clip headers move; bodies select time

**Date:** 2026-10-11. **Status:** accepted by the owner.

### Context
Empty timeline space already selects arbitrary time ranges (ADR-0049).
The clip's old upper/lower-half division adds an invisible boundary that
does not follow the visible title bar. Selecting clips during playback
also unexpectedly seeks the transport.

### Decision
Dragging the visible clip header moves it; Ctrl-drag copies and Ctrl+Alt
drags slip audio. Dragging the waveform or MIDI-note body selects time,
as empty lane space does. Existing trim, fade and gain handles retain
their priority. Remove the dashed halfway divider and describe the
header/body gestures in the info line.

Plain clip and lane clicks seek only while stopped. While playing they
update selection without changing playback position; ruler and scrub
strip clicks still seek. This supersedes the split-clip interaction and
halfway hover retained by ADR-0052.

### Consequences
The move target follows the visible title at every track height. Range
selection remains available from the body, empty lanes and time ruler.
Regression coverage must distinguish header/body drags, preserve the
editing handles, and check both stopped and playing click behavior.


## ADR-0054 — Quantize and its tools (M5 slice 5, no format change)

### Context
Slice 5 adds quantize (grid, four modes, strength, swing, sensitivity
window, iterative use), a one-key apply, an options popover with a live
preview, and related tools (humanize, fixed length, scale constrain,
reverse; legato and transpose exist from slice 3). The owner chose from the
"Lumen Quantize Specimens" artifact: **ghost trails** for the preview (the
recorded position stays as a dashed outline, the new one is solid, a line
shows the move), **comet sliders** in the popover (the send knob's trail and
glowing head laid flat), **Q with nothing picked quantizes every note in the
clip**, and **"restore recorded timing" deferred**.

### Decision
- The arithmetic is pure and in `lumen_project::edit` (`quantize`,
  `note_tools`), working on picked indices under the slice 3 selection
  contract. Each note is moved independently, so the preview is the same
  function applied per note (the order is kept, so picks keep their indices).
- `Document::{quantize,humanize,fixed_length,scale_constrain,reverse}_notes`
  are one undo step each; locked and audio clips refuse them.
- **Q** applies the last settings (`Options` in `note_editor/quantize.rs`),
  **Shift+Q** opens the popover below a toolbar Quantize button. The popover
  uses `Menu::staying` (no fade when the pointer leaves) and
  `Menu::open_for` / `close_for`. While it is open the roll draws a stand-in
  clip with the quantized notes and the trails; the document changes only on
  Apply. Esc or a click elsewhere leaves it untouched.
- **Iterative quantize is strength below 100% applied again.** A separate
  switch (in the mock-up) would do nothing a repeat of Q does not, so the
  popover has none.
- **Comet sliders** are `SliderStyle::comet` / `widgets::comet_slider`: the
  same slider with a trail brightening to a glowing head. Only the popover
  uses them; the other value sliders keep the line handle until the owner
  asks.
- The tools' amounts are fixed for now (humanize 1/64 note and 8 velocity,
  different offsets on each press; fixed length is the grid step; scale and
  root chosen from the context menu and remembered).

### Consequences
No persisted change (format v14). The options live for the session. A
popover for the tools' amounts, quantize of clips selected in the arrangement
(without opening the editor), and saving the options in the settings file are
follow-ups (KNOWN_ISSUES).

### Amendment (2026-10-12, MT-MIDI-6 run 1)
The owner found four things, all fixed:
- Choosing a Grid in the popover closed it: a click in a menu opened from a
  `Menu::staying` menu counted as a click elsewhere. A staying menu now
  stays open while another menu is drawn in the same pass (`note_drawn`,
  `other_drawn` in `widgets/menu.rs`).
- At 100% the comet's trail ran past its head; it now ends at the head.
- The head's glow was too strong; one soft ring, 0.10 and 0.22 alpha.
- It was not clear what grid Q uses. The editor has its own adaptive grid, so
  the toolbar now shows what Q does beside the Quantize button ("1/16",
  "1/8T · ends · 50%"), and Q shows a toast ("Quantized 4 notes to 1/16").
  This label and toast were not mocked up first; the owner can veto or
  restyle them.
E2E: `choosing_a_grid_in_the_popover_keeps_the_popover_open` (fails without
the fix), `the_label_beside_quantize_says_what_q_will_do`,
`q_with_nothing_picked_says_what_it_quantized_in_a_note`.

### Amendment (2026-10-12, MT-MIDI-6 run 1, notes shorter than the grid)
The owner saw End and Both turn short notes into slivers: a note shorter
than the grid has its start and end snap to the same line, and the 1/128
floor made it a hairline. In End and Both a note is now never quantized
shorter than one step: an end whose nearest line would leave it shorter goes
to the first line a step or more after the start (for Both, after the line
the start is headed for), so a short note grows to a step. Strength and the
window still scale the move. Start and Length are unchanged. The popover says
that these modes change note lengths. Unit tests in `edit/quantize.rs`
(`end_mode_never_leaves_a_note_shorter_than_a_step` and four more).

### Amendment (2026-10-12, based on the owner's comparison test)
The owner tested one scenario in both applications (two adjacent eighth
notes at 1.3 and 1.3.3, grid 1/4) and the results differed. Lumen's rules
had no source (they were written from memory); the rules now follow the observed behavior, and the previous amendment's "one step minimum" is
withdrawn:
- **A tie goes to the later line**: a note exactly halfway moves forward
  (1.3.3 goes to 1.4). It went to the earlier line.
- **End** pulls the end to its nearest line and leaves the start. If that
  line is at or before the start, the end goes to the first line after the
  start (so nothing collapses). An end already on the grid stays (the old
  floor had pushed the second note's end from 1.4 to 2.0).
- **Both** quantizes in this order: Start first (the length stays), then the end
  of the note as it now is. Adjacent notes stay adjacent. They had been
  quantized independently from their original positions and overlapped.
Unit tests `ableton_start_*`, `ableton_end_*` and `ableton_both_*` in
`edit/quantize.rs` are the owner's scenario. The owner has not yet verified
swing in quantize, the window, what Ctrl+U does with no selection,
and a note that collapses in End (Lumen takes the next line).

## ADR-0055 — Recording MIDI (M5 slice 6, no format change)

### Context
Before slice 6 the Record button was inert, F9 was unbound, and nothing kept
live input. Slice 6 records MIDI: Record plays from the play marker and keeps
what the armed MIDI tracks are sent; the take lands as a clip in Replace or
Overdub mode; a count-in of 0, 1, 2 or 4 bars with the metronome clicking; a
record-only metronome; punch from the loop brace's switches. The owner chose
from the plan (`docs/milestones/M5.md`, slice 6) and the "Lumen Recording
Specimens" canvas, on 2026-10-12:
- **Replace by default, with an Overdub switch** (C4, the plan's choice).
- **Loop passes layer**: with Loop on, every pass adds to the same clip, in
  Replace and Overdub alike. Separate takes are M9.
- **Settings live in the app settings file** (`settings.record`: count-in,
  record-only metronome, Overdub), not in the project, so no format change
  (v14 stays).
- **A clip runs exactly from record start to stop**, not rounded to a bar
  line (the owner took this over the recommended bar-line end).
- **1-A**: the count-in shows on the position field, which counts up through
  negative bars to 1.1.1, and on Record, which pulses on each beat. No new
  widget. **2-B**: Overdub is a toggle beside Record; the count-in and "only
  while recording" are in the metronome's menu. **3-A**: a clip being recorded
  is record red (body, outline, notes, a "REC" tag) and drops into its track
  colour on stop.

### Decision
- **Engine** (`lumen-engine`, `engine.rs`): `Command::Record { count_in }` and
  `Command::EndRecord`. Record from stop holds the song at the play marker
  while the metronome clicks (the first beat of each bar accented), whether or
  not the metronome is on; live instruments still sound. Playback and
  recording start mid-block, on the exact frame, when the count-in ends.
  Record while playing starts at once. Play is ignored while counting. Stop,
  Pause, Locate while counting and Panic cancel a count-in (Stop and Panic
  also end recording). EndRecord ends recording and playback carries on.
  `RenderContext::{recording, looping}` tell the tracks; `LiveIn::played` is the
  device frame a message was played at, before the feed's one block of delay.
- **Capture** (`midi/capture.rs`, `player.rs`): a track with `recording(Listen)`
  set (its MIDI From; Monitor does not matter) keeps every message it takes
  as `Captured { track, source, at, msg }`, where `at` is the timeline sample
  heard when it was played: the part's start less the read ahead, plus the
  device frames since, wrapped into the loop when played just before a wrap,
  and never below 0. The app takes off the output latency. The queue is
  lock-free, 8,192 deep; a loss is counted in `MidiPlayer::dropped()`.
  `MidiHandle::take_captured` hands the consumer over once. An armed track
  with Monitor Off records and plays nothing. The snapshot publishes
  `recording`, `count_in_left` and `count_in_total` (frames).
- **Take builder** (`lumen_project::edit::record`): `place(project, base,
  track, take, Placement { mode, punch, take_clip }, fresh)` starts again from
  `base`, the project as it was when recording began, so a growing take
  committed repeatedly gives the same result as one commit of the final take.
  *Replace*: a new clip over the range, and `overwrite` cuts what is under it.
  *Overdub*: merged into the MIDI clips under the range (a looping clip takes
  the take at its place in the loop, so passes layer), and a new clip for each
  gap no clip covers that holds notes or events. Audio clips count as covered.
  A locked clip overlapping the range gives `Blocked`. Notes are cut at the
  range's end and the clip's end and keep at least a tick; a note that starts
  before the range is dropped.
- **Document** (`document/record.rs`): `begin_recording` and `record_take`. All
  takes of one recording share a merge key, so the recording is one undo step.
  A blocked take changes nothing; an unchanged one makes no step. Undo and redo
  are refused while recording ("Stop recording first").
- **Recorder** (`app/record.rs`, `recorder.rs`): each frame the app drains the
  queue into `recorder::Capture`, which pairs note-on and note-off by channel
  and key, drops the channel (clips have none, and playback sends channel 0),
  keeps controllers, bend, channel pressure and program (with no bank) as clip
  events, and drops key pressure and system messages. A note still held is not
  in a take until it ends; at the stop `close_all` ends it. Takes are committed
  about once a second while playing and recording. Autosave's delay is 10 s
  while recording (60 s otherwise).
- **Range**: the take starts where the playhead was when Record was pressed
  (the marker when stopped, after the count-in) and ends where it is when
  recording stops. With Loop on and the playhead inside the loop, the take
  starts at the loop's start; once playback has wrapped, it covers the whole
  loop. This is what the code does; see Consequences.
- **Punch** uses the loop brace's switches (`punch_in`, `punch_out` in the
  project's transport). With punch in, the take keeps nothing before the
  brace's start; with punch out, nothing after its end. With punch out, recording ends at the brace's end (or at
  the first wrap) while playback carries on. Auto-punch needs no switch.
- **Settings** (`record_settings.rs`, `settings.record`): `count_in` is Off,
  1, 2 or 4 bars, read as bars times the project's numerator when Record is
  pressed; `only_while_recording` keeps the metronome silent except while
  recording and in the count-in (the app gates the engine's metronome in
  `Transport::send`); `overdub`. Names are stored as words, an unknown word
  reads as Off, and every field has a default, so no version change.
- **Commands** (`commands/`): Record (F9), Overdub, Count-in: off, 1, 2 and 4
  bars, and Metronome only while recording, in the palette and the menus. F9
  while counting in cancels the count-in and stops; F9 while recording ends
  it and playback carries on.
- **Interface**: the Record button (`transport/bar.rs`; the pure helpers in
  `transport/record.rs`) shows the record glyph at rest, a hollow ring that
  pulses on each count-in beat, and a lit fill while recording; its tooltip
  names the count-in. The arrangement (`arrangement/recording_look.rs`) paints
  a take in record red. In Replace the take's clip has its body tinted (20%),
  its outline at 78%, its notes in `record_note`, and a "REC · name" tag. In
  Overdub the clip already there keeps its body and notes and takes the red
  outline and tag; its new notes are not yet red. Clips return to their track
  colour when recording ends.
- **Not in this slice**: recording armed audio tracks (M9). Record with no armed
  MIDI track does not start, and sets a notice ("Arm a MIDI track to record.")
  that the audio panel and the Audio settings window show. The plan said the
  info line; the notice is where the app already reports audio problems.
- **No format change** (v14). The project holds nothing new.

### Consequences
- A crash costs the notes still held and about a second of finished notes at
  most; autosave (10 s while recording) holds the rest. The recovery path is
  checked by hand only (MT-MIDI-7); no E2E scenario kills the app.
- Each commit copies the take's notes and rebuilds the recorded track from its
  base. The cost grows with the take and is not measured for long takes.
- While a take is recording, an edit to the same track other than the take is
  lost at the next commit, because the track is rebuilt from `base`. Other
  tracks are safe. Blocking such edits, or merging them, is for the owner to
  choose (KNOWN_ISSUES).
- Clips a recording makes in Overdub gaps, after the first, get new ids at each
  commit. The take's own clip keeps its id. The UI must not hold these ids.
- The output latency is taken off automatically. There is no manual offset
  until audio recording (M9).
- With Loop on, a take can begin at the loop's start, earlier than the playhead
  was when Record was pressed. The owner has not judged this yet (MT-MIDI-7,
  step 6). The "start to stop" rule holds with Loop off.
- Interface figures, as built: compact wells pad 4 pt (`space::XS`) and the
  position well's inner gap is 4 pt compact; the keyboard readout shows from
  1,320 pt wide (`barfit`), where it showed from 1,280 pt before. The owner
  has not seen these (MT-MIDI-7, step 13).

### Tests
- Engine `tests/record.rs` (8): notes kept at the sample they were played, at
  any block size; only recording tracks keep input, and only while recording;
  the count-in holds the song and recording starts mid-block; the accent on
  each bar's first beat; Stop and Locate end a count-in without moving the song;
  Stop ends recording and the next Play does not record; a note played just
  before the loop wraps lands at the loop end; recording while playing starts
  at once.
- `midi/capture.rs` (5 unit tests on `heard`). Allocation guard
  `tests/realtime.rs`, `midi_never_touches_the_heap`, sends Record (with and
  without a count-in), EndRecord, Locate, Stop, Play and Panic.
- `lumen-project` `edit/record.rs` (11): Replace, Overdub merging, gap clips,
  looping clips, punch, lengths, a growing take equal to one call, locked
  clips, empty takes.
- `document/record.rs` (4), `recorder.rs` (17), `transport/record.rs` (7),
  `arrangement/recording_look.rs` (4), `record_settings.rs` (6), and the
  recording listen rule in `midi_play.rs`.
- E2E `tests/e2e/record.rs` (9 scenarios): a take starts at the marker and ends
  where playback stops; a take is one undo step and undo is refused while
  recording; Replace cuts the clip under it; Overdub keeps the notes and adds to
  the same clip; a count-in holds the playhead and keeps what is played during
  it out; no armed track means nothing starts and it says so; a held note is
  closed at the stop; loop passes layer into one clip spanning the loop; punch
  out ends the take at the brace and playback carries on.
- `tests/shots_recording.rs` (ignored; needs a GPU and writes shots).
- Not run in this container: the WSL performance gate, the macOS cross-clippy,
  and `--bench-frames` with a take open.

### Amendment: MT-MIDI-7 run 1 (2026-10-13)

The owner recorded with a real keyboard and found three faults:
1. **The take painted in chunks.** The clip and its notes appeared only at each commit, about once a second,
   rather than updating as recording progressed.
2. **The clips under the take were redrawn at the same interval.** Each commit
   rebuilt the track from `base`, and the pieces a Replace cut off clips got
   fresh ids every time, so the arrangement treated them as new clips.
3. **Notes at the edges of the take were lost.** A key already down when
   Record was pressed had no note-on in the take; and a note held across the
   loop's end had its note-off earlier in time than its note-on, which became a
   one-tick note, so a note changed just before the wrap was lost.

Changes:
- **The live take** (`arrangement/live_take.rs`, drawn by `clips.rs`
  `draw_live_take`): every frame the app gives the arrangement the take as
  captured so far (`Capture::so_far`, held notes ending at the playhead). It is
  drawn over the lanes after the clips: in Replace an opaque record-red body
  from the take's start to the playhead (covering whatever the document has of
  it), in Overdub a body only over the gaps between the clips under it, and in
  both the notes as they are played, in record red, a held one growing. The
  document's commits are no longer what the eye follows. They stay about once a
  second (and at each loop wrap, so the next pass plays what this one kept).
- **Stable ids**: `Recording` remembers the ids a track's commits gave the
  pieces they cut (`pieces`, per track), and the next commit gives the same
  pieces the same ids in the same order. A clip a take only shortens stays the
  clip it was; the Overdub gap clips keep their ids too (a KNOWN_ISSUES row is
  gone).
- **Held keys** (`lumen-engine` `midi/player.rs`, `hold.rs`): a track that can
  record keeps the keys its input holds (`TrackState::down`, a preallocated
  list), and when its recording begins (`cx.recording` turns on for it) the
  player queues a note-on for each, as played at the start. `Capture::take`
  starts a note that began before the range (these, and notes the output
  latency put just before it) at the range's start.
- **Loop passes**: `Captured` gains `pass`, which counts up at each loop wrap
  (a message played just before a wrap is of the pass before), and a marker
  (`source: WRAP`) is queued for each recording track at every wrap. The app's
  `Capture::looping` cuts a note that was held across a wrap at the loop's end
  and goes on from the loop's start, filling the loop for each whole pass in
  between; notes still held at the stop end where the playhead is, in the pass
  it is in.

Tests: engine `tests/record.rs` (a held key is taken at the start; wraps mark
passes for every recording track; the wrap case keeps its pass), `recorder.rs`
(notes across one wrap, several, at the stop and with the off arriving after
the marker; held notes at the start; `so_far`), `document/record.rs` (ids stable
across commits), `arrangement/live_take.rs`, and E2E `record` (a key held when
Record is pressed; a note held across the wrap; the take follows the playhead
and the clips under it keep their ids).

Not verified: how it looks and feels with a real controller (MT-MIDI-7 run 2),
and the cost of the per-frame snapshot of a very long take (it copies the
take's notes each frame; the frame benchmark has not been run with one).

### Amendment: MT-MIDI-7 run 2 findings (2026-10-13)

The owner found two more faults:
1. **Replace still played the clip it recorded over.** The document only cuts
   the old clip as the playhead passes (at a commit), so the engine had played
   its notes before the cut. Now, while a Replace take is recorded, the app
   leaves out of the engine's list whatever the track held from the take's
   start on, except the take's own clip (which loop passes hear)
   (`midi_play::Replacing`, in `Inputs`; `Inputs::playing_length` cuts a clip
   that begins before the take at the take's start). Overdub is unchanged.
   With punch in, the start is the brace's start. With punch out, content
   after the brace comes back when the recording ends (about a frame after the
   brace's end). The project itself is untouched.
2. **A take's notes were centred and moved.** The live take fitted its notes
   to the pitch range played so far, so the first note sat mid-clip and every
   note outside the range moved the rest. It now draws on a fixed layout
   (`mini::live_band`): high pitches above low, two semitones a band over C1 to
   D#6 (pitches outside stay on the first or last band). A note keeps its place
   as more are played. Finished clips are still fitted to their own range
   (slice 3b), so a take refits when recording stops. Whether every clip
   should use the fixed layout, so the same pitch is at the same height in all
   of them, is for the owner to choose (it would change slice 3b's look).

Tests: `midi_play.rs` (a Replace take silences and cuts the clips it records
over, not its own), `piano_roll/mini.rs` (a live note's band depends on its
pitch alone), E2E `record` (Replace sends the output none of the old notes;
Overdub sends the one under the take).

### Amendment: MIDI clip previews (2026-10-13, after MT-MIDI-7 run 2)

The owner chose one consistent arrangement preview for every MIDI clip and the
take, settling the question left open above: notes inside a clip are
fitted to its own pitch range, and the take is drawn exactly as its clip will
be once committed. Three findings, fixed:
1. **Clip previews did not have the piano roll's proportions.** The 32 bands
   a track had were shared by a range padded to 16 semitones, so a semitone
   was about two bands, rounded: notes one band tall with uneven gaps between
   neighbours. Now a track has 128 bands (`mini::BANDS`) and a clip divides
   them evenly among the pitches of its range (`mini::Fit`, still padded to
   16 and centred): each pitch is a row of the same height (to within a band)
   and neighbouring pitches touch. A note covers its row's bands: a box may
   now span several row slots, given in the top 8 bits of its `row`
   (`lumen_render::spanning`; `boxes.wgsl` reads the last slot's bottom).
   Clip boxes and the piano roll's notes never set those bits.
2. **The take moved when recording stopped.** The fixed live layout
   (`mini::live_band`) is gone. Each live note carries the range of the clip
   it lands in (`LiveNote::range`, from `record.rs` `live_ranges`): the whole
   take in Replace; in Overdub the clip under it (its own notes and the new
   ones) or the gap it is in. It is drawn on the same bands as the GPU
   (`Fit::span`), so stopping changes nothing on screen. A
   note outside the range played so far refits the take.
3. **A clip recorded over slid in place.** Each commit cuts the clips under
   the take, and a moved clip glides to its new place (M4.5's M1), so the
   remainder of a clip under a Replace take slid every second. Nothing glides
   while recording, nor for the commit that ends it (`ClipGlides::update`'s
   `recording`).

**Superseded in part (run 3, same day):** fitting each clip to its own range
made a clip of one note, or one chord shape, sit mid-clip at any pitch, so
clips an octave apart, and different melodies, looked the same. The owner
chose a shared pitch layout per track: every MIDI clip on a track shares one
`Fit` over the pitches all of them play (`mini::track_range`), padded to at
least 48 semitones (four octaves, centred), so a pitch is at the same height
in every clip and notes are thin rows. The take uses the track's fit too
(`LiveTrack::range`: the track's
clips as committed so far and the notes played), which replaces the
per-region ranges above (`live_ranges` is gone). A note played outside the
track's range refits the whole track. In Replace, an old
clip's notes in the second since the last commit still count until that
commit cuts them. Shot `mini_notes_octaves`; E2E
`the_take_is_laid_out_as_its_track_will_be`.

**Overdubbing the same pitch (after run 3):** an Overdub only appended the
take's notes to the clip, so playing a pitch the clip already held there
stacked a second note on the first (and the two fought over one key when
played). Now a note played wins its pitch
(`edit::record::settle`): an old note of the same pitch still sounding where
the new one starts is cut there, one that starts under it is dropped, and
other pitches are untouched. A take's own notes settle the same way, the
later winning, so a loop pass over a note of the pass before (in Replace or
Overdub) does not stack either. Tests: `edit/record.rs` (three), E2E
`overdubbing_the_same_pitch_cuts_the_old_note`.

That fixed what is kept, not what is heard while recording (owner): the
engine plays the clip as the document has it, which gets the take about once
a second, so the old note sounded on under the key played, and its note-off
could cut it. The engine now applies the same rule as the key is played, on a
recording track (`TrackState::yields` and `give_way` in `midi/hold.rs`): a
key going down ends the clip's note sounding on it, and while the key is held
the clip's notes on it neither start nor are chased; keys already held when
recording begins give way too. Other keys, and tracks not recording, play as
before. Engine test `tests/record.rs`
`a_key_played_while_recording_ends_the_clips_note_on_that_key`.

Not changed: the clip editor shows the document's clip, so it updates at
each commit (about once a second), not every frame, and in Replace the clip
it had open is cut away by the take.

Tests: `piano_roll/mini.rs` (even rows that touch; the CPU span is the GPU's
bands), `app/record.rs` (a Replace take is one range; an Overdub fits each
clip and each gap), `arrangement/glide.rs` (nothing glides while recording or
for the last commit), E2E `record`
(`the_take_is_drawn_to_the_range_its_clip_will_have`), shot `mini_notes`
(now with a tall track).

### Not verified
- Timing with a real MIDI controller, and with the computer keyboard as a
  source over real drivers (MT-MIDI-7, step 2).
- The count-in pulse and the negative-bar count on screen: unit tests only.
- The light theme and 150% and 200% scale of the new controls and the record
  look.
- Frame times with a take open, on the 200-track stress project.

## ADR-0056 — A MIDI clip is a window onto its notes (format v15)

**Context.** The owner asked that covering one MIDI clip with another stop
deleting the covered notes (after MT-MIDI-7 run 3). Lumen cut a
MIDI clip's content to its edges whenever it was trimmed, covered (moves,
copies, trims and Replace recording all cut what they cover, slice 6c's
clip-window rule), split or slipped (`edit::shift_notes`): the notes outside were
dropped. Audio clips were already windows onto their file, so this was MIDI
only.

**Options** (pitched to the owner; the owner chose A).
- **A. the clip window.** The covered clip is still trimmed or split where
  the new clip sits, but an edge only moves the clip's window: its notes are
  never deleted, and dragging the edge back shows them again. Moving the top
  clip away leaves a gap.
- **B. Stacked clips** (the request read literally): clips
  overlap, the top one plays and hides the one under it, and moving the top
  clip away brings the lower one back by itself. A stacking order, masked
  drawing, an engine that plays only the top clip and hit-testing through
  overlaps, for audio clips too.

**Choice: A.** A note's start stays in clip time (ticks from the clip's
start), so visible notes keep their positions and no offset field is needed.
A note is in the clip's **window** when it starts at or after 0 and before the
end of the clip's content (its loop length, or the clip when shorter;
`edit::in_window`, `edit::shown_notes`). Outside the window it is kept but
hidden. Trims, slices (covering, splitting, time-range edits, the clipboard)
and slips now only move notes in clip time (`shift_notes`, `shift_events`
drop and shorten nothing). A note that starts in the window and runs past its
end is kept whole and cut when played, as before.

**Format v15.** Validation now allows a note or event to start before its
clip (a negative start); the migration from v14 changes nothing (an older
file holds no hidden note). Golden fixture `tests/fixtures/v15/golden.lumen`
holds a note before the window and one past it.

**What reads the window.** Playback (`midi_play` sends only notes and events
in the window; `sounding_keys` agrees), the arrangement's mini notes and the
piano roll (`ClipNotes::each_placed` and `pitch_range` already kept to it),
the clip editor's view (`EditorView::window`: `shows` and `note_at`, so box
select and clicks skip hidden notes), the lane under the roll
(`LaneView::window`; its stems and points are built from the window), and
"every note" for Ctrl+A and the tools given no picks (`shown_notes`).

**Consequences.** Clips can carry notes nobody sees: a clip split many times
keeps every note in each piece. Recording's same-pitch rule
(`edit::record::settle`) may shorten a hidden note that overlaps a played one
in time; that note does not play either way. Join (M7) must take the left
piece's notes before its end and the right piece's from its start, so split
then join returns the original. The clip editor does not yet show hidden notes greyed outside the clip.

Tests: `edit/mod.rs` (a slice is a window that keeps every note; trimming
hides and dragging back shows), `edit/record.rs` (Replace keeps the covered
notes in the pieces), `document/clips.rs`
(`a_covered_midi_clip_keeps_its_notes_and_shows_them_when_extended`),
`midi_play.rs` (notes outside the window do not play), `tests/format.rs`
(v15 golden), E2E `record` (Replace keeps the covered note, hidden).

Overdub across clips (the owner's second point) was checked at the project
level and already behaves as follows: notes merge into every clip they
pass through and a gap gets a new clip. The owner will reproduce what they saw
before anything changes there.

## ADR-0057 — Recording at clip edges, and MIDI trims that follow the hand

**Status:** Accepted (owner's request); decisions 2 and 3's Overdub merge
superseded by ADR-0060 · **Date:** 2026-10-13

**Context.** The owner reported four faults with MIDI clips and recording:
1. Dragging a MIDI clip's left or right edge jittered and landed off the
   pointer.
2. Overdub begun at or before an existing clip made a separate clip beside it
   instead of growing the clip.
3. Recording begun in the middle of a clip should make a new clip from the
   playhead to where recording stops.
4. While recording over a clip, the clip under the take redrew in chunks
   (about once a second), not continuously.

**Causes found.**
1. Three things. (a) The trim gesture put the edge at the pointer's time, so an
   edge picked up a few points inside jumped by that much on the first move.
   (b) A start trim moves `start` and shortens `length`, and the clip glides
   (M4.5) only offset `start`: each snapped step drew the clip at its old start
   with its new length, so the right edge and the notes inside (in clip time)
   jumped and slid back. (c) The track's note layout (`mini::track_range`)
   counted only notes in each clip's window, so a trim that hid or showed a
   note rescaled every clip's notes on the track.
2. Overdub's rule (ADR-0055) gave every uncovered part of the range a new clip.
3. With Loop on, a take pressed inside the loop started at the loop's start
   (a KNOWN_ISSUES row); with Overdub, a take begun inside a clip merged into
   it.
4. The arrangement drew the document (committed about once a second) and laid
   the take over it (`live_take.rs`). In Overdub the clip under the take only
   got the new notes, its new edges and its new layout at each commit.

**Decisions.**
1. `Gesture::Trim` keeps `grab`, the edge's distance from the pointer when it
   was picked up; the edge goes to the pointer plus `grab`, then snaps. Only a
   moved clip glides (its length unchanged); a trimmed one lands at once.
   `mini::track_range` counts every note a clip holds, hidden ones too, so the
   layout holds while an edge is dragged (and stays the same before and after
   a split or cover).
2. and 3. `edit::record` Overdub now depends on where the range begins:
   - **inside a MIDI clip that does not loop**: a new clip over the range, as in
     Replace, carrying what the clips under it played there (their windows,
     every repeat, muted clips excepted) with the take settled over it (a note
     played wins its pitch). It takes the name and colour of the clip it began
     in. What it covers is cut away and stays hidden in the pieces (ADR-0056).
   - **otherwise** (at or before a clip, or on an empty lane): the take merges
     into the clips under it, and they grow over the rest of the range instead
     of leaving pieces beside them: a part after a MIDI clip extends its end,
     the part before the first clip moves its start back (notes moved later in
     clip time, so they sound where they did; not for a looping clip, whose
     loop would shift). Only a part no MIDI clip borders gets a new clip, when
     it holds something. Extension happens even with nothing played there, so
     the clip runs from record start to stop as a Replace take does.
   - A looping clip still layers each pass into its loop, wherever the take
     begins.
   With Loop on, a take begins at the playhead (`Session::from`), and covers
   the whole loop only once it has wrapped.
4. While recording, the app builds `Document::preview_takes` every frame: the
   project as the next commit will make it, from each track's take so far (held
   notes running to the playhead), with the ids the commits use (pieces already
   cut keep theirs; new ones are the ids the next commit will mint), and no
   history step. The arrangement draws it instead of the document. The tracks
   being recorded go on two GPU box layers of their own (`canvas.rs` `live`,
   `live_mini`), rebuilt every frame; the main layers leave those tracks out
   and, while recording, are keyed on the other tracks' identities rather than
   the document's state, so neither the frames nor the commits rebuild them.
   `live_take.rs` and its CPU drawing are gone.

**Measured** (release, the 200-track stress project, in the cloud container):
the preview and the live layers cost about 0.19 ms a frame when recording a
normal MIDI track, and about 7.5 ms when recording into the 100,000-note clip
(its notes are copied and settled each frame); KNOWN_ISSUES.

**Consequences.** What the arrangement shows while recording is exactly what
the commit will hold. Overdub begun inside a clip splits it, so an empty take
there still leaves three clips (the sound is unchanged). Recording before a
clip can show its hidden notes (ADR-0056) when its start moves back over them,
as dragging its edge would. With Loop on and a take begun inside a clip, the
first pass is a new clip and, after the wrap, the whole loop follows the rule
for where the loop starts.

Tests: `edit/record.rs` (begun at, inside and before clips; past ends and
between clips; muted and looping clips; growing takes match one call),
`document/record.rs` (the preview equals the commit and makes no step),
`glide.rs` (a trimmed start lands at once), `mini.rs` (hidden notes count in
the layout); E2E `midi_trim` (start and end edges picked up inside; the layout
holds; all three fail without the fix) and `record` (Overdub before a clip
grows it; inside one makes a new clip from the playhead; Record pressed inside
the loop begins at the playhead; the clip recorded into is drawn growing on
every frame).

Not verified: the look and feel by hand (MT-MIDI-7 run 5), frame times on a
GPU, and the WSL perf gate (`lumen-project` changed; `place` is not in a perf
scenario).

## ADR-0058 — Notes carry on across seams, and Overdub shows its clip at once

**Status:** Accepted (owner's report) · **Date:** 2026-10-13

**Context.** The owner reported three faults with MIDI Arrangement Overdub,
with two screenshots of the boundary case:
1. Overdub begun on an armed track with no clip at the playhead showed no clip
   until the first note was played.
2. Overdub begun inside a clip over a note already sounding: the note looked
   cut off at the new clip's start, and came and went as the boundary was
   moved or resized.
3. While recording that way, existing notes sustaining across the new clip's
   start were cut off, and sounded like short stabs.

**Causes found.**
1. `edit::record` Overdub gave a part of the range that no MIDI clip borders a
   new clip only "when it holds something" (ADR-0057), so an empty take over an
   empty lane placed nothing: neither the preview drawn every frame nor the
   commits showed a clip.
2. The clip begun inside an existing one carried only the old notes that
   *start* inside the take's range (`heard_in`). The note sounding at the
   record start stayed in the old clip, which the new clip cuts there: the
   note was kept (ADR-0056) but hidden past the old clip's end. Moving the
   boundary showed or hid it.
3. Three things. (a) That same note ended at the old clip's new end, the
   record start. (b) The carried notes were cut at the take's current end,
   and a commit (about once a second) moves that end: each commit changed the
   notes, so the engine (which knows a held note by its clip, the clip's
   start and the note's start and length, `hold.rs`) let them go and struck
   them again. (c) The rest of the clip recorded over is a piece that starts
   at the take's end, so its start also moved with every commit, and a note
   it was sounding was let go and struck again for the same reason.

**Options** for the sustain (behaviour; the owner's report asked for the
outcome, "existing notes continue to play for their full duration unless the
new input or an intentional clip boundary changes them").
- **A. Notes carry on across seams.** Where one clip begins exactly where the
  one before it ends and *holds the same note* (same pitch and arrangement
  start, kept hidden before its start, as a split's right piece and the
  Overdub clip do), the note sounds and is drawn on through the boundary. A
  clip that does not hold it (one moved up against another, a note cut where
  a new one was played) still cuts it at the clip's end.
- **B. Play whatever a window shows, including the part of a note that
  starts before it.** Simpler, but it changes ADR-0056's clip-window rule for
  every trimmed clip (a trimmed start would sound the tail of a note), and
  still strikes the note again at the seam.
- **C. Overdub merges into the clip it begins in** instead of making a new
  clip. Avoids the seam, but goes against the owner's choice in ADR-0057
  (point 3: a new clip from the playhead).

**Choice: A.**
- `edit::seams` (pure, `lumen-project`): `is_seam`, `carried_on` and
  `carried(track)`, the notes each clip carries on from the clip before.
  Only unmuted clips that do not loop meet at seams; muted notes carry nothing.
- **Recording.** `record::overdub_from_inside` carries what was heard
  *whole* (`carry`): a note sounding at the record start keeps its start
  before the new clip (hidden), and a note running past the take's end is not
  cut there. So both carry on across the seams with the pieces around the new
  clip, and every commit leaves them as they were. A note played over a
  carried one of the same pitch still wins it (`settle`), which ends the
  carried note there and so stops it carrying on. Overdub over a part no MIDI
  clip borders now makes the clip at once, empty until a note is played, as
  Replace always has (this amends ADR-0057's "when it holds something").
- **Playback.** `midi_runs` (app): touching clips play as one engine clip, a
  *run*, known by its first clip. Note lengths are worked out there: cut at
  the clip's end unless the next clip carries the note on. A run's first clip
  keeps its id and start while a recording moves the seams after it, so the
  notes playing keep their owner in the engine across commits and are neither
  released nor struck again. The engine is unchanged. `sounding_keys` reads
  the same runs.
- **Drawing.** The arrangement's mini notes draw a carried note from the
  clip's start (`ClipNotes::carried`), so the note reads as one line across
  the boundary, as it sounds.

**Consequences.** A note held across a cut that leaves two touching pieces
(an Overdub begun inside a clip, and clip split when M7 builds it) sustains
through it instead of stopping there, so a cut does not change what is heard. Clips that merely touch
play as one engine clip: an edit that separates two touching clips while a
note of the second is sounding lets it go and starts it again (it changes
owner), where before it carried on. The clip editor (piano roll) does not show
carried notes; they are edited in the clip where they start (KNOWN_ISSUES).

Tests: `edit/seams.rs` (splits carry on, other notes and gaps do not, muted
and looping clips carry nothing), `edit/record.rs` (a sounding note carried
whole and on through both seams; carried notes unchanged as the take grows; a
note played over a carried one ends it; an empty Overdub over an empty lane is
a clip at once), `document/record.rs`, `midi_runs.rs` (a split note plays
whole; a clip that does not hold it cuts it; apart and looping clips are runs
of their own; a run cut short by Replace stops; a run is the same engine clip
while the seams after it move; sounding keys follow a seam), `mini_notes.rs`
(a carried note is drawn into the next clip, and not once the clip is moved
away); E2E `overdub_seams` (the empty Overdub clip shows before any note; a
note held where Overdub begins is struck once, through recording, two commits
and playback). Both E2E scenarios fail without the fix.

Not verified: by hand (MT-MIDI-7 run 6), and the WSL perf gate (`lumen-project`
changed; `place` is not in a perf scenario, and the engine is unchanged).

## ADR-0059 — A carried note belongs to its clip, and the clip editor shows hidden notes

**Status:** Accepted (owner's report) · **Date:** 2026-10-13

**Context.** Two findings after ADR-0058:
1. A note carried on across a seam (an Overdub begun inside a clip while it
   sounded) was struck again when playback started at the new clip's start.
2. Notes outside a clip's window were hidden in the clip editor as well as
   on the arrangement (ADR-0056 had left this open). The owner expected notes
   outside the window to remain hidden on the arrangement and visible and editable
   in the clip view.

**Causes found.**
1. Touching clips play as one engine clip (a run, ADR-0058), so the carried
   note is one long note there. Play and every jump **chase** (ADR-0045, H1):
   a note sounding at the position starts at once. Started at the new clip,
   the chase struck the note that began in the clip before.
2. `EditorView::shows`, `LaneView`, the GPU note boxes (`ClipNotes::
   each_placed`), Ctrl+A and the tools given no picks (`shown_notes`) all
   kept to the window; the note edits clamped every note into `[0, length]`.

**Choices.**
1. **A note belongs to the clip it begins in.** `MidiClip::seams` (engine)
   says where each clip of a run begins; `midi_runs` fills it. A chase starts
   only notes that began in the clip the position is in (`hold::owning_start`;
   a position on a seam is in the clip that begins there). Played through the
   seam the note sounds on as before; played from the next clip (its start or
   inside it) it does not sound, because it belongs to the preceding clip.
   Chase inside the clip a note began in is unchanged (H1).
2. **The clip editor shows every note.** `EditorView::shows` checks only the
   view; `ClipNotes::outside` (on in the editor, off on the arrangement)
   places notes outside the window whole at their own times and draws the
   content's notes whole past the window's end. They sit in the time the
   editor already dims outside the clip, so they read as outside (no new
   token or colour). The lane shows their velocity stems and controller
   events. They can be picked, moved, resized, deleted and edited; Ctrl+A and
   the tools given no picks take every note. The note edits allow a note to
   stay as far outside as it already is, no further
   (`edit::notes::time_room`, `resize`), so a hidden note can be dragged back
   into the clip. Drawing a new note is still only inside the clip. The
   arrangement still hides them and they still do not play.

**Consequences.** The ADR-0058 known issue (the clip editor not showing a
note carried into a clip) is closed: the carried copy shows, whole, starting
in the dimmed time before the clip. `sounding_keys` (busy keys for hardware
previews) still counts a carried note after playback started past its seam,
where nothing sounds: at worst a preview of that key on a hardware track is
skipped (KNOWN_ISSUES). The velocity lane does not dim the time outside the
clip as the roll does (KNOWN_ISSUES, a look for the owner).

Tests: engine `tests/midi.rs`
(`play_from_the_next_clip_of_a_run_does_not_strike_a_note_carried_into_it`,
fails without the fix), `midi_runs.rs` (seams), `edit/notes.rs` (a note
outside moves back in but no further out), `piano_roll/notes.rs` (the editor
places notes outside whole; the arrangement does not); E2E `overdub_seams`
(played from the take's start the note is not struck) and `hidden_notes`
(a note past the end is shown, picked, dragged back in, undone, and picked by
Ctrl+A); both fail without the fix. Shot `hidden_notes_editor` (both themes,
100 to 200%) looked at.

Not verified: by hand (MT-MIDI-7 run 7), the WSL perf gate (the engine's
chase changed; it runs on Play and jumps only, not per block).

## ADR-0060 — Overdub always records a new clip

**Status:** Accepted (owner's report, MT-MIDI-7 run 7) · **Date:** 2026-10-13

**Context.** The owner found, in run 7: Overdub begun before an existing MIDI
clip that reaches its start, or begun exactly at its start, merged the take
into that clip (ADR-0057: the clip grew back to the record start, or took the
new notes). The owner asked for arrangement recording instead:
recording makes new clips; with MIDI Arrangement Overdub on, the new clip
includes the track's existing MIDI along with the new input; the existing
clip is kept as its own clip, not merged with the new one.

**Cause.** ADR-0057 chose the merge on the owner's earlier request ("Overdub
begun at or before a clip extends that clip and merges into it"). Only a take
begun strictly inside a clip that does not loop made a new clip.

**Decision.** `edit::record` Overdub now makes one new clip over the range
wherever it begins, the rule ADR-0057 and ADR-0058 built for a take begun
inside a clip:
- the new clip carries what the clips under the range played there (their
  windows, every repeat of a looping clip, muted clips and notes excepted),
  whole, so a note sounding at the record start or running past the stop
  carries on across the seams (ADR-0058), with the take settled over it (a
  note played wins its pitch);
- what it covers is cut away from the clips under it (`overwrite`), as a
  Replace take's is: the parts outside the range stay clips of their own, in
  place, keeping what was covered hidden (format v15). A clip the range covers
  end to end is recorded over; its notes are in the new clip;
- begun in a clip (from its start, before its end) the new clip takes that
  clip's name and colour; otherwise it is "MIDI clip";
- the clip is there from the moment recording begins, empty until something
  is played (ADR-0058, unchanged).
Clip growing (`extended`), merging into clips (`merge_into_clips`) and the
gap rule (`gaps`) are gone. A looping clip no longer takes the take into its
loop: recording creates a new clip there and carries the repeats it covers.
With Loop on, every transport pass still layers into the one take clip
(ADR-0055).

**Consequences.** Overdub and Replace place the same clips; they differ only
in whether the new clip carries what was there. An empty Overdub over a clip
is now a step (a new clip holding the old notes, the old clip cut around it),
where a take begun at a clip's start used to change nothing. Audio clips on a
MIDI track under the range are cut as Replace cuts them (they used to count as
covered and keep the take out). The recording look is unchanged: in Overdub
the take's clip keeps its body and takes the red outline and tag.

Tests: `edit/record.rs` (begun at a clip's start, before a clip and reaching
it, stopped before a clip, at a clip's end, across whole clips, before and
inside a looping clip, an empty take at a clip's start), `document/record.rs`
(the Overdub clip is new and the old one stays); E2E `record`
(`overdub_begun_at_a_clips_start_is_a_new_clip_and_the_old_one_stays_its_own`,
`overdub_begun_before_a_clip_is_a_new_clip_beside_it`; both fail without the
fix; `the_clip_recorded_into_is_drawn_growing_on_every_frame` now follows the
new clip).

Not verified: by hand (MT-MIDI-7 run 8, step 36), the WSL perf gate
(`lumen-project` changed; `place` is not in a perf scenario), the macOS
cross-clippy (no platform code changed).

## ADR-0061 — A sounding note is known by where it began

**Status:** Accepted (owner's report) · **Date:** 2026-10-13

**Context.** The owner found: while a recorded MIDI clip plays, resizing it
strikes its notes again. The engine knew a held note by its clip id, the
clip's start, its repeat and the note's start and length in the clip
(`hold::Owner`), and an edit that changed any of them released the note,
then the chase started it again at once. Resizing changes them while the
note is the same: the app sends each note cut at its clip's end
(`midi_runs::run_notes`), so dragging the end over a sounding note changes
its length on every frame of the drag; trimming the start moves the clip and
every note's place in it (format v15's window). A drag sends a new list each
frame, so a note was struck many times (eight in the engine test).

**Decision.** A held note is known by the absolute tick it began on and its
key (`Held::start`). After an edit, `hold::sync` looks for a note on that key
beginning at that tick in any clip of the track (in its window, in the repeat
there) and only moves its end; a note gone, or now beginning elsewhere, is
released as before and chased if it sounds at the position. The chase skips a
note held by the same start and key. This covers both edges, a note's length
changed in the piano roll, and runs splitting or joining at a seam (the note
no longer depends on which clip heads its run).

**Consequences.** Shortening a sounding note (in a clip, or by drawing the
clip's end in) ends it where it now ends, at once if that is already past;
lengthening it lets it ring on. Moving a clip or a note still
releases and re-chases what it moved. Two clips holding a note on the same
key at the same tick (not possible on one track: covers cut) would share it.

Tests: engine `midi::resizing_a_clip_or_a_note_under_the_playhead_does_not_retrigger_it`
(both edges, the note's length; fails without the fix: 8 note-ons); E2E
`midi_play::resizing_a_playing_clip_does_not_strike_its_notes_again` (drags
both edges of a playing clip; the port gets one note-on).

Not verified: by hand (MT-MIDI-7 step 37).

## ADR-0062 — Standard MIDI Files and MIDI clock out

**Status:** Accepted (plan approved, owner's F1–F4 A) · **Date:** 2026-10-13

**Context.** M5 slice 7 (plan in `docs/milestones/M5.md`, after MIDI learn
moved to M7): Standard MIDI Files in and out, and MIDI clock to hardware.
Clips hold notes and events in ticks at 1,411,200 PPQ with no channel; the
project has a tempo map and one time signature; the MIDI To sender already
times messages to the audio a port is heard with.

**Decision.**
- **Reading and writing** are pure, in `lumen_project::smf`, on `midly`
  (Unlicense, a public-domain dedication; a `cargo-deny` exception, recorded
  in THIRD_PARTY_NOTICES). Type 0 and 1 are read; type 2 is refused. Each
  file track is split by channel into parts. Common divisions come in
  exactly, others round to the nearest tick; SMPTE files are converted
  through their own tempo (120 BPM until a tempo event). A note-on with
  velocity 0 ends a note; overlapping strikes on one key pair with note-offs
  in start order (owner's round-trip report, 2026-10-13),
  a note on and off at one tick lasts one file tick, notes still on end with
  their track. A bank select (CC 0 and 32) at a program change's tick on its
  channel becomes the change's bank. Release velocity, polyphonic
  aftertouch, sysex and meta text other than track names are dropped; only
  the first time signature is kept. Tempos are clamped to 20–999 BPM.
  Files are written at **960 PPQ**: a type 1 file has a conductor track
  (tempo map, time signature), then a named track per part; order at one
  tick is meta, note-offs, controllers and programs (a bank before its
  program), note-ons.
- **Import (F1, F2).** Files are read off the UI thread
  (`midi_files::Reader`). Dropped on a MIDI track's lane, or imported with
  a MIDI track chosen, every part becomes one clip there
  (`Document::import_midi_clip`), cutting what it covers; dropped between
  tracks or on empty lanes, each part becomes a MIDI track with the synth
  (`import_midi_tracks`; a name shared by several parts gets " ch N").
  Clips are whole bars of the project's meter. Notes are placed in beats;
  a file whose tempo or meter differs offers them in a banner, and "Use its
  tempo" applies the map and first meter as one step (`use_file_tempo`).
  Drops compute a place for each kind (`document::Places`): audio files on
  audio lanes, MIDI files on MIDI lanes. Import audio… became **Import audio
  or MIDI…**, keeping its id (`file.import_audio`) so saved shortcuts hold.
- **Export.** What is written is **what plays** (`midi_export.rs`, through
  `midi_runs`): the clip window, notes carried across seams as one note,
  muted notes left out, a looping clip's repeats baked and cut where each
  repeat ends. **Export MIDI clip…** writes the selected MIDI clips as one
  type 0 part from the first one's start, with the tempo from there;
  **Export MIDI file…** writes the project's MIDI tracks (unmuted clips) as
  type 1, each on its MIDI To channel (channel 1 for the synth). The bytes
  are made when the command runs; the write is atomic and off the UI thread
  (`lumen_project::write_atomic`, the save's commit protocol).
- **Clock (F3, F4).** An output's **Sync** switch (a new `Use::sync`, which
  opens an output, not an input) and, while on, a delay field (−20 to
  +50 ms; `settings.midi`, added fields). The **engine** makes the clock,
  not the sender as the plan said: only the engine knows which song
  position each device frame plays, through loops and the delay
  compensation's read-ahead. `midi/clock.rs` is a pure generator on the
  tempo map, tested on its own and run by the player for the ports in
  `MidiHandle::set_sync` (a bit mask, ports 0–63, an atomic). It sends 24
  clocks a quarter, each on the sample of its tick; Start from the song's
  start; otherwise the Song Position Pointer of the **next** sixteenth and
  Continue, with clocks held until that sixteenth, so the first clock is
  the pointer's and a follower lines up exactly; Stop, SPP and Continue at a
  loop wrap or a jump; Stop on stop; nothing while stopped or counting in;
  a tempo change carries the count on. The sender moves a port's system
  messages (not its notes) by its delay, and sends Stop to a port whose
  clock runs when the stream stands still or the sender ends. Clock bytes
  never flash the MIDI chip (`activity::counts`, slice 2).

**Round-trip follow-up (2026-10-13).** The owner exported the blue MIDI clip
and reimported it as a purple clip: some notes were shorter. The reader
ended an earlier same-pitch note at the next note-on, discarding its real
note-off. `smf/read.rs` now queues unmatched note-ons per key and channel
and consumes them FIFO on note-off (including zero-velocity note-ons).
Every unmatched note closes at track end. Regression coverage includes
type 0 and 1 round trips, the hand-made overlapping-strike fixture, and
the app's export/reimport workflow. Ordinary SMF note-offs have no note
identity: nested same-channel, same-pitch overlaps are ambiguous; FIFO
uses start order rather than inventing a release at the next attack.

**Imported-note playback follow-up (2026-10-14).** The owner reported
missing ocarina notes in `FF4_I_LOVE_YOU_THEME.mid`, especially the second
E5, and silence when Play began on a note. Investigation plan: inspect
the source events, compare their imported ticks with sample scheduling,
reproduce both onset playback and continuous block boundaries, then run
the regression, real-time, workspace and smoke checks.

The source has separate, valid note-on/off pairs at 192 PPQ; import scales
them exactly. With the file's tempo map at 48 kHz, the second E5 at quarter
66 maps to sample 1,311,086. Converting that sample back gives a position
27 Lumen ticks later. The engine searched from the inverse sample tick
minus only two ticks, omitting the note if its sample began a block. Chase
correctly left an onset at the play position to normal scheduling, so
neither path started it. This also affects continuous playback when an
onset lands at a block boundary.

`midi::hold::gather` now widens the candidate search by one sample before
converting to ticks, retaining the exact half-open sample check. No notes,
tempo data or persisted format change; no allocation or locks added.
The same rule covers controller events. A larger fixed tick margin was
rejected because the required margin depends on tempo and sample rate.

Regression coverage: every tick rounding to a block's first sample at five
tempos (20–999 BPM) and four rates (22.05–192 kHz), both clip-relative
and absolute starts; the source's second-E5 timing on Play and a continuous
block boundary; a synthetic SMF with the same timing geometry and two
isolated test tones, imported and heard through the E2E app. Both unit
and E2E reproductions fail with the old search bounds and pass with the fix.
The original song remains outside the repository. Re-export/reimport is
not a reliable timing fix: these 192-PPQ positions survive 960-PPQ export
exactly; changed project tempo or clip placement can change sample
alignment. The owner's precise round-trip conditions remain unverified.
Manual recheck: MT-MIDI-8 run 2.

Verification: `cargo fmt --all`, workspace clippy with `-D warnings`,
all 1,453 workspace tests (including 334 E2E and seven real-time allocation
checks), and all six real-binary smoke tests pass on Windows. Independent
focused review found no actionable issues. The WSL instruction-count gate
passes all nine scenarios against the existing baseline: the largest
increase is +1.50% (transport), with the 64-track synth at +1.36%, below
the +2% limit. No performance baseline changed. Listening to the original
file on the owner's hardware remains unverified.

**Consequences.** Channels are lost when a file's parts merge into one clip
(the model has no channel). Positions finer than 1/960 quarter move by at
most half a step on export (about 0.26 ms at 120 BPM). A negative clock
delay is limited by the output latency (the sender cannot send before it
is told). The pointer reaches 16,383 sixteenths (1,023 bars of 4/4);
ports past the 64th get no clock. Turning Sync off while playing sends no
Stop. MTC out is deferred.

Tests: `lumen-project/tests/smf.rs` (hand-made files in `tests/smf/` from
`make.py`, a golden file Lumen wrote, rounding), `document/midi_import.rs`,
`midi_export.rs`, `app/midi_files.rs`, engine `midi/clock.rs` and
`tests/midi.rs` (`a_synced_port_gets_start_clocks…`), the no-allocation
run with Sync switched on and off, the sender's delay and stall, Sync
opening an output; E2E `smf` (4) and `clock` (exact clock counts between
transport messages across a tempo change and loop wraps).

Not verified: importing and exporting external Standard MIDI Files, or MIDI clock
synchronization with hardware or another application (MT-MIDI-8).

## ADR-0063 — The test audio device keeps its pace

**Context.** The first CI run of `m5-midi` (run 166) failed on all three
systems. On Linux one record E2E failed; on Windows five tests; on macOS 25,
plus two unit tests. The cause was one thing: the silent test device ran
one block per supervisor poll (10 ms), however late the poll came. A busy
runner polled late, so the device fell behind the wall clock, and the
wall-clock stamps the E2E gives live input (`Keys`) were mapped onto frames
the playhead had not reached. Notes landed beats away, or were dropped as
stale (`live::STALE_NS`, 100 ms after the audio thread last ran). On macOS
the polls came about every 50 ms, so `listen` waited out its 30 s and the
engine's answer to a click arrived after the test looked. Reproduced on
Linux by running the record scenarios on one core beside four busy loops:
5 of 19 failed, the same ones Windows failed.

**Options.** (a) Run the E2E with fewer threads (slower, and still racing
the wall clock). (b) Loosen each assertion (hides real errors). (c) Let the
device make up for late polls.

**Choice.** (c). `lumen_audio::fake::FakeDevice::pace` is `Some(Pace)`:
each poll runs the blocks that the time since the stream opened owes, up to
16, so a late poll is made up for as a hardware driver's clock is.
`Pace::Every(10 ms)` is the default test device (`test`): the same slow
motion as before, now steady. `Pace::RealTime` is the `test-paced` driver,
which `Launch::with_audio_buffer` selects for the record scenarios, where
frames and wall-clock stamps must agree. The lumen-audio supervisor tests
use `pace: None` and still get a block per poll. A test that reads the
engine's answer (the playhead) waits for it (`wait_for`), never a fixed
count of frames.

**Also fixed in the same run.** `files::tests::save_as_...` compared a
canonical path with a temp folder's own spelling (Windows short names, macOS
`/private/var`); it compares canonical paths. The MIDI sender's lateness test
asserted a 4 ms worst case on a shared runner; it holds the median to 8 ms
and the worst to 25 ms (macOS measured 5 ms and 17 ms). The note menu E2E wrote `Ctrl+D` on macOS.

**Review fixes (PR 14).** `smf::write` returns `Result<_, WriteError>`:
two events further apart than a 28-bit delta (about 78 hours at 960 per
quarter; projects reach 2^52 ticks) were truncated to a wrong position, and
are now an error that the export reports ("Not exported: ..."). Export MIDI
builds the model and the bytes on a worker (`midi_files::export`), as the
other disk and heavy work, and asks where to save when it is done.

**Found by the second macOS run.** `LiveFeed` placed each message with a
frame map whose base is the earliest `time - frame` over 256 blocks. When
that earliest block leaves the window the base jumps, and a later message
maps before an earlier one of the same source: a note-on could overtake the
note-off of its key (`audition::escape_during_a_chord…`, "key 68 struck while
sounding"). A source's messages now never go before the one placed before
them (`LiveFeed::floor`; unit test
`a_sources_messages_keep_their_order_when_the_frame_map_moves`, which fails
without it). `record()` waits for the take to be drawn before a test reads it.
The third macOS run showed the same symptom (struck ON, ON, ON, then the offs)
from the engine: live events taken in one block share a frame, events on a
frame sort offs before ons, and `apart_from_its_on` places a note-off one
frame after its own note-on, so the next chord's note-on, left on the
original frame, passed it. `MidiPlayer::play_live` now places each live
message no earlier than the one before it in the block (test
`messages_landing_on_one_frame_keep_the_order_they_came_in`, which fails
without it). The count-in test waits for the engine's answer instead of
five frames.
A full pending queue now hands over what waits, in order, before the newest
message (it handed the newest first, so a note-off could pass its note-on).
`MidirHost` finds a port in the one listing its names came from (a hot-plug
between two listings could panic `swap_remove` or open another device).
Persisted MIDI clock delays are clamped where they are read
(`Midi::sync_delay`), so a settings file cannot hold the sender's queue far
into the future.

**Consequences.** Unloaded, the default test device runs a little faster
than before (a block every 10 ms exactly, not 10 ms plus the poll's work).
The paced device is a coarser model of a driver than a thread with its own
clock; it is enough for stamps, not for jitter. macOS was not available to
run: its fixes are reasoned from the logs and from the Linux reproduction
(the first macOS CI run after this change shows whether they hold).

## ADR-0064 — MIDI recording brackets mask continuous capture

**Context.** Owner correction, 2026-10-14: punch-out ended capture and
placing a take discarded notes and controllers outside its visible range.
The loop and punch brackets must act as a non-destructive window.

**Choice.** Record captures continuously after the count-in until Stop,
Record-off or an existing safety stop. Capture snapshots preserve note
times and lengths independently of the visible range. Replace and Overdub
place the full supported MIDI performance in existing hidden clip content;
only the active window overwrites arrangement clips. Active notes retain
the existing same-pitch layering rules. Notes and events outside the
window remain available to the clip editor and ordinary edge trims.
Replace playback suppression ends at punch-out. A delayed message before
the first loop entry is mapped to the previous pass only after a real wrap.

The recording highlight stays inside those same brackets. Held-note
previews use the current pass's head after a wrap. Turning Loop off during
a take keeps the original visible window while capture continues beyond
it; held notes close at the actual stop, preserving their tails there.
Bounded Replace playback splits a looping clip into at most three engine
clips: a periodic prefix, a partial cycle after punch-out, and a periodic
suffix. Whole patterns stay shared in the cache, so cost depends on the
pattern size rather than the number of loop repeats.

**Consequences.** No schema or new controls are needed. Loop passes keep
layering into one clip, and recording remains one undo step. Because the
model requires positive clip lengths, nonempty capture wholly outside the
window uses a one-tick clip inside its nearest edge; that tick can split
an existing clip and respects the locked-clip guard. Unsupported MIDI
message kinds retain their existing behavior. Hardware timing and the
manual listening checks are pending in MT-MIDI-7 step 7.

**Recording-start follow-up (owner finding, 2026-10-14).** Removing the old
range clamp exposed output-latency compensation placing the first note
slightly before the clip when recording began away from song start. An
already-held key could therefore be hidden to the left. Capture now floors
initial-pass timestamps at the actual Record start, independently of the
loop/punch window. Subsequent passes keep their own times, including times
earlier than that initial start. Wrap markers identify a silent initial
pass, including delayed messages and pass-counter rollover. This restores
first-note visibility without cropping pre-punch performance.

**Review follow-up (PR 16, 2026-10-15).** Three edges of the continuous
capture are closed. The engine's recording tracks only let a played key
win its pitch over playback inside the recording window (punch, and the
loop once wrapped): after punch-out, recorded clips keep sounding while
the take captures on (`MidiTrack::recording_within`). A latency-compensated
message stamped just after a loop wrap that lands before the loop start
moves to the end of the pass before (`recorder::Capture::push`). A hidden
note carries an earlier clip's note across a seam only when it is a copy:
the same start, pitch and velocity, sounding no later than that note
(`seams::carried_on`), so a recorded note kept hidden before a punch-in no
longer extends an old note through the take. A captured note identical in
start, pitch and velocity and no longer than the old note remains
indistinguishable from a copy without a format change.

**Second review follow-up (PR 16).** Recording eligibility now uses the
same pure position mapping as capture: output-latency compensation,
Record-start flooring and loop-pass normalization. Saved start, punch,
loop and latency metadata reaches each recording track. The engine uses
its own pass counter to select the initial or later-pass window, so keys
played immediately after a wrap do not wait for a UI refresh to win their
pitch. Already-held keys use the same mapping when recording starts.
The saved loop end bounds placement and eligibility before the first
wrap too, including when Loop is disabled while capture continues.

The remaining hidden-note ambiguity requires explicit provenance rather
than a tighter endpoint heuristic. Format v16 adds a default-true seam
carry flag to MIDI notes; only notes captured outside the visible take
window set it false. Those notes remain recoverable through edge trims
and play normally once exposed, but cannot impersonate a copied note at
a seam. Existing four- and five-element notes retain their prior carry
behavior through the v15-to-v16 migration; old captures have no provenance
to recover retroactively. A golden fixture preserves the new encoding.

## ADR-0065 — Deterministic test plugins (`lumen-test-plugin`)

**Status:** Accepted 2026-10-15 (M6 slice 0) · **Date:** 2026-10-15

**Context.** The quality bar asks for a test plugin that simulates good and
bad behaviour, and real-time and isolation claims need a plugin whose output
is known exactly. Third-party plugins cannot supply that: they are not
repeatable, not redistributable, and not available on every CI system. The
earlier `vst3-test-module` misbehaves only at scan time and only on Windows.

**Options.** (1) Extend `vst3-test-module`: Windows-only, factory faults only.
(2) A portable VST3 module written with the `vst3` crate, the binding ADR-0005
chose: it builds and loads on Windows, macOS and Linux, so the host, the
scanner and the golden renders can be tested wherever CI runs. (3) Use
whichever third-party plugins are installed: not repeatable. Chosen: (2).

**Choice.** The crate `lumen-test-plugin` (a `cdylib` for hosts, an `rlib`
for in-process tests) holds two plugins, each an audio component with its own
edit controller:
- *Deterministic Test Instrument*: a square or saw voice bank that starts a
  note on the sample of its event and stops on the sample of its note-off
  (no release), so where a note sounds is a plain function of its events.
- *Deterministic Test Effect*: gain in dB and a whole-sample delay of 0 to
  4,096 samples, which is its reported latency and tail.
Parameters (level or gain; wave or latency) take effect on their exact
sample (a step, not a ramp). Rendering is split at every event and change, so
the output does not depend on how the host cuts blocks (tested). State is a
tagged record that loading rejects when damaged or from the other plugin.
There is no editor view on purpose, so hosts must offer generic controls.

Faults are chosen by the environment variable `LUMEN_TEST_PLUGIN_FAULT`,
which a host process inherits: factory (null, crash, hang), processing
(crash, hang, error, NaN output, slow, each from a chosen block), state
(fail, garbage, hang) and metadata (absurd bus channel count, millions of
duplicate parameters). A malformed value injects nothing and says so on
stderr, so a typo cannot pass for a plugin bug. The fault clock restarts at
each activation, so a host that reloads a failed plugin can be tested.

**Consequences.**
- Verified here on Linux: 24 unit tests and 15 host tests drive both plugins
  only through their VST3 interfaces (factory, buses, processing, sample
  offsets, state, controller, latency restart); 5 process tests run the
  crashing and hanging faults in child processes and check how they ended.
- Not covered: loading the built module with the operating system's loader
  (that is the production host, slice 0 part 2), and the Windows build.
- Multi-output, sidechain and an editor window are added by the slices that
  need them (6 and 4); dynamic bus changes are not yet exercised.
- The old `vst3-test-module` stays for the scanner's isolation test until the
  production scanner replaces it.

## ADR-0066 — The plugin host crate and its protocol

**Status:** Accepted 2026-10-15 (M6 slice 0, part 2). It decides the host's
shape and its control path. It does **not** decide the audio path's wake-up
mechanism, the real-time wait contract or the process-grouping policy: those
wait for the Windows measurements (`docs/milestones/M6.md`, §2) and a later
ADR. · **Date:** 2026-10-15

**Context.** ADR-0005 chose separate-process hosting and left the details to
M6. The prototypes proved the idea on Windows with a handful of unshared
pieces. Production needs one crate that loads a module, runs a plugin and
speaks a protocol Lumen can trust even when the plugin has corrupted the host
process, and that can be tested on every CI system with the deterministic
plugins (ADR-0065).

**Options.** (1) Grow the prototypes. Windows-only, so untestable on two of
three CI systems. (2) A portable crate with the host side (module, instance,
server) and Lumen's side (a client with deadlines) over a transport that
works everywhere, with the real-time transport added behind the same
messages. (3) Wait for the measurements before writing any host code.
Chosen: (2). The measurements need a loader that can run real processing,
and (3) would leave them without one.

**Choice.** The crate `lumen-plugin-host` and its binary of the same name:
- `Module` loads a module with the operating system's loader (a `.vst3` file
  or a bundle's binary on Windows; a shared library or a bundle's `.so` on
  Linux), calls its entry point, takes its factory and lists its classes.
  Modules are never unloaded. `libloading` (ISC) is the loader.
- `Instance` runs one audio class with its controller: bus layout (the main
  buses only, mono or stereo, refused when the plugin's real layout differs
  from the request), setup, activation, blocks with events and
  sample-offset parameter points, state, parameters, latency and tail. Output
  that is not finite becomes silence and is counted.
- `protocol` is the message set: a frame is a marker, a version, a kind and a
  length. Every count and length is bounded before anything is reserved, a
  block's events and points must fall inside it, and a frame of another
  version is refused. A request gets exactly one response.
- A block carries a **configuration generation** and a **sequence number**,
  and its answer echoes both. The host refuses a block of an older
  generation. The client drops an answer for a block it has given up on and
  counts it; it is never taken for the block being waited for.
- `client::HostProcess` starts a host and never waits without a deadline:
  writes go through a thread of their own, reads arrive on a channel. It
  reports how a host that stopped ended (a clean exit, a failure code, a
  crash by signal or exception, or killed by Lumen after a missed
  deadline), and never leaves the process behind when dropped.
- The host process keeps the protocol on the real stdout and points the
  plugin's own at stderr; on Linux it asks to be killed with its parent, so a
  host hung in a plugin does not outlive Lumen. On Windows it suppresses the
  crash dialog.
- The transport is a pair of pipes. It carries the control path and proves
  the protocol. It is not the audio path.

**Consequences.**
- Verified on Linux: 14 unit tests (the codec and the protocol's limits), 9
  in-process tests of the test plugins through the OS loader (the module,
  sample-exact notes, block-size invariance, latency, state, a refused bus
  layout), 5 tests of a real host process, 12 fault tests (a crash and a hang
  in processing, a late answer dropped as stale, a plugin error, non-finite
  output, bad buses, millions of parameters, state that fails, hangs or
  cannot be read back, a factory that returns nothing, crashes or hangs), 6
  protocol-script tests (an older generation refused, garbage ends the host
  with a code) and one that kills a stand-in for Lumen and checks that a
  hung host dies with it (it fails without the parent-death request).
  Type-checks and passes clippy for Windows (GNU) and macOS; **not run** on
  either.
- Not done, and not claimed: the shared-memory block transport and its
  signalling; the measurements; process grouping; a Windows job object (so on
  Windows a host hung in a plugin outlives a killed Lumen); macOS module
  entry (it needs a bundle reference); the editor; the scanner; the real-time
  contract for waiting in the audio callback. The host allocates for each
  block; that is fixed with the shared-memory transport.
- The control path's deadlines are the caller's: slice 5 decides what the
  save and the checkpoints give state capture.


## ADR-0067 — The shared-memory audio path and the measurement harness

**Status:** Accepted 2026-10-15 (M6 slice 0, part 3) for the transport. It
does **not** decide the wake-up mechanism, the real-time wait contract or the
process-grouping policy: those wait for MT-VST-0 on Windows, and a later ADR
records them with the numbers. · **Date:** 2026-10-15

**Context.** ADR-0066 built the host over pipes and left the audio path for
the measurements. Blocks cannot cross pipes in real time: every block is a
copy through the kernel, an allocation on the host and a wake-up on each
side. The risks the plan names (a late host writing into reused memory, an
operating-system wait in the audio callback, waking a process in time) are
properties of the transport, so the transport has to exist before they can be
measured, and the harness has to exist before anyone can say which wake-up
to use.

**Options.**
1. Two buffer sets by block parity (the prototype). A late host can write into
   the set the next-but-one block is using, so every read needs a seqlock.
2. One set of buffers and **one block in flight per host**. Lumen submits
   nothing to a host that is still on an earlier block.
3. A ring of blocks. Absorbs jitter, but adds latency Lumen cannot take
   (CLAUDE.md §4: no buffering beyond what the driver requires).

Chosen: (2). A late or hung host can never write into memory that has moved
on, because there is nothing to move on to: its answer is recognised by
sequence number when it arrives, dropped and counted, and the very next
`submit` finds the buffers free. The price is that a host that misses one
block contributes silence until it has finished that block, which is what
Lumen would do anyway. It is the same rule that makes a late answer safe.

**Choice.** `lumen_plugin_host::shm`:
- A `Region` is one named block of shared memory, made by Lumen and opened by
  name in the host: a header of control words (each on its own cache line), the
  request's counts, the answer's record, then the input audio, events,
  parameter queues and points, and the output audio, sized by a `Layout` (the
  capacity: frames, channels, events, queues, points; all inside the
  protocol's limits). Control words and counts are atomics; payloads are
  raw copies between a release store and an acquire load. A new capacity is a
  new region, made off the audio thread and attached with
  `Request::Attach` (protocol version 2); a second attach replaces the first.
- **Everything read from the region is untrusted.** The host checks the frame
  count, every count against the layout, event offsets and kinds, point
  offsets and values (finite) and that the points add up, before a plugin sees
  anything; a bad block is answered `Invalid`. Lumen copies output out and
  replaces anything not finite with silence, and an answer whose sequence or
  generation is not the block's becomes `Invalid`. A plugin that scribbles on
  the region can spoil its own audio and nothing else.
- Each block carries the configuration **generation**; the host answers a
  block of an older one `Stale` without processing it, and a block that does
  not fit the format `Invalid`, so a reconfiguration cannot let an old block
  reach the plugin.
- **Wake-ups are two independent settings** (`Wake { host, lumen }`), each
  `Spin` (poll memory, never ask the operating system to wait), `Event`
  (an operating-system signal) or `Hybrid(duration)` (poll, then signal). The
  side that is polled makes no signal call. This gives the experiments the plan
  lists, including "worker-side signalling with bounded polling" in the
  callback (`lumen: Spin`), without a second transport. The signals are named
  semaphores on Unix and auto-reset events with a high-resolution timer on
  Windows (a plain millisecond wait ends on the system tick, too coarse for a
  deadline of a few milliseconds). On Unix the names are removed as soon as
  both sides have opened them, so a crash leaves nothing behind.
- Lumen's side, `BlockLink`, **allocates nothing and takes no lock**:
  `submit`, `poll`, `wait` (bounded by the caller's deadline) and `copy_output`.
  The test runs them in a real-time section under the allocation guard.
  `wait` with an `Event` or `Hybrid` setting does ask the operating system to
  wait; that is the open question below, and nothing in this change lets the
  audio callback call it.
- The host's block loop is its own thread (`blocks.rs`), at the Pro Audio
  scheduling class on Windows. It shares the plugin with the control thread
  through one lock, so a configure or a state call waits for the block in
  progress and the reverse. A panic on it aborts the process, so Lumen sees a
  crash rather than a host that answers its pipe and never its blocks.
  `Instance::process_into` renders into buffers the caller owns, with the
  channel pointers on the stack, so a block allocates nothing once the event
  and parameter lists have grown to the busiest block; the pipe path keeps its
  allocating `process`.
- On Windows every host is assigned to a kill-on-close job object held by
  Lumen for its whole life, so a host hung in a plugin cannot outlive a killed
  Lumen (P-ISO finding 4). A failed assignment fails the spawn.
- `lumen-host-bench` is the measurement harness: real host processes, the
  production link, paced blocks, under the allocation guard. It reports block
  time (submit to every answer, copies included) as p50, p99, p99.9 and
  max, deadline misses, busy skips, stale answers, failed blocks, hosts that
  disagree (they play identical streams), heap operations in the loop and the
  hosts' CPU while idle, and can inject a crash or hang in one host and report
  how soon it is seen and what the others did. Plugin load per block uses the
  test plugin's `process_slow` fault. MT-VST-0 is the script for running it.

**Still open, and why it cannot be closed here.**
- *Which wake-up.* The harness exists and runs on Linux, where it shows the
  mechanics work (below); the numbers that decide the policy are Windows
  numbers at 64/128/512 frames, 1/8/16 hosts, with and without contention, in
  the Balanced and High-performance power plans. A sandbox with four shared
  cores says nothing about them and none are claimed.
- *May the callback wait on the operating system.* ADR-0005 proposed it; the
  architecture rule says the audio thread never blocks. Either bounded polling
  in the callback with the signalling on the host's side meets the budget
  (then no exemption is needed) or it does not (then an ADR must state the
  exemption, its bound and what is done about it). The default policy that
  ships must be the one the numbers support.
- *Process per instance or per chain.* One process per instance is what the
  harness measures (N hosts); a grouped design needs a second link per host
  and is considered only if the numbers force it.

**Consequences.**
- Verified on Linux: 84 tests pass in the crate (47 before). New: 9 region
  tests (round trip; a block replacing the last without leftovers; every
  rejection of a request that does not fit; a request that lies about
  its counts; scribbled events and points; a region of another layout; output
  scrubbed on the way back), 8 link tests with a stand-in host in five wake
  modes (including a late answer dropped and never taken for the next block,
  a host that never answers, an answer for another generation), 15 tests
  against real host processes (the same sound as the pipe path in four wake
  modes, an effect with input, a stale generation, not ready, a layout that
  does not match the format, non-finite output, a plugin error, a slow block,
  a crash and a hang in processing, re-attaching, six hosts side by side, no
  orphan with the link attached, a clean quit), the allocation test, the
  attach request's round trip and refusals, and the harness's own parsing and
  percentile tests. Mutation checks: ignoring the generation, or letting a
  busy link accept a block, each fail the tests they should.
- The Windows code (file mapping, events and timers, the job object, MMCSS)
  type-checks and passes clippy for `x86_64-pc-windows-gnu`, as does macOS
  for `aarch64-apple-darwin`; **neither has been run.** macOS has no timed
  semaphore wait, so there the wait polls with short sleeps: it runs the
  tests and is not a measurement.
- Not done, and not claimed: any measurement on Windows; MMCSS taking effect;
  a third-party plugin through the link; the editor; the scanner; wiring the
  link into the engine's graph (nothing outside this crate uses it); rebuilding
  the graph when a plugin reports a latency or bus change (the answer carries
  `latency` and `restart`; acting on them is slice 6); the supervisor that
  restarts a failed host and swaps its link in off the audio thread (slice 2).
- Cost of the design: each active host holds a region (about 1 MiB at the
  largest layout, 8192 frames of 16 channels both ways, and a few kilobytes
  at a typical one; untouched pages cost nothing) and, in `Spin` or a
  `Hybrid`'s polling, a core while waiting. The harness reports the idle cost so the choice is made with it.
