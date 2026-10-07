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

### MT-UI-2 · Accessibility tree (screen-reader part ⏸️ deferred by the owner)
1. `target/release/ui-stress --close-after 60`
2. Inspect with Accessibility Insights for Windows, or start Narrator
   (Ctrl+Win+Enter) and Tab through the transport and track headers.

**Expected:** buttons are announced as "Play", "Stop", "Record", "Loop";
track toggles as "Mute Track N" / "Solo Track N" check boxes with their
state.

**Last run (2026-10-05):** tree verified programmatically through UI
Automation (58 named elements), twice. **Still needs a human:** listening through Narrator.

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
Interactive checks of zoom, scroll and Shift+Tab not yet done by a human.

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
locates; the drag indicator and the hover card work. The 64-sample
dropout check (step 9) was not reported separately.

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
The 64-sample dropout check (step 4) was not reported separately.

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
   - fast drags may step audibly until the M4 mixer smooths them.
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

**Last run:** run 2 with findings (above); run 3 pending.

