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

### MT-MIX-1 · The mixer graph (needs a human, with the audio interface)
Run `cargo run --release -p lumen-app` with ASIO on the Focusrite, and a
project with about ten tracks of audio (drums, bass, pads: anything
continuous is best), on headphones.
1. **Levels:** play. Centred tracks sound about 3 dB quieter than before
   M4 (the −3 dB pan law, owner decision); nothing else about the sound
   changes.
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

### MT-MIX-2 · Projects from before the mixer (needs a human)
Run `cargo run --release -p lumen-app`. Use a copy of a project saved
before M4 (keep the original).
1. **Opens:** open the copy. It looks as before, with the same tracks; no
   master track appears yet (slice 4 shows it).
2. **Plays:** play it: everything is heard, centred tracks 3 dB quieter
   than before (the −3 dB pan law).
3. **Saves and reopens:** change something small, save, close Lumen, and
   open the project again: it opens unchanged, and plays the same.
4. **Undo still works:** change volume, pan and mute on a track, then undo
   each: every change undoes in one step, as before.

**Expected:** every step as described, no warnings about the file.

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
