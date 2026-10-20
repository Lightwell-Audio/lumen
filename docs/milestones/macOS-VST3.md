# Native VST3 hosting on macOS

Approved scope, 2026-10-19: bring the existing VST3 workflow to macOS,
including scanning, isolated instruments and effects, state, presets and
native plugin windows. Plugins must contain code for the host architecture;
bridging Intel-only plugins into an Apple silicon host is outside this slice.
M5/M6 closeout and M7 retain their existing status.

## Plan and acceptance

1. Resolve a bundle's declared executable and call its `BundleEntry` with
   a valid, retained `CFBundleRef` before requesting its factory. Keep bare
   test libraries usable and test initializer failure.
2. Implement child-owned AppKit editor windows on the main thread, attach
   the VST3 `NSView`, service requests alongside events, and preserve editor
   size, position, close, visibility and keyboard behavior. Reuse the existing
   device cards and commands.
3. Validate shared-memory wakeups and cleanup after the parent dies,
   including a host hung inside plugin code. Keep callback contracts intact.
4. Run native host/catalog/hosting tests, the existing application workflows,
   smoke tests and workspace lint/test gates. Run installed plugins through
   the isolated compatibility probe; distinguish processing evidence from
   audible playback and interactive editor checks.
5. Record measurements, anonymous compatibility records and remaining manual
   checks; update progress, architecture and handoff. Stop before starting
   another milestone.

The repository's architecture skill calls for bounded delegated tasks.
Separate tasks cover bundle loading, editor windows and process lifecycle;
integration and final checks stay with the coordinator.

## Verification

Native run on Apple silicon (arm64, macOS 15), 2026-10-19, on the branch's
uncommitted tree:

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings`
  are clean; `cargo test --workspace --no-fail-fast` passes, including 539 E2E
  scenarios and the 6 smoke tests under a real window.
- `lumen-host-probe` over the five installed bundles (labels Probe 1 to 5):
  three instruments pass every stage (scan, load, buses, configure, process,
  link, state, parameters, editor open and close, shutdown). The link stage at
  128 frames/48 kHz (period 2,666 µs) measured a round-trip median of 66 to
  68 µs and p99 of 83 to 97 µs, with the plugin's own work at a median of 5 to
  10 µs and nothing over 75% of the period. One of them declares 59 audio buses
  and loads (the bus-count change in ADR-0085). Two sample-library instruments
  were silent for the probe's note because no patch was loaded (a note, not a
  failure); their saved state differs after a restore (a note: the plugin
  writes something that changes).
- Probe 1 and Probe 2 (an effect and a synth) contain only `x86_64` code and
  are refused at scan with a reason that names the architecture. This is the
  out-of-scope Intel-only case above; it is shown as a recoverable failed card.

Found by running the app (2026-10-19): the plugin window's button did nothing
because the window opened behind Lumen's. Fixed in the host (ADR-0085: activate
on open, floating level while Lumen or the host is active). Checked by hand in
the release app with one real instrument: the window opens in front, stays in
front when Lumen is clicked, and drops behind another application's windows
when that is active. No automated test covers window stacking.

Not run, and not claimed: audible playback through a real device, typing and
user resizing in a real plugin window, multiple displays, `--bench-callback`
numbers, the Windows and Linux cross-target clippy (those Rust targets are not
installed here), and RealtimeSanitizer. These are MT-VST-MAC for the owner.
