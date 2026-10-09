# Lumen — Known issues

Problems found and not fixed yet, with what they affect and where they are
addressed. Fixed issues move to the relevant decision record or manual test
run. Add new ones here as they are found.

| Found | Issue | Effect | Addressed in |
|---|---|---|---|
| 2026-10-07 (M3 4c) | **A second instance offers to recover a running instance's autosave.** Recovery only skips its own session (`app/recovery.rs`, `autosave::leftovers`), so a second Lumen shows "did not close normally" for a session that is still open. | Pressing Discard there deletes the live session's autosave; that session writes a new one at its next autosave, so work is at risk only until then. Avoid running two instances. | Single instance (M9, PROGRESS "Single instance"); or sooner, a lock per running session that recovery skips |
| 2026-10-07 (M3 2) | **Audio clips do not follow tempo.** Changing the tempo keeps each clip's start in beats; its audio plays at its own speed, so playback continues from the same bar at a different point in the file, and a clip's region can cut its audio short. | Expected until warping | Warp (S, later). Clip trimming (M3 6c) shipped and does not change how a clip follows tempo |
| 2026-10-08 (M3 6d) | **egui names a submenu button with its arrow**: screen readers hear "Fade-out shape ⏵" (and "Open recent ⏵", "Colour ⏵"). | Accessible names read the arrow | The accessibility pass (M10), or a submenu helper in `widgets.rs` |
| 2026-10-08 (MT-APP-14 run 3) | **Files dragged in on macOS and Linux may not follow the pointer.** winit 0.30 gives no position while files are dragged over the window, and Lumen reads the system's cursor only on Windows (ADR-0032), so elsewhere a drop lands where egui last saw the pointer, or on new tracks at the end. | Dropping on a chosen track or between tracks may not work there; drops still import | winit 0.31 (drag positions on every system), or a cursor read per system, before the macOS and Linux builds are tested by hand |
| 2026-10-07 (M3 4c) | **Waveform peaks are not reclaimed** after a device-rate change until Lumen restarts (ADR-0027). | The atlas holds about 130 five-minute stereo files | Streaming (M9) |

Follow-ups owned by a later slice of the current milestone are also listed
in the milestone plan (for M3: `docs/milestones/M3.md`, "Follow-ups found on
the way").
