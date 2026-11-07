+++
title = "Trim and fade inside an audio clip"
description = "Trim a clip's ends, draw its fades, set its gain and slide its audio, all in the Clip tab, with the mouse or the keyboard."
weight = 20

[extra]
kind = "Guide"
area = "clip"
minutes = 4
also = ["clip handles", "fade in", "fade out", "clip gain", "slip edit", "slide audio in a clip", "trim audio"]
related = [
  { kind = "Guide", title = "Look inside an audio clip" },
  { kind = "Guide", title = "Split a clip" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Look inside an audio clip"
next = "Split a clip"

[taxonomies]
actions = ["edit.slip_clip_earlier", "edit.slip_clip_later", "gesture.audio_editor_trim", "gesture.audio_editor_fade", "gesture.audio_editor_fade_bend", "gesture.audio_editor_gain", "gesture.audio_editor_slip"]
+++

{{ clip(id="audio-editor-handles-lead", lead=true, focus="50%", alt="In the Clip tab, the Drums clip's fade in handle is dragged a beat in, its end is dragged a beat earlier, and its gain line is pulled down.") }}

The Clip tab is the place to shape one audio clip up close. Its edges,
fades and gain are always drawn there, so you can grab them straight away,
and every change is the same one you would make on the arrangement: it
sounds the same and takes one undo step.

{{ shot(id="audio-editor-handles", focus="50%", alt="The audio editor with the Drums clip open: a grip at the clip's end, a round handle at the top of its start, and a line across the wave for its gain", callouts=["Trim grip", "Fade handle", "Gain line"]) }}

## With the mouse

Open the clip first: double-click it, or select it and press
{{ keys(id="midi.open_editor") }}.

- **Trim:** drag the grip at either edge of the clip {{ callout(n=1) }}.
  It lands on the grid; hold <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
  to place it anywhere. Hold <kbd>Shift</kbd> while you drag the end to
  loop the clip up to there.
  {{ clip(id="audio-editor-trim", focus="50%", alt="The grip at the Drums clip's end is dragged two beats earlier, and the bright part of the wave gets shorter.") }}
- **Fade:** drag the round handle at a top corner {{ callout(n=2) }}
  inwards. Drag the fade's middle dot up or down to bend it, and
  right-click the fade for its shapes. Double-click the handle to remove
  the fade, or the dot to straighten it.
  {{ clip(id="audio-editor-fade", focus="50%", alt="The fade in handle is dragged two beats in, and a curve fades the wave in from silence.") }}
  {{ clip(id="audio-editor-fade-shape", focus="50%", alt="The fade's middle dot is dragged upwards, and the curve bends so the sound comes in sooner.") }}
- **Fade into the next clip:** drag the fade out handle past the clip's
  end. When another clip follows on the same track, the two crossfade,
  just as they do on the arrangement.
- **Gain:** drag the line across the wave {{ callout(n=3) }} up or down.
  Double-click it to go back to 0 dB.
  {{ clip(id="audio-editor-gain", focus="50%", alt="The gain line is pulled down, and the wave gets smaller as the clip gets quieter.") }}
- **Slide the audio:** click **Whole file**, then drag the grey part of
  the file. The audio slides under the clip while the clip stays where it
  is in the song. To slide from anywhere, hold
  <kbd class="k-win">Ctrl</kbd><kbd class="k-mac" data-pagefind-ignore>Cmd</kbd>+<kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
  and drag.
  {{ clip(id="audio-editor-slip", focus="50%", alt="With the whole file showing, its grey end is dragged to the left, and a later part of the file now plays in the clip.") }}

Drag a grip or a fade towards the edge of the editor and the view scrolls
along with you, so you can reach past what is on screen in one go.

## With the keyboard

1. Press <kbd>F6</kbd> until the detail panel has the keyboard, then
   <kbd>Tab</kbd> to the handles. They come in this order: trim start, fade
   in, gain, fade out, trim end. A screen reader hears each one with its
   value, such as "Fade in 125 ms".
2. On a grip or a fade, <kbd>Left</kbd> and <kbd>Right</kbd> move it by the
   grid, and with <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
   finely. On a fade, <kbd>Up</kbd> and <kbd>Down</kbd> bend it. On the
   gain, <kbd>Up</kbd> and <kbd>Down</kbd> change it by 1 dB, and with
   <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
   by 0.1 dB.
3. Press <kbd>Enter</kbd> to type a value: a place in bars and beats for a
   grip, a length such as "250 ms" for a fade, or a number of dB for the
   gain. A fade's box also lists its shapes. Press <kbd>Enter</kbd> again,
   and the handle has the keyboard back.
4. Press <kbd>Delete</kbd> on a fade to remove it, or on the gain to go
   back to 0 dB. The clip itself stays.

To slide the audio inside the clip, press
{{ keys(id="edit.slip_clip_earlier") }} or
{{ keys(id="edit.slip_clip_later") }} while the editor is in use. Each
press moves it by one grid step.

## Good to know

- Each drag, key press or typed value is one step to undo.
- A locked clip, or a clip on a frozen track, shows its handles but does
  not change. The editor says why, and how to free it.
- On a clip that loops, the end grip and the fade out sit at the clip's
  real end, after its repeats. Click **Whole file** to see them.
- When the header gets narrow, its buttons lose their words and then move
  into the **⋯** menu. They are also in the command palette
  ({{ keys(id="view.palette") }}).

{% tip() %}
Trim a little past where you want the clip to end, then pull the fade out
back over the extra. The sound dies away instead of stopping short.
{% end %}
