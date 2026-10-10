# Lumen design system and key flows

**Status:** v1, M3 slice 2 (2026-10-07), extended by slices 4 to 6e (2026-10-08). Code: `crates/lumen-app/src/theme.rs`
(colour, type, spacing, shape and the egui style built from them) and
`crates/lumen-app/src/motion.rs` (motion). Every screen takes its values from
there. A colour or duration written inline in UI code is a bug.

The direction comes from `CLAUDE.md` §5: Ableton's arrangement-first
workflow, with FL Studio's clarity but not its density. Minimal and quiet
chrome, strong contrast on content, one accent colour, and motion that is
restrained and purposeful.

---

## 1. Principles

1. **Content is loud, chrome is quiet.** Clips, notes, waveforms and the
   playhead carry the contrast. Panels, borders and controls stay a step or
   two off the background.
2. **One accent.** Blue marks what is active, selected or playing. Record red
   and warning amber are the only other signal colours, and each means one
   thing.
3. **Progressive disclosure.** Show the common case. Put the rest one click
   away (popover, context menu, settings section), never in a modal.
4. **Motion explains, never decorates.** Something animates only to show
   where it came from or went (a panel sliding, a popover growing from its
   button, a playhead gliding to a new position). Nothing loops or bounces
   for attention.
5. **Input is never delayed.** Animation follows state. It never holds it
   back. A click acts on the frame it lands on.
6. **Calm when idle.** Nothing redraws when nothing changes (near-zero idle
   CPU, quality bar §4).

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│ Lumen   │ transport │ tempo · meter │ position │ CPU · status │ panels │  transport bar, 36 pt
├──────────────────────────────────────────────────────────────────────┤
│ (device banner, only when the audio device needs attention)          │
├────────────┬─────────────────────────────────────────────────────────┤
│ Browser    │ ruler (bars · loop · markers)                            │
│ (left,     ├─────────────────────────────────────────────────────────┤
│ collapsible│ arrangement: track headers │ lanes, clips, playhead      │
│ 220 pt)    │                                                         │
├────────────┴─────────────────────────────────────────────────────────┤
│ Detail panel (bottom, collapsible): Devices · Clip · Mixer           │
├──────────────────────────────────────────────────────────────────────┤
│ Info line: what is under the pointer · its gestures and keys, 32 pt  │
└──────────────────────────────────────────────────────────────────────┘
```

**Text is the last resort (M4 slice 0, the owner's UX pass on hints).**
Behaviour comes across first through the cursor, handle shapes,
highlights and motion. Words go in three places only:
- **The info line** along the bottom of the window (`app/info.rs`, ADR-0034):
  what is under the pointer, then what can be done there, each gesture with
  a mouse and its modifiers as plain keycaps, each command with its
  shortcut as K5 keycaps from the keymap. Held modifiers light the drag
  they choose. New interactive things set an `Info` when hovered; nothing
  else describes a drag.
- **Live values by the pointer, only while dragging** (a fade's length, a
  gain, the tape speed, "new track here"): feedback, not instructions
  (`widgets::draw_hint`).
- **Tooltips** on controls after the pointer rests: the control's name and
  its keys, in a few words, with the detail left to the info line.
Permanent captions are avoided: an empty state says what to do in one line
or offers the button that does it.

- **Minimum window:** 960 × 540 pt, where the whole transport bar fits.
  Window sizes are in points, so the minimum holds at any UI scale.
- Panels **slide** open and closed (motion: `GLIDE`), their contents moving
  at full size rather than re-wrapping, and keep their size
  between sessions (from M3 slice 3).
- Windows (audio settings, later preferences) open at the **top centre** of
  the app, have a **fixed width**, and wrap long text instead of growing.
  They draw their own title row (title on the left, ✕ on the right and
  level with the title's letters; Esc closes) on one flat surface, rather
  than egui's tinted title bar. The row is `widgets::title_row`, shared
  with cards that float beside what they edit.
  **Every window is a `widgets::Dialog`**, so the ✕ sits at the same place
  in all of them; a question (the unsaved-changes prompt) has no ✕ and is
  answered by its buttons.
- Popovers (the audio panel, menus) open under the control that opened them,
  with a small gap.
- **Menus with settings in them** (the owner's C, M4.5): a switch,
  segmented choice or menu toggle leaves the menu open, so a change can be
  followed by another. Once something has changed and the pointer leaves
  the menu (8 pt of slack), it waits 0.4 s and fades; coming back cancels
  it. Esc, a click elsewhere and commands (New, Audio settings…) close it
  at once, with the fade. Every menu is a `widgets::Menu`
  (`Menu::below`, `Menu::context`, `.keeping_open()` for a context menu
  with settings).

## 3. Colour

Tokens are semantic: code asks for `text_secondary`, never a hex value. Both
themes define every token. **Dark** is the default; **Light** and **Match
Windows** are in the app menu (click **Lumen** at the left of the transport bar).

| Token | Dark | Light | Use |
|---|---|---|---|
| `app` | `#17191D` | `#F1F2F4` | Window, panels, transport bar |
| `canvas` | `#1B1D22` | `#FAFAFB` | Arrangement background |
| `raised` | `#23262D` | `#E9EBEE` | Ruler, track headers, inputs |
| `overlay` | `#262A31` | `#FFFFFF` | Popovers, windows, hover cards |
| `control` | `#31353E` | `#E2E5E9` | Button at rest |
| `control_hover` | `#3A3F4A` | `#D7DBE0` | Button under the pointer |
| `control_press` | `#444A56` | `#CBD0D7` | Button held down; checkbox and radio wells |
| `border` | `#2E323A` | `#DADDE2` | Separators, panel edges |
| `border_strong` | `#3C414B` | `#C3C8CF` | Window and popover outlines, focus-free inputs |
| `text` | `#E6E8EC` | `#1C1F24` | Primary text, icons |
| `text_secondary` | `#9AA0AC` | `#5A6170` | Labels, ruler numbers |
| `text_hint` | `#6E7480` | `#878D98` | Hints, empty states |
| `text_disabled` | `#555A65` | `#B3B8C0` | Disabled controls |
| `accent` | `#4FA3FF` | `#1F6FD1` | Active, selected, playhead, focus ring |
| `accent_muted` | `#243A55` | `#D5E5FA` | Background of active toggles |
| `record` | `#E5484D` | `#D33A40` | Record only |
| `warning` | `#F2B14C` | `#A86400` | Problems that need attention |
| `grid_bar` | `#343842` | `#D9DCE1` | Bar lines |
| `grid_beat` | `#262930` | `#ECEEF1` | Beat lines |
| `grid_sub` | `#212329` | `#F3F4F6` | Grid lines between beats |
| `start_marker` | `#C8CCD4` | `#6B717C` | Where Play starts |
| `lane` | `#1B1D22` | `#F6F7F8` | Arrangement lanes behind clips |
| `header` | `#1E2126` | `#ECEEF1` | Track header column |
| `selected_row` | `#223449` | `#DBE7F8` | A selected track header |
| `selected_control` | `#434D5E` | `#FFFFFF` | Controls on a selected row |
| `clip_body` | `#262A31` | `#FFFFFF` | Clip body (the title bar and waveform carry the track colour) |
| `well` | `#141619` | `#D6DAE0` | Inset wells: fader grooves, meter and loudness backgrounds; below every surface in both themes (M4.5) |
| `shadow` | black at 38% | black at 14% | The one shadow, under windows and popovers |

