+++
title = "Draw a clip's envelope"
description = "Draw gain and pan moves that belong to one audio clip, and loop the clip's audio with the brace under the ruler."
weight = 40

[extra]
kind = "Guide"
area = "clip"
minutes = 5
also = ["clip envelope", "gain envelope", "pan envelope", "volume curve on a clip", "loop audio in a clip", "loop brace"]
related = [
  { kind = "Guide", title = "Pick part of a clip in its editor" },
  { kind = "Guide", title = "Trim and fade inside an audio clip" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Pick part of a clip in its editor"
next = "Hear a clip while you edit it"

[taxonomies]
actions = ["view.audio_editor_gain_envelope", "view.audio_editor_pan_envelope", "midi.draw_tool", "clip.audio_editor_clip_loop", "gesture.audio_editor_envelope_point", "gesture.audio_editor_envelope_draw", "gesture.audio_editor_loop_brace"]
+++

{{ clip(id="audio-editor-envelopes-lead", lead=true, focus="50%", alt="In the Clip tab, the Gain button is lit and three points are clicked on the wave, which make a dip in the clip's volume.") }}

An envelope is a line that changes a sound over time. A clip's gain and
pan envelopes move with the clip, so if you move the clip, they come
along. They sit on top of the clip's own gain and fades.

{{ shot(id="audio-editor-envelopes", focus="50%", alt="The audio editor with Gain lit, a dip drawn across the wave, and the loop brace under the ruler", callouts=["Gain and Pan", "Envelope", "Loop brace"]) }}

## Show an envelope

Click **Gain** or **Pan** {{ callout(n=1) }} in the editor's header. One
envelope shows at a time. Click the same button again to hide it. The
buttons are also in the command palette ({{ keys(id="view.palette") }}).

Showing an envelope in the editor does not show it on the arrangement's
lanes, and the points you pick here are not the ones picked there.

## Add and move points

- **Add a point:** click the line {{ callout(n=2) }} where you want one.
  {{ clip(id="audio-editor-envelope-point", focus="50%", alt="A click on the wave adds a point to the gain envelope.") }}
- **Move a point:** drag it. It follows the grid in time and snaps to
  round values; hold <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
  to place it freely.
- **Remove a point:** pick it and press <kbd>Delete</kbd>.

## Draw a stroke

Press {{ keys(id="midi.draw_tool") }} to take the pencil. If no envelope is
showing, Gain lights up for you. Drag across the wave and Lumen draws the
line and thins out the points it does not need. The whole stroke is one
step to undo. Press the same key again to go back to the pointer.
{{ clip(id="audio-editor-envelope-draw", focus="50%", alt="With the pencil, a drag across the wave draws a falling and rising gain line.") }}

## Loop the clip's audio

The brace {{ callout(n=3) }} under the editor's ruler loops the clip. Click
its band to loop the audio the clip plays now, then drag either end of the
brace to set where the loop starts and ends. Click the band again, or choose
**Loop audio in clip** in the command palette ({{ keys(id="view.palette") }}),
to stop looping.
{{ clip(id="audio-editor-loop-brace", focus="50%", alt="A click on the band under the ruler loops the clip and the brace shows its two ends.") }}

What the clip plays up to the first repeat stays the same. Envelope points
stay on the same sound when you move the brace, and they follow the audio
when the tempo changes.

## With the keyboard

1. Press <kbd>F6</kbd> to reach the detail panel, then <kbd>Tab</kbd> to the
   envelope. A screen reader hears "Gain envelope, 3 points".
2. <kbd>Enter</kbd> adds a point at the edit cursor. <kbd class="k-win">Ctrl</kbd><kbd class="k-mac" data-pagefind-ignore>Cmd</kbd>+<kbd>Left</kbd>
   and <kbd>Right</kbd> pick the point before or after.
3. <kbd>Left</kbd> and <kbd>Right</kbd> move the picked point in time,
   <kbd>Up</kbd> and <kbd>Down</kbd> change its value, and
   <kbd>Delete</kbd> removes it.
4. <kbd>Tab</kbd> on to the loop. <kbd>Enter</kbd> loops or unloops the
   clip, and when the loop's start or end has the keyboard, <kbd>Left</kbd>
   and <kbd>Right</kbd> move it by the grid.

## Good to know

- Every click, stroke, key press and brace drag is a single step to undo.
- A locked clip, or a clip on a frozen track, shows its envelope but does
  not change.
- Dragging a brace end past the edge of the editor scrolls the view.

{% tip() %}
Draw the whole shape roughly with the pencil, then switch back to the
pointer and nudge the few points that matter.
{% end %}
