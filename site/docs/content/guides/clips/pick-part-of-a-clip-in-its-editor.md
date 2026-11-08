+++
title = "Pick part of a clip in its editor"
description = "Drag across a clip's wave to pick a range, put the edit cursor where you want to cut, and split, crop, delete or join exactly there."
weight = 30

[extra]
kind = "Guide"
area = "clip"
minutes = 4
also = ["select part of a clip", "time range in the audio editor", "edit cursor", "zero crossing", "cut audio at a point"]
related = [
  { kind = "Guide", title = "Trim and fade inside an audio clip" },
  { kind = "Guide", title = "Split a clip" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Set a clip's gain, fades and processing"
next = "Draw a clip's envelope"

[taxonomies]
actions = ["edit.audio_editor_zero_crossings", "gesture.audio_editor_range", "gesture.audio_editor_cursor"]
+++

{{ clip(id="audio-editor-range-lead", lead=true, focus="50%", alt="In the Clip tab, a range is dragged across the Drums clip's wave from bar 2 to bar 3, and Ctrl+E cuts the clip at both edges of the range.") }}

You can pick a stretch of a clip's wave right where you see it. The range
is the same time range you use on the arrangement, so Split, Crop, Delete
and Join all act on it, and each one is a single step to undo.

{{ shot(id="audio-editor-range", focus="50%", alt="The audio editor with a stretch of the Drums clip's wave picked out in the accent colour", callouts=["Range"]) }}

## Pick a range

1. Drag across the wave {{ callout(n=1) }}. The range snaps to the grid;
   hold <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
   to place its edges anywhere.
   {{ clip(id="audio-editor-range", focus="50%", alt="A range is dragged across the wave and stays picked once the mouse is released.") }}
2. Press {{ keys(id="midi.split_notes") }} to cut the clip at both edges, or
   press <kbd>Delete</kbd> to take the range out, or use Crop to keep only
   the range.
3. Press <kbd>Esc</kbd> to let go of the range.

The range only covers the part of the clip that is heard before it starts
to repeat, so on a looping clip you cannot pick something that plays again
later.

## Put the edit cursor

Click the wave once to put the edit cursor there. It is separate from the
playhead, so you can cut in one place and listen from another.
{{ clip(id="audio-editor-cursor", focus="50%", alt="A click on the wave puts a thin cursor line where the pointer was.") }}

With a cursor and no range, {{ keys(id="midi.split_notes") }} splits the clip at
the cursor.

## Land on a quiet spot

A cut made in the middle of a loud sound can click. With **Zero
crossings** on, trim edges, range edges and the cursor settle on the
quietest frame close to where you let go, so the cut is clean. Hold
<kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
to skip both the grid and the crossing. You will find the switch in the
editor's header and in the command palette ({{ keys(id="view.palette") }}).
A trim you make with the arrow keys moves by the grid and does not look for a
crossing; the range and the cursor placed with the keys do.

## With the keyboard

1. Press <kbd>F6</kbd> until the detail panel has the keyboard, then
   <kbd>Tab</kbd> to the clip's wave. A screen reader hears "Clip wave",
   and where the cursor is.
2. <kbd>Left</kbd> and <kbd>Right</kbd> move the cursor by the grid, and
   with <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
   finely. The first press puts it at the playhead, or at the start of the
   heard part when the playhead is elsewhere.
3. <kbd>Home</kbd> and <kbd>End</kbd> go to the ends of the heard part.
   Hold <kbd>Shift</kbd> with any of these to pick a range from where the
   cursor was.
4. Press {{ keys(id="midi.split_notes") }}, <kbd>Delete</kbd> or
   {{ keys(id="edit.undo") }} as you would with a range from the mouse.

## Good to know

- Each split, crop or delete is one step to undo, and a split followed by
  Join brings back the clip you started with.
- A range you drag past the edge of the editor scrolls the view along with
  you.
- A locked clip, or a clip on a frozen track, shows its range but does not
  change. The editor says why.

{% tip() %}
Place the cursor with the arrow keys, then press
{{ keys(id="midi.split_notes") }}: you never need the mouse to cut at a beat.
{% end %}
