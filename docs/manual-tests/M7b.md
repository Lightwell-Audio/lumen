# Advanced editing and render: manual tests

## MT-EDIT-1 — Tools and split (E0)

**State:** Passed (owner, Windows, 2026-10-23; no step detail). Branch: `m7b-s0-tools-split`.
General clip join/glue is E1; this test checks undo and continuous playback
across a cut. Consolidate, bounce and freeze follow in E3–E5.

From the repository root, build and start the app:

```powershell
cargo run --release -p lumen-app
```

Use a scratch project and a copy of an audio file.

1. Find the tool switch above the track names. Choose each of Pointer, Draw,
   Erase, Split, Mute and Zoom. Find each in Ctrl+K too. B alternates Pointer
   and Draw when the arrangement has the keyboard; Escape returns to Pointer.
   The switch is quiet while Pointer is chosen and lit for any other tool; its
   menu marks the current tool and one click picks and closes it. With each
   tool chosen, hover a lane: the info line at the bottom says what a click
   and a drag do (Split mentions aiming and Alt; Zoom mentions Alt to zoom out).
   Right-click a clip, a track, a locator and a lane's chip: every row in each
   menu has the same height and spacing, and hover (including Lock clip) fills
   the same width.
2. Import audio, choose Split and click inside it. The cut follows the grid;
   Alt places it freely. The source remains in place and both parts touch.
   Undo restores exactly one clip; redo restores both parts.
3. With Split, press inside a clip, move to another point and release. No
   edit happens while aiming. A line marks the cut; release cuts there.
   Repeat, pressing Escape before releasing: no cut should remain.
4. In Pointer, select a clip and click its body while stopped to place the
   edit position. Ctrl+E cuts there. While playing, Ctrl+E cuts at the
   playhead without interrupting playback. The clip menu offers Split too.
5. Drag a time range through two audio tracks and use Ctrl+E (or Split at
   range edges in the menu). Both edges cut every unlocked clip they cross
   on the range's tracks; one Undo restores the whole edit.
6. Split inside a long fade-in and fade-out, then inside a gain/pan envelope.
   Listen across each cut: the shape and level should continue unchanged.
   Undo/redo while playing should produce no click or pause. Use 44.1 kHz
   source audio with a 48 kHz device (and the reverse), set 123 BPM, move the
   clip away from the start and split repeatedly: no source shift should
   accumulate, and the waveform must agree with what plays.
7. Shift-drag a clip's right edge to repeat it, split halfway through a
   repeat, and listen across the cut and the next original repeat boundary.
   It must continue the same source position. Repeat with a tempo change
   in the clip. Check that the waveforms show what plays.
8. Make a MIDI clip with held notes and controller moves, repeat it, and
   split within a repeat. Held notes should carry through the split during
   continuous playback, with no retrigger or hanging note; later repeats
   and controller moves keep their original timing. In the piano roll,
   Ctrl+E still splits picked notes rather than arrangement clips.
9. Lock a clip from its menu. Split, Erase and Mute leave it untouched.
   Unlock it; Mute toggles the clicked clip and Erase removes it, each one
   Undo step. Draw on an empty MIDI lane: a click makes a one-bar clip; press,
   drag and release makes a clip of the dragged length on the grid (Alt for
   off the grid). While aiming, an outline and its length show on the lane and
   nothing is in the project; Escape before release leaves no clip, and one
   Undo removes a drawn clip. Dragging leftwards works too. Draw does not
   create MIDI on an audio lane.
10. With Zoom, click to zoom in, Alt-click out, and drag to fit a time span.
    X returns to the previous view. With Pointer, the existing move, trim,
    slip, range and marquee gestures still work.
11. Save, close and reopen. The split positions, fade fragments, envelopes
    and loop source positions remain. A fresh session begins with Pointer.
12. Check the switch, menu, cut guide and clip feedback in both themes at
    100%, 150% and 200% UI scale, including a 960 × 540 window. Navigate the
    switch and palette with the keyboard; labels and focus remain clear.

### Runs

Run 1 (owner, Windows, 2026-10-23, steps 1 to 12 reported as passed with no
step detail). Findings made during the run, both fixed in the same branch:

- The Lock clip row (and 22 other menu rows built from plain egui buttons)
  differed in height, spacing and hover from the other menu rows. Every menu
  row is now a `menu_item` or `menu_toggle` (`c506e78`).
- The Draw tool duplicated the Pointer's double-click in the arrangement. A
  drag now aims the new clip's length (owner's option A, `84008a1`).

Automated verification is recorded in the slice report; it does not establish
playback by ear or physical-device behavior beyond what the owner reported.

## MT-EDIT-2 — Clip commands (E1)

**State:** Waiting for the owner. Branch: `m7b-s1-clip-commands`.
Consolidate for audio with gaps is E3; here Join is seamless only.

```powershell
cargo build --release -p lumen-plugin-host
cargo run --release -p lumen-app
```

Use a scratch project and two or three audio files, one of them a rising or
falling sound (a fade-in, a drum hit, a spoken word) so reverse is obvious.

1. Import an audio file. Split it twice (Ctrl+E). Select the three parts and
   press Ctrl+J: one clip returns and it sounds the same. Undo gives three
   parts; redo joins them. Change one part's colour before joining: it still
   joins.
2. Move one part away so there is a gap and press Ctrl+J: a note says to use
   Consolidate and nothing changes (audio with gaps is E3). Select two MIDI
   clips with a gap and press Ctrl+J: they merge.
3. Select a looping clip and choose Unloop in the clip menu: real clips, one
   per repeat, sound the same; undo gives the loop back.
4. Make a time range over part of two clips on one track and choose Crop to
   selection (Ctrl+K): only the range remains; undo restores everything.
5. Select a clip and press Alt+Right: it moves one grid step; Alt+Shift+Right
   moves it a 128th note; Alt+Left goes back. Each press is one undo step.
6. Select a clip and choose Reverse (clip menu or Ctrl+K). The waveform
   mirrors (after a moment for a long file) and the clip plays backwards.
   Reverse again: it returns. Trim it, split it, slip it and loop-extend it
   while reversed: each acts as it does on a forward clip. Undo each.
7. Choose Invert polarity on one of two copies of the same clip on two
   tracks and play: they cancel (silence or a thin sound). Undo restores.
8. Select a quiet clip and choose Normalize. Progress shows as a note; the
   clip gets louder and its gain reads as the amount needed (clip card:
   Alt+Enter); with other tracks off the loudest part reaches 0 dB on the
   master. Normalize a second time: nothing changes. Undo gives the old
   gain. Normalize a clip of silence: a note says so. Normalize a long file
   and press Esc: it stops and says so.
9. Use Invert selection, All in track, Select to start and Select to end from
   the clip menu's Select submenu (and Ctrl+K).
10. Check the clip menu in both themes at 100, 150 and 200% scaling: the new
    rows match the others and the menu fits your window.
11. Save, close and reopen: reversed, inverted and normalized clips come back
    as they were.
