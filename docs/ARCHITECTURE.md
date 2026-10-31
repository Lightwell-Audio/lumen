# Lumen — Architecture

**M9 R6 Playing audio from disk (2026-10-28, ADR-0109, format unchanged at
v32).** `lumen-engine::stream`: `PagePool` (fixed pages, the audio thread's
grace-period count), `StreamAudio` (a file's page table; `ClipAudio::Stream`),
`PoolWriter` (the one writer), `OfflineLoader` (a render's own pool, filled
on the render thread) and `stream_plan` (`Scene`: the pages wanted, nearest
first, and whether a span is ready). `lumen-app::disk`: the disk thread
(`Disk`, `Cuer`, `Stats`; `Pages` reads a page) and its tests, with the
automated soak in `disk/soak.rs`. `media/streaming.rs`: the live pool, the
disk thread and the cache while "Play audio from disk" is on, and the 30 s
rule; `media/streams.rs`: stream ids, never reused, and the blocking
`Readers`, `read_all` and `offline` every other reader uses, `Unread` (what a
render could not read; its worker fails the job) and `in_memory` (a real-time
export's streamed files, read whole before the callback plays them).
`transport/tests.rs`: what happens to a held start when another command comes.
`app/streaming.rs`: the switch, the disk thread's scene and what the
transport shows; `transport/hold.rs`: the 150 ms wait before a start (L A);
`transport/status.rs`: the Disk row and the "Disk late" chip.
`arrangement/samples.rs` reads deep-zoom strips on a worker.

**M9 R5 Export (2026-10-26, ADR-0107, format unchanged at v32).**
`app/audio_export.rs` owns the Export window's choices, plugin-state wait,
snapshot and polling; `app/audio_export/window.rs` uses the existing controls.
`export.rs` and `export/worker.rs` render, encode, measure and publish a job;
`export/publish.rs` stages an entire set and restores existing files if a
publish fails. `export_render.rs` builds the full mixer, clip and MIDI graph
with separate plugin hosts, preserving upstream and sidechain dependencies
for stems. `export_live.rs` drives that same graph in the actual audio-device
callback, handing stereo frames to the disk worker through an SPSC ring.
Prepared sources and their host managers return through a retirement queue;
the callback never destroys them. `lumen-media::export` writes integer/float
WAV and frame-by-frame FLAC, with conversion, normalization and dither.
`lumen-engine::Output::planar` is a checked allocation-free output adapter;
`render_source` accepts dynamically sized sources. Export does not change
the project or its undo history.

Plugin reply waits (ADR-0103, 2026-10-25): on macOS the callback polls until
its existing absolute deadline while each plugin worker blocks on its
request semaphore when idle. Other platforms retain event/event waits.
This avoids repeated sleep polling in Darwin's callback-side semaphore
fallback; it trades CPU during an active wait for response latency.

**Built so far:** the current slice, the chain of what each earlier slice
added, and its ADR, are in `docs/PROGRESS.md` (milestone table and reports) and
`CLAUDE.md` §12; the reasoning and evidence behind each is in
`docs/DECISIONS.md`. This page documents how the parts fit now, by crate and
module, not the order they arrived in — a few recent slices (noted by name
below) are still written up as their own module list because that detail is
not yet folded into the tables.

Recording-window correction (2026-10-14, ADR-0064): `app/record.rs` keeps
capture running past punch-out; `recorder.rs` snapshots full timing;
`edit/record.rs` places hidden captured content behind the clip window.
`midi_runs.rs` caches bounded Replace suppression, including loop repeats,
and `midi/capture.rs` distinguishes first loop entry from a real wrap.

## Processes and threads

E2 module additions (crossfade as a true overlap, ADR-0094): app
`crossfades.rs` works out each crossing (the zone, its limits from the hidden
audio) and the played clips: the left clip extended with a blend fade-out, the
right clip's hidden head restored with a blend fade-in; arrangement previews
and `lanes::engine_clips` share it. `document/crossfades.rs` owns the undoable
edits (`crossfade_across`, `set_crossfade_length`, `set_crossfade`, the project
setting). `lumen-project/src/edit/crossfade.rs` is the one rule run by the
history after every edit: a clip's crossfade flag stands only while an audio
clip ends at its start. The engine has no join machinery: overlapping clips
mix additively, and `FadeFrames.blend` gives an equal-power half-crossfade
without the edge fade at that end. All discovery, allocation and project edits
remain off the audio thread. The model's `Clip.crossfade` and
`Project.auto_crossfade` are stored in format v28 with a migration that
preserves older playback.

```text
┌──────────────────────────── lumen (main process) ─────────────────────────────┐
│                                                                               │
│  UI thread ── egui/eframe (wgpu) ── Document: History of shared snapshots     │
│     │  every edit is one undo step (ADR-0024)                                 │
│     │                                                                         │
│     ├── background threads: media load/convert · project save/open ·          │
│     │   autosave · file dialogs · ASIO driver checks (child process)          │
│     │                                                                         │
│     ├── MIDI worker (lumen-midi): lists and opens ports, hot-plug             │
│     └── MIDI sender (starts with the first track that names a port): times    │
│         the engine's hardware events against the audio clock (ADR-0045)       │
│     ▼  commands, timelines, clip lists, MIDI track lists (lock-free queues;   │
│        retired data comes back to be freed off the audio thread)              │
│  audio control thread (supervisor): open/close/reset devices, recovery,       │
│     run-time driver switching (hands the engine to a new thread)              │
│     ▼                                                                         │
│  audio driver thread (RT): ASIO / WASAPI / CoreAudio / ALSA callback          │
│     runs the engine directly, no extra buffering                              │
│     (M6: per block, signal plugin hosts and wait until the deadline)          │
└───────────────────────────────────────────────────────────────────────────────┘
   lumen --probe-asio <clsid>: short-lived helper that loads one ASIO driver
   (M6) lumen-plugin-host processes and scanning processes (prototyped in M1)
```

| Concern | Design | Decisions |
|---|---|---|
| UI | egui host for chrome, widgets, text and accessibility; `lumen-render` (wgpu + Vello) for dense content (the arrangement grid now, clips and waveforms from M3 slice 4) | ADR-0002 |
| Design system | Semantic colour tokens (dark and light), type, spacing and motion, in `theme.rs`, `motion.rs` and `fonts.rs`; spec in `docs/DESIGN.md` | ADR-0023 |
| Real-time engine | Allocation-free, block-size-invariant; transport, tempo and meter maps, declick crossfades, clip player, MIDI player and the built-in instruments, the mixer graph (a plan compiled on the UI side), metronome | ADR-0003, 0017, 0019, 0020, 0035, 0045 |
| Audio devices | Lumen's own backend layer: native ASIO over COM, WASAPI shared and exclusive, cpal for macOS and Linux; a supervisor that recovers from loss, resets and sleep | ADR-0006, 0016, 0021 |
| Projects | Folder with versioned JSON (format v26; loop source phase and partial fade windows; automation and hidden captured MIDI provenance retained), migrations over untyped JSON, atomic saves, rolling backups, relative media paths | ADR-0004, 0064 |
| Undo | Shared snapshots: tracks and clips in `Arc`, every step a cheap copy | ADR-0024 |
| Time | Integer ticks at 1,411,200 PPQ, with one rounding rule to samples; MIDI searches sample-expanded tick bounds and assigns events by their exact sample, including at a block's start | ADR-0009, 0062 follow-up |
| Plugins (M6) | `vst3` crate bindings, sandboxed host processes (`lumen-plugin-host`: module loading, a bounded versioned protocol, stale-block rejection, a shared-memory audio path), out-of-process scanning (a child per module with a deadline, a cache keyed by file stamp, a blocked list with reasons: `lumen-plugin-catalog`) | ADR-0005, 0066, 0067, 0068, 0071 |
| Real-time safety | Allocation guard in tests; RealtimeSanitizer in Linux CI with live canaries | ADR-0010 |
| Performance | divan wall-clock benchmarks, plus instruction-count gates under Callgrind in CI (+2%) | ADR-0011, 0022 |

## Crates

| Crate | Role |
|---|---|
| `lumen-core` | Product identity (the name, defined once), time base, the MIDI message (`midi`: up to three bytes, and an event at a frame; shared by the engine, the devices and the app, ADR-0045), tempo and meter maps, the editing grid (`grid`: steps, snapping, lines), curves for fades and envelopes (`curve`), automation lanes (`automation`: points, segment shapes, `value_at` and `simplify`, ADR-0081), fades (`fade`), pan laws (`pan`) and loudness (`loudness`: K-weighting, 100 ms blocks, gated LUFS and 4× true peak, BS.1770-4, ADR-0040) |
| `lumen-rt` | Real-time safety tooling: allocation guard and `rt_section` (RTSan hooks) |
| `lumen-audio` | Device layer: `Backend` and `Stream` traits; ASIO, WASAPI and cpal backends; `AudioSystem` supervisor; `Unavailable` (a driver that could not start, with the reason); a fake backend for tests |
| `lumen-devices` | Built-in devices (ADR-0045): real-time-safe DSP on `lumen-core` only, which the engine runs and the app builds. The `Instrument` trait and the polyphonic synth (`synth/`: `mod.rs` the presets and the `Synth`, `voice.rs` one voice's DSP; 16 voices, polyBLEP oscillators, a state-variable low-pass, ADSR, five presets); plugin hosts (`hosted/` (`guard.rs` the Guard, `instrument.rs` the hosted instrument), ADR-0070: the `Hosted` contract the engine uses without knowing the host, `Guard` with its rules for a late or failed host and `Status` the interface reads, and `HostedInstrument`); effects join in M8; `hosted/params.rs` the `ParamBlock`: the parameter changes of one block, fixed room, filled on the audio thread (M7 slice A3, ADR-0084) |
| `lumen-engine` | Engine on the audio thread: transport (`snapshot.rs`: the transport state, recording and the count-in included, published to the UI as a seqlock over atomics), timeline, the mixer graph (`mixer/`: `spec.rs` the routing types (nodes, sends, sidechains and their taps, devices); `compile.rs` checks and orders a routing and works out delay compensation; `plan.rs` runs the compiled plan with a bus per node; `delay.rs` the delay lines that line paths up, ADR-0041; `hosts.rs` the pool of attached plugin hosts, the hosted-device runner (host output, fallback and the glide between) and the early hand-over of a node's block, ADR-0070; `auto.rs` automation lanes baked to samples and the stages that play them, ADR-0081; `params.rs` what a plugin parameter's lane says in one block (points on their samples, curves by knots), ADR-0084; `smooth.rs`: gliding levels; `meter.rs`: every node's peak, RMS and clip in a lock-free bank, and the measured node's loudness and true peak, ADR-0040; `mod.rs`: plan swaps and crossfades, ADR-0035), clip player (plays each clip into its track's bus, with the clip's gain and pan envelope baked on the clip's own clock, ADR-0090), MIDI (`midi/`, ADR-0045: `mod.rs` the clip, track and message types; `hold.rs` the note tracker, reading clips a block at a time, chase and releases; `player.rs` the track-list swap and the player, `index.rs` what the chase searches instead of reading every note (the latest note end by groups of 64, each controller's events; M6 review M7), `tracks.rs` one track's part of a block (gathering, filling a plugin instrument's parameter lanes, handing a hosted instrument its block early, rendering into the track's bus and sending hardware events out); `live.rs` live input, `Listen` and the live note tracker, ADR-0046, with the piano roll's audition as its own source, ADR-0047; `recording.rs` the engine-side recording window and compensated position, `capture.rs` recording: a shared position mapping and a track set to record keeps what it takes, with the timeline sample it was heard at, for the app to commit (ADR-0055); `clock.rs` MIDI clock out: 24 clocks a quarter on their samples, Start, Stop, the song position and Continue, for the ports `MidiHandle::set_sync` names (ADR-0062); `Layered` plays it beside the clip player), metronome (with the count-in, ADR-0055) and test tone, the loopback helper's player and analysis (`loopback.rs`: five clicks out of one output while everything else is silenced, the input kept beside them, and the cross-correlation that finds the round trip, ADR-0110; it runs after the engine in the device callback), declick, scrubbing (`scrub.rs`: loop and tape styles, while stopped), the playhead while rendering ahead of it (`playhead.rs`, ADR-0041); `testing.rs` (the routing tests, benchmarks and the performance gate share); `EngineHandle` and `MixerHandle` on the UI side |
| `lumen-midi` | MIDI device layer (ADR-0044): `Backend` trait, the midir backend (WinMM, CoreMIDI, ALSA), a fake for tests, and `MidiSystem`, a worker thread that lists ports, notices hot-plug, opens the ports the user switched on and carries timestamped messages in a queue; `activity.rs` counts messages per port for the lights, and a waker tells the UI (ADR-0046) |
| `lumen-test-plugin` | Deterministic VST3 test plugins (M6, ADR-0065): an instrument and an effect that sound identically every time, with faults switched on by `LUMEN_TEST_PLUGIN_FAULT` (`faults.rs`); `render.rs` the exact-sample DSP, `component.rs` and `controller.rs` the VST3 interfaces, `factory.rs` the class factory. A `cdylib` for hosts and an `rlib` for in-process tests; stands alone, used by tests only |
| `lumen-plugin-host` | The plugin host (M6, ADR-0066, ADR-0067): runs in its own process, one plugin per host. `module.rs` loads a VST3 module with the OS loader; `instance/` (`mod.rs`, `setup.rs` buses and activation, `controls.rs` the controller's side, `events.rs`) runs one audio class and its controller (buses, blocks, state, parameters, latency; `process_into` renders into buffers the caller owns, allocating nothing); `protocol/` (`types.rs`, `codec.rs`, `frame.rs`) and `wire.rs` the versioned, bounded messages (a block carries a configuration generation and a sequence number); `server.rs` answers requests and starts the audio thread on `Attach` (`serve_pumped`: requests are read on a thread of their own and the main thread, which made the plugin, runs them and pumps the editor window; a window that stops answering is reported `Unresponsive`); `editor.rs` the plugin's window (`Editors`: `editor/win.rs` on Windows, a stand-in for tests, none elsewhere); `blocks.rs` is that thread (waits for blocks, processes them, shares the plugin with the control thread through one lock); `client.rs` is Lumen's side (`HostProcess`: deadlines on everything, stale answers dropped, how a stopped host ended, `attach` for the audio path, a kill-on-close job on Windows); `process.rs` start-up (protocol on the real stdout, die with the parent, no crash dialog); `com/` (`host.rs`, `handler.rs`, `events.rs`, `stream.rs`) the objects a host gives a plugin. `shm/` is the audio path: `region.rs` the shared block and the only code that reads or writes it (every count checked, output scrubbed), `client.rs` Lumen's `BlockLink` (no allocation, no lock), `host.rs` the host's end, `sys/` the operating system (mapping, wake-up signals, scheduling class, job object); `Wake` sets how each side waits (spin, event, hybrid). The binary `lumen-plugin-host` is the host; `lumen-host-bench` is the measurement harness (MT-VST-0). Pipes carry the control path, shared memory the blocks. Lumen's own process loads no plugin code. The `scan` mode of the binary (`scan/`: `report.rs` the bounded, versioned reply, `host.rs` the child's work, `run.rs` Lumen's side: one child per module, a deadline, cancel, every way it can end named) is used by `lumen-plugin-catalog`; the rest is not used outside this crate yet. `bin/lumen-host-probe/` (M6 slice 7a: `stages.rs` the ten stages run against one class in a host process of its own, `report.rs` verdicts and their words, JSON line and table row, `main.rs` options and the scan) runs installed plugins through the stages a host needs and writes the compatibility table's rows |
| `lumen-hosting` | Lumen's side of plugin hosting (M6 slice 2c, ADR-0071), with no UI and no project types; depends on `lumen-plugin-host`, `lumen-devices` and `lumen-core`. `link.rs` is `Link`, a `Hosted` over the shared-memory `BlockLink` for the audio thread (no allocation, no lock, no system call; notes only, the rest counted as dropped); `process/` (`handle.rs` the handle, `run.rs` the thread, `command.rs`, `gate.rs`) is one device's host and the thread that owns it (start, load, restore state, configure, attach, each with a deadline; then a look at the process every 10 ms that sets the flag the callback reads; a fresh link on request); `manager.rs` is the set of devices (`want`, `reload`, `relink`, `relink_with` (with the auxiliary buses on: sidechain input, extra outputs, M6 slice 6, ADR-0078), `forget`, `poll`, `shutdown`, `open_editor`, `close_editor`, `end`; four loads at a time; messages from a replaced start are dropped). The app's `hosting.rs` is the bridge to the project, the catalog and the engine's handles; the link writes a block's `ParamBlock` into the region whole queue by whole queue (ADR-0084) |
| `lumen-plugin-catalog` | Finding, scanning, caching and listing plugins (M6 slice 1, ADR-0068), with no COM and no UI; depends on `lumen-plugin-host` for the scan reply and runner. `discover.rs` (standard and user folders, bundle and bare-library layouts, links followed with a loop guard, a depth limit), `scan.rs` (the plan from file stamps, a pool of scan processes, progress as events, cancel), `cache.rs` (versioned JSON, a blocked module is an entry with its reason; unreadable or newer caches are discarded, never fatal), `blocklist.rs` (the words for a blocked plugin), `catalog.rs` (cache plus Lumen's own devices; kind, vendor, categories; search), `stamp.rs`. The app owns the thread, the disk and the interface |
| `lumen-media` | Audio files: decoding (symphonia), sample-rate conversion (rubato), waveform peaks (ADR-0027); `stream/` (ADR-0109): positioned reads of WAV, RF64 and AIFF frames (`Reader`), block conversion bit-identical to a whole-file load, the cache of converted copies and their peaks (`cache.rs`: key, trim to a limit, reversed copies), peaks scanned from a stream file; `read_blocks` and the long WAV writer that becomes RF64 past 4 GiB |
| `lumen-project` | Document model (`model/`: `mod.rs` the project, its view and its time signature; `track.rs` tracks, outputs, sends and MIDI routing; `device.rs` devices and what plugins report; `clip.rs` clips, notes and media; all re-exported from `model`), `.lumen` format and migrations, `automation` (what a track's lanes drive and the range of each target, ADR-0081), `envelope` (an audio clip's gain and pan envelopes, measured from its start, and cutting one to the clip's window, ADR-0090; `edit/carry.rs`: the track's automation under a clip going with it when it moves, is copied or deleted), atomic save and backups, media locating, `History` (undo), `routing` (where a track's audio goes, and loops: one rule for validation, edits and the engine, ADR-0036), `groups` (a group's tracks follow it in the list: spans, nesting, order, ADR-0037), `solo` (who is heard: mute and solo across groups, returns and the master, the owner's rules, ADR-0038), `edit` (clip and time-range edits: cut, delete and insert time, trim, slip, and the clip overlap rule; `edit/clipboard.rs`: copy and paste; `edit/automation.rs`: pure edits of a lane's points, move, shape, replace, copy, paste, duplicate, drawn shapes and freehand strokes, ADR-0083; `edit/record.rs`: placing a recorded take as Replace or Overdub, with loops and punch, ADR-0055; `edit/seams.rs`: notes that carry on across touching clips, ADR-0058), `smf` (Standard MIDI Files: `read.rs` type 0 and 1 into parts split by channel in Lumen's ticks, `write.rs` type 0 or 1 at 960 PPQ; pure, through `midly`, ADR-0062), `write_atomic` (any file by the save's commit protocol) |
| `lumen-render` | Toolkit-independent renderer: GPU-resident instanced layers (rectangles; boxes with a per-frame row table, ADR-0026; waveforms drawn per pixel from a peak atlas, ADR-0027) plus Vello; GPU pipeline cache |
| `lumen-perf` | Performance-gate scenarios and the `perf-gate` runner |
| `lumen-shader-check` | Build-time shader checks: build scripts validate every `.wgsl` file with naga and generate each shader's source and uniform size |
| `lumen-app` | The `lumen` application (modules below) |

`lumen-app` modules:

| Area | Modules |
|---|---|
| Projects | `document/` (`mod.rs`: the open project, its history and track edits; `automation.rs`: a track's lanes, made, replaced, switched and removed, ADR-0081; `overrides.rs`: the lanes the user has taken over by moving the control they drive, session state outside the history, ADR-0086; `instrument.rs`: a MIDI track's instrument and MIDI output, ADR-0045; `clips.rs`: clip edits, fades, gain and clip properties, and the track's automation going with a clip unless the song locks envelopes (ADR-0090); `mixer.rs`: outputs, sends, polarity, trim and the pan law; `groups.rs`: grouping, ungrouping and moving tracks between groups; `import.rs`: loaded files onto new tracks or the track they were dropped on; `record.rs`: a recording as one undo step, placing its takes, ADR-0055), `files.rs` (project folders, save, open, recent), `autosave.rs` (autosave and recovery) |
| Arrangement | `arrangement/` (`mod.rs`: the view state, constants and colours; `rows.rs`: rows and zones, heights, scrolling, revealing a track; `tracks.rs`: track selection and the commands on selected tracks; `headers.rs`: the header column and the empty area; `header_row.rs`: one track's header; `menus.rs`: the track and add-track menus; `nesting.rs`: how groups look (indents and rails, lane bars, a group's arm state); `clips.rs`: lanes and clips; `selection_look.rs`: how the clip selection, hover, time range and marquee are drawn; `recording_look.rs`: clips being recorded, in record red with the REC tag (ADR-0055); `auto_view.rs`: the automation lane over a track's clips, its chip and chooser, the point edits and the frame that maps time and value to the screen, and `auto_scale.rs`: where a value sits in a lane, how it reads and where it rests (M7 slice A1, ADR-0082); `auto_state.rs`: which lane each track shows, the picked points, the tool and the gesture under way, the plugin parameter chooser's state and the plugins' names for parameters, `auto_params.rs`: which parameters of a plugin a lane can drive and the search over them (M7 slice A3, ADR-0084), and `auto_tools.rs`: the tool strip, the box, the bend dot, the live value (M7 slice A2, ADR-0083) and the Re-enable button; `follow.rs`: the value a control shows when its lane plays, and the amber ring of a lane taken over (M7 slice A4a, ADR-0086); `select.rs`: hit-testing and drag gestures; `handles.rs`: fade and gain handles, where they are and how they are drawn; `waves.rs`: which audio each clip's waveform shows; `samples.rs`: samples for deep zoom; `reorder.rs`: dragging tracks; `drop.rs`: where dropped files go, a track or between tracks, and how that shows; `pinned.rs`: returns and the master pinned at the bottom, the zones, divider and caption, the master's header and Solo/Cue switch; `sends.rs`: the send well of slots on headers and a return's senders (ADR-0101); `buttons.rs`: M and S, shared by headers (well pieces) and strips (raised); `levels.rs`: the level strip beside a header's rail and its clip mark (ADR-0101); `strips.rs`: the Mixer tab, its loudness column and menu; `strip.rs`: one channel strip and its sections; `strip_output.rs`: a strip's output button and menu, ADR-0040), `meters.rs` (meter ballistics and the master's loudness, moved on each frame), `timeview.rs` (the horizontal view, f64), `lanes.rs` (the clip library, the engine's clip list, and sending the routing after it), `lane_bake.rs` (lanes on the device's sample clock through the tempo map, for the routing and the MIDI list, ADR-0081, ADR-0084), `auto_record.rs` (recording a lane: a track's mode and the pure state machine that turns a frame's moves into passes, ADR-0087), `routing.rs` (the engine's routing from the project and the cue, with each track's lanes baked to the device's samples through the tempo map, ADR-0081: a node per track, a master, the cue bus, the device's output pairs where the stream opened them), `media.rs` (loading audio in the background), `ruler.rs`, `position.rs`, `canvas.rs` (GPU grid and clip bodies) |
| Transport and audio | `transport/` (`mod.rs`: what drives the engine: play, stop, locate, scrub, Follow, the timeline it plays; `bar.rs`: the bar's controls, tempo, the Tap piece, time signature, position field, the loop and punch menu; `status.rs`: CPU, the dropout chip, the device readout and its panel, and carrying out what the Audio page asks of the device; `compensation.rs`: the delay compensation readout, the owner's D1; `record.rs`: the Record button's phase, the count-in readout and its commands (ADR-0055); `dropouts.rs`: dropouts as episodes and a last-minute tally), `playhead.rs` (the smooth playhead drawn and followed while playing), `audio.rs` (device choice and status), `preferences/` (the Preferences window's state, its pages and the rows search finds, M9 R7: `mod.rs` the pages, `ROWS`, search, the fit and the outline a result leaves; `audio.rs` the Audio page; `midi.rs` the MIDI page), `app/preferences/` (the app menu and settings saving in `mod.rs`; `window.rs` the window, its list, search and page menu; `pages.rs` the pages that change the app's own settings; `files.rs` the Files page's folders and cache rows), `app/cache_folder.rs` (the cache's folder, its count, limit and Clear on a worker, and the folder left behind, M9 R7), `tap_tempo.rs` and `app/tap_tempo.rs` (tap tempo: the pure model of a run, and the command and run in the app, ADR-0112), `measure_latency/` (the loopback helper, ADR-0110: `mod.rs` one measurement from the clicks going out to the offset offered, its analysis on a thread of its own; `card.rs` its card in the Latency section), `probe.rs` (ASIO helper process) |
| Design system | `theme.rs`, `motion.rs`, `fonts.rs`, `panels.rs` (sliding panels), `icons.rs` (vector icons, track-type icons, close button), `widgets/` (`dialog.rs`: dialogs and their title row; `form.rs`: the label column, sections, hints and warnings of settings pages; `slider.rs`: the value slider and value field; `format.rs`: how values read as text; `mod.rs`: the fold arrow, gradients, glow line; `keys.rs`: shortcuts as K5 keycaps, menu rows and tooltips drawn from the keymap; `knob.rs`: the comet send knob, ADR-0038, and the header's send slot and its flag, ADR-0101; `fader.rs`: the channel fader; `meter.rs`: a strip's stereo meter and scale, a level's zones; `wheel.rs`: the latching wheel on value controls, ADR-0040; `toggle.rs`: the switch, the segmented control, choice rows and menu captions, M4.5; `menu.rs`: every menu, closing gently and fading, M4.5; `empty.rs`: empty states; `signature.rs`: the time signature field; `scroll_bar.rs`: the fading scroll bar; `fade_out.rs`: closed windows fading; `mark.rs`: the Dawn mark, a meter of the master, ADR-0043), `light.rs` (the five levels of light and their helpers, ADR-0043), `brand.rs` (the window icon drawn as pixels from the mark), `owner.rs` (dialogs owned by the main window; the system's cursor during a file drag) |
| App | A library with a thin binary (`main.rs` calls `lumen_app::run`), so tests in `tests/` start the real app. `lib.rs` (the `App` state and the frame loop); `app/startup.rs` (start-up, `App::new`); `launch.rs` (what start-up needs, from the command line or a test); `testing/` (the small public API the end-to-end tests use, ADR-0029, by area: `mod.rs` the general seams, `automation.rs`, `devices.rs`, `midi.rs` and `editor.rs`; the tests themselves are in `tests/e2e/` and `tests/smoke.rs`); `app/` holds its behaviour by concern: `projects.rs` (new, open, save, the unsaved-changes prompt, Import audio, file dialogs, answered by tests instead of shown), `recovery.rs` (autosave and the recovery banner), `history.rs` (undo, redo, History window, toasts), `preferences.rs` (app menu, appearance, settings saves), `view.rs` (transport bar, arrangement composition, ruler, playhead, device banner), `navigate.rs` (zoom, fit, back, Follow), `grid.rs` (snapping, grid keys, menu and button), `rulers.rs` (the locator strip, bar ruler and time ruler, layout A), `regions.rs` (the loop brace and punch), `locators.rs` (locator flags, Set ◂ ▸, their keys), `scrub.rs` (the scrub strip), `editing.rs` (clip selection, the arrow keys between clips, drags and editing commands), `handles.rs` (dragging fade and gain handles), `clip_envelopes.rs` (showing a clip's envelope, ADR-0090), `clip_card.rs` (clip mute, lock, rename and colour, and the pinned properties card), `info.rs` (the info line: what is under the pointer and what can be done there), `cue.rs` (the cue as the engine plays it, the master's switch and cue level lent to the headers), `mixer.rs` (meters moved on each frame, and the device's output pairs opened and lent to the strips), `test_modes.rs` (the command-line test modes around each frame: save as, autoplay, trace, screenshot, smoke test), `clipboard_keys.rs` (puts back the Copy, Cut and Paste key presses egui-winit turns into clipboard events, outside text fields); `auto_editing.rs` (the pointer on automation lanes: what a press, click and hover do for the tool in hand, M7 slices A1 and A2), `auto_gestures.rs` (the gestures: move, box, pencil, shape and bend, each rebuilt from the lane at the press, and the resting places) and `auto_params.rs` (the plugin parameter chooser dialog and the names plugins give lane parameters, ADR-0084), `auto_recording.rs` (recording a lane, the app's side: the per-frame step after the interface is drawn, writing a pass into its lane in parts every five seconds and as one undo step, forgetting the passes when another project opens, ADR-0091, the plugin gestures that are open; M7 slice A4b, ADR-0087), `auto_keys.rs` (the keys and commands on a lane: the arrows, delete, copy, paste, duplicate, select all, segment shapes and tools; ADR-0082, ADR-0083); `song_lanes.rs` (the pointer, keys and typing on the tempo and meter lanes, M7 slice A6, ADR-0089; the lanes themselves are the top-level `song_lanes/`: `mod.rs` state, `scale.rs` where tempos and chips sit, pure and tested, `paint.rs` the look; the pure edits are `lumen_project::edit::maps` and the Document methods `document/song_maps.rs`); `timefmt.rs` (time formats: minutes and seconds, samples, timecode); `plugins/` (M6 slice 1: the scan thread's lifetime and the catalog the browser shows: `mod.rs` the source (system, off, or chosen by a test or flag) and status, `worker.rs` reads the cache, scans and saves it off the UI thread, `settings.rs` favorites, recents and folders in the settings file); `browser/` (the Devices tab: `view.rs` what is shown, `rows.rs` the flat list the catalog and filters make, pure and tested, `list.rs` paints only the rows in view, `controls.rs` the search field, type switch, chips and scan line, `panel.rs` composes the pane, applies what was done and runs the keys while the pane has the keyboard); `chain/` (M6 slice 2d, ADR-0072: the Devices tab: `place.rs` where a chosen device goes, pure and tested; `geometry.rs` the layout of chips, cards, cables and drop gaps; `art.rs` a card's picture from its identity; `card/` (`mod.rs`, `picture.rs`, `body.rs`) paints one card in its four faces; `view.rs` the tray, cables, chips and the drag's drop mark; `actions.rs` what a choice, a drop and the commands do, with moving, copying, pasting and folding (M6 slice 3, ADR-0073); `sidechain.rs` the effect's Sidechain row, its menu and the light (6c), `outputs.rs` the instrument's Outputs row, its menu and the question on a track that holds work (6d); `pictures.rs` the plugin's own picture, read and decoded by a worker thread within limits; `presets.rs` the card's preset field and menu, the sound each card has, a save waiting for its plugin and the names kept honest, M6 slice 5f, ADR-0077); `preset_files.rs` (where `.vstpreset` files live, listing, reading and writing them on a thread of its own, M6 slice 5f); `app/presets.rs` (loading a program or a preset as one undo step, 5c); `app/safe_mode.rs` (safe mode: the flag, Shift at start, the marker file and the offer, 5e) |
| Commands | `commands/` (`mod.rs`: every command once, with its id, name, area and default chords; `keys.rs`: chords matched exactly and the keymap, ADR-0033); `app/commands/` (`mod.rs`: the helpers and the one keyboard dispatcher; `availability.rs`: whether a command can run now and why not; `run.rs`: what each command does); `app/palette.rs` (the command palette: rows, fuzzy search, typed values, context labels, drawing); `app/shortcuts.rs` (recording chords for the window and the palette, the Keyboard shortcuts window and its keyboard map); `app/panes.rs` (F6 between panes, their focus anchors) |
| MIDI | `midi.rs` (the device layer with the user's switches), `preferences/midi.rs` (the MIDI page of Preferences), `midi_play.rs` (the engine's MIDI tracks from the project: notes, controllers, loops, instruments, targets, the port table; pure and tested), `midi_runs.rs` (touching clips as one engine clip, so notes carry on across seams, ADR-0058), `midi_out.rs` (MIDI to hardware: the stream clock, the frame-to-time map, the scheduler and the sender thread, which joins the Pro Audio class through `lumen_audio::priority` on Windows like the audio threads), `app/hardware.rs` (choosing instruments, opening the outputs tracks name, the outputs and inputs the menus offer, handing the live feed to the engine, Panic), `live.rs` (the live feed on the audio thread: driver stamps and the computer keyboard to device frames, ADR-0046), `live_tap.rs` (the tap on controller messages for MIDI learn, and the set of controllers a mapping takes from the engine, ADR-0088), `app/controllers.rs` (draining the tap, learning, mapped moves as a hand on the control), `app/controller_requests.rs` and `app/mappings_window.rs` (what the controls' menus ask, and the Mappings window), `arrangement/control_menu.rs` and `widgets/control_items.rs` (the menu rows every automatable control ends with), `document/mappings.rs` and `lumen_project::mapping` (the mappings in the model, format v23), `app/keyboard.rs` (the computer keyboard mode), `transport/midi_chip.rs` (the MIDI chip and its popover), `transport/fit.rs` (what the transport bar shows in the room it has), `arrangement/midi_io.rs` (MIDI From and Monitor menus, shared by the track menu and the strips), `arrangement/listen_light.rs` (header lights), `app/record.rs` (recording: Record, the commits, punch out, the fast autosave while recording, ADR-0055; each frame the preview the arrangement draws, `Document::preview_takes`, on the live GPU layers, ADR-0057), `recorder.rs` (the pure half: live messages into a take's notes and events), `midi_files.rs` (reading and writing MIDI files off the UI thread, ADR-0062), `midi_export.rs` (what Export MIDI writes: what plays, loops baked; pure and tested), `app/midi_files.rs` (dropped and imported MIDI files placed as the owner's F1 says, the tempo offer banner F2, the export commands), `document/midi_import.rs` (`import_midi_clip`, `import_midi_tracks`, `use_file_tempo`) |
| Clip editor | `note_editor/` (the piano roll's state, gestures, overlay, input, note commands and the pop-out window, ADR-0047; `ui.rs` draws it, `pointer.rs` the presses, drags and releases), `piano_roll/` (notes and keys on the GPU: `notes.rs` the instance builder, `draw.rs` lanes, grid, notes and keys per surface, `mini.rs` and `arrangement/mini_notes.rs` the notes on arrangement clips), `audition.rs` (one previewed note at a time and every way of letting go), `app/detail.rs` (the detail panel's tabs), `note_editor/lane/` (the velocity and controller lane under the roll, ADR-0050: `geometry.rs` the lane's kinds, its value and x mapping and its hit tests; `gesture.rs` the pure arithmetic of Pencil, Line, Move, Box and Bend; `paint.rs` the stems, heads, traces, previews and value scale; `ui.rs` the header, the pointer and the commit; `mod.rs` the state and the cache), `document/lanes.rs` (the lane's undoable edits, one step each), `lumen_project::edit::{events, velocity}` (the pure transforms behind them), `note_editor/quantize.rs` (slice 5, ADR-0054: the Quantize button and its popover, `Options`, the preview stand-in clip and ghost trails, the tools' context-menu rows), `lumen_project::edit::{quantize, note_tools}` (the pure quantize, humanize, fixed length, scale constrain and reverse) |
| Plugins and devices (M6) | `hosting/` (`mod.rs` the state, `update.rs` the per-frame step, `editor.rs`, `parameters.rs`, `presets.rs`, `outputs.rs`, `capture.rs` and `pretend.rs` by concern: which devices of the project want a host process, what became of each, and how a running host reaches the engine: the bridge to `lumen-hosting`, ADR-0071; `app/plugin_edits.rs` turns what a plugin does inside itself into edits, one undo step per gesture, ADR-0076), `plugins/` (`mod.rs` the installed catalog and the scan thread's lifetime, `worker.rs` the scan thread that reads and saves the cache, `settings.rs` folders, favorites and recents in the settings file, ADR-0068), `browser/` (the Devices tab: `rows.rs` the flat rows, pure; `view.rs` the pure state; `list.rs` paints only the rows in view; `controls.rs` the search, type and category controls; `panel.rs` applies what was done), `preset_files.rs` (`.vstpreset` files on disk, off the UI thread), `app/safe_mode.rs` (start without loading plugins, ADR-0077) |
| The Devices tab (M6) | `chain/` (`mod.rs` the tab; `place.rs` where a chosen device goes, pure; `actions.rs` the placement applied through the `Document`; `geometry.rs`, `art.rs`, `pictures.rs` the layout and a card's picture; `card.rs` one card in four faces; `view.rs` the tray, cables and chips; `strip.rs` the eight knobs; `stepped.rs` switches and choosers for stepped parameters; `params.rs` the All parameters panel; `presets.rs` the preset menu; `sidechain.rs` and `outputs.rs` the rows for a key input and extra outputs; `editor.rs` the plugin's own window and Learn), with `widgets/param.rs` (the knob and bar for a parameter) |
| Block budget and benchmark (M6) | `transport/budget.rs` and `budget_view.rs` (the audio panel's block budget, ADR-0075), `callback_bench.rs` and `app/bench_callback.rs` (`--bench-callback`: plays a project through the real device and reports the callback's time against the block, M6 slice 7c) |
| Other | `settings.rs` (user settings file and its background writer), `record_settings.rs` (the record settings: count-in, record-only metronome, Overdub; ADR-0055), `trace.rs` (`LUMEN_TRACE` logging), `bench.rs` (the frame-time benchmark, `--bench-frames`) |

Tools (binaries of `lumen-plugin-host`): `lumen-host-probe` runs installed plugins
through scan, load, buses, configure, process, link, state, parameters, editor and
shutdown, each in a host process of its own, and prints a compatibility table
(M6 slice 7a); `lumen-host-bench` measures the audio path between Lumen and host
processes (MT-VST-0); `lumen-editor-spike` settled how plugin windows take focus
(ADR-0067). The fault soak is `lumen-hosting/tests/soak.rs`.

Native macOS hosting (ADR-0085): `module/macos.rs` resolves bundle metadata
without loading code and initializes the retained `CFBundleRef` in the child;
`editor/mac.rs` owns the AppKit window, event pump and `NSView` lifecycle on
the child's main thread. `process.rs` arms a `kqueue` parent-exit watcher
before loading plugins. The macOS worker waits in `sem_wait`; request and
stop publish their state before posting the semaphore. Bounded client waits
retain the existing deadline path. `Stopper::stop` is the way to wake a worker
for shutdown; the shared region's quit setter is internal.

## Prototypes (M1)

| Prototype | Status |
|---|---|
| `engine-core` | Answered; rewritten as `lumen-engine` (M2) |
| `asio-probe` | Answered; became the ASIO backend in `lumen-audio` (M2) |
| `isolation-ipc`, `plugin-host` | Seed the plugin host process (M6) |
| `vst3-scan`, `vst3-test-module` | Seed the scanner (M6); the misbehaving module stays for the scanner's isolation self-test on Windows until the production scanner replaces it. The test plugin itself is now `lumen-test-plugin` |
| `ui-stress`, `render-shell` | Reference for dense rendering in the arrangement (M3 slice 4) |

## Rules every crate follows
- Layering: `lumen-core` at the bottom; `lumen-engine` depends only on `lumen-core`, `lumen-rt` and `lumen-devices`; `lumen-devices` only on `lumen-core`; `lumen-project` only on `lumen-core`; `lumen-audio`, `lumen-media`, `lumen-midi` and `lumen-render` stand alone (`lumen-engine`'s demo example uses `lumen-audio` as a dev-dependency); only `lumen-app` joins them. No cycles, and no UI types below the app.
- Audio-thread code never allocates, locks, blocks or panics. Tests enforce
  it: the allocation guard, plus RealtimeSanitizer in CI with canaries that
  prove the sanitizer is active.
- Third-party native code (plugins, drivers during probing) runs only where
  a crash cannot take Lumen down.
- Lumen never writes a file it cannot read back. Every persisted change
  bumps the format version, with a migration and a frozen fixture.
- Every edit is a named `Document` method that goes through the history's `apply`, and is one undo step (UI code never edits the project in a closure; `Document::apply` is for tests).
- Disk work runs off the UI thread: the interface and the audio never wait
  on files.
- Colours, sizes and durations come from the design-system modules, never
  inline.
- User-visible strings come from `lumen_core::product` where they name the
  product.


### M7b E0 source windows (ADR-0092)

- `lumen-project::edit::split::clips`: pure clip/range-edge splitting;
  `edit/windows.rs`: slicing and partial fade windows; `document/split.rs`:
  one history step and fresh IDs. Format v26 has an identity migration over
  older JSON (new fields default to the old behavior) and a frozen golden.
- `lumen-core::fade::Fade`: visible length plus original curve offset/span.
  A newly drawn fade resets the source window. `Clip.loop_phase` retains
  elapsed ticks from a loop's source origin, not just a remainder.
  `Clip.audio_phase_seconds` preserves nonloop source time without rounding
  through the native sample rate; the adapter differences absolute device
  sample positions, including clips with nonzero arrangement starts.
- `lumen-engine::clips::seams`: source-neighbor eligibility off the audio
  thread, sample-clock adjacency on it; preallocated outer-edge state keeps
  short pieces from shortening the source's edge fade. The engine's
  `ClipShape.phase` and `FadeFrames.offset/span` are device sample positions.
- `lanes.rs`: converts source phase and fade windows through the tempo map,
  bakes complete envelope knots so a split does not approximate a curved
  segment, and retains the original loop clock across tempo changes.
- `midi_runs.rs`: contiguous source-loop pieces share a run; a phased run
  begins with a source fragment and then full repeats, retaining sustain
  seams. `midi_export.rs` bakes that same phase; `piano_roll/notes.rs` and
  `arrangement/waves.rs` show the source windows that play.

### M7b E1 clip commands (ADR-0093)

- `lumen-project::edit::clip_ops`: pure `join`, `crop`, `unloop` and nudge
  edits; `edit::reverse`: reverse (mirrors a clip's place in its file about
  the file's end) and polarity; `edit::normalize`: the stretch of the file a
  clip plays and the gain a peak asks for. `document/clip_ops.rs` wraps
  each in one history step (`join_clips`, `crop_clips`, `unloop_clips`,
  `nudge_clips`, `reverse_clips`, `invert_clips`, `normalize_clips`).
  Format v27 adds `Clip.reversed` and `Clip.inverted` (identity migration).
- `lumen-app::media/twins.rs`: reversed copies of files (audio and peaks),
  built on a worker while a reversed clip needs them and read under
  `media::reversed_key`; `lanes::engine_clips`, `arrangement/waves.rs` and
  the sample view use that key. The engine is unchanged; polarity is the sign
  of the engine clip's gain and `waves.wgsl` accepts a negative gain.
- `normalize.rs`: the peak scan on a worker (progress, cancel);
  `app/clip_normalize.rs`: builds the jobs from the selection, shows
  progress in the toast, stops on Esc, applies one undo step.
- `lumen_media::wav`: the WAV writer (16, 24 and 32-bit float, mono or
  stereo, atomic, streamed). `lumen_engine::render_clips` (`render.rs`):
  the clip player run without a device, with bare outer ends (ADR-0095).
  `lumen_project::edit::consolidate`: the planner, the swap and the MIDI
  merge. `lumen-app::consolidate`: the render worker; `app/clip_consolidate.rs`:
  the command, progress and Esc; `document/consolidate.rs`: one undo step.
  `app/clip_commands.rs` holds the other selection commands.
- Bounce (ADR-0096, ADR-0097): `lumen_engine::render_source` (`render.rs`) plays
  a `Source` (the mixer) without a device, with a pre-roll and a tail;
  `Engine::set_offline` waits for plugin hosts. `lumen_project::edit::bounce`:
  the planner and the swap. `lumen-app::bounce`: the worker (a host process per
  plugin, `routing::bounce_routing` for the track alone); `app/clip_bounce.rs`:
  the popover, plugin-state capture, progress and Esc; `document/bounce.rs`:
  one undo step.
- Freeze (ADR-0098, format v29): `lumen_project::edit::freeze` holds the planner,
  the refusals, `apply`, `unfreeze`, `flatten`, `played` (the project as it
  sounds: a frozen track is one audio clip of its file, no devices) and
  `violations` (what `History::apply` refuses on a frozen track).
  `Track.frozen` is the model. `lumen-app::bounce` renders with `Bake::Sound`;
  `app/track_freeze.rs`: the commands, plugin-state capture, progress, Esc and
  the refusal messages; `document/freeze.rs`: one undo step each;
  `arrangement/frozen_badge.rs`: the header pill. `lanes` and `hosting::wanted`
  use `played`, so a frozen track runs no plugin.
