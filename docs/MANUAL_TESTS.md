# Lumen — Manual Test Scripts

Tests that need real hardware, third-party software or a human. Each entry
lists steps, the expected result and its last recorded run.

Run every tool with `cargo run --release` from the repository folder
(`D:\Projects\lumen`). It builds first and stops if the build fails, so it
always runs the current code. Starting an `.exe` by path can run a stale
copy (MT-APP-3; see `docs/DEVELOPMENT.md`). Older M1 binaries remain in
`%USERPROFILE%\lumen-bin`.

---

## M1 · P-ASIO — native ASIO host (`prototypes/asio-probe`)

### MT-ASIO-1 · Enumerate and query every driver safely
1. `target/release/asio-probe list`
2. `target/release/asio-probe query --timeout 15`

**Expected:** every registered driver is listed. Each one is either queried
(channels, names, buffer sizes, rates, clocks) or reported unavailable with
the driver's own message. A hanging or crashing driver is reported as
TIMED OUT or CRASHED and the probe continues.

**Last run (2026-10-05, dev machine):** pass. 7 drivers. Focusrite USB ASIO
and ASIO4ALL queried. Ableton Move and Push, Alpha, Focusrite Thunderbolt
and FlexASIO (misconfigured device) returned clean errors. No timeouts or
crashes.
Re-run 2026-10-05 (second session): pass.

### MT-ASIO-2 · Stable streaming at the interface's buffer size
1. `target/release/asio-probe stream "Focusrite USB ASIO" --seconds 30`
2. Repeat with `--engine-tracks 64`.

**Expected:** callback count matches duration; 0 sample-position
discontinuities; 0 real-time heap operations; worst callback well under the
period. Output is silent.

**Last run (2026-10-05):** pass. Results in ADR-0006.
Re-run (second session): pass. 11,252 callbacks, 0 discontinuities, 0 heap operations, worst callback 0.14%. The Focusrite now reports buffer 16–1024 and 44.1–192 kHz; the first session saw a fixed 128 at 48 kHz only, probably because another client held the device.

### MT-ASIO-3 · Small buffer
`target/release/asio-probe stream "ASIO4ALL v2" --seconds 15 --buffer 64 --engine-tracks 64`

**Expected:** as MT-ASIO-2 at a 64-frame buffer.

**Last run (2026-10-05):** pass (1.45 ms period, worst callback 3.8%).

### MT-ASIO-4 · Driver reset request (needs a human)
1. `target/release/asio-probe stream "Focusrite USB ASIO" --seconds 60`
2. While it runs, open Focusrite Control and change the buffer size (or the
   sample rate).

**Expected:** the probe prints `kAsioResetRequest received: recreating
buffers` and reports a second segment at the new buffer size. No crash and no
hang.

**Last run (2026-10-05, run by the owner):** pass. Buffer changed
128 → 256 → 128 during one 60 s stream. Two `kAsioResetRequest`s were
handled, giving three segments (5,348 / 970 / 14,709 callbacks), all with 0
discontinuities. Worst callback 1.6% of the period; 0 real-time heap
operations; reported latencies updated (14.5 → 25.9 → 14.5 ms round trip).
Each reset cost about 0.65 s of silence (58.7 s of audio in 60 s), which
M2 should track. Cosmetic bug: the per-segment "expected callbacks" figure
uses the whole run's duration.

### MT-ASIO-5 · Hardware removed while streaming (needs a human)
1. Start a 60 s stream on the Focusrite.
2. Unplug the interface's USB cable after about 10 s.

**Expected:** the probe does not crash. It stops receiving callbacks and
reports the reset or error messages the driver sends. (Recovery UX is
designed in M2.)

**Last run (2026-10-05, run by the owner):** pass (no crash). Unplugging
made the driver send `kAsioResetRequest`; recreating buffers then failed with
`ASE_NotPresent (-1000)` and the probe exited with a clear message. **M2
requirement:** treat this as "device lost": keep the session, show the
recovery state, and retry when the device returns, rather than stopping.

### MT-ASIO-6 · Driver busy in another application (needs a human)
1. Open another ASIO application (for example Ableton Live) on the Focusrite
   driver and start playback.
2. Run `asio-probe query` and `asio-probe stream "Focusrite USB ASIO" --seconds 10`.

**Expected:** a clear error or shared operation, depending on the driver. No
crash and no hang.

**Last run (2026-10-05, run by the owner):** pass. Streamed normally alongside
Ableton Live (3,750 callbacks in 10 s, 0 discontinuities, 0 heap operations):
the Focusrite USB driver allows multiple clients. Busy-driver error handling
is still untested; it needs a single-client driver.

---

## M1 · P-ISO — plugin isolation transport (`prototypes/isolation-ipc`)

### MT-ISO-1 · Transport latency, fan-out, crash and hang survival
`target/release/isolation-ipc all --seconds 20`

**Expected:** every scenario reports 0 deadline misses, 0 output mismatches,
0 loop overruns and 0 real-time heap operations, except for the deliberately
faulted host. The crash scenario reports exit code 0xC0000005 and a
replacement. The hang scenario reports "hung (still running; killed)" and a
replacement. The healthy host shows 0 misses in both.

**Last run (2026-10-05):** functional pass (second session too). **Latency regressed in the second session:** 1-host p99 about 235 µs, reproduced 3 times, against 19.5 µs earlier. See ADR-0005. Automated in the sense
that no human input is needed, but it needs a quiet machine. Not in CI:
shared runners are too noisy for latency numbers.

### MT-ISO-2 · No orphaned hosts (automatable)
1. Run `target/release/isolation-ipc bench --hosts 8 --seconds 60`.
2. Kill the parent from Task Manager after a few seconds.

**Expected:** all 8 `isolation-ipc.exe child` processes disappear at once
(kill-on-close Job object).

**Last run (2026-10-05):** pass, automated with `Stop-Process` on the parent: 9 processes before, 0 after.

---

## M1 · P-VST3 — out-of-process scanning (`prototypes/vst3-scan`)

### MT-VST3-1 · Scan every installed plugin
`target/release/vst3-scan scan --jobs 4 --timeout 30 --json prototypes/vst3-scan/out/scan-full.json`

**Expected:** every module is either enumerated or classified (failed,
timed out, crashed) and the scanner finishes. Every module with
`moduleinfo.json` matches its factory. Record results in
`docs/PLUGIN_COMPATIBILITY.md`.

**Last run (2026-10-05, twice):** pass. 39 of 40 enumerated, Opus failed in its own
`DllMain`, 4 of 4 `moduleinfo.json` files match.

### MT-VST3-2 · Isolation self-test (automated in CI on Windows)
`cargo build --release -p vst3-scan -p vst3-test-module && target/release/vst3-scan selftest`

**Expected:** PASS for crash contained, hang contained, and missing factory
reported.

**Last run (2026-10-05):** pass.

---

## M1 · P-UI — dense-view rendering (`prototypes/ui-stress`)

### MT-UI-1 · Frame cost, idle CPU, start-up
`target/release/ui-stress --bench` (opens a 3400×1300 window for about a
minute; do not touch it, especially during the final 10 s idle phase).

**Expected:** a table of CPU frame times for 10 scenarios, start-up times and
idle CPU. Compare with ADR-0002. Normal editing views and the GPU-instanced
overview p99 ≤ 8.3 ms; idle < 0.5%; first frame < 1 s warm.

**Last run (2026-10-05, twice):** normal views and the GPU path pass (p99 ≤ 6.1 ms; second run ≤ 4.4 ms). The plain-egui 100k-note overview fails (20.0 / 15.2 ms). Idle 0.00% / 0.47%, which is borderline against the 0.5% bar. First frame 545 / 490 ms.

### MT-UI-2 · Accessibility tree
1. `target/release/ui-stress --close-after 60`
2. Inspect with Accessibility Insights for Windows, or start Narrator
   (Ctrl+Win+Enter) and Tab through the transport and track headers.

**Expected:** buttons are announced as "Play", "Stop", "Record", "Loop";
track toggles as "Mute Track N" / "Solo Track N" check boxes with their
state.

**Last run (2026-10-05):** tree verified programmatically through UI
Automation (58 named elements), twice.

**Narrator run (2026-10-12, owner):** pass. The transport and track headers
were checked by tabbing through them with Narrator.

### MT-UI-3 · Visual quality at 100%, 150%, 200% (needs a human)
1. Run `target/release/ui-stress` at each Windows display scale.
2. Judge text crispness, line sharpness and waveform appearance.

**Last run (2026-10-05, run by the owner):** pass at 100%, 150% and 200%.
Text was not blurry; it looked fine.

---

## M1 · P-HOST — real plugin in an isolated host (`prototypes/plugin-host`)

### MT-HOST-1 · Host a real instrument and round-trip its state
`plugin-host render "C:\Program Files\Common Files\VST3\TyrellN6(x64).vst3"`

**Expected:** audible render (peak above −60 dBFS), all samples finite,
state round trip "IDENTICAL". The audio is written to
`prototypes/plugin-host/out/render-1.wav`.

**Last run (2026-10-06):** pass (peak −13.4 dBFS, 37 µs per block, 2,783-byte state identical).

### MT-HOST-2 · Survive a plugin crash and restore it
`plugin-host isolate "C:\Program Files\Common Files\VST3\TyrellN6(x64).vst3"`

**Expected:** crash detected with exit code `0xC0000005`; host restarted
with the last known state ("re-saves identical: true"); audio at the same
level after recovery; 0 loop overruns; no `plugin-host` processes left
afterwards. Listen to `prototypes/plugin-host/out/isolated-with-crash.wav`
for the gap.

**Last run (2026-10-06):** pass (restart 33 ms, 15 silent blocks, 0 overruns, 0 leftover processes).

---

## M1 · Application shell (`crates/lumen-app`)

### MT-APP-1 · The shell launches and shows the specified layout
1. `lumen --smoke-test` (renders 30 frames, prints start-up time, exits 0).
2. `lumen`, then look at it: transport bar on top, browser on the left,
   arrangement grid with ruler in the centre, detail panel at the bottom.
   Ctrl+wheel zooms the grid around the pointer; the wheel scrolls; Shift+Tab
   hides and shows the detail panel; the Browser and Detail buttons toggle
   their panels.

**Expected:** exit code 0; first frame under 1 s with a warm cache;
disabled controls show "Arrives in Milestone N" on hover.

**Last run (2026-10-06):** pass. 394 ms warm / 1,110 ms cold. Screenshot
checked; UI Automation shows 21 elements, disabled controls as disabled.
Interactive checks of zoom, scroll, Shift+Tab and the panel toggles passed
(owner, 2026-10-12).

---

## M2 · Audio device layer (`crates/lumen-audio`)

Run: `cargo run --release -p lumen-audio --example audio_tone -- <args>`
(for example `-- "Focusrite USB ASIO" --seconds 60 --tone`). The runs below
show `audio_tone <args>` for short.
`--driver` picks `asio` (the default), `wasapi` or `exclusive`. (Before
slice 2 the example was called `asio_tone` and spoke only ASIO.)

### MT-AUDIO-1 · Device loss and recovery (needs a human)
1. `audio_tone "Focusrite USB ASIO" --seconds 60 --tone`
2. After about 10 s, unplug the Focusrite's USB cable. Wait about 10 s.
3. Plug it back in. Let the run finish.

**Expected:** `event: Lost(...)` shortly after unplugging; the state reports
`Lost` while unplugged; `event: Restored(...)` within a few seconds of
plugging back in; the tone resumes; no crash and no hang. Exactly one
`Lost` and one `Restored` per unplug.

**Run 1 (2026-10-06):** pass on the essentials, with one bug. The driver
never asked for a reset; the watchdog reported `Lost` 0.7 s after the
unplug. `Restored` came 1 s after the replug and the tone resumed; there
were 0 overloads and the peak load was 0.8%. Bug: while the cable was
still out, the driver accepted a reopen. That produced a false `Restored`
at 13.2 s and a second `Lost` (0x54f) at 13.7 s. Fixed the same day: a
reopened device now counts as restored only after callbacks have flowed
for one second, and failed probation stays quiet. Covered by the test
`a_reopen_that_fails_at_once_is_not_reported_restored`. Run 2 confirms the
fix on hardware.

**Run 2 (2026-10-06):** pass. One `Lost` at 12.4 s (watchdog), one
`Restored` at 35.8 s, no false restore in between. About 400 callbacks ran
during the one-second probation, then audio continued cleanly to the end
(0 overloads, peak load 0.7%). `Restored` came about 2 s after the
replug (up to 1 s waiting for the next retry, plus the 1 s confirmation).

### MT-AUDIO-2 · Driver reset from the control panel (needs a human)
1. `audio_tone "Focusrite USB ASIO" --seconds 60 --tone`
2. Change the buffer size in Focusrite Control while it runs.

**Expected:** `event: Reset(StreamInfo { buffer_frames: <new size>, ... })`,
and the tone continues.

**Last run (2026-10-06):** pass. Buffer changed from 128 to 256 at 11.6 s →
`Reset(StreamInfo { buffer_frames: 256, input_latency: 550,
output_latency: 638, .. })`. The callback rate halved from about 344 to
about 172 per second, the tone continued, and there were 0 overloads.

### MT-AUDIO-3 · Error paths (automatable)
- `audio_tone "Focusrite Thunderbolt ASIO" --seconds 2` →
  `ConfigFailed { DeviceNotPresent("Cannot open ... 0x54f"), reverted: false }`
- `audio_tone "Focusrite USB ASIO" --seconds 2 --buffer 100` →
  `ConfigFailed { Unsupported("buffer size 100"), reverted: false }`

**Last run (2026-10-06):** pass, both. The normal run also passed: Started
at 44.1 kHz / 128 frames, about 344 callbacks per second, clean Stopped.

### MT-AUDIO-4 · WASAPI device loss and recovery (needs a human)
1. `audio_tone "Focusrite USB" --driver wasapi --seconds 60 --tone`
2. Unplug the Focusrite's USB cable at about 10 s. Plug it back in at about
   20 s.
3. Repeat with `--driver exclusive`.

**Expected:** one `Lost` after the unplug and one `Restored` about 2 s after
the replug, the tone resuming in both modes, no crash and no hang. In
exclusive mode at 441 frames and 44.1 kHz, about 100 callbacks per second.

