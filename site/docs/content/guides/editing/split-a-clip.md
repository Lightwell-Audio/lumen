+++
title = "Split a clip"
description = "Cut a clip in two at the playhead, where you click, or at the edges of a time range."
weight = 10

[extra]
kind = "Guide"
area = "edit"
minutes = 3
also = ["cut a clip", "slice", "razor", "chop"]
related = [
  { kind = "Guide", title = "Join clips" },
  { kind = "Guide", title = "Select a time range" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Clips and selection"
next = "Join clips"

[taxonomies]
actions = ["midi.split_notes", "view.tool_split"]
+++

{{ clip(id="split-lead", lead=true, focus="62%", alt="A drum clip is clicked at bar 3, then Ctrl+E cuts it in two there. The two halves sit side by side.") }}

Split cuts a clip in two right at the playhead. Both halves keep playing
exactly as before, so nothing you hear changes until you move them.

## With the keyboard

1. Click the clip where you want the cut. The clip is selected and the
   playhead jumps to that spot.
   {{ shot(id="split-before", focus="62%", alt="The Drums clip selected, with the playhead across it at bar 3") }}
2. Press {{ keys(id="midi.split_notes") }}.
   {{ shot(id="split-after", focus="62%", alt="Two Drums clips where there was one, meeting at bar 3") }}

The cut lands on the playhead, so you can also split while the song plays:
press {{ keys(id="midi.split_notes") }} at the moment you hear the spot.

## With the mouse

1. Press {{ keys(id="view.tool_split") }}, or choose **Split** from the tool
   switch above the track headers.
2. Click a clip where the cut goes. It snaps to the grid as you aim;
   hold <kbd class="k-win">Alt</kbd><kbd class="k-mac">Option</kbd> to cut
   anywhere.
3. Press <kbd>Esc</kbd> to go back to the pointer.

## On a time range

Drag across the lower half of a clip, or across empty lanes, to select a
time range. Then press
{{ keys(id="midi.split_notes") }}. Every clip the range crosses is cut at both
of its edges, ready to move, mute or delete as one block.

## Good to know

- Splitting never changes the sound. Played back to back, the two halves
  sound exactly like the clip did, with no click at the cut.
- {{ keys(id="edit.undo") }} puts the clip back in one step.
- Locked clips are left whole.

{% tip() %}
Split on the beat, not by ear. Leave snapping on and the cut lands on the
grid, so the halves stay in time when you move them around.
{% end %}

{% tryit(answer="split-lead", answer_focus="62%", answer_alt="The Drums clip is clicked at bar 3 and Ctrl+E cuts it there") %}
Split the **Drums** clip at bar 3, then drag the second half one bar later.
Can you hear the gap?
{% end %}
