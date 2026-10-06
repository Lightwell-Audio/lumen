# Lumen — Manual Test Scripts

Tests that need real hardware, third-party software or a human. Each entry
lists steps, the expected result and its last recorded run.

Binaries are in `%USERPROFILE%\lumen-target\release` (older copies in `%USERPROFILE%\lumen-bin`) (see the AppData warning in
`docs/DEVELOPMENT.md`). To rebuild: `cargo build --release --workspace` (see
`docs/DEVELOPMENT.md` for the toolchain).

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

Build once: `cargo build --release -p lumen-audio --example audio_tone`
(binary: `%USERPROFILE%\lumen-target\release\examples\audio_tone.exe`). In
PowerShell, run it as
`& "$env:USERPROFILE\lumen-target\release\examples\audio_tone.exe" <args>`.
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
are probably the report loop catching up at wake; to confirm with the
user: how long after waking the tone returned.
