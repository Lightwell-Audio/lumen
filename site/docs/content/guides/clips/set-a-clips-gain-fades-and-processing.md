+++
title = "Set a clip's gain, fades and processing"
description = "Use the strip under the audio editor's header to set a clip's gain and fades, mute or lock it, reverse it, flip its polarity, normalize it, and find or relink its file."
weight = 25

[extra]
kind = "Guide"
area = "clip"
minutes = 4
also = ["clip properties", "reverse audio", "invert polarity", "normalize", "relink missing file", "show file in explorer", "lock a clip", "mute a clip", "clip name and color"]
related = [
  { kind = "Guide", title = "Trim and fade inside an audio clip" },
  { kind = "Guide", title = "Look inside an audio clip" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Trim and fade inside an audio clip"
next = "Pick part of a clip in its editor"

[taxonomies]
actions = ["clip.properties", "clip.mute", "clip.lock", "clip.reverse", "clip.polarity", "clip.normalize", "clip.reveal_file"]
+++

{{ clip(id="audio-clip-properties-lead", lead=true, focus="30%", alt="In the strip under the Clip tab's header, Reverse is clicked and the Drums clip's wave turns round, then Polarity is clicked and a small mark appears on the clip.") }}

Everything you set on one audio clip as a number or a switch is in one row
under the Clip tab's header: its gain, its two fades, Mute and Lock, and
Reverse, Polarity and Normalize. They always act on the clip that is open,
even if you have since selected another one on the arrangement.

{{ shot(id="audio-clip-properties", focus="30%", alt="The strip under the Clip tab's header, with the clip's name at the left, then Gain, Mute and Lock, the two fades, Reverse, Polarity and Normalize, and the file's name at the right", callouts=["Gain", "Fades", "Processing", "File"]) }}

## With the mouse

Open the clip first: double-click it, or select it and press
{{ keys(id="midi.open_editor") }}.

- **Gain** {{ callout(n=1) }}: drag the slider left or right, or click it and
  type a number of dB. Double-click it to go back to 0 dB. It is the same
  gain as the line across the wave.
  {{ clip(id="audio-clip-gain", focus="30%", alt="The Gain slider is dragged to the left and the wave gets smaller.") }}
- **Fades** {{ callout(n=2) }}: set each fade's length in milliseconds, and
  pick its shape from the list beside it.
- **Mute** and **Lock**: the **M** button silences the clip, and the padlock
  fixes it in place. A locked clip keeps its gain, fades and processing as
  they are, but you can still mute it, rename it and unlock it.
- **Reverse**, **Polarity** and **Normalize** {{ callout(n=3) }}: Reverse
  plays the clip backwards, Polarity turns the wave upside down, and
  Normalize raises or lowers the gain until the loudest sample reaches full
  scale. The audio file is never changed.
- **The clip's name** at the left opens a small panel to rename the clip or
  give it a colour of its own.
- **The file** {{ callout(n=4) }} at the right shows which file the clip
  plays. Click it to see its length, rate and where it is, and **Show in
  file manager** to open that folder.
  {{ clip(id="audio-clip-file", focus="30%", alt="The file chip at the right of the strip is clicked, and a panel opens with the file's name, its facts and Show in file manager.") }}

A reversed clip, or one with its polarity turned over, carries a small mark
at the right end of its title on the arrangement, so you can tell it from
the others. Rest the pointer on the title and the line at the bottom of the
window says what was done.

## With the keyboard

1. With the pointer over the editor, press {{ keys(id="clip.properties") }}.
   The Gain slider has the keyboard.
2. Press <kbd>Tab</kbd> to walk the strip: Gain, Mute, Lock, the fades,
   Reverse, Polarity, Normalize and the file. A screen reader hears each
   one with its name and value.
3. On a slider, press <kbd>Enter</kbd> and type a value, then
   <kbd>Enter</kbd> again. <kbd>Esc</kbd> keeps what was there.
4. On a button, press <kbd>Space</kbd> or <kbd>Enter</kbd>.

When the pointer is somewhere else, {{ keys(id="clip.properties") }} opens
the clip's properties card on the arrangement instead.

Every part of the strip is also in the command palette
({{ keys(id="view.palette") }}) as "Reverse", "Invert polarity",
"Normalize", "Lock clip" and "Show the clip's file", and they work on
several selected clips at once from the arrangement.

## Good to know

- Each change is one step to undo.
- A missing file is flagged in the strip: the file chip says **Missing**.
  Click it and choose **Relink…** to pick the file that stands in for it.
  Lumen shows the Project media tab while it checks the file, and says
  what it found.
- On a narrow window the strip folds: the fades and then the processing
  buttons move into the **⋯** menu, and the clip's name becomes its colour
  dot. Nothing is lost, and every part keeps its place in the Tab order.
- A clip on a frozen track keeps its marks and values, but nothing in the
  strip changes it until you unfreeze the track.

{% tip() %}
Normalize and then lower the gain by a few dB. Every clip then starts from
the same level, with room to spare before the mix gets loud.
{% end %}
