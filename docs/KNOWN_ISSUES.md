# Lumen — Known issues

Problems found and not fixed yet, with what they affect and where they are
addressed. Fixed issues move to the relevant decision record or manual test
run. Add new ones here as they are found.

| Found | Issue | Effect | Addressed in |
|---|---|---|---|
| 2026-10-07 (M3 4c) | **A second instance offers to recover a running instance's autosave.** Recovery only skips its own session (`app/recovery.rs`, `autosave::leftovers`), so a second Lumen shows "did not close normally" for a session that is still open. | Pressing Discard there deletes the live session's autosave; that session writes a new one at its next autosave, so work is at risk only until then. Avoid running two instances. | Single instance (M9, PROGRESS "Single instance"); or sooner, a lock per running session that recovery skips |
| 2026-10-07 (M3 4) | **Deleting a clip or track while it plays stops its audio at once**, rather than fading (mute, solo, volume and pan glide). | A click | M4 mixer (ADR-0025) |
| 2026-10-07 (M3 2) | **Audio clips do not follow tempo.** Changing the tempo keeps each clip's start in beats; its audio plays at its own speed, so playback continues from the same bar at a different point in the file, and a clip's region can cut its audio short. | Expected until warping | Warp (S); clip trimming (M3 slice 6) |
| 2026-10-07 (M3 4c) | **Waveform peaks are not reclaimed** after a device-rate change until Lumen restarts (ADR-0027). | The atlas holds about 130 five-minute stereo files | Streaming (M9) |
| 2026-10-07 (M3 4d) | **At very deep zoom the ruler labels only sixteenth notes** (bar.beat.sixteenth), so a narrow view can show no label at all; zooming out a little brings them back. | Navigation by the ruler alone is harder at sample-level zoom | Ruler formats (time, samples, timecode), M3 slice 5 |
| 2026-10-07 (M3 4c) | **Waveforms are drawn from 16-frame buckets.** Zoom now goes to 8 points per sample (slice 4d part 1), where buckets are blocks wider than a pixel. | Blocky waveforms at the deepest zooms | Slice 4d part 4 (sample-level drawing) |

Follow-ups owned by a later slice of the current milestone are also listed
in the milestone plan (for M3: `docs/milestones/M3.md`, "Follow-ups found on
the way").
