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
actions = ["midi.split_notes", "view.tool_split", "gesture.split_tool_click"]
+++

{{ clip(id="split-lead", lead=true, focus="62%", alt="A drum clip is clicked at bar 3, then Ctrl+E cuts it in two there. The two halves sit side by side.") }}

Split cuts a clip in two right at the playhead. Both halves keep playing
exactly as before, so nothing you hear changes until you move them.

## With the keyboard

1. With the song stopped, click the clip below its name bar, where you
   want the cut. The clip is selected and the playhead jumps to that spot,
   on the grid.
   {{ shot(id="split-before", focus="62%", alt="The Drums clip selected, with the playhead across it at bar 3") }}
2. Press {{ keys(id="midi.split_notes") }}.
   {{ shot(id="split-after", focus="62%", alt="Two Drums clips where there was one, meeting at bar 3", callouts=["First half", "Second half"]) }}
   The first half {{ callout(n=1) }} ends exactly where the second
   {{ callout(n=2) }} begins.

The cut lands on the playhead, so you can also split while the song plays:
select the clip, then press {{ keys(id="midi.split_notes") }} at the moment
you hear the spot. While the song plays, a click selects a clip without
moving the playhead.

{{ video(id="split-playing", alt="The song plays from bar 1. On bar 3, Ctrl+E cuts the selected Drums clip, and the song plays on without a break.", caption="Turn the sound on: the cut lands on the beat and nothing you hear changes.") }}

## With the mouse

{{ clip(id="split-tool", focus="40%", alt="C picks the Split tool. A line follows the pointer across the Bass clip, snapping to the grid, and a click cuts the clip there. Esc goes back to the pointer.") }}

1. Press {{ keys(id="view.tool_split") }}, or choose **Split** from the tool
   switch above the track headers.
2. Click a clip where the cut goes. It snaps to the grid as you aim;
   hold <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd> to cut
   anywhere.
3. Press <kbd>Esc</kbd> to go back to the pointer.

## In the audio editor

When an audio clip is open in its editor and you have clicked its wave to put
the edit cursor there, {{ keys(id="midi.split_notes") }} cuts the clip at the
cursor rather than at the playhead. With a range picked on the wave, it cuts
at both edges. See
[Pick part of a clip in its editor](@/guides/clips/pick-part-of-a-clip-in-its-editor.md).

## On a time range

Drag across a clip below its name bar, or across empty lanes, to select a
time range. Drag up or down as well to take in more tracks. Then press
{{ keys(id="midi.split_notes") }}. Every clip the range crosses on those
tracks is cut at both of its edges, ready to move, mute or delete as one
block.

## Good to know

- Splitting never changes the sound. Played back to back, the two halves
  sound exactly like the clip did, with no click at the cut.
- {{ keys(id="edit.undo") }} puts the clip back in one step.
- Locked clips are left whole, even inside a range.
- Nothing is cut when the playhead is outside the selected clip or right on
  its edge. A click on the name bar puts the playhead at the clip's start,
  so click below it.

{% tip() %}
Split on the beat, not by ear. Leave snapping on and the cut lands on the
grid, so the halves stay in time when you move them around.
{% end %}

{% tryit(answer="split-lead", answer_focus="62%", answer_alt="The Drums clip is clicked at bar 3 and Ctrl+E cuts it there") %}
Split the **Drums** clip at bar 3, then drag the second half one bar later.
Can you hear the gap?
{% end %}
