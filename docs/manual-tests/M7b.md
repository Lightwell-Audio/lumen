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
   the clip menu's Edit → Select submenu (and Ctrl+K).
10. Check the clip menu in both themes at 100, 150 and 200% scaling: the new
    rows match the others and the menu fits your window.
11. Save, close and reopen: reversed, inverted and normalized clips come back
    as they were.

## MT-EDIT-3 — Crossfades (E2)

**State:** Passed (owner, Windows, 2026-10-24, after the rework; no step detail).
Branch: `m7b-s2-join-crossfades`. Use a scratch project.

```powershell
cargo run --release -p lumen-app
```

1. Drop an audio file on a track and split it (Ctrl+E). With the halves
   against each other, grab the left half's fade-out handle (the ring in its
   top-right corner) and drag it right, over the right half. The left clip
   is extended over the right one and fades into it; the farther you drag,
   the longer the crossfade. Both curves cross the zone from corner to
   corner; the length shows beside the pointer.
2. The right clip starts where the crossfade ends, and the left clip ends
   there too. Select the right clip: its outline begins at the end of the
   crossfade. Drag the right clip to reposition it: it moves from that
   start, and it is an ordinary clip again (no crossfade left behind).
3. Drag the handle back to the split in the same gesture: the clips are as
   they were. Undo and Redo treat the whole drag as one step.
4. Grab the handle of an existing crossfade: moving it left lengthens the
   crossfade and moving it right shortens it, the clips staying put.
   The right clip's fade handle lengthens it to the right: the crossfade
   begins where it did and ends further along, as pulling the left clip over
   does, and it stays a crossfade. Drag the left clip's end edge further over
   the right clip: the crossfade moves along and keeps its length.
   Double-click a handle to remove the crossfade. Right-click the handle for the
   preset shapes and bend the curve; check the sound.
5. Place two different audio clips against each other and do the same drag.
   Play through the crossfade and check for a clean transition, with the
   outgoing clip's sound fading out as the incoming one's fades in.
6. Select the right clip and use Toggle crossfade with the clip before
   (Ctrl+Shift+X, the palette or the clip menu): a 5 ms crossfade is made or
   removed. With nothing before the clip, the hint says so.
7. Find Automatic crossfades in the project menu or the palette and toggle
   it; Undo and Redo restore the setting. An older project opens with it
   off. Automatic crossfades are played but not drawn.
8. Look at both themes at 100, 150 and 200% scale, including the smallest
   window; the menus scroll to their lower controls.
9. Save, close and reopen: the crossfade is as you left it.

## MT-EDIT-4 — Consolidate (E3)

Start from a project with an audio track. Work on a copy of a project you do
not mind changing.

1. Import or record an audio clip about ten seconds long. Split it into three
   parts (Ctrl+E) and move the middle part later so there are gaps, and give
   the first part a fade-in and the last a fade-out. Select the three parts
   (Ctrl+A) and play them; remember the sound.
2. Press Ctrl+J. A message with a percentage and "Esc to stop" shows while
   it renders. Afterwards one clip stands where the three were, named after the
   first with "(consolidated)", spanning from the first start to the last end,
   and the new clip is selected. Play it: it sounds the same as before, with
   the same silences and the same fades. There is no click at either end.
3. Undo: the three parts are back exactly. Redo: the single clip is back.
4. Do the same from the clip menu (right-click, Edit, Consolidate) and from the
   palette (Ctrl+K, Consolidate) with clips that have different gains, a
   reversed clip, a clip envelope and a crossfade. Compare by ear with undo and
   redo, soloing the track. Anything the clips did is in the sound; the
   track's fader and pan still apply on top of it, as before.
5. Select one audio clip and Consolidate it: it is rendered into a new clip of
   the same length. Select nothing: the command is greyed in the palette.
6. Start Consolidate on a long clip (a few minutes) and press Esc: it stops, the
   clips are unchanged and no clip is added. Start it again and edit something
   in the project while it runs: when it finishes it says the clips changed
   and nothing is replaced.
7. Make a MIDI clip that loops, and a second MIDI clip after a gap on the same
   track. Select both and Consolidate: one MIDI clip remains with the loop
   written out as real notes. Undo restores the loop and both clips. A
   selection of MIDI and audio clips on different tracks changes both in one
   undo step.
8. Look at the media pool's source (the project's media): the original file is
   still listed.
9. Save the project to a new folder. The rendered file is in
   `Samples/Rendered`. Close Lumen and open it: the clip plays. Close and
   open a project saved earlier than the consolidate: nothing is missing.
10. Look at the clip menu in both themes at 100, 150 and 200% scale: the
    Consolidate row is whole and the menu still scrolls to its lower controls.