**Run 1 (2026-10-06):** recovery passes in both modes: one `Lost` and one
`Restored`, about 4 s after the replug (Windows re-registering the USB
endpoint, plus Lumen's retry and confirmation). Two findings:
- The unplug was reported as `Driver("… 0x80070003")` rather than
  `DeviceNotPresent`. Fixed: the file-not-found and path-not-found errors
  now map to `DeviceNotPresent`.
- Exclusive mode delivered only about 65 callbacks per second instead of
  100. A probe showed the Focusrite's device clock running at 60% of real
  time in event-driven exclusive mode, while polling kept exact time.
  Fixed: exclusive mode now checks the device clock at start and falls
  back to polling (ADR-0016). It now runs at 101 callbacks/s.

**Run 2 (2026-10-06, exclusive with the tone, after the fix):** pass.
Started at 1.1 s (calibration chose polling), 99–102 callbacks/s against
100 expected, clean tone. The unplug was reported as
`Lost(DeviceNotPresent(..))` at 13.6 s, `Restored` at 27.9 s, then
99–102/s again until the end. 0 overloads, peak load 0.4%.

### MT-AUDIO-5 · WASAPI shared and exclusive streams (automatable)
- `audio_tone list --driver wasapi` and `list --driver exclusive`: every
  output endpoint is listed with its channels, rates and buffer sizes.
- `audio_tone G34WQC --driver wasapi --seconds 3`: Started at 48 kHz /
  480 frames, about 100 callbacks/s.
- `audio_tone G34WQC --driver exclusive --seconds 3 --buffer 144`: Started
  at 48 kHz / 144 frames, about 333 callbacks/s.
- `audio_tone G34WQC --driver wasapi --rate 44100`: `ConfigFailed(Unsupported)`
  with a message that names the Windows rate and the fix.

**Last run (2026-10-06):** pass, all four (ADR-0016 has the numbers).

### MT-AUDIO-6 · Sleep/resume and Windows format changes (needs a human)
1. `audio_tone "Focusrite USB" --driver wasapi --seconds 120 --tone`. While
   it runs, open Windows Sound settings → the Focusrite output → Properties
   → Advanced, and change the default format (for example 44100 Hz to
   48000 Hz).
2. `audio_tone "Focusrite USB ASIO" --seconds 300 --tone`. While it runs,
   put the PC to sleep, wake it, and let the run finish.

**Expected:** (1) a `Reset` event with the new `sample_rate` and the tone
continuing (or `Lost` then `Restored`), with no crash. (2) After waking,
either the stream was never interrupted or there is a `Lost` followed by
`Restored`; audio resumes and nothing hangs.

**Part 1, run 1 (2026-10-06):** pass for a format change. The Focusrite
offers only 44.1 kHz in Windows (its driver sets the rate), so the switch
was 16-bit ↔ 24-bit. Each change gave one `Reset` (at 15.0 s and 25.7 s),
back to 98–102 callbacks/s within a second; about 0.4 s of audio was lost
during each reopen. No `Lost`, no crash.

**Part 1, run 2 (2026-10-06, rate change on the NVIDIA HDMI output):**
pass. 48 → 44.1 kHz at 9.0 s gave `Reset(StreamInfo { sample_rate: 44100,
buffer_frames: 448, .. })` (Windows' 44.1 kHz period is 448 frames), then
about 98 callbacks/s as expected. Back to 48 kHz at 19.6 s gave `Reset` at
480 frames and 100/s. Only one or two callbacks were missed at each switch.

**Part 2, run 1 (2026-10-06, ASIO, 44.1 kHz / 256):** pass. The PC slept at
about 25 s; callbacks stopped. After waking, `Lost(DeviceNotPresent("the
driver stopped delivering audio"))` came from the watchdog, then `Restored`
2.4 s later, and the tone resumed at 170–175 callbacks/s (172 expected) to
the end. No crash, no hang. The twenty `0/s` report lines before `Lost`
are the report loop catching up at wake: the user confirmed the tone came
back promptly after waking.

---

## M2 · Engine and transport (`crates/lumen-engine`)

Run: `cargo run --release -p lumen-engine --example transport_demo -- "Focusrite USB ASIO"`.
Type a command and press Enter: `p` play · `a` pause · `s` stop · `g 5` go to
bar 5 · `l 2 4` loop bars 2–3 · `l` loop off · `t 140` tempo · `m 7/8` time
signature · `q` quit.

### MT-ENGINE-1 · Transport by ear (needs a human)
(Since 2026-10-06, `p` plays from the start marker and `c` continues from
the playhead; `s` leaves the playhead where it stopped.)
1. `p`: a steady click, higher on each downbeat; the bar display counts up.
2. `l 2 3` while playing: once bar 3 arrives, playback jumps back to bar 2
   with no gap, double click or stumble; the downbeat lands right on the wrap.
3. `t 90`, then `t 160`, while looping: the tempo changes at once; the bar
   number doesn't jump.
4. `m 3/4`, then `m 7/8`: the accent pattern follows (every 3rd, then every
   7th click is high).
5. `a` (pause), then `p`: playback continues where it paused.
6. `s`: playback stops and the display returns to where it last started.
   `s` again returns it to 1.1.
7. `g 9` while playing: playback jumps to bar 9 on a downbeat.

**Expected:** every step as described, with clicks evenly spaced and no
crackles.

**Last run (2026-10-06, Focusrite USB ASIO, 44.1 kHz / 256):** pass, all
seven steps. The position display matched the expected samples (bar 4 from
264,600, bar 5 from 352,800) and advanced at 44,065 samples/s over 7.75 s.
Tip found during the run: a loop only catches playback that is before its
end, so `g 2` before `l 2 3` if playback is already past bar 3.

### MT-APP-2 · Transport bar (needs a human)
Run: `cargo run --release -p lumen-app -- --audio asio:"Focusrite USB ASIO"`
(or with no `--audio` for the Windows default output).
1. The status dot is blue and the text names the device, rate, buffer and
   latency. Hover it for the full text.
2. Press **Space**: the playhead moves along the ruler and the position
   counts up (bars.beats.sixteenths, then m:ss.mmm). Space again stops and
   returns to where playback started; Space then Home returns to 1.1.1.
3. **Shift+Space** pauses and continues in place.
4. Click **Loop** (the circular arrow): it turns blue, bars 1–4 are shaded on
   the ruler, and playback wraps from bar 5 back to bar 1.
5. Drag the tempo, or click it and type 90: the position keeps counting at
   the new speed without jumping. Set the time signature to 7/8: the ruler
   and grid re-bar.
6. Click the position, type `9` and press Enter: the playhead jumps to bar
   9. Type `2.5` (4/4): a warning sign explains that beats are 1–4.
7. Hover each icon: tooltips name it and its shortcut; Record and Metronome
   say when they arrive.
8. Tab through the transport: each control shows a blue focus ring; Enter
   activates a focused button.
9. Run with `--scale 2`: icons stay sharp; the Browser and Detail toggles
   stay visible.

**Expected:** every step as described, with no stutter in the playhead.

**Last run (2026-10-06, Focusrite USB ASIO):** pass, all steps. Run 1
reported 1–2 s from clicking Play to the playhead moving. Two causes:
- A real bug: after a click, the frame still saw the engine stopped (it
  applies commands at its next block), so the app waited for its 1 s idle
  repaint. Fixed: commands keep the UI redrawing until they take effect.
- The build tested had not picked up that fix: `cargo build` cannot replace
  `lumen.exe` while the app is open, and the failure is easy to miss.
Tracing (`LUMEN_TRACE`, `--autoplay-ms`) then measured 14–19 ms from command
to screen over 28 play/stop cycles each on WASAPI and ASIO, and the delay
was gone in the owner's run.

Automated: screenshots at 100%, 150% and 200%
(`--screenshot`) checked on 2026-10-06; the first round found the panel
toggles pushed off-screen at 200% and the status text unreadable. Both fixed.

### MT-APP-3 · Click-to-locate on the ruler (needs a human)
Close Lumen, then build and start it in one step (a failed build then
stops instead of starting an old one):
`cargo run --release -p lumen-app -- --audio asio:"Focusrite USB ASIO"`.
Run 1 (2026-10-06) tested a stale build, not a result. The owner's
`cargo build` wrote to `target\`, but the command started the agent's copy
in `%USERPROFILE%\lumen-target`, which predated slice 5. Both now use
`target\`, started with `cargo run`.

1. Move the pointer along the ruler (the strip with bar numbers): a faint
   blue line follows it, jumping between grid lines; the pointer is a hand.
2. Click: the playhead moves to that line and the position display shows
   it. While stopped, Space then plays from there.
3. Press Space to play, then click elsewhere on the ruler: playback jumps
   there immediately and continues.
4. Hold **Alt** while moving and clicking: the line follows the pointer
   freely, without snapping.
5. Ctrl+wheel to zoom in: the snap gets finer (beats, eighths, sixteenths…);
   zoom out and it snaps to whole bars.
6. Set 7/8: snapping follows the new bars and beats.
7. Scrolling and Ctrl+wheel zoom still work with the pointer over the ruler.

**Expected:** every step as described.

**Last run (2026-10-06, run 2, started with `cargo run`):** pass, all steps.

---

## M2 · Audio files (`crates/lumen-media`, clips in the app)

### MT-MEDIA-1 · Playing audio files (needs a human)
Run `cargo run --release -p lumen-app -- --audio asio:"Focusrite USB ASIO"`.
Use your own files: ideally a long song (5+ minutes), a short drum loop, and
files at 44.1 kHz and 48 kHz, in more than one format (WAV, FLAC, MP3, Ogg).
1. Drag one file from Explorer onto the arrangement. It appears on a lane
   with a loading bar, then as a coloured block with its name.
2. Press Space: it plays cleanly, with no clicks, crackles or dropouts, all
   the way through a long file.
3. Click the ruler mid-clip while playing: playback jumps there without a
   click.
4. Turn Loop on and drag a drum loop so it starts at bar 1: the loop of bars
   1–4 should sound seamless.
5. Drop several files at once: each gets its own lane and they play together.
6. Drop a non-audio file (a .txt): a note in the bottom-left says it was not
   added.
7. Drop a large file and press Cancel while it loads: it disappears.
8. Hover the status dot: the tooltip shows how much clip audio is in memory.
9. Repeat 2 at 64 and 128 samples in Focusrite Control, listening for
   dropouts.

**Expected:** every step as described.

**Run 1 (2026-10-06, owner):** playback good. Findings:
- **Bug, fixed:** a small pop on loop wraps and when stopping. Jumps faded in
  but never faded the old audio out. The engine now crossfades every jump
  (ADR-0019; golden tests cover the crossfade and the stop fade-out).
- **Added at the owner's request:** while dragging, the arrangement lists
  each file with ✓ (format) or ✕ (not supported) before the drop. Hovering a
  clip shows its format, original rate, channels, length, size and path.
- **Planned elsewhere:** dropping onto a specific track (M3, now a
  PROGRESS row); scrubbing with the mouse (M3, the owner's Ableton-style
  scrubbing request).

**Run 2 (2026-10-06, owner):** pass. No pop on loop wraps, stops or
locates; the drag indicator and the hover card work.
**Run 3 (2026-10-12, owner):** pass. The 64-sample dropout check (step 9)
passed.

Automated: `--open-audio` with a 5 s WAV loads
in 38 ms and redraws at 2.5 bars; the rate change to 44.1 kHz converts again
automatically.

### MT-APP-4 · Metronome, test tone, meters, Audio On/Off, panic (needs a human)
Run `cargo run --release -p lumen-app -- --audio asio:"Focusrite USB ASIO"`.
1. Click the **metronome** icon, then press Space: steady clicks, higher and
   louder on each downbeat. Right-click the icon: change the volume and
   switch to Wood. Set 7/8: the accent comes every 7 clicks. Loop bars 1–4:
   the clicks stay on the beat through the wrap.
2. The bar shows **CPU n%**; it should stay low.
3. Click the **status dot** (or the device text): the audio panel opens.
   - Tick **Test tone**: a steady 1 kHz tone, even when stopped. Untick it.
   - Untick **Audio on**: the dot turns amber and audio stops. Tick it again:
     audio comes back.
   - Check the CPU, overloads and dropouts readouts; **Reset counts** zeroes
     them.
   - With the metronome and test tone on and playing, click **Panic**:
     everything goes silent at once and the transport stops.
4. Set the Focusrite to 64 samples and play a long file with the metronome
   for a minute: the dropout count should stay at 0 (no ⚠ on the bar).

**Expected:** every step as described.

**Run 1 (2026-10-06, owner):** the metronome sounds good. Findings:
- "Reset counts" seemed to do nothing. It resets overloads and dropouts,
  which were already 0; CPU is a live reading, not a count. The button is
  now "Reset overload and dropout counts" and is disabled while both are 0.
- The owner asked for Ableton's two playheads. This was built: the start
  marker and the playhead, with Play from the marker, Shift+Space
  continuing, and Stop leaving the playhead in place (ADR-0017 revised).

**Run 2 (2026-10-06, owner):** pass: test tone, Audio On/Off and Panic.
**Run 3 (2026-10-12, owner):** pass. At 64 samples, the dropout count
stayed at 0 while the long file and metronome played for one minute.

### MT-APP-5 · Start marker and playhead (needs a human)
1. Click the ruler at bar 3: the grey arrow (the start marker, bottom of the
   ruler) and the blue
   playhead (top of the ruler, with a line) both move there.
2. Space to play, Space to stop: the playhead stays where it stopped, and
   the arrow stays at bar 3.
3. Space again: playback starts from bar 3 (the arrow).
4. Stop, then Shift+Space: playback continues from where the playhead
   stopped.
5. Stop twice: both return to 1.1.1.

**Expected:** every step as described, matching Ableton's arrangement view.

**Last run (2026-10-06, owner):** pass, after restyling the start marker at
the owner's request (grey, ruler only, bottom edge).

### MT-APP-6 · Audio settings (needs a human)
Run `cargo run --release -p lumen-app` (no `--audio`: the saved or default
device is used). Open the audio panel (click the status dot), then **Audio
settings…**.
1. The window opens in front of everything, with the audio panel closed.
   It lists the driver, device, rate, buffer, outputs and latency of what
   is running, in four sections with labels in one aligned column.
2. Switch **Driver** to ASIO and pick **Focusrite USB ASIO**: audio moves to
   it (the status dot goes blue again), and the window shows its inputs and
   outputs by name.
3. Click **Control panel…**: the button shows "opening…" while Focusrite
   Control starts, and Lumen stays responsive meanwhile. Change the buffer
   size there: the window and status follow (a reset). No error text stays
   behind.
4. Change **Sample rate** to 44.1 kHz and back. Each change takes about
   1–2 s (the interface re-locking its clock); the window says so while it
   happens.
5. Pick an input pair (Input 1/2): the latency line now shows input, output
   and round trip.
6. Switch back to Windows audio (shared) and its Focusrite device, then to
   ASIO again: playback keeps working each time; a playing file and the
   metronome survive the switch.
7. Close Lumen and start it again: the last working device comes back.
8. Pick **Focusrite Thunderbolt ASIO** (no hardware): the window says it
   could not start and that Lumen is still using the previous device; the
   status shows that device running (no endless spinner), and audio keeps
   playing.
9. With Lumen on ASIO, unplug the Focusrite: a banner says it was
   disconnected; plug it back in: the banner goes away.

**Expected:** every step as described, without a crash or a stuck UI.

**Run 1 (2026-10-06, Focusrite USB): 6 findings, all fixed:**
- The window opened behind the audio panel's dropdown. The panel now
  closes when the window opens.
- Opening the control panel froze Lumen for seconds, and "the driver did
  not respond" stayed visible after it did respond. It now opens in the
  background with an "opening…" state, and the error clears on success.
- Rate changes took 1–2 s. Measured with `examples/rate_switch.rs`, this
  is hardware: about 900 ms in the driver's rate change and about 390 ms
  in its next start; Lumen's own part is about 20 ms. The window now shows
  progress.
- The layout was untidy, the latency readout unclear, and channel spacing
  uneven. The window is now four sections (Device, Format, Channels,
  Latency) with one aligned label column, plus a latency table in ms and
  samples.
- Thunderbolt ASIO showed its error, but "Starting" spun forever. A failed
  switch now returns to the previous device and says so.

**Run 2 (2026-10-06):** all steps pass. One finding: the failed-switch
notice widened the window. The window now has a fixed width, notices wrap
and have a Dismiss button, and the driver's own error text shows on hover.

**Run 3 (2026-10-06, owner):** pass. The window keeps its width while a
failing device is tried (every warning now wraps).

**Last run:** run 3, pass.

### MT-APP-7 · Design system and motion (needs a human)
Run `cargo run --release -p lumen-app`. This test is about feel: does Lumen
move the way it should (`docs/DESIGN.md` §6)?
1. **Panels:** click **Browser** and **Detail** (or Shift+Tab). The panels
   slide closed and open, quickly and smoothly, without a jump at the end.
2. **Transport buttons:** move the pointer across play, stop and loop. The
   hover fill fades in and out; pressing darkens it. A click still acts at
   once.
3. **Playhead:** while stopped, click the ruler in a few places. The playhead
   glides to each spot and settles softly, without overshooting. Press Space
   to play: the playhead follows the audio exactly (no lag). Press Space to
   stop, then Stop again: it glides back to the start.
4. **Popovers and windows:** open the audio panel (status dot) and **Audio
   settings…**. Both fade in. Esc or ✕ closes the window.
5. **Notices:** in Audio settings, pick **Focusrite Thunderbolt ASIO**. The
   "could not start" notice fades in with a small drop; **Dismiss** fades it
   out.
6. **Clips:** drop an audio file on the arrangement. The clip fades in, and
   its loading fill moves smoothly.
7. **Appearance:** click **Lumen** (top left). Switch Theme to Light, then
   Match system, then Dark; try Interface size 150% and 200%; everything
   stays readable and nothing overlaps.
8. **Reduce motion:** turn it on in the same menu and repeat steps 1, 3
   and 5. Nothing slides or glides any more: things change in place. Turn
   it off again.
9. Close and restart Lumen: the theme, size and Reduce motion setting are
   remembered.

**Expected:** every step as described. Motion should feel quick and
calm: responsive at the start, soft at the end, never sluggish, bouncy or
delayed. Say what feels off (too slow, too fast, too much, too little).

**Run 1 (2026-10-07, owner):** everything looked good except closing
panels, which felt choppy (egui closes with the mirror of its opening
curve). Fixed in `panels.rs`: the spring in both directions, and contents
slide at full size. The owner also asked for a typeface with more
character. After two rounds of rendered specimens they chose Inter, with
Unbounded for titles.

**Run 2 (2026-10-07, owner):** pass. The owner approved and moved on to
slice 3.

**Last run:** run 2, pass.

### MT-APP-8 · Undo, saving, opening and crash recovery (needs a human)
Run `cargo run --release -p lumen-app`.
1. **Undo:** drop two audio files on the arrangement (one at a time). Drag
   the tempo field, then change the time signature. Press Ctrl+Z four
   times: each press undoes one step (the whole tempo drag is one step), a
   short note says what was undone, and the tracks disappear in reverse
   order. Ctrl+Shift+Z (or Ctrl+Y) brings them back.
2. **History:** open **Lumen** (top left) → **History…**. Every step is
   listed. Click the first import: the project goes back to that point;
   click the last step to return.
3. **Unsaved dot:** the window title reads "Untitled • — Lumen" while
   there are unsaved changes.
4. **Save:** press Ctrl+S. A dialog asks where to save; type a name such
   as "Test Song" in your Documents folder. Lumen creates a folder
   "Test Song" holding "Test Song.lumen" and `Samples/Imported` with
   copies of the two files. The title loses its dot, and a note says what
   was saved.
5. **Reopen:** scroll and zoom the arrangement, close the Browser panel,
   and turn the loop on. Save again (Ctrl+S, no dialog this time). Close
   Lumen and start it again, then **Lumen** → **Open recent** → Test
   Song. The tracks, tempo, scroll, zoom, panels and loop are as you left
   them, and the clips play.
6. **Unsaved changes prompt:** change the tempo, then close the window. A
   prompt asks to save: Cancel keeps Lumen open; Don't save closes it.
7. **Crash recovery:** start Lumen, drop a file, wait more than a minute,
   then end Lumen from Task Manager (End task). Start Lumen again: a
   banner offers to recover the unsaved changes. Recover brings the track
   back with the title showing unsaved changes; Discard deletes the copy.

**Expected:** every step as described. Nothing waits on the disk: the
interface keeps moving while dialogs are open and while saving.

**Automated part:** `--save-as` and `--recover-latest` drive steps 4, 5
and 7 without a person (see DEVELOPMENT.md). Both passed on 2026-10-07:
a project saved, its source audio deleted and the project reopened plays
from its own copies, and a killed process was recovered.

**Run 1 (2026-10-07, owner):** three findings, all addressed:
- The undo note wrapped into a narrow column in the middle of the
  timeline. It is now one line in the arrangement's top-right corner under
  the ruler, shortened with "…" when long.
- Explorer shows a generic icon for `.lumen` files. The file association
  and a project file icon are scheduled for M9 (ROADMAP).
- A minimized or hidden Open dialog could get lost, and Open did not bring
  it back. Dialogs are now owned by Lumen's window: they stay above it,
  minimize and restore with it, and come forward when Lumen is clicked.

**Run 2 (2026-10-07, owner):** pass, all steps, including the one-line
note and the owned file dialogs.

**Last run:** run 2, pass.

### MT-APP-9 · Tracks (needs a human)
Run `cargo run --release -p lumen-app`.
1. **Add:** press Ctrl+T twice and Ctrl+Shift+T once; then use
   "+ Add track ▾" below the tracks (it lists every type with its
   shortcut); then double-click the empty space, as its hint says.
   - Each track has a colour rail and a type icon (waveform for audio, keys
     for MIDI).
   - Right-click a header or the empty space: the same menu.
2. **Rename and recolour:** double-click a name (or press F2), type,
   press Enter. Esc cancels. Right-click → Colour → pick one: the rail and
   the clips change colour.
3. **Select:** click headers; Ctrl+click adds or removes a track;
   Shift+click selects a range. Selected headers are tinted.
4. **Mute, solo, arm:**
   - drop an audio file, then play it;
   - **M** silences the track and dims its clips;
   - **S** plays only that track; the other tracks' clips dim and the
     soloed lane gets a faint blue wash;
   - Ctrl+click **S** on a second track adds it to the solo;
   - the round **●** lights red: the rail glows red into the header, a
     ring breathes slowly around the dot (centred on it), and the lane
     gets a red gradient with thin red edges;
   - with several tracks selected, a button acts on all of them.
5. **Fold and resize:**
   - click a track's arrow: it unfolds smoothly, showing Vol and Pan;
     click again to fold it;
   - hover a track's bottom edge: it glows;
   - drag it: the track resizes in fixed steps; dragging a folded track
     taller unfolds it, and dragging an unfolded one small folds it.
6. **Volume and pan:** on an unfolded track playing audio:
   - drag the volume slider down: it gets quieter, and the value shows
     inside the slider;
   - drag pan to L 100: the sound moves left, and the fill turns from grey
     to blue toward the edge;
   - double-click either to reset;
   - click one and type a value (−6, or L 30), then press Enter;
   - drags glide smoothly, without crackle (levels glide since run 2).
7. **Move, duplicate, delete:**
   - Alt+↑ and Alt+↓ move the selected tracks;
   - Ctrl+D duplicates them, with their clips;
   - Delete removes them;
   - Ctrl+Z undoes each of these as one step.
8. **Scroll:** with more tracks than fit, the mouse wheel scrolls them;
   Shift+wheel scrolls the timeline; Ctrl+wheel zooms.
9. **Saved:** save, close and reopen. Folding, heights, the scroll
   position, mute, solo and arm are as you left them.
10. Try steps 1–5 in the light theme and at 150%.

**Expected:** every step as described, smooth and without lag.

**Run 1 (2026-10-07, owner):** findings, all addressed:
- MIDI tracks had no visual cue: type icons added.
- The unfolded controls were poorly organised: sliders with the values
  inside.
- Pan should be an Ableton-style slider with the label inside, and its
  saturation should grow toward the edges: done.
- Arm needed more presence: "glow and breathe", with a centred ring.
- The double-click to add a track gave no hint, and offered no way to add
  MIDI: a "+ Add track ▾" menu button with a hint.
- More room was needed between the arrow and the name: added, and the
  header widened.
- The resize handle needed a cue: it glows.
- Heights should come in fixed steps: 22 pt steps.

**Run 2 (2026-10-07, owner):** findings, all addressed:
- The close buttons sat in different places in the History and Audio
  settings windows: every window is now one shared `Dialog`, with the ✕ at
  its right edge.
- The arm button looked misaligned and its ring off-centre: M, S and arm
  are snapped to whole pixels together and evenly spaced.
- Typing a tempo while playing silenced the audio until the field lost
  focus: each keystroke applied (clamped: "1" on the way to "125" was
  20 BPM). Typed tempo and beats per bar now apply on Enter or leaving the
  field; dragging stays live.
- M, S and the sliders lost contrast on a selected (blue) header: they
  step up on selected rows (DESIGN §3).
- Also: the pan slider's centre line ran through the "C"; it is now short
  ticks at the edges.

Check in run 3:
- Open History and Audio settings: the ✕ sits at the same place in both.
- Zoom in on M, S and arm, at 100% and 150%: same top and bottom, even
  gaps, the ring centred on the dot.
- Play audio, click the tempo, type 125: playback carries on unchanged
  while you type; on Enter the tempo changes once, and History gains one
  "Change tempo" step. (Audio clips do not follow tempo until warping in
  M8, so after the change the audio continues from the same bar, which is
  a different point in the file.)
- Select a track: its M, S and sliders stay clearly visible, in both
  themes.
- While audio plays: arm and disarm tracks, drag volume and pan, fast and
  slow, and mute, unmute and solo. No pop, no crackle, no click: mute and
  solo fade over a few milliseconds.
- Start Lumen, open a project with audio, and press Play at once (while
  its audio may still be loading): no pop. If it still pops, note whether
  the audio panel's dropout count went up.

**Run 3 (2026-10-07, owner):** passed, including the audio fixes found
during it (arming, volume and pan drags, mute and solo, the first play
after opening).

**Last run:** run 3, passed.

### MT-APP-10 · Clips at scale (needs a human)
1. **Benchmark:** run
   `cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json"`.
   A window scrolls and zooms by itself for about 15 seconds, then
   closes. The console shows each phase; every line should say well under
   16.7 ms at p99, and "PASS". Note the numbers in the run record.
2. **By hand:** run `cargo run --release -p lumen-app -- --stress`.
   - Scroll through all 200 tracks with the wheel, Shift+wheel through the
     timeline, and Ctrl+wheel from close up to the whole project: smooth,
     no stutter.
   - Fold, unfold and resize tracks while scrolled down: the clips follow
     their tracks smoothly.
   - Clips look as before: dark body, coloured title bar with the name,
     a thin coloured border, rounded corners. Muted tracks' clips are
     dimmed; the generated audio files are missing, so audio clips show
     red bars with "file missing".
   - Zoomed far out, names disappear from narrow clips; zoomed in, they
     return.
3. **Your own project:** open a real project, and drop a long audio file
   onto it: while it loads, its clip shows the loading fill growing, then
   becomes a normal clip.
4. Try steps 2 and 3 in the light theme and at 150%.

**Expected:** every step as described, and no visual difference from
before apart from smoother scrolling.

**Run 1 (2026-10-07, owner):** "it looks good". The owner reported faint
static on the metronome during the run; fixed (ADR-0020 update).

**Last run:** run 1, passed.

### MT-APP-11 · Waveforms (needs a human)
1. Drop a few of your own audio files (a drum loop, a vocal, a long mix)
   onto the arrangement. As each finishes loading, its clip shows its
   waveform in the track colour.
2. Unfold a track and make it taller: a stereo file shows left above
   right; folded or short, one combined waveform.
3. Ctrl+wheel from the closest zoom to the whole song: the waveform stays
   sharp at every step, with no blocky or blurry stage, and transients
   line up with what you hear when you play from them.
4. Mute a track: its waveform dims with its clip. Solo another: the rest
   dim.
5. Change the tempo: clips keep their audio where it was; the waveforms
   stretch or squeeze with the beats as the audio does.
6. Run `cargo run --release -p lumen-app -- --stress`: every audio clip
   has a waveform; scrolling and zooming stay smooth.
7. Try steps 1–4 in the light theme and at 150%.

**Expected:** every step as described.

**Run 1 (2026-10-07, owner):** one finding, fixed: the waveform shimmered
while scrolling the timeline (fine when still). Each pixel showed frames
from the unsnapped scroll position while the clip moved by whole pixels;
columns are now anchored to the clip's pixel edge (ADR-0027). Check in run
2: scroll slowly and quickly with Shift+wheel, and scroll tracks: the
waveform moves steadily, without shimmer.

**Run 2 (2026-10-07, owner):** scrolling fixed; two findings, fixed:
- A little of the same shimmer while zooming: the shader now reads four or
  more buckets per pixel.
- At the closest zoom the waveform disappeared (a flat line) near the
  start of a long file: the atlas copy overwrote the newest peaks when it
  grew (ADR-0027).

Check in run 3: zoom in and out slowly over a long file; then zoom all the
way in at its start, middle and end: the waveform is there everywhere and
steady while zooming.

**Run 3 (2026-10-07, owner):** passed ("it looks good").

**Last run:** run 3, passed.

### MT-APP-12 · Navigation (needs a human)
Run `cargo run --release -p lumen-app` with a project that has a few audio
tracks (one long file helps).
1. **Zoom:** Ctrl+wheel over a clip zooms around the pointer; keep going
   until single samples show as a connected line (8 points apart). The
   waveform stays sharp all the way, with no blocks or flicker, and the
   ruler shows beats and sixteenths when zoomed in.
2. **Keys:** `+` and `-` glide in and out around the pointer (or the
   playhead); W shows the whole song; X steps back through earlier views;
   H fits every track's height, and H again puts them back; Alt+wheel over
   a track makes it taller or shorter in steps.
3. **Deep and far:** zoom all the way in an hour into a long session (or
   near the end of a long file) and scroll: nothing jitters or drifts.
   Zoom all the way out: it stops with the whole song (and a little room
   after it) across the width; the grid stays even, with lines every few
   bars, and Shift+wheel scrolls until the song's end reaches the right
   edge.
4. **Follow:** play; when the playhead reaches the right edge the view
   turns a page. Right-click the Follow button (the arrow next to the
   metronome) and choose Continuous: the playhead stays put and the song
   scrolls. Scroll by hand while playing: Follow pauses (the button
   unlights); press Play again, or click the button: it follows again.
   Ctrl+Shift+F turns it on and off. It is remembered after a restart.
5. **Reorder:** drag a track's header up or down: a blue line shows where
   it lands; drop it. Select several and drag them together. Drag toward
   the top or bottom edge of a long list: it scrolls. Esc cancels a drag.
   Ctrl+Z undoes a move in one step.
6. Try steps 1, 2 and 5 in the light theme and at 150%.

**Expected:** every step as described, smooth and without lag.

**Run 1 (2026-10-07, owner):** sample-level zoom "looks solid". One
finding: zooming far out had no limit, the grid ended at bar 512 (empty
past it, and dense stripes before it), and the timeline seemed not to
scroll. Fixed: the song sets the widest zoom and how far the view
scrolls, as in Ableton, and grid lines cover any view and thin out
(ADR-0028). Re-run step 3.

**Run 2 (2026-10-08, owner):** passed ("it looks good"); zoom-out stops
at the song and the grid stays even.

**Last run:** run 2, passed.

### MT-APP-13 · Grid, rulers, loop, locators and scrubbing (needs a human)
Run `cargo run --release -p lumen-app` with a project that has a drum loop
and a melodic or vocal track, through the Focusrite.
1. **Grid:** right-click the lanes: the grid menu. Try Narrow and Wide,
   a fixed 1/16, Triplet and Dotted; the lines follow and the grid button
   (top-left, by "Grid") shows the setting. Ctrl+1 and Ctrl+2 make it
   finer and coarser, Ctrl+3 triplet, Ctrl+4 snap on or off, Ctrl+5 fixed
   or adaptive. Click the ruler: the playhead lands on a line; hold Alt:
   it lands exactly where you click.
2. **Rulers:** the time ruler along the bottom; click its corner and try
   each format (minutes and seconds, samples, timecode). Drag down on the
   bar ruler: it zooms in around the pointer; up zooms out; sideways
   scrolls; the playhead does not move.
3. **Loop:** drag in the strip's bottom band (below the flags) to draw a
   loop; it snaps and plays. Drag it to move it, drag an edge to resize,
   click it to turn looping off and on. Each is one Ctrl+Z. Right-click
   the brace or the Loop button: punch in and out show as red posts at
   the loop's edges.
4. **Locators:** press M while stopped: a flag appears at the playhead.
   Move the playhead and click Set (top-left). Ctrl+Left and Ctrl+Right,
   and ◂ ▸, jump between them; clicking a flag jumps there. Drag a flag:
   it snaps (Alt frees it). Double-click it (or select it and press F2),
   type a name, Enter. Delete removes the selected one. M on a flag
   removes it. Save, reopen: the flags are there.
5. **Scrub, by ear:** stopped, drag slowly along the top of the strip
   (above the loop band). You hear the beat under the pointer looping at
   normal pitch, the step lit on the strip; moving to the next step
   changes cleanly, without clicks. Change the grid to 1/16 and 1 bar and
   try again: the loops follow the grid. Let go: silence, and the
   playhead stays there.
6. **Tape scrub, by ear:** hold Alt and drag: the audio follows your hand,
   higher and faster when you move fast, backwards when you drag left,
   silent when you hold still. No clicks when you start, stop or change
   direction.
7. **While playing:** drag along the strip: playback jumps from step to
   step. A click on the strip jumps there too.
8. Try steps 3 to 5 in the light theme and at 150%.

**Expected:** every step as described; the scrub is clean (no clicks,
crackles or dropouts) at a 128-sample buffer.

**Run 1 (2026-10-08, owner):** four findings.
1. Resizing the loop by its edges is hard and confusing: nothing shows
   whether a drag will move or resize it. Hover hints, from specimens.
2. Scrubbing needs a clearer cursor and hints. From specimens.
3. The grid button shows "Off" when snapping is off, hiding the grid
   size; it isn't clear that "Snap to grid" changes it. From specimens.
4. A locator could only be added at the playhead. Fixed: right-click the
   scrub strip, "Add locator here" (snapped), as in Ableton; the playhead
   stays (E2E `a_locator_is_added_where_the_strip_is_right_clicked_without_moving_the_playhead`).
   Re-run step 4.

Fixes from the owner's choices of specimens (A1, B2 on trial, C1):
1. The brace: near an edge a bracket rises and a chip says "Loop start" or
   "Loop end"; inside it lights and says "Move loop · click: on or off";
   elsewhere "Drag to draw a loop". The edge zone grew from 5 to 8
   points, and a bug that turned edge grabs into moves (the grab was read
   a few points into the drag) is fixed.
2. The scrub strip: the step a drag would play is washed and a caption
   says "drag to listen · Alt tape" (with Alt: "drag to scrub like tape";
   while playing: "click or drag to jump"); a tape scrub shows its speed.
3. The grid button always shows the size, after a magnet: blue while
   snapping, struck through when not.
E2E: `hovering_the_brace_says_what_a_drag_will_do`,
`the_strip_hints_what_a_drag_will_do`, and the grid keys scenario
("Grid: 1/2T, snap off"). Owner, after run 1: the magnet is also a
switch, one click turns snapping on or off; the menu item stays
(`the_magnet_turns_snapping_on_and_off`). Re-run steps 1, 3, 4 and 5.

**Run 2 (2026-10-08, owner):** passed ("looks great"), with the hints,
the magnet switch and adding locators on the strip.

**Last run:** run 2, passed.

### MT-APP-14 · Clip editing (needs a human)
Run `cargo run --release -p lumen-app` with a project that has a drum loop
and a long melodic or vocal clip, through the Focusrite. Make the track
taller for steps 1 to 4, then try them again at the default height.
1. **Fades:** hover a clip: open rings wait in its top corners. Drag the
   left one in: the fade in follows the pointer with its length beside
   it, and the waveform tapers along the glowing curve. Do the same on
   the right. A fade never runs past the other.
2. **Bending:** drag a fade's dot anywhere, left and right as well as up
   and down: the curve follows through it, and the waveform with it. Hold
   Alt and drag up: the curve grows an S (flat at both ends) without the
   dot moving; down takes it away. Right-click the clip: Fade-in shape and
   Fade-out shape list the four presets. Double-click a dot: straight
   again; double-click a fade's handle: no fade. Save, close and reopen:
   the curves are as you left them.
3. **Gain:** hover the clip; the thin line near its top is its gain. Drag
   it down: the dB shows beside the pointer and the waveform shrinks;
   hold Shift for fine steps. Double-click it: 0 dB.
4. **Heard as drawn:** play the clip through its fades and listen: they
   sound as drawn, with no clicks. Ctrl+Z takes back each drag in one
   step.
5. **While playing:** drag a clip back and forth under the playhead, trim
   it and delete it: no clicks or crackle (owner, after 6c).
6. Try steps 1 to 3 in the light theme and at 150%.

Slice 6e (clip properties and drops):
7. **Mute:** click a clip and press 0: it turns grey (name and waveform
   still readable) and goes silent while playing, without a click. 0
   again brings it back. Ctrl+Z takes back one step.
8. **Lock:** right-click a clip, Lock clip: a padlock ends its title bar
   and light bars clamp both ends. It cannot be dragged, trimmed, faded or
   deleted (hovering says "locked"), but 0 still mutes it. Right-click,
   Unlock clip.
9. **Rename:** select a clip, press Ctrl+R: a field opens over its title
   bar. Type a name, Enter. F2 does the same; Esc keeps the old name.
10. **Colour:** right-click a clip, Colour: pick a swatch; the title bar,
    border and waveform take it. Colour, Track colour (the round one)
    follows the track again.
11. **The card:** hover a clip: the hover card ends with "Alt+Enter: edit
    its properties". Select the clip, press Alt+Enter: the card opens
    beside it with the name ready to type. Using only the keyboard (Tab
    between fields, type values, Enter): rename it, set the gain to −6 dB
    and a 250 ms fade in, choose a fade shape. Each change is heard and
    drawn. Click another clip: the card moves to it. Esc closes it. Lock
    a clip in its card: gain and fades grey out with the reason.
12. **Dropping files:** drag a WAV from Explorer over an audio track's
    lane: the lane lights, with a ghost clip named after the file at the
    snapped point. Drop: it lands on that track there (cutting away what
    it covers). Drag two files over the line between two tracks: an
    accent line with "new track here"; drop: two new tracks there. Below
    the last track: new tracks at the end.
13. Try steps 10 and 11 in the light theme and at 150%.

**Expected:** every step as described; no clicks.

**Run 1 (2026-10-08, owner):** steps 1 to 6 (slice 6d: fades, free bend
curves, clip gain, heard as drawn) passed ("looks good"). The steps for
6e (mute, lock, rename, recolour, the properties view, drops on a chosen
track) are added with that part and run as run 2.

**Run 2 (2026-10-08, owner):** three changes asked for (two not about 6e),
all made:
1. The right-click menu had grown too long (33 rows). The owner chose
   proposal A from specimens: each right-click gets the menu for what it
   landed on. Also fixed: Esc did not close the menu while clips were
   selected. Steps 14 and 15.
2. The grid should show a little through clips, as in Ableton. The owner
   chose G1 (a see-through clip body). Step 16.
3. A click anywhere on the lanes should move the playhead, as in Ableton.
   Step 17.

14. **Menus:** right-click a clip: 11 rows (Cut, Copy, Duplicate, Delete;
    Rename, Colour, Mute clip, Lock clip, Fades, Properties), no grid.
    Fades holds both fade shapes under headings. Right-click empty lane
    space: Grid (the whole grid menu one level in) and Snap to grid;
    Paste too once something is copied. Drag a time range, right-click
    inside it: its edits with Delete time and Insert time.
15. With a clip selected, right-click it and press Esc: the menu closes
    and the clip stays selected. No drag hint sits on an open menu.
16. **Grid behind clips:** at any zoom, the grid lines show faintly
    through clip bodies (title bars and waveforms stay solid), in both
    themes.
17. **Click to move the playhead:** click empty lane space: the playhead
    jumps there, snapped to the grid (Alt: exactly there). Click a clip:
    it is selected and the playhead jumps too. Ctrl+click and Shift+click
    only change the selection. While playing, a click jumps playback.

**Run 3 (2026-10-08, owner):** steps 7 to 11 and 13 to 17 passed. Two
findings, both fixed:
1. The card's ✕ sat higher than its title "Clip". It was centred on the
   title's line, which leaves room below for descenders; every dialog had
   the same offset (5.5 points). One title row (`widgets::title_row`) now
   centres it on the title's letters, for the card and every dialog. Step
   18.
2. Step 12: dropping between tracks was never offered. On Windows, egui
   gets no pointer while Explorer drags files over the window, so every
   drop went to new tracks at the end. Lumen now reads the system's
   cursor during the drag (ADR-0032). The E2E scenarios now drag files in
   without a pointer, as Windows does. Step 19.

18. **Title rows:** open the clip card (Alt+Enter) and Audio settings:
    the ✕ is level with the middle of the title's letters, at 100% and
    150%.
19. **Drops, again:** drag a WAV from Explorer over the lanes and move it
    around without letting go: the lit lane and ghost clip, or the
    "new track here" line, follow the pointer. Drop on an audio track's
    lane: it lands there. Drop on the line between two tracks: a new
    track there. Below the last track: a new track at the end.

**Run 4 (2026-10-08, owner):** steps 18 and 19 passed ("looks good").

**Last run:** run 4, passed. MT-APP-14 is done.

### MT-APP-15 · Commands, the palette and shortcuts (needs a human)
Run `cargo run --release -p lumen-app` with a project that has a few
tracks with clips and a locator or two. Steps 1 to 7 cover 7a and 7b,
steps 8 to 12 7c, steps 13 to 19 7d.
1. **Keys as before:** Space, Shift+Space, Home, Ctrl+Z, Ctrl+Shift+Z,
   Ctrl+D, Delete, 0, Alt+Enter, Ctrl+T, Alt+Up, the arrows, `+`, `-`, X,
   W, H, Ctrl+1 to Ctrl+5, M and Ctrl+Left or Right all do what they did.
   Ctrl+R now renames a selected track or locator too.
2. **Exact chords:** with a clip selected, Alt+0 and Shift+Delete do
   nothing (they used to mute and delete).
3. **Opening:** Ctrl+K opens the palette under the transport bar, with
   the cursor in its field; the magnifier at the right of the transport
   bar opens and closes it too. Esc, Ctrl+K or a click elsewhere closes it.
4. **Searching:** type "dup" with two clips selected: "Duplicate 2 clips"
   comes first, its matched letters in blue, with Ctrl and an outlined D
   with a blue light under it. The arrows move the choice (the strip
   below says what it does); Enter runs it. Open it again empty: the
   command just run is under "Recent".
5. **Can't run:** type "paste" with nothing copied: Paste is greyed, the
   strip says why in amber, and Enter leaves the palette open.
6. **Values and names:** type "128": "Set tempo to 128 BPM" and "Go to bar
   128". Run the first: the tempo changes (Ctrl+Z takes it back). Type a
   track's name: "Go to track “…”" selects it and scrolls to it. Type a
   locator's name: the playhead jumps there.
7. Try steps 3 to 6 in the light theme and at 150%; at a small window
   the list gets shorter rather than covering the transport bar.
8. **The shortcuts window:** open it from the Lumen menu (Keyboard
   shortcuts…) or the palette. The keyboard shows what each key does,
   with a coloured light per area. Click Ctrl (or hold Ctrl): the Ctrl
   layer shows. Click a key: the list below finds its command.
9. **Changing one:** search "fit song", click its W, press Alt+W. Close
   the window: Alt+W now fits the song and W does nothing. Restart Lumen:
   Alt+W still fits it.
10. **A clash:** click a shortcut and press Ctrl+D: the row says Ctrl+D is
    used by Duplicate, with Reassign and Cancel. Esc while recording
    cancels the recording, not the window. ↺ and Reset all put things
    back.
11. **From the palette:** search a command, press Ctrl+Enter, press new
    keys: it changes there.
12. Try steps 8 to 10 in the light theme and at 150%.
13. **Menus and tooltips:** right-click a clip, a track (its name too) and
    the bar ruler: every shortcut shows as keycaps at the right, with the
    area's light. Hover Play, Stop and Follow: the tooltip shows the keys
    and a hint. Change a shortcut (step 9): the menus and tooltips show
    the new one.
14. **The rulers' menus:** right-click the bar ruler: Zoom in, Zoom out,
    Zoom back, Zoom to selection, Fit song to width, Fit loop, Fit tracks
    to height and Follow playhead. Right-click the time ruler: the time
    formats.
15. **New keys:** select a clip and press Z: it fills the view; X goes
    back. Ctrl+A selects every clip; Ctrl+L loops them (the brace and the
    loop button light). Fit loop (palette or ruler menu) shows the loop.
16. **Panes:** press F6: a blue ring goes round the arrangement; again:
    the detail panel; again: the browser. Shift+F6 goes back. With a pane
    ringed, Tab goes on to its controls.
17. **Shift+Tab:** with nothing being typed, it hides and shows the
    detail panel. While renaming a track (F2), it moves the cursor back
    instead, and the panel stays.
18. **Keys with a control focused:** press Tab until a button has a blue
    ring: Ctrl+T still adds a track, and Space presses the button rather
    than playing.
19. Try steps 13 and 16 in the light theme and at 150%.

**Expected:** every step as described.

**Run 1 (2026-10-08, owner):** passed ("looks good").

**Last run:** run 1, passed. MT-APP-15 is done.

### MT-APP-16 · The M3 exit flow and the review's fixes (needs a human)
Run `cargo run --release -p lumen-app`. Have three or four short audio
files in a folder (any of WAV, AIFF, FLAC, MP3, Ogg). Steps 1 to 4 are
the milestone's exit flow; steps 5 to 9 cover what the end-of-milestone
review changed.
1. **Create and import:** start a New project (Lumen menu). The empty
   arrangement shows "Drop audio files here" with an **Import audio…**
   button under it. Press Ctrl+T, then Ctrl+Shift+I: the system's file
   dialog opens in front of Lumen, offering audio files only. Pick two
   files: they land one after the other on the new track, at the
   playhead. Cancel the dialog another time: nothing changes.
2. **Arrange:** drag a file from Explorer to just below the track: a new
   track. Drag a clip along, Ctrl+D one, rename one (F2), mute one (0).
   Open the palette (Ctrl+K), type "128", Enter: the tempo is 128. Press M
   for a locator; select a clip and press Ctrl+L to loop it.
3. **Save and reopen:** Ctrl+S and save it somewhere new. Close Lumen,
   start it again and open the project from Open recent: the tracks,
   clips, names, mute, tempo, locator, loop and view are as saved, and it
   plays the same.
4. Import from the other places too: the Lumen menu (Import audio…), a
   track's right-click menu (onto that track; onto new tracks after it if
   it is a MIDI track), and the palette.
5. **Clips from the keyboard (the owner's choice for H2):** with several
   clips on two or three tracks, press Ctrl+A, then ←: one clip is
   selected. → and ← go along its track; ↓ and ↑ go to the nearest clip on
   the next track with clips. Shift+→ and Shift+↓ add to the selection.
   Press 0: the chosen clip mutes. Zoom in until a clip is off screen and
   arrow to it: the view slides to show it.
6. With a track selected (click its header), ↑ and ↓ still select tracks
   and ← and → fold and unfold, as before.
7. **The smooth playhead:** turn Follow on, zoom in a little and play: the
   playhead and the page turns move evenly, with no stepping or jitter.
   With a loop on, the wrap back is smooth. Stop: the playhead stops
   exactly where playback stopped.
8. **Time signature:** drag "Beats per bar" up: it stops at 64.
9. Try steps 1, 5 and 7 in the light theme and at 150%.

**Expected:** every step as described.

**Run 1 (2026-10-09, owner):** passed ("everything passed").

**Last run:** run 1, passed. MT-APP-16 is done.

### MT-APP-17 · The info line (needs a human)
Run `cargo run --release -p lumen-app` with a project that has a few
clips with fades, a loop and a locator.
1. **Nothing floats:** hover a clip's top and bottom halves, its edges,
   its fade and gain handles, the loop brace, the scrub strip and both
   rulers. No chip appears by the pointer; the cursor changes, and the
   info line along the bottom names the thing and what you can do there.
2. **Keys light up:** hover a clip's top half and hold Ctrl: "Ctrl drag
   copy" lights and the cursor shows a copy; add Alt: "slip the audio"
   lights instead. On a clip's end, Shift lights "drag loop".
3. **Keycaps match:** shortcuts in the line look like the palette's and the
   menus' (outlined key, coloured light). Change Duplicate's shortcut in
   Keyboard shortcuts: the info line shows the new keys.
4. **Values still follow the pointer while dragging:** drag a fade (its
   length), the gain line (dB), and Alt-drag the scrub strip (tape speed).
5. **Narrow window:** make the window narrow: the line stops with "…"
   instead of overlapping.
6. **Show info line** (palette): hides the strip; again brings it back; it
   stays as set after restarting Lumen.
7. Try steps 1 to 3 in the light theme and at 150%.

**Expected:** every step as described, and the arrangement reads as
calmer than before.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4.

### MT-MIX-1 · The mixer graph (needs a human, with the audio interface)
Run `cargo run --release -p lumen-app` with ASIO on the Focusrite, and a
project with about ten tracks of audio (drums, bass, pads: anything
continuous is best), on headphones.
1. **Levels:** play. Centred stereo tracks sound as loud as before M4;
   tracks of mono files sound about 3 dB quieter (the −3 dB pan law applies
   to mono tracks only, the owner's B after slice 7); nothing else about
   the sound changes.
2. **Pan:** pan one track hard left, then hard right: the other side goes
   silent, the near side is a little louder than at the centre. Drag pan
   back and forth quickly: no crackle or zipper noise.
3. **Volume:** drag a track's volume up and down quickly while playing:
   smooth, no crackle. Double-click it back to 0 dB: a quick glide, no
   click.
4. **Mute and solo:** mute and unmute a track, solo one and then another,
   in time with the music: each change is a fast fade, never a click.
5. **Adding and removing:** while playing, drop a new file onto the
   arrangement (a new track): it comes in without a pop. Delete a playing
   track, then undo: it goes and comes back without a click.
6. **Buffer sizes:** in Audio settings, and in the Focusrite's own
   control panel, try 64, 128 and 512 samples while playing all tracks:
   no amber "Dropouts" chip appears for the changes themselves, and CPU
   in the transport bar is about what it was before M4.
7. **Loop and locate:** loop two bars over all tracks and click around
   the ruler while playing: seamless loop, click-free jumps, as before.
8. **Dropouts in words:** make the audio drop out on purpose (32 samples
   with all tracks playing, or a heavy app starting): "⚠ Dropouts ×N"
   appears after the CPU readout and fades about 10 s after the audio is
   clean again; while it shows, the audio panel says how many just now
   and since audio started, and what helps, and both go together. Hover
   the Dropouts count among the meters: it says how many the driver
   skipped and how many arrived late. Reset clears them.

**Expected:** every step as described.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4. The dropout count on the Focusrite (ASIO) is confirmed: the KNOWN_ISSUES entry is closed.

### MT-MIX-2 · Projects from before the mixer (needs a human)
Run `cargo run --release -p lumen-app`. Use a copy of a project saved
before M4 (keep the original).
1. **Opens:** open the copy. It looks as before, with the same tracks; no
   master track appears yet (slice 4 shows it).
2. **Plays:** play it: everything is heard, stereo tracks as loud as
   before, tracks of mono files 3 dB quieter (the −3 dB pan law, for mono
   tracks).
3. **Saves and reopens:** change something small, save, close Lumen, and
   open the project again: it opens unchanged, and plays the same.
4. **Undo still works:** change volume, pan and mute on a track, then undo
   each: every change undoes in one step, as before.

**Expected:** every step as described, no warnings about the file.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4.

### MT-MIX-3 · Group tracks (needs a human)
Run `cargo run --release -p lumen-app`. Start a new project and drop in
five or six audio files (drums, bass, anything): one track each.
1. **Group:** select the first three tracks (click, Shift+click) and press
   Ctrl+G. A group named "Group 1" appears where the first was, its name a
   little bolder, and the three tracks step in under it with the group's
   colour rail running down beside them. Ctrl+Z takes it back in one step;
   Ctrl+Shift+Z brings it back.
2. **The group's lane:** it shows a thin bar per track inside, in each
   track's colour, wherever that track has clips. Play: the group sounds
   as the three tracks did; mute the group and they all go silent.
3. **Nest:** select one track inside the group and press Ctrl+G: a second
   group inside the first, one step further in, with both rails beside
   its track.
4. **Fold:** click the group's arrow: its tracks hide, the name reads
   "Group 1 · 4", and the lane keeps its bars. Click again to show them.
   Alt+click folds or unfolds every group. With the group selected, Left
   and Right do the same. Save, close and reopen: the group is still
   folded.
5. **Drag in and out:** drag a track outside the group onto the middle of
   the group's header: the header lights, and the track goes in at the
   end. Drag a track from inside to between two tracks below the group:
   the line steps out to the left edge, and it lands outside. Drag the
   group itself: everything inside moves with it.
6. **Arm:** click the group's arm button: every track inside arms, and
   the group's button breathes. Disarm one track: the group's button shows
   a lit dot with half a ring. Click it again: all armed.
7. **Ungroup and take out:** select a track inside the group and press
   Ctrl+Shift+G: it comes out to just below the group. Select the group
   and press Ctrl+Shift+G: the group goes and its tracks stay, a level up,
   selected.
8. **Delete and duplicate:** group two tracks again, then Ctrl+D with the
   group selected: a copy of the group with copies of its tracks appears
   below it. Delete the copy: it goes with its tracks, in one undo step.
9. **Files onto a group:** drag an audio file from Explorer onto the
   group's lane: the hint says "new track in Group 1" and the file lands
   on a new track at the end of the group.
10. **Looks:** check steps 1, 4 and 6 in the light theme and at 150% and
    200%: rails line up with their groups, nothing overlaps, long names end
    in "…".
11. **Three deep** (run 1): with a track inside three nested groups,
    select it and press Ctrl+G: nothing changes and a note says "Groups go
    three deep at most". Drag a group onto the innermost group's header:
    the frame and cursor turn grey (no entry), and letting go leaves it
    where it was, with the same note.
12. **Solo inside a group** (run 1): play, and solo a track inside a
    nested group: it is heard (alone); the other tracks in its group are
    silent. Solo the group itself: everything inside it is heard.
13. **Header width** (run 1): drag the edge between the track headers and
    the lanes to the right: the headers widen (up to about twice), long
    names show in full, the edge glows while dragged. Save, close and
    reopen: the width is kept. Double-click the edge: back to the narrow
    default.

**Expected:** every step as described, each edit undoes in one step, and
nothing clicks while grouping during playback.

**Run 1 (2026-10-09, owner):** three findings, all fixed (ADR-0037
amendment): nesting went on without limit (the owner: three deep is
enough); names were cut short in the fixed-width headers (the column is
now resizable, saved per project, format v9); soloing a track inside a
group silenced it (solo now keeps its groups open). Steps 11 to 13
added.

**Run 2 (2026-10-09, owner):** two more findings, both fixed (ADR-0037
amendment): deleting a track from its right-click menu crashed Lumen;
tracks in groups played 3 dB quieter a level (buses now stay at unity
when centred). Then passed: "everything looks good".

### MT-MIX-4 · Mute, solo, returns, master and cue (needs a human)
Run `cargo run --release -p lumen-app`. Start a new project, drop in four
audio files (drums, bass, a vocal, anything) and group the first two
(Ctrl+G).
1. **Solo and the hollow M:** play, and click S on the bass. Only the bass
   is heard. The other tracks' M buttons show a hollow ring and their
   names dim: silent, but not muted by you. The transport shows a lit
   circular "S" chip. Ctrl+click S on the vocal: both are heard, and
   hovering the chip reports two solos. Click it: every solo clears,
   in one undo step.
2. **Solo a group:** click S on the group. Its tracks show a hollow S
   (heard through the group) and play; the others are silent. Mute one
   track inside: it goes silent although the group is soloed (mute beats
   solo).
3. **Returns at the bottom:** open "+ Add track ▾" and add two return
   tracks. They appear under a divider at the bottom, lettered A and B in
   their colours, with the master last; they stay put while the tracks
   above scroll. Click the "Returns" caption: they fold away (the master
   stays); click again. Drag the divider up and down: the area resizes and
   the returns scroll inside it; double-click it for its natural size.
   Save, close and reopen: the size and fold are kept.
4. **Sends (comet knobs):** unfold the vocal (its arrow). Under volume and
   pan sit two knobs, A and B, in the returns' colours. Drag A upwards:
   a trail of light grows round the ring to a glowing head; the level
   shows beside it while dragging. Hover the knob: the A return's header
   lights up; hover the A return: the knob lights. Double-click the knob:
   silent. Right-click it: Pre-fader puts a small dot at the top. Click
   it and type -12, Enter: the send is at −12 dB.
5. **Soloed return, wet only:** turn the vocal's send to A up and solo
   return A. You hear only what the vocal sends there (until M8's reverb,
   the return plays it unchanged), not the vocal itself; the vocal's M is
   hollow. Mute the vocal: the send goes too.
6. **Master:** unfold the master: volume and pan work as on a track.
7. **Additive solo:** in the Lumen menu tick "Additive solo" (or run it
   from Ctrl+K). Now a click on S adds the track and Ctrl+click solos it
   alone. Untick it again.
8. **Palette:** select a track, Ctrl+K "Solo selected tracks", then "Mute
   selected tracks", then "Clear all solos": each acts and undoes in one
   step.
9. **Cue (no second output pair):** your Focusrite has only outputs 1/2,
   so the master's Solo/Cue switch is grey; clicking it says to choose a
   Cue output in Audio settings, and Audio settings › Cue says this device
   has no second pair. (Cue itself is checked by the E2E tests on the
   four-output test device; on a four-output interface: choose Cue 3/4,
   switch to Cue, click a track's headphone button, and it plays in the
   headphones before its fader while the speakers play on.)
10. **Metronome:** right-click the metronome: Output Main is chosen, Cue
    is grey without a cue pair. Clicks play on 1/2 and are never in the
    master (turning the master down leaves them as loud).
11. **Looks:** check steps 1, 3 and 4 in the light theme and at 150% and
    200%: letters, knobs and the hollow rings line up; nothing overlaps.

**Expected:** every step as described; solo, mute and send edits undo in
one step; nothing clicks while soloing, muting or turning sends during
playback.

**Run 1 (2026-10-09, owner):** four findings. (1) Sending to a return
makes the track louder: expected while returns hold no effect (the return
plays a copy of what is sent; a 0 dB send adds +6 dB), as in every DAW;
explained, waiting for the owner to confirm it was not louder at −∞. (2)
The send knobs are hard to read on a selected (blue) row. (3) With many
returns, the sends past "+N" cannot be reached. (4) A return should show
which tracks send to it. Mock-ups for 2 to 4 shown before building (the
owner asked for mocks for every visual change); the owner chose C4 (after
a second round of ideas), O1 with a redrawn button, and R2. The owner
confirmed (1) also happened at the knob's lowest: the bottom stored
−80 dB instead of silence (fixed; inaudible, but not off). A louder track
with a send at the bottom was not reproduced (E2E measures the same level
before sending and at the bottom); a pre-fader send ignores the track's
fader, which can sound louder than the fader says. Fixed and built
(ADR-0038 amendment); steps 12 to 14 added.
12. **Knobs on a selected track** (run 1): select an unfolded track with
    sends: the knobs' rings and letters stay clear on the blue row, in
    both themes.
13. **Many returns** (run 1): add returns until the header shows "+N ▾"
    after its knobs (drawn like "+ Add track ▾"). Click it: "Sends from …"
    lists every return with its knob, letter, name and level; set one
    there. A dot before the number shows when a hidden send is up.
14. **Who sends to a return** (run 1): unfold a return: under volume and
    pan, every track that sends to it is listed, in its colour, with its
    send knob and level; turn one there. With nothing sending: "No tracks
    send here yet". Turn a send to the bottom: it reads −∞ and the track
    is exactly as loud as with no send.

**Run 2 (2026-10-09, owner):** five findings. (1) A return's sender list
does not fit many senders; (2) its knobs touch each other (20 pt knobs
with hover rings on 22 pt rows, every one ringed while the return is
hovered); (3) senders turned off stay listed; (4) cap returns at 12
(built: the menu greys the item, Ctrl+D copies no return past twelve, a
note says why); (5) a 0 dB send to an empty return still makes the track
louder: explained again (+6 dB, the return plays a copy until it holds an
effect, as in every DAW), options offered. Mock-ups for 1 to 3 shown
before building. The owner chose L4 (the four loudest senders, then
"+N more ▾" for all of them) and S1 (a send turned off leaves the list
when let go), and kept the standard behaviour for (5). Built (ADR-0038
amendment 2); steps 15 and 16 added.
15. **Many senders** (run 2): send six tracks into one return at
    different levels and unfold it: the four loudest are listed, then
    "+2 more ▾", which opens all six, loudest first, each with its knob.
    The knobs do not touch. While you drag a knob the rows keep still.
16. **Senders turned off** (run 2): turn a sender down to the bottom from
    the return's list: its row stays while you hold it and goes when you
    let go. Double-click a send knob on the track: it leaves the list too;
    its pre/post-fader setting is kept for when it is turned up again.
    Add twelve returns: "Add return track" greys out, saying why.

**Run 3 (2026-10-09, owner):** three findings, all fixed. (1) Dragging
a knob in a return's list drew its live level over the row's level (two
values, blurred): list knobs no longer draw a live value; their level
column follows. (2) On a track's header the live level ran over the next
knob: it is now a chip above the knob, over everything (`draw_hint` on a
tooltip layer). (3) "+N more" with many senders ran off the window: both
"+N" lists scroll past ten rows.

**Run 4 (2026-10-09, owner):** after duplicating a track, setting one's
send seemed to change the other's. Sends and their knobs were already
keyed by track and return ids (E2E: the copy's send is its own in the
project and by ear). The cause was the return's list re-sorting by level
after each change: the two senders, both named after the original,
swapped rows, so the row just set showed the other track. The list now
re-sorts only when a sender joins or leaves (E2E
`turning_a_sender_down_in_a_returns_list_leaves_the_rows_in_place`
failed before the fix).

**Run 5 (2026-10-09, owner):** hard to turn a send up from −∞. Each frame
the knob started again from its level plus that frame's movement, and
below the silence floor (about 7 points of travel) a slow drag was
rounded back to silence every frame, so only a quick flick got out. A
drag now keeps its own position from where it was pressed: 16 points up
from −∞ gives about −42 dB, 32 about −28 dB (E2E
`a_slow_drag_turns_a_silent_send_up` stayed at −∞ before the fix).

**Passed (2026-10-09, owner):** "looks good now". Step 9's cue on a
four-output interface is still to be tried by hand (KNOWN_ISSUES); cue is
checked by E2E on the four-output test device.


### MT-MIX-5 · The mixer panel and meters (needs a human)
Run `cargo run --release -p lumen-app`. Start a new project, drop in four
audio files (drums, bass, a vocal, anything loud), group the first two
(Ctrl+G) and add two return tracks ("+ Add track ▾").
1. **Header meters (H2):** play. Each unfolded track's level runs along
   the bottom of its volume slider, under the value that sets it: a faint
   peak behind a solid level, a small tick holding the highest peak for a
   moment. Folded tracks show it as a thin line along the header's bottom
   edge. Below −6 dB it is blue, then amber, then red past 0 dB.
2. **Stopping:** stop. Every meter falls and comes to rest within about
   three seconds; nothing keeps moving. Task Manager shows Lumen's CPU
   back near idle.
3. **Clip lights:** push a loud track's volume to +6 dB and play until it
   clips: a red mark lights at the end of its meter and stays after you
   stop. Click it: it clears. Make two tracks clip, then Alt+click one
   mark: both clear. Clip again and run "Clear clip lights" from Ctrl+K.
4. **The Mixer tab:** open the detail panel (Shift+Tab, or the Detail
   button). The Mixer tab shows a strip per track in the arrangement's
   order, the returns after a gap, and the master pinned at the right with
   the loudness column. Fold the group in the arrangement: its tracks'
   strips go too; unfold it from its strip's arrow: they come back in both.
5. **A strip:** on one strip, drag the fader (Shift for fine; double-click
   for 0 dB), click the value under it and type -12, Enter. Drag pan, flip
   Ø, drag the trim. Turn send A up on its knob; with more than two
   returns, "+N ▾" lists them all. Click M and S: the header shows the
   same. Each change undoes in one step (Ctrl+Z).
6. **The wheel (B3):** rest the pointer on a fader and turn the wheel: a
   dB a notch (Shift: a tenth), and Ctrl+Z undoes the whole turn. Start
   scrolling the track list with the pointer away from the headers, then
   move it over a volume slider while still scrolling: the list keeps
   scrolling and the volume does not change. Rest a moment, then wheel on
   the slider: now it changes.
7. **Outputs:** click a strip's output ("Master ▾"). It lists the master
   and the groups (never a group into itself), then the Focusrite's
   outputs: "1/2 Main". Route a track into the group from here.
8. **Reordering (B8):** drag a strip by its name or meter to another
   place: the track moves in the arrangement too. Drop it on the middle of
   the group's strip: it goes into the group. Drag return B before return
   A: their letters swap.
9. **Loudness (L2):** play a song for a minute. The column shows the
   integrated loudness large, a short-term bar with a line for the
   momentary loudness, the −14 LUFS band and the last minute's history,
   then M, S and TP. Pause and continue (Shift+Space): integrated keeps
   counting. Stop and play (Space): it starts again. Click Reset: it starts
   again.
10. **Pan law:** right-click the mixer's empty area: "Pan law (mono
    tracks)" and the four laws, −3 dB chosen; pick −6 dB, linear: centred
    tracks of mono files get quieter, stereo tracks stay as they are.
    Ctrl+Z returns it.
11. **Panel size:** drag the detail panel taller and shorter. Short, the
    strips drop their sends, then trim, then the output, but keep the
    fader and meter; it never gets so tall that the arrangement loses its
    rulers. Save, close and reopen: the panel's size comes back.
12. **Meter colours:** tick "Meters in track colours" in the Lumen menu:
    meters take their track's colour (still amber and red near the top).
13. **Looks:** check steps 1, 4 and 9 in the light theme and at 150% and
    200%.

**Expected:** every step as described; nothing clicks or drops out while
metering, dragging strips or turning the wheel; undo restores each change
in one step.

**Run 1 (2026-10-10, owner, in progress):** three findings, mock-ups
first (the owner's rule): (1) strips are too narrow, so track names are
nearly always cut off ("Heavy …", "Retu…"); (2) on a header, the level
inside the volume slider runs right against the value's text; (3) the
volume and peak values overflow their fields past two digits ("−16.3").
**Fixes (2026-10-10, ADR-0040 amendment, mock-ups approved by the owner):**
(1) strips are 96 pt wide, so about eleven characters of a name show; (2)
the header's value text rises 3 pt clear of the level bars; (3) peak
readouts from 10 dB up drop the decimal ("−12"), and the volume field
fits a typed "−16.3" at 96 pt and 150% (checked by screenshot). Checked
without audio: the header's level bars did not draw on the silent test
device, so (2) needs a re-check with a track playing. **Run 2 waits for
the owner.**

**Run 2: passed (2026-10-10, owner),** including the header's level bars with a track playing.

### MT-MIX-6 · Delay compensation and its readout (needs a human, with the audio interface)
Nothing in Lumen adds latency until devices arrive (M6), so this test
puts the engine's synthetic latent device on the first track with a
developer flag. Run
`cargo run --release -p lumen-app -- --test-latency 50`
(50 ms on the first track). Start a new project and drop the same drum
loop on two tracks (two files, or the same file twice), so the first
track is the latent one.
1. **The readout (D1):** before dropping files, the device line ends
   with the round trip as before, nothing more. Once the first track
   exists, it ends with "· 50.0 ms compensation", a step brighter.
   Hover it: the info line along the bottom reads "Delay compensation
   50.0 ms · longest path: <first track>".
2. **The audio panel:** click the device line. Under the device, a
   Latency group: output, round trip, and "Delay compensation 50.0 ms ·
   2400 samples" (at 48 kHz), then "Longest path: <first track>".
3. **In time (read ahead):** turn the metronome on and play. The two
   tracks sound as one (no flam, no echo), and both are in time with the
   metronome and the playhead: the kick lands on the click and on the
   grid line as the playhead crosses it. Mute the second track and
   unmute it: no flam appears.
4. **Pressing Play:** the playhead waits about 50 ms at the start before
   it moves; nothing is lost from the start of the song.
5. **Loop:** set a one-bar loop over the loop and play: every wrap is
   seamless and on the click, as without the flag.
6. **Jumps:** click on the ruler while playing, a few times: each jump is
   clean (no click) and the audio after it is in time again at once.
7. **Polarity check (optional):** flip the second track's polarity (Ø on
   its strip). The two tracks cancel to silence while playing, which they
   only do if they are lined up to the sample.
8. **Without the flag:** quit and start Lumen normally: the readout is
   gone, playback starts at once, everything sounds as before.
9. **Looks:** check step 1 in the light theme and at 150%. In a narrow
   window the device's name shortens before the compensation does.

**Expected:** as described; nothing clicks while starting, looping or
jumping with compensation on; the latent track never drifts from the
others.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4.

### MT-MIX-7 · The M4 exit walkthrough (needs a human, with the audio interface)
The milestone's exit criterion by hand (ADR-0042), with two fixes from
this slice. Run `cargo run --release -p lumen-app`. You need a drum loop
split into two files (kick and snare, or any two drum sounds) and a vocal
or any melodic file.
1. **Build the mix:** a new project; drop the kick, snare and vocal files
   together on the lanes (three tracks). Select kick and snare (click the
   kick's name, Shift+Down) and press Ctrl+G: a group with both inside.
   Add a return track ("+ Add track ▾", Add return track).
2. **Sends:** open the Mixer tab (Shift+Tab if the panel is closed). On
   the group's strip, turn its send to Return 1 up to about 0 dB. Unfold
   the vocal's header and turn its send knob up too. Play: everything is
   heard, a little louder than before the sends (the return adds a copy).
3. **Solo the drums:** click S on the group. The drums play with their
   share of the return; the vocal is silent, and its M is hollow
   (silenced). The kick and snare show a hollow S (soloed through the
   group).
4. **Mute inside a soloed group:** click M on the kick: it goes, its share
   of the return too. The snare plays on.
5. **Add a solo:** Ctrl+click S on the vocal: snare and vocal play.
6. **The return alone:** click S on Return 1: only the wet copies play
   (snare and vocal through the return), quieter than before.
7. **Back to the mix:** click the transport's S chip to clear the solos,
   unmute the kick: the whole mix again. Mute Return 1: dry only.
8. **Save and reopen:** Ctrl+S, close Lumen, start it again and open the
   project: the group, sends, return and mutes are as you left them, and
   it sounds the same.
9. **Edits while stopped are heard from the first note** (a bug fixed in
   this slice): stop. Without playing, make five or more edits (move a
   clip, mute and unmute something), then mute the vocal and press Play
   from the start. The vocal is silent from the very first beat: no blip
   of it before the mute takes.
10. **Two Lumens and autosave** (fixed in this slice): with Lumen open,
    make an edit and wait a little over a minute (the autosave). Start a
    second Lumen from another PowerShell window with the same command.
    The second shows no "did not close normally" banner for the first's
    session. Close the second, then close the first normally.

**Expected:** each step sounds as described, with no clicks when soloing,
muting or clearing; the reopened project matches; step 9 has no blip;
step 10 has no banner.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4.

### MT-MIX-8 · Clipboard keys and the pan law for mono (needs a human)
Fixes after MT-MIX-7 was written (ADR-0042). Run
`cargo run --release -p lumen-app`.
1. **Ctrl+C and Ctrl+V:** two audio tracks with a clip on the first. Click
   the clip, press Ctrl+C. Click the second track's empty lane at bar 3,
   press Ctrl+V: the copy lands there. Try it with text on the Windows
   clipboard (copy a word in Notepad first) and with nothing copied
   elsewhere: both work.
2. **Ctrl+X:** click the copy, Ctrl+X: it goes; Ctrl+V puts it back at the
   playhead.
3. **Right-click Paste:** right-click an empty lane at bar 5 on the
   second track, Paste: the clip lands there; the playhead does not move.
4. **Delete time (Ctrl+Shift+Delete):** drag a time range on a clip's
   lower half, press Ctrl+Shift+Delete: the time goes and what follows
   moves back.
5. **Typing still pastes text:** rename a track (double-click its name),
   Ctrl+V pastes the clipboard's text into the name, not a clip.
6. **Loudness:** play a mastered song file (a stereo file whose loudness
   you know, for example from another DAW or a loudness meter): the
   integrated LUFS matches it within a few tenths. A mono file plays 3 dB
   down per side at the centre.

**Expected:** as described.

**Passed (2026-10-10, owner):** the owner reported every step passing at the end of M4.

### MT-APP-18 · Light and the mark (needs a human)
The light layer and the logo (ADR-0043, DESIGN.md §6a and §6b). Run
`cargo run --release -p lumen-app`.
1. **Window icon:** the taskbar and Alt+Tab show the Dawn mark (an amber
   sun over blue water on a dark rounded square), not the default icon.
2. **The mark at rest:** with nothing playing, the mark before "Lumen" in
   the transport bar shows the sun just peeking over the horizon, dimmed.
3. **Empty arrangement:** File › New. Above "Drop audio files here" the
   mark sits half risen. Drag an audio file from Explorer over the lanes:
   the mark rises fully above "Drop to add 1 audio file". Drag a text file
   instead: it stays half risen.
4. **The mark as a meter:** drop a song and play it. The sun rises and
   falls with the master, fully risen near full scale, and settles when
   you stop. It never flickers between frames.
5. **The playhead:** stopped, it is a thin blue line. Playing, it becomes
   a beam: a white core, a soft halo, a white cap, and a faint band of
   light on the lanes it passes. Stop: it is a thin line again.
6. **Play:** while playing, the Play button's icon is white with a soft
   blue halo around the button.
7. **Meter heads:** open the Mixer (Detail). Each playing meter has a thin
   white line with a soft glow at the top of its level, in amber and red
   zones too. The header sliders' level bars end in a short bright tick.
8. **Clip lights:** push a track past 0 dB: its clip light glows red in
   the header and on the strip; click it to clear.
9. **Existing glows:** select a clip (accent glow), hover a track's bottom
   edge and a clip's fade: they glow as before, the edges slightly softer
   and wider.
10. **Light theme:** repeat 2, 5 and 7 in the light theme: the glows are
    softer, the core is blue rather than white, the sun is a deeper amber.
11. **At 150% and 200%:** the mark stays crisp and centred in the
    transport bar, with nothing overlapping at the minimum window size.

**Expected:** as described; no glow appears on anything at rest.

## Milestone 4.5 — Polish pass

### MT-POL-1 · The audit's bugs (needs a human)
Slice 1 of M4.5 (`docs/milestones/M4.5.md`): findings T1, T2 and U1 of
`docs/audits/2026-10-10-design.md`. Run `cargo run --release -p lumen-app`.
1. **Fields on every strip (T1):** open a project with a return (or add
   one: + Add track ▾, Return track). In the Mixer tab, the volume and
   peak fields under the faders have the same grey fill on track strips,
   on the return and on the master. Hover one: it lightens. Select a track:
   its fields step up with the other controls.
2. **Wells in the light theme (T2):** Lumen menu → Light. Fader grooves,
   meter backgrounds and the loudness bar are darker than the strip under
   them, so you can see where each meter is with the transport stopped.
   Back to Dark: they look as before, a touch deeper.
3. **Scroll stays on the tracks (U1):** add about 20 tracks and scroll to
   the bottom. Select the last ten and delete them: the remaining tracks
   slide into view; no empty lanes with tracks hidden above. Group five
   tracks (Ctrl+G), scroll down, fold the group: the same.

**Expected:** as described.

**Passed (2026-10-10, owner):** "looks good".

### MT-POL-2 · Shared widgets and sizes (needs a human)
Slice 2 of M4.5: findings S2, S3, S4 and T3. Run
`cargo run --release -p lumen-app`.
1. **The slider handle (S2, your C):** unfold a track. Drag its volume
   slowly from −60 to +6 dB: away from the value the handle is a solid
   line; as it nears the value it opens, and over the value only a tick
   below the letters is left (the value sits high to leave room for the
   meter). It never crosses a letter. Pan the track left and right: the
   fill's edge does the same around "L 25". Check the mixer strip's pan
   too.
2. **Scale numbers (S3):** in the Mixer tab, drag the panel's top edge down
   to its smallest: the dB numbers beside each fader never touch; the ones
   without room become short ticks, and 0 always shows. Drag it tall:
   every number comes back.
3. **Switches and choices (T3):** open the Lumen menu. "Copy imported
   audio", "Reduce motion", "Additive solo" and "Meters in track colours"
   are switches: click one, the menu stays open and its knob glides over
   and lights. Theme and Interface size are segmented: click Light, then
   125%, and the thumb slides to each. Right-click the metronome: Sound and Output are
   segmented, and Cue is greyed with why on hover until a cue pair is set.
   Right-click Follow: Page, Continuous and Off. Right-click a send knob:
   Pre-fader and Post-fader. A clip's menu, Fades: Linear, Slow, Fast and
   S-curve for each end. Audio settings: the channel pairs are rows, the
   chosen one lit; Test tone is a switch. Each works from the keyboard
   (Tab to it, Space or Enter).
4. **Menus close gently (your C):** with the Lumen menu open and a switch
   flipped, move the pointer off the menu: after a moment it fades away.
   Flip one, move off and quickly back on: it stays. Open it again and
   press Esc, or click elsewhere, or choose History…: it closes at once,
   fading. Right-click a clip and choose Rename: that menu fades too. Turn
   on Reduce motion: menus close without fading.
5. **Sizes (S4):** M, S and arm are the same size on headers and strips
   (20 pt); send knobs on a mixer strip are as large as on a header (24 pt);
   buttons, fields and combos in windows are a touch taller (24 pt). Check
   both themes at 100%, 150% and 200%: nothing overlaps, and the mixer and
   clip card still fit.

**Expected:** as described.

**Run 1 (2026-10-10, owner):** menus did not fade; they disappeared.
The frame that started a fade asked for no next frame, so an idle window
drew the next one only after the fade was over (the harness steps every
frame, so the shots showed it). Fixed: a fade's first frame asks for the
next; E2E `a_closing_menu_asks_for_the_frames_its_fade_needs`.

**Passed (2026-10-10, owner, run 2).**

### MT-POL-3 · Chrome (needs a human)
Slice 3 of M4.5: findings T4, T5, S5, S6, S7, and the metronome's volume.
Run `cargo run --release -p lumen-app`.
1. **Toggles and tabs (T4):** in the transport, Browser and Detail always
   look like buttons; on, they are lit. Under the arrangement, the detail
   panel's tabs are words: Mixer has an accent underline; Devices and Clip
   are greyed, and hovering one says which milestone brings it.
2. **Arm at rest (T5):** on a project with several tracks the arm buttons
   are quiet grey dots; hover one: it turns red. Arm a track: it glows and
   breathes as before.
3. **Empty states (S5):** File → New. The lanes say "Drop audio files
   here" with the formats under it and an Import audio… button, all
   centred. The mixer shows a dashed ghost strip with "Add track": click
   it, an audio track is added and the ghost goes. The browser keeps its
   title, inset from the corner, and says when it arrives.
4. **Clip card (S6):** import a file from a long folder path; select its
   clip, Alt+Enter. The colours are two even rows of six, then a "Track
   colour" pill. The path at the bottom keeps the drive and the file's name,
   with "…" in the middle; hover it for the whole path.
5. **Time signature (S7):** the transport shows one "4 / 4" field. Wheel
   over the left number: the beats change; over the right: the unit steps
   through 1, 2, 4, 8, 16… Drag up and down on each half. Click and type
   7/8, Enter: it reads 7 / 8 and the grid follows. Type 7/7: nothing
   changes. Double-click: 4 / 4.
6. **Metronome volume:** right-click the metronome. The volume is a slider
   like the others (value inside it); drag it; double-click it: back to
   50%. Move off the menu: it fades away.

**Expected:** as described, in both themes.

**Passed (2026-10-10, owner):** "looks good".

### MT-POL-4 · Numbers in Martian Mono (needs a human)
Slice 4 of M4.5: finding S1. Run `cargo run --release -p lumen-app`.
1. The position (1.1.1 0:00.000), tempo, CPU, the time ruler, the grid
   size, every meter scale and fader readout, and the loudness figure are
   in Martian Mono: square, slightly wide, plain zeros. Play: the position
   counts without jittering sideways.
2. At 100%, 150% and 200% (Lumen menu → Interface size): readouts fit
   their boxes (the strip's "−10.9", the loudness "−14.2"), and the
   transport bar still fits a 960 × 540 window as before.
3. Both themes.

**Expected:** as described.

**Passed (2026-10-10, owner).**

### MT-POL-5 · Motion (needs a human)
Slice 5 of M4.5: findings M1, M4, M2, M3, U2 and M5. Run
`cargo run --release -p lumen-app`.
1. **Clips glide (M1):** drag a clip along its track: it steps from grid
   line to grid line smoothly instead of jumping. Ctrl+Z: it glides back,
   and it is washed in the accent for a moment (M4). Ctrl+Shift+Z: the
   same, forward. Duplicate (Ctrl+D) and nudge-like edits glide too.
2. **Rows (M2, M3):** add a track (Ctrl+T): it grows in. Delete one: the
   rows below glide up. Drag a track to another place: the rows glide to
   their new places. Group a few tracks and fold the group: they glide
   shut into it; unfold: they grow back.
3. **Scroll bars (U2):** with 20 or more tracks, scroll the tracks: a thin
   bar appears at the right of the lanes and fades away a moment after you
   stop. Move the pointer to the right edge: it comes back; drag it. In
   the mixer with many strips, scroll sideways: the same bar along the
   bottom.
4. **Highlights and lit states (M5):** Ctrl+K, press ↓ several times: the
   highlight slides. Click M, S, loop, metronome: they light and go out
   with a short cross-fade. Open Audio settings and close it (✕ or Esc): it
   fades out.
5. **Reduce motion** (Lumen menu): everything above still works, without
   travelling or fading.
6. **Calm when idle:** with nothing moving, Task Manager shows Lumen near
   0% CPU, as before.

**Expected:** as described, in both themes.

**Owner walkthrough (2026-10-12):** pass. Motion and reduced-motion
behavior looked good; no issues reported.


### MT-POL-6 · The close (needs a human)
Slice 6 of M4.5: what the design sweep (`shots::polish_sweep`, both
themes at 100%, 150% and 200%) changed. Run
`cargo run --release -p lumen-app`.
1. **Menus line up:** open the Lumen menu. "Copy imported audio into the
   project", "Theme", the Dark / Light / Match system choices, "Interface
   size", "Reduce motion", "Mixing" and the two switches under it start
   where "New" and "History…" start, and each switch ends where the
   keycaps end. Before, they sat about 1.5 pt further left and the
   switches about 2.5 pt further right (before and after shown in chat).
   The menu is as tall as before.
2. **Other menus:** right-click the metronome: "Sound" and "Output" are
   small grey captions, in line with their choices. Right-click Follow:
   "Follow the playhead" the same. A clip's menu, Fades: "Fade in" and
   "Fade out" the same.
3. **Mixer edges:** add tracks until the mixer scrolls (about 12). Scroll
   it partway: the strip cut at the left edge fades into the background,
   and the one cut by the master's divider fades out before it. At either
   end only the far edge fades. Check both themes.

**Expected:** as described.

**Owner walkthrough (2026-10-12):** pass. Menu alignment and mixer edge
fades looked good; no issues reported.

### MT-MIDI-1 · MIDI devices (needs a human and a MIDI device)
Slice 0 of M5. Needs a USB MIDI keyboard or controller, or a virtual port
such as loopMIDI. Run `cargo run --release -p lumen-app`.
1. Open the Lumen menu, **Audio and MIDI settings…**, then the **MIDI**
   tab. Your device is listed under Inputs (and Outputs, if it has one),
   with a grey dot and both switches off.
2. Switch **Track** on for the input: the dot lights. Close and reopen
   Lumen: the switch is still on and the dot lights without another click.
3. Unplug the device: it leaves the list within a second or two, the
   window stays responsive. Plug it back in: it returns, lit again.
4. With another program holding the device open (Windows' WinMM allows one
   program per input), switch Track on: the dot warns and the row says
   "Could not open: …". Close the other program and switch Track off and
   on: it opens.
5. Remote is a separate switch (outputs have only Track).
6. Notes are not heard or recorded yet (slices 1, 2 and 6).

**Expected:** as described. Record what your device is called, and whether
WinMM shows it once or twice.

**Last run (2026-10-11, owner):** pass. Enumeration, Track and Remote
switches, persistence, hot-plug and replug all behaved as expected. The
device name was not recorded.

### MT-MIDI-2 · Hearing MIDI (needs a human and ears; Windows' GS Wavetable Synth is the "hardware")
Slice 1 of M5. Notes cannot be drawn until the piano roll (slice 3), so the
project is made by code. **Status: run 2026-10-11; see findings below.** In PowerShell,
from the repository root:

```powershell
cargo run --release -p lumen-app -- tests\projects\midi-demo.lumen
```

(If Lumen opens empty, use **Open…** and choose
`tests\projects\midi-demo.lumen`.) The project is 120 BPM, eight bars, five
MIDI tracks: **Chords** (Poly), **Pad** (Pad), **Bass** (Bass), **Lead**
(Keys) and **Hardware** (Pluck, sent to *Microsoft GS Wavetable Synth*,
channel 1). The loop brace covers bars 1 to 4 and is off.

1. **Chords and the pedal.** Press Space. You hear an A minor, F, C and G
   chord, one per bar, repeating in bars 5 to 8, over a held pad and a bass
   line. In bar 3 the C chord is only a beat long but rings for the whole
   bar: the sustain pedal is down from the start of bar 3 to the start of bar 4. It must stop
   cleanly when the pedal comes up, not hang.
2. **The Hardware track.** Without anything else, you also hear a plucked
   arpeggio in bars 1 to 4 (the same chords, eighth notes, its first note on
   the chord root on every downbeat) from the Windows
   synthesiser, with a different, GM sound. The first time, Lumen switches
   the output on itself: open **Audio and MIDI settings… → MIDI** and see
   Microsoft GS Wavetable Synth under Outputs with Track on. Does the
   arpeggio line up with the other tracks, or is it early or late? Note
   roughly how many milliseconds (clap along to the bass) and which way.
3. **Bend and filter.** In bar 5 the Lead plays a long note that bends up
   two semitones over four beats while the filter opens, then falls back.
4. **Chase.** Stop. Click the ruler in the middle of bar 6 and press Space:
   the pad is there at once (its note began in bar 1, so playback starts it
   part-way), and so are the notes of the other tracks that are held at that
   point.
5. **Jump.** While playing, click the ruler somewhere else a few times: no
   stuck notes, no clicks other than the usual 2 ms crossfade.
6. **Loop wrap.** Click the loop brace above bars 1 to 4 to turn the loop on, and play: the loop repeats bars 1 to
   4. The pad and the Chords' notes are cut at each wrap and start again
   with the loop. No note rings across the wrap.
7. **Stop.** Press Space: the sound fades out with each instrument's
   release (a quarter of a second for Poly), and the Hardware track goes
   silent. Nothing keeps sounding afterwards.
8. **Presets from the menu.** Right-click the Chords track name:
   **Instrument ▸** shows Poly (ticked), Keys, Bass, Pluck, Pad and None.
   Choose each while the song plays: the next notes take the new sound; the
   notes already sounding finish as they were. Ctrl+Z undoes the choice
   ("Instrument: Pad").
9. **MIDI To.** Right-click the Pad track: **MIDI To ▸** lists
   **Instrument** (ticked) and **Microsoft GS Wavetable Synth ▸** with
   Channel 1 to 16. Choose Channel 1 while playing: the pad now comes from
   the Windows synthesiser and stops being heard from Lumen's synth, with no
   stuck note on either. Choose **Instrument** to go back. If your system
   has other MIDI outputs, they are listed too.
10. **Mute and solo.** Mute the Hardware track: it falls silent and its
    notes are not sent. Solo Bass: only the bass is heard (the Hardware
    track is silenced because it is not soloed). Unmute and unsolo.
11. **Panic.** While playing, open the command palette (Ctrl+K), type
    "Panic" and press Enter, or use **Panic** in the audio panel: everything
    stops at once, including the Windows synthesiser, and nothing rings.
12. **Unplug.** If you have a USB MIDI output (or loopMIDI), assign the
    Hardware track to it, play, unplug it: Lumen keeps playing. Plug it in
    again: it works once more. Its name stays in the track menu as "(not
    connected)" meanwhile.
13. **Save and reopen.** Save the project under a new name, reopen it: the
    instruments and outputs are as you left them.

**Expected:** as described. **Record:** how far the Hardware track is out of
time with the rest (step 2) and in which direction, at which audio buffer
size and driver; whether the MIDI To menu's sixteen channel rows are too long
to use comfortably (a compact grid would need a mock-up first); any stuck
note or click.

**Run 2026-10-11 (owner):** playback, sustain, chase, stop, presets, MIDI To,
mute/solo, Panic and save/reopen passed. The Hardware output sounded broadly
aligned, but the owner shifted the clip right by an eighth note because the
melodic root seemed off; the MIDI notes cannot yet be inspected visually
until the piano roll slice. Jumping during playback produced static/popping,
and loop boundaries produced the same artifact. Preset changes and undo took
about 1–2 seconds to become audible. The Windows GS Wavetable Synth output
worked, but its latency was noticeable even with ASIO at 128 samples. A
physical output unplug/reconnect was not tested.

**Fixes after run 2026-10-11** (re-run steps 2, 5, 6 and 8):
- The demo's Hardware arpeggio was a C major pattern over every chord (and
  the bass played A C F G against Am F C G), so the root sounded wrong:
  both now start on each chord's root on the downbeat. **Undo your eighth
  note shift first** (or reopen the regenerated file). Any lateness left in
  step 2 is the GS synth's own; note it in milliseconds.
- Jumps and loop wraps under a sounding synth popped: the engine's 2 ms
  jump crossfade had no instrument audio to fade out. Fixed; clicking the
  ruler while chords sound and letting the loop run should be clean.
- Presets: a change reaches the engine within one block (measured), but a
  sounding note keeps its sound, so the new preset is heard at the next
  chord. Held notes now glide to the new preset over 25 ms (your choice):
  in step 8, a held pad changes tone at once, with no click and no
  re-attack, and Ctrl+Z glides back.

**Follow-up rerun (2026-10-11, owner):** jump (step 5), loop wrap (step 6),
and preset glide/undo (step 8) passed. The Monitor badge check also passed:
In, Off and Auto showed the expected badge states. The badge crowded the
track name and caused truncation; accepted for this run, with the name still
readable.

**Also new (MT-MIDI-3 re-run, step 4):** a MIDI track set to Monitor In
shows a blue speaker before M, Off a slashed grey one, Auto nothing. Click
it to cycle In → Off → Auto; Tab reaches it before M and Space cycles it.
Check how long names shorten with the badge at the default header width.


### MT-MIDI-3 · Playing live (needs a human; a MIDI keyboard for steps 1–6, or loopMIDI)
Slice 2 of M5 (ADR-0046). **Status: run 2026-10-11; see findings below.** In PowerShell,
from the repository root:

```powershell
cargo run --release -p lumen-app -- tests\projects\midi-demo.lumen
```

1. **The chip.** At the right of the transport bar, beside the magnifier,
   is a small round MIDI socket with two lights (IN above, OUT below).
   Click it: a list of every MIDI input and output, each with its own
   light and **Track** / **Remote** pills, then **MIDI settings…**. Plug in
   your keyboard and switch its **Track** pill on. The chip's tooltip and
   name say how many inputs and outputs are on.
2. **Play the selected track.** Click the **Chords** track's name and play
   your keyboard: you hear the Poly synth. The arm button shows a hollow
   red ring (hover: "Listening because selected"), a small light after the
   name flashes with each note, and so do the chip's IN light and your
   keyboard's row in the popover. Select **Bass** and play: the bass sound,
   and Chords stays dark. Select an audio track (add one with Ctrl+T): your
   keyboard plays nothing.
3. **Timing.** Play a fast, even phrase (sixteenths at 120 BPM) on a held
   rhythm. It should feel as tight as in your other DAW, with no flamming
   or wobble at your usual buffer size. Note the buffer size and how it
   feels; try 256 and 1024 too.
4. **Layering and Monitor.** Arm **Lead** (its record button), select
   Chords and play: both sound. Right-click Chords: **MIDI From ▸** and a
   **Monitor** row (In · Auto · Off). Choose Off: Chords is silent even
   when selected. Choose In, then select another track: Chords still
   plays. Back to Auto. Ctrl+Z undoes each change.
5. **MIDI From.** On Chords choose **MIDI From ▸ <your keyboard> ▸
   Channel 2** (set your keyboard to send on channel 1): silent; change
   your keyboard to channel 2: it plays. Choose **Computer keyboard**: your
   hardware keyboard no longer plays it. Back to **All inputs**.
6. **Nothing hangs.** Hold a chord and, while holding: press Space twice
   (play, stop) — the chord keeps sounding until you let go; click another
   track — the chord stops at once on the old one; set Monitor to Off —
   it stops; unplug the keyboard — nothing keeps sounding. Plug it back:
   its row comes back, Track still on.
7. **The computer keyboard.** Press **Ctrl+Shift+K**: the keycap button
   after Follow lights and, in a wide window, `C3–C4 · vel 100` sits
   beside it; the info line along the bottom shows A–K, Z X, C V, Esc.
   Select Chords and press A S D F (C D E F), W and E (black keys). Hold A,
   press X, let go of A: the note stops and the next A is an octave
   higher. C and V change the velocity. H, W, X, Z play or move the octave
   instead of zooming; Ctrl+D, Space and the arrows still work. Esc leaves
   the mode; hold a note and Alt+Tab away: it stops. Rename a track (F2)
   while the mode is on: typing plays nothing.
8. **MIDI To.** On the **Hardware** track, select it and play: your notes
   go to the Windows GS synth (OUT on the chip flashes, and that output's
   row). Ctrl+Z restores anything you changed.
9. **The mixer.** Open the Mixer tab: MIDI strips show **From** and **To**
   rows above the output; each opens the same choices.
10. **The narrowest window.** Make the window as small as it goes (960 ×
    540 points): the position (bars and time) shows whole, the audio dot
    and the MIDI chip stay; the CPU readout, device text and keyboard
    readout give way (the keyboard readout moves to the info line). In this
    width the bar is compact: buttons 4 pt apart, the tempo without "BPM",
    no divider before the audio dot (judge whether this reads well).
11. **Idle.** Leave an input switched on and play nothing for a minute:
    Task Manager shows Lumen near 0% CPU.
12. **Save and reopen.** Save, close and reopen: MIDI From and Monitor are
    kept; the computer keyboard mode is off again.
13. **Themes and scale.** Light theme and 150% / 200% interface size: the
    chip, popover, header lights, hollow ring and menus look right.

**Run 2026-10-11 (owner):** pass for the available hardware. Live input,
monitoring, MIDI From, computer keyboard, MIDI To, mixer routing, the
960 × 540 layout, idle CPU, save/reopen, themes and scaling all behaved as
described. The physical controller did not expose a channel selector, so the
channel-filter subtest was not verifiable. At ASIO 128 samples, external
MIDI output latency was noticeable. The Auto monitor hollow ring was
understandable but less clear than the In/Off states.

### MT-MIDI-4 · MIDI clips and the piano roll (needs a human, ears and two monitors if you have them)
Slice 3 of M5 (ADR-0047). **Status: follow-up passed 2026-10-12.** In PowerShell,
from the repository root:

```powershell
cargo run --release -p lumen-app -- tests\projects\midi-demo.lumen
```

1. **Open a clip.** Double-click the **Chords** clip: the detail panel
   opens on the **Clip** tab with a piano roll: piano keys on the left
   (black keys darker and shorter, "C4" on middle C), darker lanes for
   the black keys, notes in the track's colour, lighter where velocity is
   high. Bars 5 to 8 (the loop's repeats) are drawn fainter and cannot be
   picked. The clip tab's zoom follows the arrangement's: zoom the
   arrangement (Ctrl+wheel) and the roll's bars widen with it. Wheel over
   the roll scrolls the pitches.
2. **Notes on clips.** In the arrangement, MIDI clips show their notes as
   thin lines under the title bar. Judge whether they read well; they are
   bands (32 per track), not one row per semitone.
3. **Pick, move, resize.** Click a note: it gets an outline and you hear
   it once (short). The playhead does not move. Hover a note: small
   handles appear at its ends. Drag it up two rows and right one beat: it
   follows the pointer with a chip saying its pitch and position, and you
   hear each new pitch. Release: one undo step (Ctrl+Z puts it back). Drag
   an end to resize. Shift-click to pick more, drag a box over empty space
   to pick several, Ctrl-drag to copy. Hold Alt while dragging to ignore
   the grid. Press Esc during a drag: it snaps back and nothing changes.
4. **Edit while it plays.** Press Space and, while the loop plays, drag a
   note somewhere else. While you hold it, the loop still plays the note
   where it was; on release it plays the new place. Delete a note that is
   sounding, mute one (0), transpose one (Up): nothing keeps ringing.
   On a **hardware** track (MIDI To set to a port), with the clip playing,
   press a note that is sounding under the playhead: the port gets no extra
   note-on or note-off for that key, and the clip's note still ends once.
   Press a note that is not sounding: it plays as before. (Added 2026-10-11;
   the built-in synth is not skipped.)
5. **Draw.** Press **B** (the Draw button lights). Click an empty spot:
   a note one grid step long appears and sounds. Click and drag right: the
   new note grows with the pointer, snapped. Click an existing note: it is
   removed. Press B again for the pointer.
6. **The keys.** Pick a few notes with the pointer still over the roll:
   **Up/Down** transpose a semitone, **Shift+Up/Down** an octave,
   **Left/Right** nudge a grid step (**Alt** finely), **Ctrl+D**
   duplicates after them, **0** mutes (grey) and unmutes, **Delete**
   removes, **Ctrl+A** picks all, **Ctrl+E** splits at the playhead,
   **Ctrl+J** glues touching notes of one pitch, **L** makes them legato.
   Each is one Ctrl+Z. Move the pointer back over the arrangement: the
   same keys act on clips and tracks again. Right-click a note and empty
   space in the roll: menus with these commands and their keys. Ctrl+K
   finds them all ("notes", "MIDI clip").
7. **Make clips.** Double-click an empty spot on the **Lead** track's
   lane: a one-bar clip appears, snapped, and opens. Then drag across empty
   lane space beside that clip, with no modifier, to select a time range on
   Lead: the range is snapped and Lead is selected. A drag that goes down
   to another track takes in every track it crosses (ADR-0049). Press
   **Ctrl+Shift+M**: a second clip fills the range on Lead. Ctrl+Shift+drag
   in empty lanes still draws a clip marquee: check that it selects clips
   and makes no range. Then select a track by its header (Lead), and drag
   the time ruler: the range covers the selected track only (with no track
   selected it covers every track), and the selection stays. Ctrl+Shift+M
   puts the clip on Lead. Report anything that feels wrong about how the
   ruler, the lanes and the track selection work together.
8. **Keyboard only.** Click a MIDI clip, press **Enter**: it opens. Press
   **F6** until the Clip tab has focus, then Ctrl+A, the arrows and Delete
   work without touching the mouse.
9. **Pop out.** Click **Pop out** (or Ctrl+Shift+E): the editor opens in
   its own window and the Clip tab shows **Dock editor**. Move it to your
   second monitor, resize it, check it looks sharp there (DPI), and edit
   notes in it with mouse and keys. Close the window: the editor docks
   back. (Its size and place are not remembered yet.)
10. **Looks.** Settings → light theme, then 150% and 200% scale: check the
    roll, the keys and the picked notes in each. Report anything that
    looks off.
11. **Save and reopen.** Save the demo under a new name (to preserve the
    fixture), close Lumen, reopen that copy: every note edit, the muted notes
    and the new clips are there.

**Run (2026-10-11, owner, in progress):** step 1 passed. The owner noted
that arrangement zoom is mirrored in the piano roll and questioned whether
that should be the final behavior. The current spec calls for synchronized
zoom; revisit this UX choice at the slice close. Step 2 passed: MIDI note
bands in the arrangement read clearly enough. In step 3, click, audition,
move, undo and resize worked. The owner would like the selection outline to
look better and suggested a bracket cursor for resizing; review both at the
slice close. Shift-click multi-select works, but when moving multiple notes,
only the grabbed note auditions rather than all moved notes; assess whether
this is the desired preview behavior at the slice close. Box selection works,
but its blue border is hard to distinguish over light-blue notes; improve
the selection-preview contrast. Ctrl-drag copy and undo worked, and Alt-drag
successfully bypassed the grid. Esc cancelled a drag and restored the note.
Step 4: playback continued unchanged while dragging a note, and the next loop
pass played the note at its new position after release. Deleting a sounding
note stopped it without hanging. Mute and unmute worked. The owner also
noticed that touching a note under the playhead auditions it and suppresses
its scheduled playback for that pass, then the note returns on the next
trigger; review whether this is intended. Transposing a note with Up while
playing worked and sounded good. In step 5, Draw mode lit and clicking an
empty spot created an audible one-grid-step note.
Dragging grew the note and clicking an existing note removed it; B returned
to the pointer tool. The owner noticed notes can only be drawn left to right;
assess whether reverse-direction drawing should be supported. Shift+Up and
Shift+Down transposed a selected note by an octave and back (step 6).
Left/Right nudged by a grid step and Alt+Left/Right provided fine nudging.
Ctrl+D duplicated selected notes after the originals.
The 0 shortcut muted notes with the grey appearance and unmuted them again.
Delete removed the selected note and Ctrl+A selected all notes in the clip.
Ctrl+E failed the split-at-playhead check: the split did not occur at the
playhead and appeared random. Investigate before passing MT-MIDI-4.
Ctrl+J joined touching same-pitch notes successfully.
L made selected notes legato, and note/empty-space context menus showed
relevant commands and shortcuts.
Ctrl+K found the relevant commands for both "notes" and "MIDI clip".
In step 7, double-clicking an empty Lead lane created and opened a snapped
one-bar clip. The owner noted that a time range is discoverable from the
time ruler (across all tracks) and the lower half of an existing clip (one
track); the prior instruction to drag in empty space was incorrect (empty
space draws a clip marquee). The owner found that ruler selection clears the
selected track, so the Lead-specific clip creation check uses the clip-half
gesture. Ctrl+Shift+M then created a second clip on Lead filling that range.
Step 8 passed: Enter opened the selected MIDI clip, and keyboard-only
navigation and note actions worked with the Clip tab focused.
Step 9 passed: the editor popped out, looked good, and docked back on close.
Step 10: the piano roll, keys and selected notes looked good in the light
theme and remained clear and sharp at 150% and 200% scale.
Step 11 passed: a renamed copy saved and reopened with note edits, muted
notes and new clips intact. At that time, MT-MIDI-4 was blocked from passing
by the Ctrl+E split failure recorded above; the follow-up below rechecked
the corrected behavior.

**Follow-up (2026-10-12, owner):** pass after rechecking the MIDI clip and
piano-roll interactions, including the corrected split behavior. One note
in a clip did not fire through the Poly synth once; the owner could not
reproduce it. Logged in `docs/KNOWN_ISSUES.md` for follow-up if it recurs.

### MT-MIDI-5 · Velocity and controller lanes (needs a human; a MIDI controller helps but is not required)
Slice 4 of M5 (ADR-0050). **Status: passed 2026-10-12.** In PowerShell,
from the repository root:

```powershell
cargo run --release -p lumen-app -- tests\projects\midi-demo.lumen
```

1. **The lane.** Double-click the **Chords** clip. Under the piano roll is a
   lane with a header: the tabs **Velocity**, **Mod wheel**, **Pitch bend**,
   **Pressure** and **+ CC**, and the tools **Pencil**, **Line** and **Curve**
   at the right. Its plot is 28% of the editor's height, between 48 and
   150 pt. Check it at 100%, 150% and 200% scale, in the dark and the light
   theme, and report anything that looks off.
2. **Velocity stems.** On **Velocity**, each note start has a stem with a
   round head; picked notes' stems are lit. Judge whether they still read
   well where the clip is dense. Choose **Pencil** (its button lights) and
   drag across several stems: every stem the stroke passes changes, chords
   change together, and a fast stroke does not skip stems. Ctrl+Z undoes the
   whole stroke.
3. **Velocity ramps.** Pick a run of notes. With **Line**, drag from one stem
   to another: the notes between the two ends ramp to the new velocities. Do
   the same with **Curve**; its dot at the middle bends the ramp. Ctrl+Z after
   each.
4. **Mod wheel and the pencil.** Choose **Mod wheel**. With **Pencil**, a click
   sets one value and a drag draws a stroke; with snapping on, the times follow
   the grid. The keys **1**, **2** and **3** choose Pencil, Line and Curve (Shift
   in the lane starts a box, not a line: say whether you expected it to draw a
   line). Press Space to play the loop and listen. The built-in synth's
   presets may not respond to the mod wheel: note whether you hear any change.
5. **Pitch bend.** Choose **Pitch bend**: the centre line is no bend. Draw a
   **Line** down and back up, then a **Curve** and drag its dot. Play it and
   listen for steps in the glide. Each drawn bend is one undo step.
6. **Move, box and Delete.** On **Mod wheel**, press on a point and drag it: its
   value and time snap, and release is one undo step. Shift-drag a box over
   several points: they pick. Press **Delete** (the lane was the last place
   pressed): the picked points go, and Ctrl+Z brings them back. Then click a
   note in the roll and press **Delete**: the note goes and the lane's points
   stay.
7. **Esc.** Start dragging a point, or a stroke with the Pencil, and press Esc
   before you let go: it snaps back and nothing changes.
8. **Hide and show.** Press the **Lane** button in the editor's toolbar: the
   lane closes and the roll takes its height. Press it again: the lane comes
   back.
9. **Palette.** Pick some notes, then press Ctrl+K and type "velocity":
   **Raise velocity** and **Lower velocity** each move the picked notes by 8,
   one Ctrl+Z each. Type "lane" and check the lane commands switch the tab.
10. **What you draw reaches the port.** If a MIDI device is available, set a
    track's **MIDI To** to its port, draw a pitch bend and a mod wheel curve,
    and play the loop: check the device receives them. Not required; if you
    do not run it, say so.
11. **Live input.** With a MIDI controller and the lane open, play on the track
    that listens (slice 2): notes still sound. Record nothing; recording is
    slice 6.
12. **Frame times.** In PowerShell, from the repository root:

    ```powershell
    cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json" --bench-editor
    cargo run --release -p lumen-app -- --stress --bench-frames "$env:TEMP\frames.json" --bench-editor --bench-size 3440x1380
    ```

    Report p50, p95, p99 and the worst frame per phase, the idle repaints in
    3 s, and the exit status (3 means a phase missed 60 fps at p99). These
    numbers are what ADR-0050 is waiting for.
13. **Save and reopen.** Save the demo under a new name and reopen the copy:
    the drawn velocities and controller points are all there. The lane's tab
    and whether it is open are not saved; that is a known limit.

**Expected:** each step does what is described. Every gesture is one undo
step, and the clip changes only when you let go.

**Run 1 (2026-10-12, owner):** pass. Velocity and controller lane gestures,
undo, keyboard commands, display, frame-time checks and save/reopen passed.

### MT-UX-1 · Clip gestures, playback selection and control spacing

1. Open a project with audio and MIDI clips. Drag a clip's title/header:
   it moves; Ctrl-drag copies. Undo restores the original arrangement.
2. Drag from the waveform or MIDI-note body, including just below its
   title: a time range forms and the clip stays put. Repeat from empty
   lane space. No dashed halfway divider appears when hovering a clip.
3. Check trim, loop extension, audio fades and gain handles still work.
4. While stopped, click a clip or empty lane: the playhead moves to the
   snapped time. Double-click a MIDI clip to open its editor.
5. Start playback. Click clip headers, clip bodies and empty lanes:
   selection changes but playback continues without jumping. Ctrl-click
   still adds/removes clips from selection. Click the ruler and scrub
   strip: these deliberately seek while playback continues.
6. In the velocity/controller panel, check the pills have clear vertical
   margins and both the left tabs and right tools have horizontal insets.
7. The transport's global S button stays circular with zero, one and
   multiple soloed tracks. Check its count hint, clearing solos and
   keyboard focus. Repeat the visual checks in both themes at 100%,
   150% and 200% scale.

**Agent verification (2026-10-11):** all 428 application unit tests and
277 end-to-end tests and all six real-binary smoke tests pass; application
clippy (`--all-targets -D warnings`) and formatting pass. GPU screenshots
of the lane and zero/one/multiple
solo states were inspected in both themes at 100%, 150% and 200%.
The first parallel test run hit existing audition/MIDI scheduling checks;
they passed when rerun without concurrent GPU rendering. Builds and test
temporary files used `D:/Projects/lumen/target/codex-61b4` because C: was
full. The owner's manual listening/interaction check remained available.

**Owner walkthrough (2026-10-12):** pass. Clip gestures, playback selection,
lane control spacing and the global Solo button checks passed.

### MT-DS-1 · The design system's look (needs a human; both themes)

Checks ADR-0052 by eye. Start Lumen (`cargo run --release -p lumen-app`),
open a project with a few audio and MIDI tracks, a group and a return.

1. **Buttons** (Audio settings, a banner, the shortcuts window): raised,
   a faint light along the top edge, no shadow under them; hovering
   washes them lighter and strengthens the edge; pressing sinks them and
   moves the label 1 pt down; 24 pt buttons have 6 pt corners.
2. **On states:** Loop, Metronome and Follow when on are a quiet blue
   fill with blue icons and a thin blue edge, with no glow. Play while
   playing still beams.
3. **Solo and mute:** S lit is a blue gradient with a dark letter (no
   amber anywhere, no halo); M lit is light grey with a dark letter (on
   the light theme a dark grey with a white letter). A track silenced by
   another's solo shows a hollow grey M.
4. **Arm:** round; a grey dot at rest that turns red under the pointer;
   armed, a faint red body, a red dot and the breathing ring.
5. **Segmented and switches** (the app menu, the clip card): the trough
   is cut in, the chosen piece a neutral raised thumb with white letters;
   a switch that is on is blue with a blue knob.
6. **Fields and sliders:** header sliders, the strip's volume field and
   the transport's position and tempo are wells (darker than around them,
   a dark lip on top); typed tempo applies on Enter.
7. **Windows and menus:** windows have 12 pt corners, menus 8 pt; both a
   faint light along the top edge; menu rows highlight with a soft wash.
8. **Focus:** Tab through the transport: the ring sits just outside each
   control, rounded like it.
9. Repeat 1–8 on the light theme and at 150% and 200% scale.

Results:
- **Owner walkthrough (2026-10-12):** pass. The design system checks looked
  good, including the light theme and larger interface scales; no issues
  reported.


### MT-MIDI-6 · Quantize (needs a human and ears)
Slice 5 of M5 (ADR-0054). **Status: passed 2026-10-12 (run 2).** In PowerShell, from the
repository root:

```powershell
cargo run --release -p lumen-app -- tests\projects\midi-demo.lumen
```

1. **Open the options.** Double-click the **Chords** clip, then press
   **Shift+Q** (or press the **Quantize** button). A popover opens under the
   button with Grid, a Straight / Triplet / Dotted choice, Quantize (Start,
   End, Both, Length), and three sliders: **Strength**, **Swing** and
   **Window**. The sliders are comets: a trail that brightens to a glowing
   head. Check the popover at 100%, 150% and 200%, dark and light.
2. **Ghost trails.** Change a setting (Strength to 50%, Swing up). Notes that
   would move slide to their new place; a dashed outline stays where each was
   recorded, with a line along its bottom to the new place. Notes that stay
   have no outline. Judge whether it reads well when notes are dense.
3. **Nothing changes yet.** While the popover is open, press Ctrl+Z (nothing
   to undo) and look at the arrangement: the clip's notes are as recorded.
   Press **Esc**: the popover closes and the notes are as they were.
4. **Apply.** Reopen, press **Apply**: the notes move, the popover closes,
   one Ctrl+Z restores them exactly.
5. **Q.** With nothing picked press **Q**: every note quantizes with the last
   settings. Pick two notes and press Q: only those. Set Strength to 50% and
   press Q three times: the notes close in on the grid each time.
6. **Modes and window.** Try End, Both and Length, a triplet grid, and a
   narrow Window (notes far from a line stay put). Listen to the result.
7. **Tools.** Right-click the roll: **Humanize**, **Fixed length**,
   **Reverse**, and **Constrain to scale** with a root and a scale. Each is
   one undo step, works on the picked notes (or every note), and sounds as
   expected. Press Ctrl+K and type "humanize", "reverse", "scale".
8. **Slider feel.** Drag a comet slider (Shift for fine), double-click it
   (back to its default), click it and type a value, use the wheel.

**Run 1 (2026-10-12, owner):** four findings, fixed (ADR-0054 amendment): the
Grid dropdown closed the popover; the slider trail ran past its head at 100%;
too much glow on the head; unclear which grid Q uses (now a readout beside
the button and a toast). **Re-run: steps 1, 5 and 8 again**, and check the
readout beside Quantize changes with the popover's settings.

**Run 2 (2026-10-12, owner): passed.** Steps 1, 5 and 8 re-run and passed.



### MT-MIDI-7 · Recording MIDI (needs a human and a MIDI controller; the computer keyboard also works)
Slice 6 of M5 (ADR-0055). **Status: run 4 waits for the owner.** Start
Lumen from the repository root:

```powershell
cargo run --release -p lumen-app
```

Set up: open `tests\projects\midi-demo.lumen`, or a new project with a MIDI
track. Arm one MIDI track (its arm button) and set its Monitor to Auto or In.
Connect a MIDI controller, or press **Ctrl+Shift+K** for the computer keyboard
(a mode: the keys play notes). Turn the metronome off.

1. **Count-in.** In the metronome's menu choose **Count-in: 1 bar**. Press
   **F9**. Expect: the Record button rings and pulses on each beat; the
   position field counts up through −1.1.1 to 1.1.1; four clicks sound with
   the metronome off, the first of them accented; the song waits at the play
   marker. Then recording and playback start together.
2. **Timing.** Play a simple part on the beat (quarter notes, then eighths).
   Stop, and look at the clip: each note should sit where you heard the beat.
   Repeat with the buffer at 64, then 128, then 512 (Audio settings). Say
   whether anything is early or late, and at which buffer.
3. **Clip extent and colour.** Start at the marker, play for a few seconds,
   stop in the middle of a beat. Expect: the clip runs from the start point to
   the stop point, not to the bar line. While recording it is record red with
   a "REC · name" tag and light-red notes; on stop it returns to its track
   colour.
4. **Undo.** Press **Ctrl+Z** once: the whole take goes. **Ctrl+Shift+Z**
   brings it back. While recording, Ctrl+Z is refused ("Stop recording first").
5. **Replace and Overdub.** Put a MIDI clip with notes where you will record.
   With Overdub off, record over it: the old notes under the take are gone.
   Turn **Overdub** on (the button beside Record) and record again: the new
   notes are added to the same clip and the old ones stay. Say whether the
   record red reads clearly in Overdub, where only the outline and the tag
   are red.
6. **Loop.** Turn on Loop over two bars and press F9. Let it go round three
   passes: expect one clip, each pass layered in. Then press F9 with the
   playhead inside the loop, but not at its start: say where the clip begins.
   In this build it begins at the loop's start. Is that what you expect?
7. **Punch.** Set the loop brace's punch in and punch out switches and record
   across the brace. Expect: notes before the brace's start are not kept; at
   the brace's end recording ends and playback carries on.
8. **Metronome only while recording.** Turn the metronome on and choose
   **Metronome only while recording**. While stopped and while playing without
   recording: silent. While recording, and in a count-in: it sounds.
9. **F9 again.** While recording, F9 ends recording and playback carries on.
   During a count-in, F9 cancels the count-in and stops; nothing plays.
10. **Crash.** Record for about twenty seconds, then end Lumen in Task Manager
    (End task). Reopen it: the recovery offers the take up to about ten seconds
    before the end. Notes that were still held are lost; that is expected.
11. **Monitor Off.** Set the armed track's Monitor to Off and record. Expect:
    notes appear in the clip, and nothing sounds while recording.
12. **Nothing armed.** Disarm every MIDI track and press F9. Expect: nothing
    starts, and a banner appears: "Arm a MIDI track to record." Say whether
    the banner is where you would look for it.
13. **Layout.** Make the window as narrow as it goes (960 pt wide). Expect:
    Record, Overdub, tempo, the position and the audio dot are whole and do not
    overlap. The keyboard readout ("Computer keyboard") appears from about
    1,320 pt. At 1,280 pt, 1,320 pt and 1,600 pt: say whether the bar reads
    well, and whether the position well's inner spacing (4 pt when compact) is
    too tight.
14. **Light theme and scale.** Switch to the light theme and check the Record
    button (ringing, recording), Overdub, and a take in record red. Repeat at
    150% and 200% scale.

Record each step's result below as a run, with what was heard and seen. Steps
2, 6, 12 and 13 are open questions as well as tests: the answers decide the
KNOWN_ISSUES rows for the loop start, the notice's place and the bar's layout.

**Run 1 (owner, 2026-10-13)** found three faults, fixed in ADR-0055's
amendment and waiting for run 2:
- the take painted in chunks instead of flowing with the playhead;
- recording over an existing clip redrew the existing clip at the same
  interval;
- a key held when recording started was not captured, and a note changed just
  before the loop wrapped was lost.

**Run 2 steps (new, after run 1).**
15. **Flow.** Record a few bars. Expect the red clip to reach the playhead
    smoothly with no steps, and each note to appear as you press it and grow
    while you hold it. Say whether it flows like Ableton's.
16. **Over a clip.** Record over an existing clip, with Overdub off and then on.
    Expect nothing under or beside the take to flicker or redraw while you
    record.
17. **Held key.** Hold a key, then press F9 (also with a count-in). Expect the
    note in the take from its very start, lasting until you let go.
18. **Loop wrap.** With Loop on over two bars, hold a note across the loop's end
    and let go in the next pass; then change notes just before the wrap.
    Expect the held note cut at the loop's end and carrying on from its start,
    and the change just before the wrap kept in its own pass.

**Run 2 (owner, 2026-10-13)** found three faults, fixed in ADR-0055's
amendment "clip previews as Ableton's", and waiting for run 3:
- notes inside clips did not have the piano roll's proportions (thin notes
  with uneven gaps between neighbouring pitches), and a take's notes moved
  when recording stopped;
- notes did not seem to appear live, and where they were drawn while
  recording did not match where they landed;
- recording over an existing clip, the clip slid in place at every commit.

**Run 3 steps (new, after run 2).**
19. **Proportions.** Make a MIDI track tall and open one of its clips in the
    Clip tab. Expect the clip's notes on the arrangement to look like a small
    piano roll: every pitch the same height, neighbouring pitches touching,
    higher pitches higher, as in Ableton. Add a note above the clip's highest
    one: the clip refits (as in Ableton).
20. **Live is final.** Record a few bars with notes far apart (a low note,
    then a high one). Expect each note to appear as you press it, the take to
    refit when you play outside its range, and nothing to move when you stop.
    With Overdub on over an existing clip, the same: the new notes sit among
    the old ones as they will once stopped.
21. **Still.** Record over an existing clip, Overdub off and on. Expect the
    clip under and after the take to stay still, with no sliding or jitter,
    while recording and as it stops.

**Run 3 (owner, 2026-10-13, steps 19 to 21 in part)** found that clips an
octave apart drew their note at the same height, and different melodies
looked the same: each clip was fitted and centred on its own. Fixed (ADR-0055
amendment, "Superseded in part"): a track's clips share one layout. Run 4
waits for the owner.

**Run 4 steps (new, after run 3).**
22. **Octaves.** On a tall MIDI track, record (or draw) one note in a clip,
    then the same note an octave up in a second clip. Expect the second one
    clearly higher. Record two different melodies in two clips: expect each
    clip's shape to show where the pitch moves, as in Ableton.
23. **Thin rows.** Say whether the notes' thickness (a track's range is at
    least four octaves tall) reads as Ableton's at the default and a tall
    track height.
