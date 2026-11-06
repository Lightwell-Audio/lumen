+++
title = "Look inside an audio clip"
description = "Open an audio clip in the Clip tab to see its whole file, zoom down to single samples, and move the playhead."
weight = 10

[extra]
kind = "Guide"
area = "clip"
minutes = 3
also = ["audio editor", "sample editor", "waveform", "zoom into audio"]
related = [
  { kind = "Guide", title = "Split a clip" },
  { kind = "Reference", title = "Commands and shortcuts" },
]
next = "Split a clip"

[taxonomies]
actions = ["midi.open_editor", "view.audio_editor_whole_file", "view.audio_editor_zoom_to_clip"]
+++

{{ clip(id="audio-editor-lead", lead=true, focus="62%", alt="The Drums clip, trimmed to bars 2 and 3, is selected and Enter opens it in the Clip tab. Whole file shows the four-bar file it comes from, then Zoom to clip goes back to the two bars it plays.") }}

A clip plays part of an audio file. Open it and the Clip tab shows that
file across the panel, with the part the clip plays lit up, so you can see
what is around it before you trim, and zoom in until you see every sample.

## With the mouse

1. Double-click the **Drums** clip. The Clip tab opens at the bottom of the
   window with the clip's wave, fitted to the part it plays.
2. Click **Whole file**. The whole file shows: the part the clip plays
   {{ callout(n=1) }} is bright in the track's colour, the rest
   {{ callout(n=2) }} is grey.
   {{ shot(id="audio-editor-whole", focus="40%", alt="The Clip tab's editor showing a four-bar drum file: bars 2 and 3 bright blue, the bars before and after grey", callouts=["The part the clip plays", "The rest of the file"]) }}
3. Click **Zoom to clip** to go back to the part the clip plays.
4. Hold <kbd class="k-win">Ctrl</kbd><kbd class="k-mac" data-pagefind-ignore>Cmd</kbd>
   and turn the mouse wheel over the wave to zoom around the pointer. The
   wheel on its own scrolls. Zoom in far enough and the wave becomes a line
   through the file's samples.
   {{ shot(id="audio-editor-samples", focus="20%", alt="The editor zoomed in to a few hundredths of a second at the start of a kick: a smooth line through the samples, with times like 0:04.005 on the ruler") }}
5. Click the editor's ruler to move the playhead there.

The arrangement above stays where it is while you look around the editor.

## With the keyboard

1. Select the clip and press {{ keys(id="midi.open_editor") }}.
2. With the pointer over the editor, press {{ keys(id="view.zoom_in") }}
   and {{ keys(id="view.zoom_out") }} to zoom around the pointer.
3. Press {{ keys(id="view.zoom_selection") }} to zoom the editor to the
   clip, and {{ keys(id="view.zoom_back") }} to go back to where you were.

**Whole file** and **Zoom to clip** are also in the command palette
({{ keys(id="view.palette") }}), as "Show the whole file in the audio
editor" and "Zoom the audio editor to the clip".

## Good to know

- With **Follow playhead** on, the editor keeps the playhead in view while
  the song plays. Scroll or zoom and it stays where you put it, until the
  song plays again or you press {{ keys(id="transport.follow") }}.
- While the song plays, a click on the ruler moves the playhead when you
  let go, so you can drag to the spot first.
- A long file played from disk shows its samples a moment after you zoom
  in: until then you see its outline.
- If the file is missing, the editor says so and offers **Open the media
  pool**, where you can find it again.
- Looking around changes nothing in the song, so there is nothing to undo.

{% tip() %}
Before you trim a clip, click **Whole file**. You can see at once whether a
hit you want is just outside the clip, or a noise you don't want is waiting
past its end.
{% end %}
