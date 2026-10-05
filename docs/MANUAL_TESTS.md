# Lumen — Manual Test Scripts

Tests that need real hardware, third-party software or a human. Each entry
lists steps, the expected result and its last recorded run.

Prototype binaries are in `%USERPROFILE%umen-bin` (see the AppData warning in
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

### MT-ASIO-2 · Stable streaming at the interface's buffer size
1. `target/release/asio-probe stream "Focusrite USB ASIO" --seconds 30`
2. Repeat with `--engine-tracks 64`.

**Expected:** callback count matches duration; 0 sample-position
discontinuities; 0 real-time heap operations; worst callback well under the
period. Output is silent.

**Last run (2026-10-05):** pass. Results in ADR-0006.

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

**Last run:** not yet run. Needs a person at the machine.

### MT-ASIO-5 · Hardware removed while streaming (needs a human)
1. Start a 60 s stream on the Focusrite.
2. Unplug the interface's USB cable after about 10 s.

**Expected:** the probe does not crash. It stops receiving callbacks and
reports the reset or error messages the driver sends. (Recovery UX is
designed in M2.)

**Last run:** not yet run.

### MT-ASIO-6 · Driver busy in another application (needs a human)
1. Open another ASIO application (for example Ableton Live) on the Focusrite
   driver and start playback.
2. Run `asio-probe query` and `asio-probe stream "Focusrite USB ASIO" --seconds 10`.

**Expected:** a clear error or shared operation, depending on the driver. No
crash and no hang.

**Last run:** not yet run.

---

## M1 · P-ISO — plugin isolation transport (`prototypes/isolation-ipc`)

### MT-ISO-1 · Transport latency, fan-out, crash and hang survival
`target/release/isolation-ipc all --seconds 20`

**Expected:** every scenario reports 0 deadline misses, 0 output mismatches,
0 loop overruns and 0 real-time heap operations, except for the deliberately
faulted host. The crash scenario reports exit code 0xC0000005 and a
replacement. The hang scenario reports "hung (still running; killed)" and a
replacement. The healthy host shows 0 misses in both.

**Last run (2026-10-05):** pass. Numbers in ADR-0005. Automated in the sense
that no human input is needed, but it needs a quiet machine. Not in CI:
shared runners are too noisy for latency numbers.

### MT-ISO-2 · No orphaned hosts (needs a human)
1. Run `target/release/isolation-ipc bench --hosts 8 --seconds 60`.
2. Kill the parent from Task Manager after a few seconds.

**Expected:** all 8 `isolation-ipc.exe child` processes disappear at once
(kill-on-close Job object).

**Last run:** not yet run.

---

## M1 · P-VST3 — out-of-process scanning (`prototypes/vst3-scan`)

### MT-VST3-1 · Scan every installed plugin
`target/release/vst3-scan scan --jobs 4 --timeout 30 --json prototypes/vst3-scan/out/scan-full.json`

**Expected:** every module is either enumerated or classified (failed,
timed out, crashed) and the scanner finishes. Every module with
`moduleinfo.json` matches its factory. Record results in
`docs/PLUGIN_COMPATIBILITY.md`.

**Last run (2026-10-05):** pass. 39 of 40 enumerated, Opus failed in its own
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

**Last run (2026-10-05):** as recorded in ADR-0002.

### MT-UI-2 · Accessibility tree (needs a human for the screen-reader part)
1. `target/release/ui-stress --close-after 60`
2. Inspect with Accessibility Insights for Windows, or start Narrator
   (Ctrl+Win+Enter) and Tab through the transport and track headers.

**Expected:** buttons are announced as "Play", "Stop", "Record", "Loop";
track toggles as "Mute Track N" / "Solo Track N" check boxes with their
state.

**Last run (2026-10-05):** tree verified programmatically through UI
Automation (58 named elements). Not yet heard through Narrator.

### MT-UI-3 · Visual quality at 100%, 150%, 200% (needs a human)
1. Run `target/release/ui-stress` at each Windows display scale.
2. Judge text crispness, line sharpness and waveform appearance.

**Last run:** not yet run.
