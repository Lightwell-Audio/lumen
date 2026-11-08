+++
title = "Hear a clip while you edit it"
description = "Loop one clip or a picked range while you work on it, scrub along the editor's ruler, and give the editor a window of its own."
weight = 50

[extra]
kind = "Guide"
area = "clip"
minutes = 4
also = ["loop a clip", "loop a range", "scrub", "listen while editing", "pop out the editor", "second monitor"]
related = [
  { kind = "Guide", title = "Pick part of a clip in its editor" },
  { kind = "Guide", title = "Draw a clip's envelope" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
prev = "Draw a clip's envelope"
next = "Split a clip"

[taxonomies]
actions = ["view.audio_editor_loop_clip", "midi.pop_out_editor", "gesture.audio_editor_scrub"]
+++

{{ clip(id="audio-editor-hearing-lead", lead=true, focus="50%", alt="In the Clip tab, Loop clip is switched on and the playhead goes round the clip again and again.") }}

When you trim, fade or draw, you want to hear it. **Loop clip** plays the
clip on repeat, with the other tracks still playing, and nothing else
changes: your project's own loop is left alone, and there is nothing to
undo.

{{ shot(id="audio-editor-hearing", focus="50%", alt="The audio editor with Loop clip switched on and the ruler above the wave", callouts=["Loop clip", "Ruler"]) }}

## Loop a clip or a range

1. Open the clip in its editor, then switch on **Loop clip** {{ callout(n=1) }}
   in the header, or use the command palette ({{ keys(id="view.palette") }}).
   The playhead goes to the start of the clip if it was somewhere else.
2. Press {{ keys(id="transport.play_stop") }} to listen. Edit as you like: the loop
   follows your trims.
3. Pick a range (see
   [Pick part of a clip in its editor](@/guides/clips/pick-part-of-a-clip-in-its-editor.md))
   and only that stretch repeats. Press <kbd>Esc</kbd> and the whole clip
   repeats again.

A clip that loops its audio repeats one turn of its loop. Loop clip switches
off by itself when you open another clip, close the editor or start
recording. It does not come back until you switch it on again.

## Scrub along the ruler

The ruler {{ callout(n=2) }} above the wave moves the playhead.

- **Click** to put the playhead there.
- **Drag** while stopped to hear the grid step under the pointer on repeat.
  Hold <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
  to follow the pointer like tape, without the grid.
  {{ clip(id="audio-editor-scrub", focus="50%", alt="A drag along the ruler plays the step under the pointer and leaves the playhead where the hand let go.") }}
- **Drag** while playing to jump playback along.

The first move pauses Follow, so the view stays where you are working.

## With the keyboard

Press <kbd>F6</kbd> until the detail panel has the keyboard, then
<kbd>Tab</kbd> to the ruler. A screen reader hears "Editor ruler" and where the
playhead is. <kbd>Left</kbd> and <kbd>Right</kbd> move the playhead by the
grid, and with <kbd class="k-win">Alt</kbd><kbd class="k-mac" data-pagefind-ignore>Option</kbd>
by a fine step.

## Give the editor a window

Click the **Pop out** button at the right end of the header, or use the
command palette, and the editor opens in its own window. Move it to a second
monitor and keep the arrangement in view. Lumen remembers where you put it.

The clip's properties stay in the detail panel, which says that the editor
is in its own window. Click **Dock editor**, or close the window, and the
editor comes home.

{% tip() %}
Loop clip and a range make a quick way to fix one hit: pick the hit, loop it,
and trim until it sounds right.
{% end %}