Text on a filled control (a lit button) is chosen by the fill's brightness:
near-black (`#0B0D10`) on light fills, white on dark ones (`theme::text_on`).

**Controls take their colours from the surface under them**
(`theme::controls(selected)`). On most surfaces that is `control`,
`control_hover`, `control_press` and `text_secondary` letters. On a selected
row the plain grey matches the selection tint's brightness and vanishes, so
controls step up to `selected_control` with `text` letters. A test keeps
them at least as distinct from the selection as plain controls are from an
unselected header.

- **Accent contrast:** the light accent is darker, keeping text and icons at
  4.5:1 or better on `app`. Warning is darker on light for the same reason.
- **Colour-vision safety:** blue against amber and red stays distinct for
  protanopia, deuteranopia and tritanopia. State is never shown by colour
  alone: there is also an icon, a label or a position. Examples: the status
  dot with its text, ⚠ on warnings, and the start marker as a shape at the
  ruler's bottom edge against the playhead at the top.
- **Track and clip colours:** a muted palette of 12 hues, of medium
  lightness, so white clip names read on them in both themes. Clips use the
  same palette in both themes (content keeps its identity). A clip can
  have its own colour from the palette, or follow its track's.
- **Muted clips:** `CLIP_MUTED` (`#6C7078`, both themes), a grey of the
  same lightness, so names and waveforms stay readable (the owner's M1).

## 4. Type

Two faces, chosen by the owner from side-by-side specimens (2026-10-07):

- **Inter** for the interface: neutral and very legible at small sizes.
  Regular (400) for text, medium (560) for section headings.
- **Unbounded** for personality: the "Lumen" wordmark, window titles and
  panel titles, at weight 560. It is wide, so it is set a step smaller than a
  narrow face would be, and only on short titles, never on body text.
- **Martian Mono** for numbers that must not jitter (M4.5, the owner's
  choice from specimens over Geist, JetBrains and IBM Plex Mono): square
  and wide like Unbounded, set at width 87.5 (`wdth`) so readouts keep
  their room. egui's Hack stays behind it as a fallback.

egui's bundled fonts stay behind Inter as fallbacks for symbols (⚠, ⟳).
Licences: SIL Open Font License, see `THIRD_PARTY_NOTICES.md`.

| Style | Face | Size (pt) | Use |
|---|---|---|---|
| `scale` | Inter or Martian Mono | 10 | Meter scales, knob values (M4) |
| `caption` | Inter | 11 | Hints, secondary facts, sample counts |
| `body` | Inter | 13 | Labels, values, menu items, buttons |
| section heading | Inter medium | 13 | "Device", "Format" in windows |
| `wordmark` | Unbounded | 14 | "Lumen" in the transport bar |
| `title` | Unbounded | 15 | Window titles |
| `heading` | Unbounded | 16 | Panel titles (Browser) |
| `mono` | Martian Mono | 13 | Position, tempo, CPU: anything that changes while you read it |
| `readout` | Martian Mono | 20 | The master's integrated loudness in the mixer (M4) |

Sentence case for every label ("Audio settings", not
"Audio Settings").

## 5. Spacing and shape

- **4 pt grid:** `XXS 2 · XS 4 · S 8 · M 12` (`theme::space`). Controls
  sit 8 apart. Sections in a window are divided by a separator with 12
  above each heading. Content insets in windows are 12, in strips 8.
- **Control sizes** (`theme::size`, M4.5): `CONTROL` 24 pt for buttons,
  fields and combos (egui's `interact_size`; transport icons 28);
  `CONTROL_SMALL` 20 pt in dense rows (M, S and arm on headers and strips,
  switches, segmented controls, strip rows, the clip card's rows); knobs
  `KNOB` 24 pt and, in lists, `KNOB_SMALL` 18 pt. A module never invents its
  own size for these.
- **Radius:** `SMALL 4` (buttons, inputs, clips), `MEDIUM 6` (popovers),
  `LARGE 8` (windows).
- **Elevation:** only windows and popovers float. They get `overlay` fill,
  a `border_strong` outline and one soft shadow. Nothing else has a shadow.
- **Settings rows:** labels in a right-aligned column (`LABEL_COLUMN` 96 pt
  in windows, `LABEL_COLUMN_NARROW` 72 pt in cards), values to the right of
  it, warnings wrapped within the column's width.

## 6. Motion

All values live in `motion.rs`. With **Reduce motion** (in the app menu) every duration becomes zero: things still change, they just do
not travel.

| Token | Duration | Curve | Use |
|---|---|---|---|
| `FAST` | 100 ms | ease-out | Hover and press feedback |
| `BASE` | 180 ms | ease-out (enter), ease-in (exit) | Popovers, windows, notices |
| `GLIDE` | 220 ms | critically damped spring | Panels opening and closing (both directions decelerate), and things that move to a new place: the playhead and start marker after a locate, clips settling after a drag (slice 6) |

- **Ease-out** (cubic) starts fast and settles gently, so the response
  feels immediate. **Ease-in** is used only for exits.
- The **spring** (critically damped, no overshoot) is the "Apple" feel for
  movement: quick to start, soft to land, no bounce.
- **What animates:**
  - panels opening and closing (slide);
  - popovers and windows (fade, egui's built-in, at `BASE`);
  - menus closing: every menu fades out (`BASE`, ease-in, drifting 4 pt up)
    instead of vanishing; the fading menu is a picture of its last frame,
    which nothing can click, focus or read out (`widgets::Menu`, M4.5);
  - notices and banners appearing (fade and a 6 pt drop);
  - hover and press fills on transport buttons;
  - the playhead and start marker moving after a locate while stopped;
  - clips appearing after an import (fade);
  - the progress fill of a loading clip;
  - (M4.5) **clips moving to a new place**: undo, redo and commands glide
    them (`GLIDE`), a drag's snapped steps over `FAST`; only the drawing
    travels: the audio changes at once and clicks go by where a clip is
    (`arrangement/glide.rs`). Moves to another track jump rows;
  - (M4.5) **what undo or redo changed**: an accent wash on those clips,
    fading over 600 ms;
  - (M4.5) **rows**: a new track grows in from no height; reordered and
    deleted tracks' rows glide to their places; a folding group's tracks
    glide shut, clipped to their shrinking rows (`arrangement/row_glide.rs`);
  - (M4.5) **scroll bars** on the arrangement's tracks and the mixer's
    strips: shown while scrolling or with the pointer near the edge, fading
    in over `FAST` and out over `BASE` 0.8 s later; draggable
    (`widgets::scroll_bar`);
  - (M4.5) the palette's chosen-row fill slides between rows (`FAST`);
    menu highlights follow the pointer directly and do not slide;
  - (M4.5) lit buttons (M, S, cue, loop, metronome, follow) cross-fade on
    and off over `FAST`;
  - (M4.5) windows fade out (`BASE`, ease-in, 4 pt up) when they close, as
    a picture of their last frame (`widgets::paint_gone`).
- **The one deliberate loop:** a record-armed track's ring breathes (2.4 s)
  so an armed track is never missed (owner's choice, M3 slice 4). It
  requests frames only while a track is armed, and Reduce motion holds it
  still.
- **What never animates:**
  - the playhead while playing (it follows the audio exactly);
  - zoom and scroll driven by the wheel or trackpad, which already follow
    the input continuously;
  - text values;
  - anything on the audio thread.
- **Costs:** an animation requests frames only while it runs. When the last
  one settles, the app returns to its idle repaint rate.

## 7. Core widgets

| Widget | Rest | Hover | Pressed | Active / on | Disabled |
|---|---|---|---|---|---|
| Icon button (transport) | icon `text`, no fill | `control_hover` fill (`FAST`) | `control_press` | `accent` icon on `accent_muted` | `text_disabled` icon |
| Button | `control` fill, `body` text | `control_hover` | `control_press` | — | `text_disabled` text, no fill change |
| Toggle (Browser, Detail; `widgets::toggle_button`) | as Button: `control` fill, never bare text (M4.5) | as Button | as Button | `accent_muted` fill, `accent` text | — |
| Tabs (the detail panel; `widgets::tabs`) | words in `text_secondary`, 16 pt apart | `text` | — | `text`, over a 2 pt `accent` underline that slides to it (`GLIDE`) | `text_disabled`, with when it arrives on hover |
| Time signature (`widgets::time_signature`) | one field, "4 / 4", `control` fill | the half under the pointer brightens: the beats left, the unit right | — | drag or wheel changes the half under the pointer; click and type "7/8"; double-click 4/4 | — |
| Empty state (`widgets::empty_state`) | centred: a 16 pt icon in `text_hint`, the title in medium `text_secondary`, a caption hint, the action as a button with its keys in the tooltip | | | | |
| Field (tempo, meter) | `raised` fill | `border_strong` outline | — | `accent` outline while editing | — |
| Combo / menu | as Field, ▼ | | | selected row `accent_muted` | |
| Popover / window | `overlay`, `border_strong`, shadow | | | | |
| Notice | `warning` text, wrapped, with Dismiss | | | | |
| Switch (on/off) | a 30 × 16 pt pill in `control_press`, the knob `text_secondary`; its label at the left of a row as wide as there is room for | the pill lightens (`FAST`) | — | the knob glides right (`GLIDE`), `accent` on `accent_muted` | `text_disabled` |
| Segmented (2–5 choices) | one `control` track, labels in `text_secondary` | label in `text` | — | an `accent_muted` thumb that slides to the choice (`GLIDE`), label in `accent` | label in `text_disabled`, with why on hover |
| Choice row (longer lists, such as a device's channel pairs) | a 24 pt row, `text` | `control` wash | — | `accent_muted` fill, `accent` text | — |

Keyboard focus shows as a 2 pt `accent` ring on every focusable control.

egui's checkboxes and radio buttons are not used (M4.5, the interface
audit's T3): on/off is a switch (`widgets::switch`), two to five choices a
segmented control (`widgets::segmented`), longer lists choice rows
(`widgets::choice_row`) or menu rows.

## 7a. Tracks and clips (M3 slice 4, owner's choices)

Chosen from rendered options on 2026-10-07: option A's headers with option
B's clips. The owner refined the design after MT-APP-9 run 1.

- **Header** (204 pt wide):
  - a 4 pt colour rail;
  - the fold arrow;
  - a **track-type icon** (waveform for audio, keys for MIDI, stacked bars
    for a group, a return arrow for a return; its tooltip names the type);
  - the name, with room after the arrow, shortened with "…" and renamed by
    double-click or F2;
  - **M**, **S** and a round record-arm button, always visible.
  - Lit states: **S** uses the accent and **M** the light neutral. Text on
    a lit button is white or near-black by the fill's brightness.
  - M, S and arm are evenly spaced (8 pt) and snapped to whole pixels
    together, so the round button lines up with the square ones at every
    scale.
  - A selected header is `selected_row`, and its controls step up (§3).
- **Unfolded header:** one row of sliders that show their value inside, with
  no separate labels:
  - **volume** fills from the left, with a handle line and the value in
    dB. The line is solid; within 8 pt of the value's box its middle opens,
    smoothly with the distance, until only a tick above and below the
    letters remains (the owner's C, M4.5), so it never crosses the value;
  - **pan** fills from the centre; its fill moves from neutral grey to the
    accent as the pan nears an edge (strength = |pan|^1.2). The centre is
    marked by short ticks at the edges, clear of the value; the fill's
    moving edge is a handle line that opens around the value as volume's
    does;
  - both: drag (Shift for fine), double-click to reset, click then type a
    value (Enter applies, Esc cancels), arrow keys when focused.
- **Record arm ("glow and breathe")**, the owner's choice:
  - the rail turns red and glows into the header;
  - the button is a red dot with a ring around it. The ring breathes
    slowly (2.4 s) while armed, its glow reaching at most 4 pt, inside the
    gap to S. Every circle shares one centre;
  - at rest (M4.5, the owner's A) the dot is grey (the control's letters
    at 55%) on the control fill, turning red under the pointer (`FAST`):
    record red means recording;
  - the lane gets a red wash, strongest at the left, with thin red edges.
- **Folding and resizing** (as in Ableton):
  - heights come in **22 pt steps**, so anything later drawn by track
    height (automation lanes, meters) lines up;
  - folded is 2 steps (44 pt); unfolded starts at 4 steps (88 pt) and
    goes up to 14;
  - drag the bottom edge to resize. The handle glows with the accent
    (header and lane) while hovered or dragged. A folded track dragged
    taller unfolds; an unfolded one dragged below 4 steps folds;
  - heights glide (`GLIDE`) and are saved with the project.
- **Group tracks** (M4 slice 3, the owner's H1 and L1, ADR-0037):
  - groups go **three deep at most**; a refused Ctrl+G or drop says why
    in a note, and a drop there greys its line with a no-entry cursor;
  - a group's tracks step in **12 pt a level** (three levels at most),
    and every group they are in keeps its **colour rail** at its own
    indent, so the rails run down beside the whole group;
  - a group's name is **medium weight**; folded, it is followed by
    "· N" (the tracks it hides) in the hint colour;
  - the group's arrow hides or shows its tracks; its height is still
    dragged at the bottom edge;
  - its lane shows a **thin bar per track inside**, in that track's
    colour, where its clips are: 4 pt bars on a 7 pt pitch, thinning as
    they crowd and sharing rows below 2 pt;
  - its arm button is lit and breathes when every track inside is armed;
    when only some are, a lit dot with **half a ring**, still;
  - dragging headers: the drop line steps in to the level the tracks
    will land at; over the middle half of a group's header, the header
    lights (accent wash and a 2 pt accent frame) instead.
- **The header column** (MT-MIX-3 run 1): 204 pt wide by default, dragged
  wider at its edge with the lanes up to 480 pt (the edge glows like a
  track's bottom edge), double-click to go back; saved with the project.
  Controls stay at the right; the name and the volume slider take the
  room.
- **Adding tracks:**
  - a **"+ Add track ▾"** button below the last track opens a menu of
    every type with its shortcut (audio Ctrl+T, MIDI Ctrl+Shift+T, group,
    return);
  - a double-click on the empty space below adds an audio track (the info
    line says so on hover);
  - the right-click menu offers the same.
- **Empty lanes** (M4.5): the empty state, "Drop audio files here" with
  the formats as its hint and **Import audio…** as its button.
- **Clip:**
  - `clip_body` (dark in the dark theme), a 1 pt border of the track
    colour at 60%;
  - a 15 pt title bar in the track colour, with the name in `caption`,
    shown on clips at least 24 pt wide (narrower, a name is a letter or
    two of noise);
  - the waveform drawn bright in the track colour (slice 4c,
    `theme::wave`: brightened on the dark body, the colour itself on the
    light one), filling the body below the title bar; stereo is left over
    right when the clip is at least 40 pt tall, otherwise combined.
- **Hints:**
  - a soloed track's lane gets a 6% `accent` wash;
  - clips on tracks that cannot be heard (muted, or not soloed while
    another track is) are drawn at 42%, with their names in
    `text_secondary` so they stay readable in both themes.

### Clip selection (M3 slice 6, owner's choices S3 and H1)

The split-clip model: a clip's top half moves it (Ctrl copies), its bottom
half selects a time range, empty lanes draw a marquee.
- **Hover (H1):** the half under the pointer brightens (`text` at 8%), a
  dashed line (`text` at 35%, 4 on, 3 off) marks the split, and a hint
  chip (`widgets::draw_hint`) says "drag to move · Ctrl copies" (move
  mark) or "drag to select time" (double arrow). Cursors: grab, copy, or
  an I-beam; locked clips show "locked" and a not-allowed cursor. Each
  half is at least 11 pt tall.
- **Selected clips (S3):** an `accent` glow (`widgets::draw_glow_rect`):
  a crisp 1.5 pt line with soft layers at 28% and 12% outside it.
- **Time range:** `accent` at 16% over the lanes it spans, with 1 pt
  edges at 60% and a 2 pt bracket along its top.
- **Marquee:** a rounded (`radius::MEDIUM`) box, `accent` at 12% with a
  1 pt edge at 60%.
- A drag shows the snap guide at the grabbed clip's new start.
- **Handles (owner's choice L3, slice 6c):** shown only near the pointer.
  Within 6 pt of a side (a quarter of a narrow clip) the side glows as a
  rail, `accent` 3 pt at 60% over 6 pt at 15%, with a resize cursor and
  the chip "drag to trim · Shift loops" ("drag to loop" with Shift).
  Ctrl+Alt over the top half: "drag to slip the audio".
- **Fades and gain (owner's choices F3 with the glow, and G1; slice 6d):**
  on audio clips at least 28 pt wide with a wave area of 10 pt or more.
  - At rest a fade is a quiet curve (`text` at 45%, 1 pt) over the
    waveform, which itself follows the fade and the clip's gain on the
    GPU, so the clip looks as it sounds.
  - On hover the curves glow (`accent` 1.5 pt over 3.5 pt at 22% and 6 pt
    at 10%), the part a fade takes away is washed in `accent` at 10%, and
    the handles show: a round `accent` dot (3.5 pt, with a 20% halo) at
    the top of the wave area where each fade reaches full level, an open
    ring in the corner (8 pt in, clear of the trim edge) where there is no
    fade yet, and a bend dot (3 pt, `clip_body` with an `accent` ring) on
    each fade at least 16 pt long. The dot moves freely (the owner's C3):
    the curve passes through it, within the fade's middle three quarters;
    Alt-drag up adds S-shape and down takes it away (40 pt for all of it).
    The cursor is a move cursor; the chip names a preset when the curve
    lands on one, and shows "S 40%" while Alt sets it.
  - The gain line crosses the wave area at the clip's gain (even in dB,
    −∞ at the bottom, 0 dB five sixths up, +12 dB at the top): `accent`
    at 55%, 1 pt, glowing only when the pointer is on it, so it does not
    crowd the fades.
  - Whatever is under the pointer grows by 1 pt and brightens. Hints are
    short ("fade in", "bend · Alt for S", "gain 0.0 dB"); a drag shows its
    value beside the pointer ("125 ms", "Fast", "−4.4 dB").
  - Fade handles win over the sides, the sides over the gain line.
- **Looping clips:** a dashed `text` line at 30% with a 2 pt dot at 60%
  where each repeat starts, and the waveform drawn again in each.

### Clip properties (M3 slice 6e, owner's choices M1, K3 and P1)

- **Muted (M1):** the title bar, border and waveform turn `CLIP_MUTED`
  grey, as in Ableton. Key 0 mutes or unmutes the selected clips.
- **Locked (K3):** a padlock (`icons::Icon::Lock`, 9 pt) ends the title
  bar, and 3 pt clamp bars (`text` at 55%) stand at both ends; a locked
  clip shows no fade or gain handles, and hovering it says "locked".
- **Own colour:** the title bar, border and waveform take the clip's
  colour when it has one.
- **Rename:** Ctrl+R or F2 opens a field over the title bar (at least
  120 pt wide); Enter or clicking away applies, Esc cancels.
- **The properties card (P1):** the hover card, pinned. Alt+Enter, or
  Properties in the right-click menu, opens it beside the clip's part in
  view (8 pt to its right, else its left, kept across the lanes and inside
  the window). 272 pt wide, a 1 pt `accent` border, the popup surface.
  - A title row like a dialog: "Clip" in the display face, ✕ at the right.
  - Rows with a 72 pt label column (`text_secondary`): Name (focused when
    it opens), Colour (two rows of six 14 pt square swatches, 8 pt apart,
    then a "Track colour" pill with a dot in the track's colour, ringed in
    `accent` when chosen; M4.5), Gain (a
    `value_slider`), Fade in and Fade out (a `value_slider` in
    milliseconds and a shape menu), State (Mute clip and Lock clip,
    framed buttons that light when on); then the file's details.
  - Gain and fades are greyed out on a locked clip, with a hint saying
    why.
  - It follows the selection; Esc while typing cancels the typing, the
    next Esc closes the card. While it is open the hover card is not
    shown.
- **The hover card** ends with "Alt+Enter: edit its properties" in
  `accent`.
- **The lanes' right-click menu** (the owner's A, MT-APP-14 run 2) is for
  what was clicked: a clip (Cut, Copy, Duplicate, Delete; Rename, Colour,
  Mute clip, Lock clip, Fades, Properties), a time range (its edits,
  Delete time, Insert time), or empty lanes (Paste when there is
  something to paste, the grid as a submenu, Snap to grid). Commands that
  cannot run are left out, not greyed. Drag hints hide while a menu is
  open.
- **The grid behind clips** (G1): clip bodies are drawn at 55% opacity,
  so the grid shows through at about half its strength; the title bar,
  border and waveform stay solid.
- **A click on the lanes** moves the playhead there (snapped; Alt
  freely), as a ruler click does; on a clip it selects it too.
- **Dropping files** (from the system): over an audio track's lane, the
  lane washes in `accent` at 8% with a 1 pt edge at 50%, and a ghost clip
  (dashed `accent` outline over `accent` at 14%, the file's name, or "3
  files") sits at the snapped drop point. Within 7 pt of the line between
  two tracks, below the last, or over a track of another kind, a 2 pt
  `accent` line (over 6 pt at 15%) marks where new tracks go, with the
  chip "new track here".

### Mute, solo, returns, master and cue (M4 slice 4, owner's choices)

Chosen from interactive specimens (ADR-0038):
- **Why a track is silent or heard.** A track silenced by another's solo,
  or by a muted group it plays through, shows a **hollow M** (a 1.5 pt
  ring in the M's lit colour, the letter in it, on a half-strength fill)
  and its name and type icon dim. A track heard because its group is
  soloed shows a **hollow S** in the accent. Lit buttons mean the user set
  them on that track.
- **The S chip** in the transport, after Follow: quiet grey "S" while
  nothing is soloed, the accent with "S" (or "S 2", "S 3") while anything
  is; a click clears every solo.
- **Returns and the master (A1):** pinned at the bottom under a 7 pt
  divider (`raised`, strong edges, a 28 × 2 pt grip; it glows with the
  accent while hovered or dragged, as edges do), then a 22 pt "Returns"
  caption with a fold arrow and the count, the returns, and the master's
  row with a grey rail. The area takes at most 60% of the lanes' height;
  its lanes have a quieter ground (`canvas` at 70%). A return's name
  follows a 16 pt **letter badge** in its colour (text by `text_on`); no
  arm button on returns or the master.
- **Send knobs (K1 "comet"):** 24 pt knobs, 4 pt apart, under volume and
  pan on unfolded audio, MIDI and group tracks (such tracks unfold five
  steps tall). A 270° ring in `control`; the level a trail in the
  return's colour, from 18% to full strength and thickening towards a
  glowing head (two soft circles, a white core, a colour centre); the
  return's letter in the middle (`text` when sending, `text_hint` when
  silent); a 1.6 pt dot at the top for pre-fader; the level beside it in
  caption size while it moves; at 40% strength while the send is not
  heard. Hovering a knob lights its return's header (10% wash, 1 pt frame
  in its colour) and hovering a return rings its knobs.
  - **On a selected row (C4, MT-MIX-4 run 1):** the ring is a quarter of
    the way from the control colour to the text colour, and a silent
    knob's letter is `text_secondary`, so the knobs stay clear of the
    row in both themes.
  - **"+N ▾" (O1):** past the header's width, a 20 pt button drawn as
    "+ Add track ▾" is (control fill, 6 pt corners, the filled arrow, an
    accent edge on hover and while open), with a dot in a return's colour
    before the number while a hidden send is up. It opens "Sends from …":
    a 20 pt knob, the letter badge, the name and the level per return.
  - **A return's senders (R2, L4, S1):** an unfolded return lists, from
    its third step, the tracks sending to it: a 4 × 14 pt bar in the
    track's colour, its name, an 18 pt knob (no hover ring there) and the
    level, the four loudest first, then "+N more ▾" opening "Sends into …"
    with them all. A send at −∞ leaves the list when let go. It unfolds
    tall enough (a step a row). "No tracks send here yet" when none do.
- **Cue (C1):** the master's **Solo/Cue switch** (46 × 18 pt at the
  header's right): "Solo" in control colours, the accent with headphones
  and "Cue" when on, greyed (`text_disabled`) without a cue pair. On Cue,
  the S buttons draw **headphones** (`icons::draw_headphones`) and light
  with the accent. The master's unfolded row adds a "Cue" level slider
  while a cue pair is open.

### Meters (M4 slice 5, owner's choices, ADR-0040)

Chosen from interactive specimens:
- **Colours (M1, M2 a setting):** a level is `accent` up to −6 dB
  (`theme::METER_WARN_DB`), `warning` up to full scale, `record` above it
  (`theme::meter`). "Meters in track colours" (app menu, palette) swaps the
  accent for the track's colour (`theme::wave`). The peak is drawn at 40%
  (`METER_PEAK_ALPHA`) behind the solid RMS; the hold is a 1.5 pt tick in
  `text` (`record` above 0 dB).
- **Ballistics (B2):** a peak jumps up at once and falls 20 dB a second;
  the hold stays 1.5 s on the highest peak, then falls 30 dB a second; the
  RMS is a 300 ms average; below −60 dB is silence. A clip stays lit until
  clicked (Alt+click clears every one; "Clear clip lights" in the
  palette). Levels are read after fader, pan and the solo gate (B1).
- **On track headers (H2):** an unfolded track's level runs along the
  bottom of its volume slider on the slider's own scale (−60 to +6 dB):
  two 2 pt bars (left over right), 2 pt above the slider's bottom, with the
  hold tick across them. A folded track has no slider: a hairline (two
  1.5 pt bars) along the header, 6 pt above its bottom edge (clear of the
  resize edge), from the name's left to 8 pt from the right. A clip lights
  a red mark at the right end (3 × 14 pt in the slider, 4 pt on the
  hairline), and hovering it sets the info line.
- Meters draw frames only while a level moves and come to rest when it
  stops, so a silent project idles.

### The Mixer tab (M4 slice 5, owner's choices, ADR-0040)

- **Layout:** the detail panel's Mixer tab (Devices and Clip arrive with
  their milestones). Strips follow the arrangement's order, skipping
  tracks hidden in folded groups (one fold for both views, B4); the
  returns after a 10 pt gap; the master pinned at the right after a
  `border_strong` line, and the loudness column (L2, 108 pt) beside it.
  The strips scroll sideways; only those in view are drawn. With no
  tracks: a **ghost strip** where the first will go (96 pt, a dashed
  `border_strong` edge, a plus and "Add track"; a click adds an audio
  track) and "Strips appear here as you add tracks" beside it (M4.5).
- **Strip (S1 console, 96 pt):** `header` fill (`raised` for returns and
  the master, `selected_row` when selected, controls stepping up as on
  headers). From the top: a 3 pt rail per group the track is in (and its
  own when it is a group), outermost first; a 3 × 14 pt colour chip, a
  group's fold arrow, a return's letter badge and the name (double-click
  renames it on the strip); the output (a menu: "Inside Lumen", the master
  and the groups that would not feed back, then "Audio outputs", every
  pair of the device as "1/2", "3/4"…, the main pair marked "Main", the cue
  pair greyed "Cue" with why, each pair's channel names and "Skips the
  master" on hover; "Looking up the outputs…" until known); Ø and input trim (±24 dB, filled from the centre;
  the master shows its Solo/Cue switch there); the sends (O2: the first
  two returns' knobs, 24 pt, then "+N ▾" listing them all); pan; the fader
  (`widgets::fader`: a 4 pt groove and a 22 × 12 pt cap with a `text` line,
  on the send knobs' taper, 0 dB at three quarters of the travel, stopping
  at −60 dB) beside the scale (monospace marks, 0 in `text_secondary`; the
  marks are placed most important first, 0, −48, +6, −12, −24, −6, and one
  within 11 pt of a numbered mark is a 3 pt tick instead) and
  the stereo meter (two 5 pt bars, 2 pt apart, with the clip light, 5 pt,
  above); the volume typed in (`widgets::value_field`) and the peak held
  (`record` when clipped; a click clears it); M, S and arm, 20 pt, centred.
- **A short panel (B7)** folds the sends away first, then trim and
  polarity, then the output; the rest always shows, with a travel of at
  least 64 pt. The panel is never shorter than that strip, and never so
  tall that the arrangement keeps less than 180 pt.
- **Loudness column (L2):** "Loudness", the integrated value in 20 pt
  monospace with "LUFS integrated" under it; a 12 pt short-term bar (the
  accent, the warning colour above −9 LUFS) with the momentary as a 2 pt
  `text` line and the −14 LUFS ± 1 band in the accent at 14%; the last
  minute's short-term loudness as a line beside it; M, S and TP (TP in the
  warning colour above −1 dBTP); "↺ Reset". Silence reads "—".
- **The wheel (the owner's B3, every value control):** a wheel gesture
  that starts on a slider, knob, fader or value field sets it, a notch a
  step (1 dB, 5% of pan, a tenth with Shift; a send turned up from
  silence starts at −60 dB), and the whole gesture is one undo step. A
  gesture that started scrolling keeps scrolling over controls until the
  wheel rests (0.3 s). Ctrl and Alt keep the wheel for zoom and track
  heights (`widgets::wheel`).
- **Names:** every strip control ends "in the mixer" ("Mute Kick in the
  mixer", "Kick fader in the mixer: −3.0 dB"), so it never shares a name
  with its header's twin; a strip itself is "Mixer strip: Kick, Audio
  track".

## 7b. Navigation (M3 slice 4d, ADR-0028)

- **Zoom** glides (`GLIDE`) for keys and fits; the wheel and pinch follow
  the hand directly. Down to 8 points per sample, where the waveform is a
  line through the samples; out to the whole song with 10% of room (at
  least 32 bars), as in Ableton.
- **Grid** lines never sit closer than 12 points: beat lines drop out
  first, then bar lines thin to every 2nd, 4th, 8th... bar.
- **Follow** is a transport-bar toggle (a playhead line with an arrow).
  Lit when following; unlit while paused by looking around, with a
  tooltip saying Play or a click resumes it.
- **Dragging tracks:** the dragged headers dim to 60%; a 2 pt accent line
  across the headers and lanes marks where they land; the cursor is a
  closed hand.
- **Ruler:** bar numbers, thinned to 40 points apart; once a bar is wider
  than 240 points, beats and sixteenths too (bar.beat.sixteenth), at least
  64 points apart.

## 7c. Grid and snapping (M3 slice 5a)

- **The grid button** sits in the ruler's corner, above the track headers:
  "Grid" in hint text at the left, the step at the right in the monospace
  caption ("1/16", "1/8T", "4 bars"), always shown. Before it, a magnet
  (`Icon::Magnet`, 14 pt): `accent` while snapping, struck through in
  `text_hint` when not, with the size in hint colour too (owner's choice
  C1, MT-APP-13 run 1). The magnet is its own switch: a click turns
  snapping on or off (accessible as the "Snap to grid" checkbox, with a
  `control_press` wash on hover). A click elsewhere on the button opens
  the grid menu, which keeps "Snap to grid"; right-clicking the lanes
  opens it too.
- **The grid menu** lists the adaptive widths, the fixed sizes, Triplet,
  Dotted and Snap to grid as selectable items with their shortcuts.
- **Lines:** bar lines `grid_bar`, beat lines `grid_beat`, lines between
  beats `grid_sub`. Never closer than 6 points; a grid finer than that on
  screen shows calmer lines 12 points apart, and still snaps to its own
  steps.
- **Snap guide:** a 1 pt accent line at half strength, from the ruler to
  the bottom, where a click on the ruler or a dropped file will land.

## 7d. Rulers (M3 slice 5b, layout A)

Chosen by the owner from rendered specimens (Ableton's split):
- **Locator strip** (24 pt, `app`), three layers:
  - the **loop brace**, a 7 pt band at the strip's bottom edge
    (`accent_muted` with `accent` edges when the loop is on, `control`
    with `text_hint` edges when off), edited in the bottom 10 pt;
  - **locator flags** (slice 5d) in the top 14 pt: a 1 pt post down the
    strip, a 6 pt right-pointing triangle and the name in a caption,
    `start_marker` (`accent` when selected, its name in `text`); the name
    gets a `control_hover` wash on hover;
  - the **scrub strip** (slice 5e): the rest of the top 14 pt. On hover, a
    pointing hand, a snap guide, the step a drag would play washed in
    `accent_muted` at 60%, and a caption in `text_hint` beside the pointer
    ("drag to listen · Alt tape"; "drag to scrub like tape" with Alt;
    "click or drag to jump" while playing). While dragging, the step that
    plays is lit in `accent_muted`; a tape scrub shows "tape 1.4×" in
    `accent` (owner's choice B2, on trial).
  - **Drag hints** on the brace (owner's choice A1): near an edge (8 pt) a
    2 pt `accent` bracket rises at that edge and a chip says "↔ Loop
    start" or "Loop end"; inside, the brace lights (`accent` at 35%) and the
    chip says "Move loop · click: on or off"; elsewhere, "Drag to draw a
    loop". Chips (`widgets::draw_hint`): `overlay` with a `border_strong`
    hairline, caption text, an accent mark drawn as arrows.
- **Strip corner** (over the headers): "Locators" in `text_hint`, then
  ◂ Set ▸ at the right edge. Set is `accent` when a locator sits at the
  playhead; the arrows dim when there is nothing that way.
- **Bar ruler** (22 pt, `raised`): bar numbers thinned to 40 points apart;
  past 240 points a bar, bar.beat.sixteenth at least 64 apart. When no
  label falls in view, the left edge's position is pinned at the left in
  `text_hint` with "…". Click locates (snapped); drag down or up zooms
  (100 points ≈ 2.7 times), sideways scrolls, with a zoom cursor.
- **Time ruler** (20 pt, `raised`, along the bottom): monospace captions in
  `text_hint` at least 90 points apart, on round values, with 5 pt ticks.
  Dragging it selects a time range on every track (owner, slice 6b), shown
  on the ruler in `accent` at 22%; a click moves the playhead. An I-beam
  and a hint chip above it say so.
  Its corner names the format ("Time", "Samples", "TC 25") with an arrow
  and opens the format menu.
- The playhead runs from the bar ruler through the time ruler.

## 7e. Commands, keycaps and the palette (M3 slice 7)

- **Keycaps (the owner's K5, `widgets::keycaps`).** Wherever a shortcut
  shows: modifiers as plain small caps on the control fill, the main key
  outlined in `border_strong`, with a short light under it in the colour
  of the command's area (`Area::light`, one of the track colours: Edit
  blue, Track violet, View green, Clip amber, Transport rose, Grid teal,
  Locators terracotta, File brown). The light groups commands the way
  track colours group tracks; it never carries state, so it does not
  break the one-accent rule, and the area is also named in text beside
  it.
- **The palette (the owner's P2 with P3's values, `app/palette.rs`).**
  Ctrl+K, or the magnifier at the right of the transport bar. It opens
  top-centre, 520 points wide, on the overlay surface with a strong
  border:
  - a frameless search field (title size) with a magnifier;
  - rows of 30 points: the label (what the command acts on: "Duplicate 2
    clips", "Undo: Move clip"), matched letters in the accent, the area
    in hint text and the keycaps at the right. The chosen row has the
    `accent_muted` fill. Rows that cannot run now use `text_disabled`;
  - with nothing typed, "Recent" (the last five run from it) heads the
    list, then "All commands";
  - a strip under the list explains the chosen row in secondary text,
    or why it cannot run in the warning colour, with every chord that
    runs it;
  - a hint line: ↑↓ move · Enter run · Esc close.
  The list shows up to ten rows, fewer when the window is short, so the
  palette never covers the transport bar. Esc, Ctrl+K or a click
  elsewhere closes it; hovering picks a row only when the pointer moves.
- **Typed values become actions:** a number offers "Set tempo to N BPM"
  (20–999) and "Go to bar N"; "bar N", "tempo N" and "N bpm" ask for one;
  a track's or locator's name offers going to it.
- **The Keyboard shortcuts window (the owner's E2, `app/shortcuts.rs`).**
  A `Dialog` 820 points wide, from the app menu or the palette:
  - a "Layer" row of Ctrl, Shift and Alt chips (holding a modifier shows
    its layer too) and, at the right, each area's light and name;
  - the keyboard: keys bound on the layer are on the overlay fill,
    outlined, with the key's label, the command's name (ending in "…" when
    it does not fit) and the area's light along the bottom; free keys are
    plain on `raised` with hint text; modifier keys light in
    `accent_muted` while their layer shows; hovering outlines a key in the
    accent, and a click searches the list for its chord;
  - a search field (names and shortcuts), "Keymap: Ableton" and Reset
    all;
  - the list, by area under a light and name: the command, "changed" in
    hint text when it differs from the default, its K5 keycaps (click to
    record over one), "+" to add, ↺ to reset. Recording shows "Press
    keys…" in the accent with Cancel (and Remove when replacing); a clash
    shows a line in the warning colour, "Ctrl+D is used by Duplicate.",
    with Reassign and Cancel.
- **Recording in the palette (E3):** Ctrl+Enter on a row, or a click on
  its keycaps; the strip says "Press the new keys for …" in the accent,
  or the clash in the warning colour with "Enter takes it".
- **Menus and tooltips:** every shortcut is drawn as K5 keycaps from the
  keymap, at the right of a menu row (`widgets::menu_item`; toggles use
  `menu_toggle`, with the selected fill when on) and after a tooltip's
  name (`widgets::tip`), with any hint on a line of its own in secondary
  text. Commands without a shortcut show none; a row whose command cannot
  run now is greyed.
- **Panes:** F6 and Shift+F6 move the keyboard between the browser, the
  arrangement and the detail panel; the pane with the keyboard has the
  2 pt accent ring round it.
- **Shift+Tab** moves focus back in a text field or an open window, and
  shows or hides the detail panel everywhere else (the owner's choice,
  ADR-0015).

**Keycap alignment (owner, 2026-10-09).** Keycaps keep the light's room
above them as well as below, so any layout that centres them centres the
caps and the light hangs below. In a menu row the highlight reaches a
little past the row, so the label and the keycaps sit equally far in from
its edges; on a highlighted row the key's outline and the modifier caps
step up (`theme::controls(true)`), or they vanish into the highlight.

## 8. Key flows

Each flow lists its steps, its keyboard path and its feedback. Flows marked
(M3) are designed here and built in later slices.

1. **Play and stop.** Space plays from the start marker and stops in place.
   Shift+Space pauses and continues. Clicking the ruler moves the playhead
   (it glides there while stopped), with a ghost line showing where it will
   land.
2. **Choose an audio device.** The status dot (or a banner when there is a
   problem) leads to the audio panel, then Audio settings. Changes apply at
   once. A failure returns to the previous device and says so, with
   Dismiss.
3. **Import audio** (M3). Drag files from Explorer. While dragging, the
   canvas says what will be added. Dropping on a track puts the clip there;
   dropping below the last track adds a new track. Each clip fades in and
   shows its loading progress.
4. **Arrange** (M3). Drag clips to move them (snapped, with a visible snap
   guide); Ctrl-drag copies; drag the edges to trim. Clips glide into place
   when released. Every edit is one undo step (Ctrl+Z).
5. **Save and reopen** (M3). Ctrl+S saves; the title shows unsaved changes.
   Reopening restores the same project, zoom and panel layout.
6. **Find any command** (M3). Ctrl+K opens the palette; type and press
   Enter. Every entry shows its shortcut.

## 9. Checklist for new UI

1. Colours, sizes and durations come from `theme` and `motion`.
2. It works in both themes and at 100%, 150% and 200% scale.
3. Hover, pressed, focused and disabled states are all designed.
4. Empty and error states are designed: what happened, and what to do.
5. Long text wraps or truncates with "…". Nothing grows a window or
   overlaps a neighbour.
5a. Windows are `widgets::Dialog`; controls on a selected surface use
   `theme::controls(true)`. Circles are snapped to pixels along with the
   rectangles beside them.
5b. Typed values apply on Enter or leaving the field, never per keystroke.
6. Motion follows §6. With Reduce motion on, it still works and simply does
   not travel.
7. Every control has an accessible name.
7a. Hovering anything that can be dragged or clicked sets the info line
   (§2); nothing floats over content to explain it.
