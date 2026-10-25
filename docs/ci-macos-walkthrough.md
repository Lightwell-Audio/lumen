# macOS CI walkthrough correction

Branch: `fix/macos-ci-walkthrough`, cut from `origin/main` on 2026-10-24.
This is a test correction, with no project-format or application changes.

## Plan

1. Read recent failed CI logs and trace the sidechain reconfiguration path.
2. Reproduce the suspected fixture expiry without changing sound thresholds.
3. Correct the fixture and platform-dependent menu query, add regression
   coverage and failure diagnostics, and run the repository checks.

## Evidence and change

Four inspected failed runs (40126685876, 40134431556, 40135770903 and
40143532508) all failed the plugin walkthrough's unkeyed-bass assertion.
The last also failed the small-window menu test's query for `New Ctrl`.
The menu's accessible name contains the platform shortcut; the test now
queries `New `, independently of its modifier spelling.

The walkthrough's notes lasted 60 quarters at 120 BPM: 30 seconds. Their
clips lasted 64 quarters. Audio advances while the UI and background work
run, and each meter probe may itself advance more than 30 seconds of audio
before giving up. Extending a wait cannot restore an expired note.

On Windows, the unchanged walkthrough passed in 14.66 seconds. Inserting a
temporary 31-second pause before the sidechain edit, then polling the UI to
catch up, reproduced the exact unkeyed-bass assertion with `f32::MIN` in
67.66 seconds. The edit began at roughly quarter 69, past the note and clip
ends. This demonstrates a fixture defect; it does not establish the transport
position of the historical macOS failures or exclude another hosting issue.

The fixture now sustains notes and clips for one hour. Every audio probe
asserts that playback is still within that span, so expiry cannot be mistaken
for successful sidechain suppression. Unsuccessful probes report transport,
host states and track meters. Host `ready` is the existing public state and
does not prove that a pending relink has completed. The UI operation also
asserts that it actually removed the sidechain from the project.

A regression scenario locates to quarter 64, beyond the old fixture, then
checks the real hosted instruments, sidechain suppression, removal and return
audio. Restoring the short fixture makes it fail immediately with the expiry
diagnostic. Neither scenario restarts playback to recover the bass after
sidechain removal. Sound thresholds, probe limits and buffer sizes are
unchanged; no test is skipped and no retries were added.

## Validation

- Windows focused walkthroughs: 2 passed in 18.50 seconds.
- The same temporary 31-second pause and catch-up passed with the extended
  fixture in 45.90 seconds. The injected pause was removed afterwards.
- Windows workspace tests (`--all-targets --locked --no-fail-fast`): 2,897
  passed, including all 663 end-to-end scenarios (98.40 seconds for that binary).
- All 6 real-binary smoke tests passed; workspace doc tests passed.
- Workspace clippy with `-D warnings`, macOS cross-target clippy and rustfmt
  checks passed. Independent read-only review found no actionable issues.
- Linux mirror clippy passed. All 664 end-to-end scenarios passed in 96.31
  seconds, including both walkthroughs and the menu scenario. The full
  workspace run had one failure:
  `lumen-plugin-host --test orphans parent_exit_during_child_startup_is_detected`
  reported a watcher child outliving its exited parent. It also failed alone
  on this patch and on the unchanged base commit `e492b95`, establishing an
  independent Linux/WSL failure. It is recorded in `KNOWN_ISSUES.md`; no host
  lifecycle code or cleanup assertions were changed here.
- Native macOS execution remains pending. The next hosted run must pass both
  walkthroughs; if it still fails, use the new state diagnostics to investigate
  hosting rather than extending waits or suppressing the assertion.
