# Lumen design system and key flows

**Status:** v1, M3 slice 2 (2026-10-07), extended through M5; **v2
(2026-10-11, ADR-0052)** follows the owner's **Lumen design system** (a
Design System artifact: tokens, a Material card and a card per component).
Where this file and the design system differ, the design system wins.
**Design system v3 (2026-10-16)** is saved as
[`docs/design-system/lumen-tokens.json`](design-system/lumen-tokens.json)
(colour for both themes, type, spacing, radius, size, duration and stroke
tokens, each with its usage). It was checked against `theme.rs`: every colour
value, the 12 clip colours and the size, radius, stroke and duration values
agree; v3 adds the spacing steps `space_xl` (24) and `space_2xl` (32), which
the code does not have yet (add them to `theme::space` when a screen needs
them, not before).
Code: `crates/lumen-app/src/theme.rs` (colour, type, spacing, shape,
strokes, the on states and the egui style built from them), `motion.rs`
(motion), `light.rs` (light) and `widgets/raised.rs` (material, §5a).
Every screen takes its values from there. A colour or duration written
inline in UI code is a bug.

The direction comes from `CLAUDE.md` §5: an arrangement-first
workflow, with clear content and restrained visual density. Minimal and quiet
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
│ Lumen   │ transport │ tempo · meter │ position │ CPU · status │ panels │  transport bar, 44 pt
├──────────────────────────────────────────────────────────────────────┤
│ (device banner, only when the audio device needs attention)          │
├────────────┬─────────────────────────────────────────────────────────┤
│ Browser    │ ruler (bars · loop · markers)                            │
│ (left,     ├─────────────────────────────────────────────────────────┤
│ collapsible│ arrangement: track headers │ lanes, clips, playhead      │
│ 220 pt)    │                                                         │
├────────────┴─────────────────────────────────────────────────────────┤
│ Detail panel (bottom): subject │ Devices · Clip │ Mixer (at right)  │
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
  shortcut as K5 keycaps from the keymap. The interface audit's I1: the
  subject's name is in `text` and the rest of it ("Studio A · Effect") in
  `text_secondary`, then a 14 pt `border_strong` hairline; the actions
  follow 14 pt apart with no dots, the gesture's word capitalised ("Drag
  copy"). Nothing is lit while no modifier is held; holding modifiers
  lights (`accent_muted`) every gesture and shortcut that takes exactly
  those keys and dims the rest to `DIMMED`. A `border` hairline runs along
  the strip's top in both themes. Verbs say what happens ("add to Bass",
  "add to favorites, rescan or show in folder"), never "choose", "more" or
  "menu". New interactive things set an `Info` when hovered; nothing else
  describes a drag.
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
  answered by its buttons. Its main answer is the filled primary (Save), and
  Enter gives it when nothing has the keyboard (the interface audit's W1).
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
- **Everything in a menu lines up** (M4.5 slice 6): switches, segmented
  controls and captions (`widgets::menu_caption`) come in from the menu's
  edges as far as its rows' labels and keycaps do (`button_padding`); the
  widgets do it themselves when they are in a menu.

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
| `control_hover` | `#3A3F4A` | `#D7DBE0` | Flat controls under the pointer; raised controls, rows and segmented pieces take `ember` instead |
| `control_hi` | `#383C46` | `#EEF0F3` | The top of a raised control's gradient (`control_hi` to `control`, a 3% step); never flat |
| `control_press` | `#444A56` | `#CBD0D7` | A control held down; the filled part of a slider; a knob's ring |
| `thumb_hi`, `thumb` | `#4A5160`, `#3E4551` | `#FFFFFF`, `#F3F4F6` | The chosen piece of a segmented control: a raised thumb in the well, `text` letters |
| `border` | `#2E323A` | `#DADDE2` | Separators, panel edges |
| `border_strong` | `#3C414B` | `#C3C8CF` | Window and popover outlines, focus-free inputs |
| `text` | `#E6E8EC` | `#1C1F24` | Primary text, icons |
| `text_secondary` | `#9AA0AC` | `#5A6170` | Labels, ruler numbers |
| `text_hint` | `#6E7480` | `#878D98` | Hints, empty states |
| `text_disabled` | `#555A65` | `#B3B8C0` | Disabled controls |
| `accent` | `#4FA3FF` | `#1F6FD1` | Active, selected, playhead, focus ring |
| `accent_muted` | `#243A55` | `#D5E5FA` | Something that is on (a toggle, pill, switch, chosen row): with `accent` letters and a 38% accent hairline |
| `accent_hi`, `accent_lo` | `#5DAEFF`, `#3F8FEA` | `#2A74D4`, `#1A62BD` | Filled accent (the one primary of a view, a lit S, the chosen tool): a gradient, no halo |
| `on_accent` | `#06182E` | `#FFFFFF` | Letters and icons on filled accent |
| `mute`, `on_mute` | `#C8CCD4`, `#0B0D10` | `#5A6170`, `#FFFFFF` | A lit M, and its letter |
| `budget` | `#2B6CB8` `#4FA3FF` `#7DBAFF` `#A9D1FF` `#D3E8FF` | `#0F3F7A` `#1F6FD1` `#4F93E3` `#86B6EE` `#B4D2F5` | The block budget bar's five costliest plugins, darkest first: the accent stepped in lightness so neighbours stay apart (§7g) |
| `record` | `#E5484D` | `#D33A40` | Record only |
| `record_note` | `#F27B7F` | `#E06066` | The notes of a clip being recorded: record red, lighter (M5 slice 6d) |
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
| `lane_black` | `#16181C` | `#ECEEF1` | Piano-roll lanes of black-key pitches (M5 slice 3) |
| `lane_split` | `#2E323A` | `#D2D6DC` | The faint lines between B and C, and E and F, in the piano roll |
| `key_white` | `#C9CDD4` | `#FFFFFF` | Piano-roll white keys |
| `key_black` | `#0E1013` | `#3A3F4A` | Piano-roll black keys |
| `key_text` | `#3A3F4A` | `#5A6170` | The C labels on the keys |
| `well` | `#141619` | `#D6DAE0` | Inset wells: fader grooves, meter and loudness backgrounds; below every surface in both themes (M4.5) |
| `ground` | `#111316` | `#E7E9ED` | The deepest layer, under `well`: behind the window, scrims, the app icon's base |
| `edge_light` | white at 7% | white at 80% | The 1 pt line along the top inside edge of every raised surface (§5a) |
| `shade` | black at 50% | `text` at 14% | The 1 pt lip along the top inside edge of a well or a pressed control (§5a) |
| `ember` | `text` at 8% | `text` at 8% | Light level 1, washed over what the pointer is on: the hover of raised controls, rows and segmented pieces (§6a) |
| `shadow` | black at 38% | black at 14% | The one shadow, under windows and popovers |
| `core` | `#FFFFFF` | `#1F6FD1` | The centre of a light (§6a); on light the accent: light on paper is ink |
| `sun` | `#FFC46B` | `#CC9D56` | The mark's sun, at its rim (§6b); never in the interface |
| `sun_core` | `#FFFFFF` | `#806236` | The mark's sun, at its centre |

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
- **Dimmed:** `DIMMED` (45% opacity) is what steps back beside something
  lit, such as the info line's other actions while a modifier is held.
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
| `mono_small` | Martian Mono | 10 | Units after a readout, ruler numbers, clip meta |
| `keycap` | Inter | 11 | The letters on K5 keycaps (the caption size) |
| `display` | Unbounded | 24 | Welcome and first-run titles only (added to `theme::size` with those screens, M9) |

Sentence case for every label ("Audio settings", not
"Audio Settings").

## 5. Spacing and shape

- **4 pt grid:** `XXS 2 · XS 4 · S 8 · M 12 · L 16 · XL 24 · XXL 32`
  (`theme::space`; L for panel toolbars and empty states; XL and XXL, for
  first-run layouts, join `theme::space` with those screens in M9). Controls
  sit 8 apart. Sections in a window are divided by a separator with 12
  above each heading. Content insets in windows are 12, in strips 8.
- **Control sizes** (`theme::size`, M4.5): `CONTROL` 24 pt for buttons,
  fields and combos (egui's `interact_size`); `CONTROL_LARGE` 30 pt for the
  transport's readouts and detail-panel toolbars; `TRANSPORT_ICON` 28 pt
  wide icon buttons in a `TRANSPORT_BAR` 44 pt bar (the toggles square,
  the wells' pieces `WELL_CELL` tall);
  `CONTROL_SMALL` 20 pt in dense rows (M, S and arm on headers and strips,
  switches, segmented controls, strip rows, the clip card's rows); knobs
  `KNOB` 24 pt and, in lists, `KNOB_SMALL` 18 pt. The transport bar's
  wells are `CONTROL_LARGE` tall, so their pieces are `WELL_CELL` 26 pt
  (§7, "The transport bar"). A tooltip from `widgets::tip` grows to
  `TIP_WIDTH` 280 pt, then its hint wraps. Track headers (M9 R1b,
  ADR-0101): `HEADER_SIDE` 60 pt for the right column (pan, the In chip),
  `WELL_PIECE_W` 18 pt for the pieces of the M S ● well, the magnet and
  the locator arrows (as tall as the well less `space::XXS` each side),
  `STRIP_PIECE_W` 22 pt for the same well's pieces on a mixer strip,
  `CORNER_ICON` for an icon in a piece of the ruler corner's wells,
  `LEVEL_STRIP` 5 pt for the level beside the rail (two 2 pt sides), and `SEND_SLOT` 18 pt
  for the widest slot of the send well, which shows its letter from
  `SEND_SLOT_LETTER` 12 pt. A module never invents its own size for these.
  The Monitor badge on a MIDI header (`BADGE_W` 16 ×
  `BADGE_H` 14 pt) is a speaker before M, shown only when Monitor is In
  (accent on `accent_muted`, a hairline when the row is selected, no glow)
  or Off (struck-through speaker, hint grey); Auto shows nothing. A click
  cycles In, Off, Auto.
- **Radius follows the height** (`theme::radius::for_height`): `SMALL 4`
  for 20 pt controls and troughs, clips, notes, keycaps and chips in 20 pt
  rows; `MEDIUM 6` for 24 pt controls, inputs and menu rows; `LARGE 8` for
  30 pt controls, popovers and menus; `XL 12` for windows, cards and the
  palette; `FULL` for pills (switches, lane tabs, chips). A piece inside a
  trough takes the trough's radius less its 2 pt inset.
- **Strokes** (`theme::stroke`): `HAIRLINE 1` for separators, outlines,
  edge lights and lips; `LIT 1.5` for a Lit or Flare line and icons;
  `FOCUS 2` for the focus ring and a Beam; `UNDERLINE 2` for the accent
  line under a toggle that is on.
- **Elevation:** only windows and popovers float. They get `overlay` fill,
  a `border_strong` outline, the edge light and one soft shadow (offset
  0, 6, blur 18). The one other shadow is the mixer's pinned master,
  which casts `shadow` over the strips passing under it (`space::M` wide,
  the interface audit's X6).
- **Settings rows:** labels in a right-aligned column (`LABEL_COLUMN` 96 pt
  in windows, `LABEL_COLUMN_NARROW` 72 pt in cards), values to the right of
  it, warnings wrapped within the column's width.

## 5a. Material (the design system's Material card, ADR-0052)

Depth without clutter: surfaces step lighter as they rise (`ground` <
`well` < `app` < `canvas` and `header` < `raised` < `overlay`), and three
painted details do the rest (`widgets/raised.rs`):
1. **Raised** (buttons, M and S at rest, the fader cap, dropdowns, toggles
   when off): a vertical gradient `control_hi` to `control` (flat
   `selected_control` on a selected row), a 1 pt `border` (`border_strong`
   under the pointer) and a 1 pt `edge_light` along the top inside edge
   (`raised::paint`).
2. **Sunk** (fields, sliders, fader grooves, meters, the segmented and
   switch troughs, the transport's readouts): `well` with a 1 pt `shade` lip
   along the top inside edge (`raised::paint_sunken`); fields add a 1 pt
   `border`, `border_strong` on hover (`raised::paint_field`). A pressed
   control sinks too: flat `control_press`, the lip, its content 1 pt down.
   The edge light and the lip are drawn as an inset shadow 1 pt down is:
   they follow the rounded top corners, fading as each corner turns to its
   side, on the top row of a surface with no border (`raised::bare_lip`)
   and under the border of one with a border (`raised::lip`).
3. **Filled** (`theme::Tone::Accent`: the one primary of a view, a lit S,
   the chosen tool; `Tone::Mute`: a lit M): its colour (a gradient
   `accent_hi` to `accent_lo`, or `mute`), its letters (`on_accent`,
   `on_mute`) and a stronger edge light (white at 35%). No halo: a filled
   control's fill is its light.

**States** of every control: rest; hover is Ember (`ember` over the fill,
never another fill); pressed is sunk; **on** is `accent_muted` with
`accent` letters and one accent hairline at 38% (`Tone::On`); recording is
record red over the control with a 45% hairline and the Flare
(`Tone::Record`, the one lit state that glows); keyboard focus is a 2 pt
`accent` ring 2 pt outside (`raised::focus_ring`); disabled is a flat
`control` with `text_disabled` letters and the reason on hover. Buttons are
`raised::button` or, at most one per view, `raised::primary_button`.

## 6. Motion

All values live in `motion.rs`. With **Reduce motion** (in the app menu) every duration becomes zero: things still change, they just do
not travel.

| Token | Duration | Curve | Use |
|---|---|---|---|
| `FAST` | 100 ms | ease-out | Hover and press feedback |
| `BASE` | 180 ms | ease-out (enter), ease-in (exit) | Popovers, windows, notices |
| `WASH` | 600 ms | ease-out | What undo or redo changed (the accent wash) |
| `BREATH` | 2.4 s | cosine | The armed ring: the one deliberate loop |
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
    (`widgets::scroll_bar`). A strip cut by the mixer's left edge fades
    into `header` over `space::L` there, so a part-scrolled strip never
    ends as a stray rail; at the right it passes under the master's shadow;
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

## 6a. Light (ADR-0043, approved from the identity canvas)

The theme of the product is light. It is one layer over the tokens, in
`light.rs`, and **every glow is one of its levels, never a one-off.**

**Rules**
1. **Light means alive.** Only what is on, playing, chosen or under the
   hand gives light. Chrome never glows.
2. **Light has a source.** A core (white in the dark), the colour in the
   halo, falling off outward, as the comet knob's head does.
3. **One colour of light:** the accent. Record red is the only other, and
   means recording or a clip. Amber warns and never glows.
4. **Light follows sound.** Meters, the mark and the lanes under the
   playhead brighten with the level, and nothing else does.
5. **Dark is calm.** Nothing glows while nothing changes; the armed ring
   is the one light that breathes (§6).

On the light theme halos are halved (`light::strength`) and the core is
the accent.

| Level | Token | Recipe | Used for |
|---|---|---|---|
| 0 · Unlit | — | no light | chrome, labels, controls at rest, a stopped playhead |
| 1 · Ember | `EMBER_WASH` | `text` at 8% over what is hovered | under the hand: the half of a clip you would grab |
| 2 · Lit | `LIT` | 1.5 pt line; halo 1.5 pt at 28%, 3 pt at 12% past it | chosen: selected clips (S3), edges and fade curves being handled |
| 3 · Beam | `BEAM` | 2 pt accent line, 1 pt `core` inside, halo 3 pt at 22%, casting 56 pt at 9% at its centre | sounding now: the playhead while playing, Play while playing, meter heads |
| 4 · Flare | `FLARE` | record red; halo 2.5 pt at 45%, 6 pt at 18% | needs you: clip lights, the record arm |

Helpers: `light::hline`, `vline`, `rect`, `halo_around`, `polyline`,
`cast` and `cap` (the playhead's cap). `widgets::draw_glow_rect` and
`draw_edge_glow` are `LIT`.

**Where it is used**
- **The playhead** is unlit while stopped (a 1 pt accent line). Playing,
  it is a beam: its cap turns `core` with a halo, and it casts light on
  the lanes it passes.
- **Play** while playing: the button's icon in `core`, with the beam's
  halo around the button.
- **Meter heads:** the top of every RMS bar (strips, header level
  strips) has a `BEAM` head in its zone's colour, so a level reads as
  light rising.
- **Clip lights** flare.
- **The mark** (§6b) in the transport bar is a meter of the master, and
  above an empty arrangement it rises when files are dragged over.

## 6b. The mark (the owner's Dawn in Sunrise, ADR-0043)

A sun rising over a horizon, its reflection breaking into a waveform: an
amber sun (`sun`, `sun_core`) over water in the accent. Drawn on a 64 grid
(`widgets/mark.rs`): sun radius 16 on the horizon at y 32, white at its
centre and amber at its rim, cut by the horizon; a horizon from 5 to 59,
2.6 wide, white in the middle and water-coloured at the ends; three
reflections, 22, 14 and 7 long, at 85, 60 and 30%. At 40 pt and under the
strokes are 4 wide and the third reflection goes.

- **A meter where it can be.** In the transport bar (22 pt, before the
  "Lumen" wordmark) its sun rises with the master's RMS (silence below
  −48 dB, fully risen at 0 dB) and its light grows with it; at rest it
  peeks over the horizon. Recording turns the sun red (M9).
- **Empty arrangement:** the mark, 48 pt, half risen above "Drop audio
  files here"; fully risen while files that can be added are dragged over.
- **Window icon:** the mark at 72% of a dark rounded square (corner 22.5%,
  `#1F2228` to `#0E0F12`), drawn as pixels by `brand.rs`. The installer's
  app and file icons (M9) come from the same geometry.
- The wordmark stays Unbounded at 560, never redrawn.

## 7. Core widgets

| Widget | Rest | Hover | Pressed | Active / on | Disabled |
|---|---|---|---|---|---|
| Icon button (transport, 28 pt square) | quiet: no fill, icon `text_secondary` | Ember wash (`FAST`) | sunk | on: `accent` icon on `accent_muted` with the 38% hairline; Play while playing: the icon in `core` with the Beam | `text_disabled` icon |
| Button (`raised::button`; 24 pt, radius 6) | raised (§5a), `body` letters in `text_secondary` | Ember, `border_strong` edge, letters `text` | sunk, letters 1 pt down | — | flat `control`, `text_disabled` letters |
| Primary (`raised::primary_button`, one per view) | filled accent, `on_accent` letters | Ember | sunk | — | as Button |
| Toggle (Browser, Detail; `widgets::toggle_button`) | as Button, never bare text (M4.5) | as Button | as Button | on: `accent_muted`, `accent` letters, the 38% hairline, no glow (cross-fades, `FAST`) | — |
| Tabs (the detail panel; `widgets::tabs`) | words in `text_secondary`, `space::M` padding each side, `space::XS` apart | `text` | — | `text`, over a 2 pt `accent` underline inset by `space::M`, with Lit's soft layer, sliding to it (`GLIDE`) | `text_disabled`, with why on hover |
| Time signature (`widgets::time_signature`) | one field, "4 / 4" in the monospace, sunk with a border | Ember on the half under the pointer: the beats left, the unit right | — | drag or wheel changes the half under the pointer; click and type "7/8"; double-click 4/4 | — |
| Empty state (`widgets::empty_state`) | centred: a 16 pt icon in `text_hint`, the title in medium `text_secondary`, a caption hint, the action as a button with its keys in the tooltip | | | | |
| Field (tempo, value fields) | sunk: `well`, the lip, a 1 pt `border`; numbers in the monospace | `border_strong` outline | — | `accent` outline while editing | — |
| Value slider (`widgets::value_slider`) | sunk; the filled part `control_press` to `control`; the handle a 2 pt `text` line that opens around the value | Ember | — | — | — |
| Dropdown (combo; `widgets::Dropdown`) | raised like Button: the value at the left, a ▼ chevron at the right | as Button | sunk, as Button | — (the open list is a menu: its chosen row `accent_muted`) | flat, `text_disabled` |
| Popover / window | `overlay`, `border_strong`, the edge light, the shadow; windows radius 12, popovers and menus 8 | | | | |
| Notice | `raised` ground, a 1 pt `warning` hairline at 45%, radius 8, ⚠ and `warning` text, wrapped, with buttons named for the action | | | | |
| Switch (on/off) | a 30 × 16 pt sunk pill (`well`, the lip), the knob a `text` to `text_secondary` gradient; its label at the left of a 20 pt row as wide as there is room for | Ember over the pill (`FAST`) | — | the knob glides right (`GLIDE`), `accent_hi` to `accent_lo`, on `accent_muted` with the 38% hairline | `text_disabled` |
| Segmented (2–5 choices; `widgets::segmented`, `segmented_styled`, `segmented_full` across the width given) | a sunk trough (`well`, the lip; radius by height), optional icon, label and keycap, the icon and keycap centred on the label's optical middle (`fonts::optical_middle`: halfway between its capitals' and its lower-case letters' middles), `text_secondary` | Ember over the piece, label `text` | — | a neutral raised thumb (`thumb_hi` to `thumb`, the edge light, the trough's radius less 2) that slides to the choice (`GLIDE`), label `text`; `segmented_styled(.., Some(Tone::Accent))` fills the chosen tool with the accent | label in `text_disabled`, with why on hover |
| Choice row (longer lists, such as a device's channel pairs) | a 24 pt row, `text`, radius 6 | Ember wash | — | `accent_muted` fill, `accent` text | — |
| Menu row (`widgets::menu_item`) | `size::MENU_ROW` (32 pt) tall whether it has keycaps or not; rows touch, so the pointer is always over one; `text`, with keycaps at the right, cap and light centred together; `menu_toggle_icon` puts a `size::MENU_ICON` icon before the label | Ember highlight, radius 6, reaching `space::XS` past the row, following the pointer | — | a toggle that is on: `accent_muted`, `accent` letters | `text_disabled` |
| Pill (lane tabs; `widgets::pills`) | 24 pt, radius `FULL`, `text_secondary` | Ember | — | `accent_muted`, `accent` letters, the 38% hairline; "+ CC" a `border_strong` hairline pill; the browser's chips (`chip_toggle`, `chip_button`) have 8 pt of room, and a button among them is a bare word | — |

Keyboard focus shows as a 2 pt `accent` ring, 2 pt outside, on every
focusable control (`raised::focus_ring`).

**The material (ADR-0052, replacing ADR-0051's lit look).** Every control
is painted by `widgets::raised` as §5a says; on states are
`theme::Tone`: `On` (toggles, pills, the transport's toggles, the Monitor
badge), `Accent` (filled: the one primary, a lit S, cue, the chosen tool),
`Mute` (a lit M) and `Record` (recording). The Monitor badge, dialog and banner buttons,
preferences-window buttons, Reset and Panic use `raised::button`,
`button_inked` or `paint`. Menu rows stay menu rows; value fields stay
fields.

**The transport bar (the interface audit's TB1, T1, T2 and TT1).** Its
controls are pieces of sunk wells (`widgets::well::row`), as the header's
M S ● well is: each piece `space::XXS` inside a `CONTROL_LARGE` well,
`WELL_CELL` tall, 1 pt from the next, bare at rest, the Ember wash under
the pointer and filled in its tone while on. Three wells, with a
hairline between one well and the next: go to start, Stop, Play and Record; the modes (Overdub, Loop,
Metronome, Follow, the computer keyboard with its octave, and the solo
piece, which clears every solo); and the numbers, tempo, meter and
position in one well with hairlines between them (`well::dividers`). The
pieces of the icon wells have no hairlines between them. The position's
wash covers its bars, the part a click types into. The position is typed in its own piece:
click or Enter, Esc puts it back. The CPU readout before it sits in a slot as wide as "CPU 100%" in
mono, its figure at the slot's right, so the chip after it never moves as
the load changes (TB2). The audio status is one raised chip with
its light and the device and rate; its tooltip has the whole status line,
and the panel it opens lists the sample rate, buffer, output latency,
round trip and delay compensation under the device's name. The palette
and panel toggles are bare icons with a 2 pt accent underline while on
(`icons::toggle`).

Dropdowns (`widgets::Dropdown`) replace egui's
`ComboBox`, which stays flat: a closed dropdown is a raised button with the
button's height, edge, hover, press and focus ring, and its list opens as a
menu (`Menu::below`, rows as `choice_row`), so it matches every menu. Its
accessible name is "<label>: <value>"; Enter or Space opens it, Up and Down
step the value when closed and move between rows when open, Esc returns
focus to the button. The list scrolls past `size::MENU_LIST`. Use it for
every choice of one of many values; two to five choices stay a segmented
control.

egui's checkboxes and radio buttons are not used (M4.5, the interface
audit's T3): on/off is a switch (`widgets::switch`), two to five choices a
segmented control (`widgets::segmented`), longer lists choice rows
(`widgets::choice_row`) or menu rows.

## 7a. Tracks and clips (M3 slice 4, owner's choices)

Chosen from rendered options on 2026-10-07: option A's headers with option
B's clips. The owner refined the design after MT-APP-9 run 1.

- **Header** (204 pt wide; redrawn in M9 R1b from the owner's B·1,
  ADR-0101). Content runs from 12 pt after the header's left edge to 8 pt
  before its right, on three rows 20 pt tall, 6 pt apart, 8 pt from the top
  and bottom:
  - a 4 pt colour rail, and beside it a 5 pt sunk **level strip** (two
    2 pt sides, 1 pt clear of the rail, 5 pt in from the top and bottom) in which the track's
    level rises, folded or not, with a 3 pt clip mark at its top that stays
    until clicked;
  - a 16 pt **fold button** showing the **track-type icon** (waveform for
    audio, keys for MIDI, stacked bars for a group, a return arrow for a
    return); under the pointer, or focused, the icon cross-fades into the
    fold arrow (a group always shows its arrow). Its tooltip names the type
    and what a click does;
  - the name, 6 pt after it, shortened with "…" and renamed by
    double-click or F2. The name row sits in the middle of a folded header
    and glides up to the first row as the track opens;
  - **M**, **S** and arm as pieces of **one sunk well** at the right
    (60 pt; a return's M and S, 41 pt): 18 × 16 pt pieces, 1 pt apart,
    2 pt in from the well's edge, 2 pt corners, snapped to whole pixels
    together. At rest a piece is flat on the well; under the pointer it
    takes the Ember wash. Lit: **S** is filled accent (`accent_hi` to
    `accent_lo`, an `on_accent` letter) and **M** is `mute` with an
    `on_mute` letter (ADR-0052). Arm is a dot: grey at rest, red under the
    pointer; armed, record red at 25% with a red dot and the breathing ring.
    A mixer strip has the same well under pan (the interface audit's X1–X4):
    22 pt pieces (`STRIP_PIECE_W`), M S ● on tracks and groups, M S on a
    return, M alone on the master.
  - A selected header is `selected_row`, and its controls step up (§3).
- **Unfolded header:** two more rows of controls that show their value
  inside, with no separate labels, in two columns: the left from the name's
  start to 6 pt before a 60 pt right column (`HEADER_SIDE`):
  - row 2: **volume** at the left, **pan** at the right;
  - row 3: the **send well** at the left and, on audio tracks, the **In
    chip** at the right (60 pt whatever the input: "In" in the hint colour,
    the input from a fixed place after it, shortened with "…", and the
    caret at the right; "None" in the hint colour without an input). A
    return lists its senders there instead;
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
  - on headers and mixer strips the button is the well's third piece;
    armed, its body is record red at about 25% with a red dot and a ring
    around it. The ring breathes slowly (2.4 s) while armed, the body
    swells with it (70 to 130% of its tint) and a soft halo grows round
    the dot (the owner: the pulse keeps its personality), all inside the
    piece. Every circle shares one centre;
  - at rest (M4.5, the owner's A) the dot is grey (the control's letters
    at 55%) on the control fill, turning red under the pointer (`FAST`):
    record red means recording;
  - the lane gets a red wash, strongest at the left, with thin red edges.
- **Folding and resizing**:
  - heights come in **22 pt steps**, so anything later drawn by track
    height (automation lanes, meters) lines up;
  - folded is 2 steps (44 pt); unfolded starts at 4 steps (88 pt: the
    name, then volume and pan, then the sends and the In chip; ADR-0101)
    and goes up to 14. Heights saved taller (110 pt from R1) stay as saved;
  - drag the bottom edge to resize: the handle is a 12 pt band centred on
    the row's bottom line, over both rows it touches (the controls stay
    8 pt clear of a row's ends). The handle glows with the accent
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

The clip's visible title/header moves it (Ctrl copies); its waveform or
MIDI-note body selects a time range, just like empty lane space. Ctrl+Shift
in empty lanes draws a clip marquee (ADR-0049). The header/body model
replaces the old arbitrary halfway division (owner, ADR-0053).
- **Hover:** the header or body under the pointer brightens (`text` at
  8%), with no dashed halfway divider. The info line describes moving or
  selecting time. Cursors: grab, copy, or an I-beam; locked clips show
  "locked" and a not-allowed cursor.
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
  Ctrl+Alt over the header: "drag to slip the audio".
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
  grey. Key 0 mutes or unmutes the selected clips.
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
    it opens), Color (two rows of six 14 pt square swatches, 8 pt apart,
    then a "Track color" pill with a dot in the track's colour, ringed in
    `accent` when chosen; M4.5), Gain (a
    `value_slider`), Fade in and Fade out (a `value_slider` in
    milliseconds and a shape menu), State (Mute and Lock,
    framed buttons that light when on); then the file's details.
  - Gain and fades are greyed out on a locked clip, with a hint saying
    why.
  - It follows the selection; Esc while typing cancels the typing, the
    next Esc closes the card. While it is open the hover card is not
    shown.
- **The hover card** ends with "Alt+Enter: edit its properties" in
  `accent`.
- **The lanes' right-click menu** (the owner's A, MT-APP-14 run 2) is for
  what was clicked: a clip (Cut, Copy, Duplicate, Delete; Rename, Color,
  Mute, Lock, Fades, Properties…), a time range (its edits,
  Delete time, Insert time), or empty lanes (Paste when there is
  something to paste, the grid as a submenu, Snap to grid). Commands that
  cannot run are left out, not greyed. Drag hints hide while a menu is
  open.
- **The grid behind clips** (G1): clip bodies are drawn at 55% opacity,
  so the grid shows through at about half its strength; the title bar,
  border and waveform stay solid.
- **A click on the lanes** moves the playhead there only while stopped
  (snapped; Alt freely); on a clip it selects it too. During playback,
  clip and empty-lane clicks keep playback running from its current
  position. Ruler and scrub-strip clicks can still seek while playing.
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
  ring in `mute`, the letter in it, on `mute` at 14%; a hollow S is the
  accent at 12%)
  and its name and type icon dim. A track heard because its group is
  soloed shows a **hollow S** in the accent. Lit buttons mean the user set
  them on that track.
- **The S chip** in the transport, after Follow: a pixel-snapped 20 pt
  circle, quiet grey "S" while nothing is soloed and accent with "S"
  while anything is. Its hint gives the solo count; a click clears every
  solo. The focus ring follows the circle.
- **Returns and the master (A1):** pinned at the bottom under a 7 pt
  divider (`raised`, strong edges, a 28 × 2 pt grip; it glows with the
  accent while hovered or dragged, as edges do), then a 22 pt "Returns"
  caption with a fold arrow and the count, the returns, and the master's
  row with a grey rail. The area takes at most 60% of the lanes' height;
  its lanes have a quieter ground (`canvas` at 70%). A return's name
  follows a 16 pt **letter badge** in its colour (text by `text_on`); no
  arm button on returns or the master.
- **The send well (B·1, ADR-0101):** on an unfolded audio, MIDI or
  group header, one sunk well on the third row holds a **slot** per return,
  every return always shown (up to twelve): 2 pt padding, 1 pt between
  slots, each as wide as fits up to `SEND_SLOT` 18 pt (8.6 pt for twelve
  at the default width, never under 4 pt). A slot fills from the bottom
  with the send's level in the return's colour (a gradient with an 8%
  sheen, at 40% strength while the send is not heard); a silent one is a
  2 pt baseline in its colour at 60%. The return's letter shows on slots
  at least `SEND_SLOT_LETTER` 12 pt wide (`text_on` over a fill at least
  half full, `text_hint` otherwise, `text_secondary` on a selected row); a
  pre-fader send has a dot. Drag up or down (Shift for fine), double-click
  for silence, click and type, right-click for pre- or post-fader. Under
  the pointer or focused, a **flag** rises 6 pt above the slot: a raised
  22 pt tag with the letter badge, the return's name and the level in mono,
  kept inside the header column. Hovering a slot lights its return's
  header, as a knob does. A slot that is sending glows like a knob's
  head: a soft halo above the level in the return's colour, stronger under
  the pointer, and a bright hairline on the level's edge.
- **Send knobs (K1 "comet"):** 18 pt knobs in lists (a return's
  senders); headers and mixer strips have the send well's slots instead
  (the interface audit's X1–X4: one look for a send wherever it sits). A
  270° ring in `control_press`; the level a trail in the
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
  - **A return's senders (R2, L4, S1):** an unfolded return lists, from
    its third row, the tracks sending to it: a 4 × 14 pt bar in the
    track's colour, its name, an 18 pt knob (no hover ring there) and the
    level, the four loudest first, then "+N more ▾" opening "Sends into …"
    with them all. A send at −∞ leaves the list when let go. It unfolds
    tall enough (a step a row). "No tracks send here yet" when none do.
- **Cue (C1):** the master's **Solo/Cue switch** (46 × 18 pt at the
  header's right): "Solo" in control colours, the accent with headphones
  and "Cue" when on. It is left out until a cue pair is chosen in Audio
  settings (the interface audit's X8: greyed, it read as broken). On Cue,
  the S buttons draw **headphones** (`icons::draw_headphones`) and light
  with the accent. The master's unfolded row adds a "Cue" level slider
  while a cue pair is open.

### Meters (M4 slice 5, owner's choices, ADR-0040)

Chosen from interactive specimens:
- **Colours (M1, M2 a setting):** a level is `accent` up to −6 dB
  (`theme::METER_WARN_DB`), `warning` up to full scale, `record` above it
  (`theme::meter`). "Meters in track colors" (app menu, palette) swaps the
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

### The detail panel's tab row (DP1, the owner's A, ADR-0108)

The panel always says which track it is about, and has no tab that opens
onto an empty page.

- **Subject, at the left:** a 3 × 14 pt bar in the track's colour, its name
  in medium 13 pt (cut with "…" past 160 pt), then a 14 pt `border_strong`
  hairline (`size::TAB_DIVIDER`, `widgets::tab_divider`). The subject is the
  track of the header or clip clicked last (`DetailState::anchor`: the clip
  selection is an unordered set, so the panel keeps its own); with nothing
  selected it is the opened clip's track. Master, groups and returns name
  themselves. "No track selected" in `text_hint` when there is none. While
  Mixer is chosen the subject is `text_hint` and its bar at half strength,
  since the mixer is not about one track. The subject is as wide as its name,
  so Devices and Clip move with it; Mixer does not.
- **Devices** shows the subject's chain.
- **Clip** is there only while a clip is open (Enter, the command "Open
  clip", or a double-click; an audio clip as well as a MIDI one), with the
  clip's name as an 11 pt `text_secondary` note 6 pt after the word, cut with
  "…" past 140 pt; the underline spans word and note. It stays while other
  things are clicked, is replaced when another clip opens, and goes when that
  clip is deleted: the panel shows Devices, and the keyboard follows if it was
  in the panel. A header click while on Clip goes to Devices.
- **Mixer** sits at the right after a hairline (`TabsLayout::right_from`).
- **An audio clip's Clip tab:** its properties (the clip card's rows,
  `docked_clip_props`) in a 296 pt column on `app` with a hairline at its
  right, scrolling when short, beside the clip's wave: bar numbers in mono
  10 pt over the clip body in its colour with the title strip, the wave drawn
  one column per physical pixel with its gain and fades.
- **Commands:** Show devices, Show clip and Show mixer (no default keys);
  Show clip needs an open clip.

### The Mixer tab (M4 slice 5, owner's choices, ADR-0040)

- **Layout:** the detail panel's Mixer tab. Strips follow the arrangement's order, skipping
  tracks hidden in folded groups (one fold for both views, B4); the
  returns after a 10 pt gap; the master pinned at the right, and the
  loudness column (L2, 108 pt) beside it. The panel is one `header`
  surface (the interface audit's X6): the strips scroll sideways under
  the master, which casts a `space::M` shadow over them; only those in
  view are drawn. With no
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
  master" on hover; "Looking up the outputs…" until known); Ø and input
  trim (±24 dB, filled from the centre; the master shows its Solo/Cue
  switch there once a cue pair is chosen; the trim's "dB" is set smaller,
  `UNIT` 9 pt in `text_hint`); a hairline, then the send well
  (every return a slot, as on the header, centred); a hairline, then pan;
  M, S and arm, one well under pan (the interface audit's X1–X4); the fader
  (`widgets::fader`: a 4 pt sunk groove and a 22 × 12 pt raised cap with a
  2 pt `text` line,
  on the send knobs' taper, 0 dB at three quarters of the travel, stopping
  at −60 dB) beside the scale (monospace marks, 0 in `text_secondary`; the
  marks are placed most important first, 0, −48, +6, −12, −24, −6, and one
  within 11 pt of a numbered mark is a 3 pt tick instead) and
  the stereo meter (two 5 pt bars, 2 pt apart, with the clip light, 5 pt,
  above); the volume typed in (`widgets::value_field`) and the peak held
  (`record` when clipped; a click clears it).
- **A short panel (B7)** folds the sends away first, then trim and
  polarity, then the output; the rest always shows, with a travel of at
  least 64 pt. The panel is never shorter than that strip, and never so
  tall that the arrangement keeps less than 180 pt.
- **Loudness column (L2):** "Loudness", the integrated value in 20 pt
  monospace with "LUFS" beside it and "Integrated" under it in
  `text_secondary`; a 12 pt short-term bar (the accent, the warning colour
  above −9 LUFS) with the momentary as a 2 pt `text` line and the −14
  LUFS ± 1 band in the accent at 14%; the last minute's short-term
  loudness as a line in a sunk well beside it, across a dashed −14 line
  (the bar's scale names it); M, S and TP with each value 28 pt from the
  column's edge, beside its label (TP in the warning colour above −1
  dBTP); "↺ Reset". At rest (the interface audit's X5) every value reads
  "––", the integrated one in `text_hint`, and the empty well says "Play
  to measure"; a screen reader hears "not measured".
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

### The velocity and controller lane (M5 slice 4, owner's V A, L A, T A, R B)

Chosen from live specimens (ADR-0050). The lane sits under the piano roll in
the clip editor, in the Clip tab and the pop-out, and shares the roll's zoom
and grid.
- **Header:** 32 pt (`CONTROL` + twice `space::XS`) on `raised`, with 4 pt
  vertical margins around the 24 pt pills and 8 pt horizontal insets;
  a 1 pt `border_strong` rule along its top and
  a 1 pt `border` rule along its bottom. The tabs (Velocity, Mod wheel, Pitch
  bend, Pressure, "+ CC") sit at the left; the tools Pencil, Line and Curve
  at the right.
- **Plot:** on `canvas`, 28% of the editor's height and 48 to 150 pt tall.
  The lane is left out when the roll above would be under 140 pt. A value
  scale column on `raised` holds the range's ends (and 0 for pitch bend),
  in `text_secondary`, with a 1 pt `border` rule on its right edge.
- **Velocity stems:** a 2 pt stem from the lane's rest line to a head of 4 pt
  radius (5 pt under the pointer), filled in the note's colour; a muted note
  uses `CLIP_MUTED`. A picked stem is lit: its stem in `accent`, its head
  filled `text` with a 1.5 pt `accent` ring.
- **Controller traces:** a 1.6 pt `accent` step line, its area filled with
  `accent` at 16% alpha down to the rest line. Points are 2.5 pt `accent`
  dots (4 pt under the pointer), ringed 1.5 pt in `text` when picked. Dots
  show only while fewer than 250 points are in view and, on average, 5 pt
  or more apart; otherwise the trace alone shows.
- **Previews** of what a gesture will write, before release: 1.4 pt in `text`.
- **Bend dot:** on the middle of a drawn curve, 5 pt radius, `text` fill,
  2 pt `accent` ring; the bend's line is 1.6 pt `accent`.
- **Colour:** every colour is an existing token or constant (`raised`,
  `canvas`, `border`, `border_strong`, `text`, `text_secondary`, `accent`,
  `CLIP_MUTED`). This slice added no tokens.

### The clip editor's look (M5 slice 5, the owner's approved canvas mock-up)

The Clip tab uses the shared controls (§5a, ADR-0052).
- **Header strip** (`note_editor/toolbar.rs`, 40 pt, `raised`): the clip's
  colour bar, its name (medium) over "N bars · N notes" in the mono face
  (`SCALE`, `text_hint`; "· locked" when it cannot be edited); a rule; the
  Pointer/Draw `segmented_styled` control with icons, the Draw key as a
  keycap from the keymap (`Choice::hint`); the grid chip (a raised button:
  magnet lit while snapping, "Grid", the value in mono, a chevron; it opens
  the grid menu; named "Editor grid: 1/8"); at the right Lane (a lit icon
  toggle) and Pop out/Dock (icon buttons), keycaps in their tooltips. No
  Quantize button yet: there is no quantize command until slice 5 of M5's plan.
- **Notes:** the GPU boxes (rounded, light top edge, velocity brightness)
  plus egui layers: a two-layer soft shadow beneath (`control_shadow`), a
  2 pt velocity bar on the bottom edge, and a `light::LIT` glow around picked
  notes (all capped at 4000 notes a frame). A note being drawn is a dashed
  ghost with a "G3 · vel 96 · 1 beat" chip.
- **Keys:** an ivory gradient, white-key dividers behind the black keys, black
  keys (62% wide) with a gradient and a shadow, C labelled in mono; the key
  under the pointer or a gesture is lit with an accent gradient.
- **Roll:** ruler bar numbers in mono; the playhead is the arrangement's
  (`light::BEAM`, its cast and cap while playing); the right edge fades into
  `canvas`.
- **Lane header:** `widgets::pill_tabs`: the chosen pill is `accent_muted`
  with a 1 px `accent` edge and `accent` letters; "+ CC" is a dashed
  `pill_button`; Pencil/Line/Curve are a segmented control with icons. The
  value scale reads 127 / 64 / 0 in mono.

## 7b. Navigation (M3 slice 4d, ADR-0028)

- **Zoom** glides (`GLIDE`) for keys and fits; the wheel and pinch follow
  the hand directly. Down to 8 points per sample, where the waveform is a
  line through the samples; out to the whole song with 10% of room (at
  least 32 bars).
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

- **The header corner** (the interface audit's H1) is two rows of sunk
  wells (`widgets::well`), `size::CONTROL_SMALL` tall with `space::S`
  margins, built like the M/S/arm well of a track header: pieces bare at
  rest, 1 pt apart, the Ember wash under the pointer, filled in their tone
  while on. The ruler row holds one well: the magnet, the grid value, the
  tool and the tempo-lanes icon. The locator row has "Locators" at the
  left and a well of ◂, Set and ▸ at the right. "Set" is a bare piece; its
  text turns `accent` while a locator sits at the playhead.
- **The magnet piece** is the "Snap to grid" checkbox: `Icon::Magnet` in
  `accent` while snapping, `Icon::MagnetOff` in the piece's ink when not
  (owner's choice C1, MT-APP-13 run 1).
- **Tool keys:** V picks Pointer, B picks Draw (and from Draw goes back
  to Pointer), and D, C and U pick Erase, Split and Mute (the interface
  audit's M6). While the computer keyboard plays notes, it takes V, D, C
  and U, as it does Z and X.
- **The grid value piece** ("Grid: 1/16", with ", snap off" while
  snapping is off) shows the step in the monospace caption ("1/16",
  "1/8T", "4 bars") and a ▾ (`widgets::ARROW_SIZE`), and opens the grid
  menu, which keeps "Snap to grid"; right-clicking the lanes opens it too.
- **The grid menu** lists the adaptive widths, the fixed sizes, Triplet,
  Dotted and Snap to grid as selectable items with their shortcuts.
- **Lines:** bar lines `grid_bar`, beat lines `grid_beat`, lines between
  beats `grid_sub`. Never closer than 6 points; a grid finer than that on
  screen shows calmer lines 12 points apart, and still snaps to its own
  steps.
- **Snap guide:** a 1 pt accent line at half strength, from the ruler to
  the bottom, where a click on the ruler or a dropped file will land.

## 7d. Rulers (M3 slice 5b, layout A)

Chosen by the owner from rendered specimens:
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
  it. Single-letter main keys share one width, including narrow glyphs such
  as I; longer key names size to their labels (owner correction, 2026-10-24).
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
    or why it cannot run in the warning colour. It keeps the room of the
    longest explanation (every command's is at most 320 characters, a
    test keeps it so), so the palette never changes height as the chosen
    row changes (the interface audit's P1);
  - a footer as tall as keycaps: the hint ↑↓ move · Enter run ·
    Ctrl+Enter change keys · Esc close, and every chord that runs the
    chosen row at the right. With no row matching, the strip is empty
    and the footer stays, so the height still holds.
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
  - a search field (names and shortcuts), the current keymap name and Reset
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
  text. Commands without a shortcut show none. In a context menu
  (`App::context_command`, the interface audit's M3) a row that cannot act
  on what was clicked is left out; one that could but waits on work under
  way (a join, a bounce, an export: the reasons in `app::commands::wait`)
  stays, greyed, and its hover says why. Main menus grey a row that
  cannot run now.
- **Context menus are grouped** (the interface audit's M2, M4, M5): a
  control's menu opens with a header naming the control and its track
  ("Volume · Bass", `widgets::menu_header`); groups are split by dividers.
  A track's menu: Rename, Color | Duplicate, Group, Move up and down |
  Input or Instrument, MIDI From and To, Monitor, the automation lane,
  Fold | Import, Bounce, Freeze | Add track ▸ | Delete. A clip's: Cut,
  Copy, Duplicate, Delete | Edit ▸, Takes ▸ | Rename, Color | Mute, Lock
  | Fades ▸, Crossfade with previous, the envelope | Export MIDI clip… |
  Properties. Project settings (automatic crossfades) live in the Project
  settings window, never in a clip's menu. A browser device's menu starts
  with what choosing it does ("Add to Bass", or "Add on a new track"),
  then favouring it, then its file.
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

## 7f. The Devices tab (M6 slice 2d, ADR-0072)

The owner's C4. A track's chain sits in a sunk tray (`well`, with the lip);
each device is a card standing out of it (`raised`, the `border_strong` edge,
a soft shadow, radius `XL`).

- **Card, top to bottom:** a picture up to 72 pt tall (a monogram in a clip
  colour chosen from the class id, the kind glyph at its right; greyscale and
  dim when bypassed, failed or missing), fading into the header; the header
  (`header`: power at the left, name in medium weight over the vendor in
  `text_secondary`, a kind badge at the right; an instrument has a 3 pt stripe
  in the track's colour); the parameter strip (below); the footer (`header`, mono
  caption: "MIDI → audio" or "audio → audio" and the latency, or "bypassed" and
  "2.7 ms kept").
- **The parameter strip** (M6 slice 4, the owner's S2 with the send knobs'
  comet style): eight comet knobs in two rows of four (`widgets::param`,
  drawn by the send knob's own `draw`: the ring in `control_press`, the trail
  and head in `accent`, dimmed when the parameter is read only), each with its
  name in `text_secondary` caption under it and the plugin's own text for its
  value in mono. Full-card knobs are 24 pt (`size::KNOB`). A card is 232 pt wide and 310 pt
  tall with its picture. An
  empty slot is a dashed ring with a "+" that opens the panel. Drag up or
  right (Shift for fine), double-click for the plugin's default, the wheel and
  arrow keys; a stepped parameter moves a step at a time. **Typing:** a
  focused control opens a field in its place when a digit, "-", "+", "." or ","
  is typed (as the send knob's does), or with Enter, F2 or the right-click
  "Type a value…" (the plugin's own text, selected); Enter or leaving the
  field confirms, Esc cancels. The plugin reads the text, so units work
  ("2.4k"); text it does not understand changes nothing and says so in a
  note. Right-click: Reset to default, Type a value…, Remove from strip. Under
  the knobs one raised button, "All
  parameters (N)". A plugin that lists none says so; the built-in synth sends
  you to the track's Instrument menu.
- **The plugin window's button** (M6 slice 5h; the owner's D from rendered
  specimens): a running plugin's card has a 24 pt square raised button with a
  window glyph in the header, where the kind badge was (the picture still says
  the kind; the built-in synth and a card that is not running keep the badge).
  Named "Open editor" or "Close editor"; lit (`Tone::On`) while the window is up
  or opening, the glyph in the warning colour when it stopped answering, dim
  when the plugin has no window (named "No editor", the tooltip says why).
- **The row under the knobs** (M6 slice 4f, changed in 5h): up to two raised
  buttons at the caption size, 20 pt tall (`CONTROL_SMALL`), 4 pt apart,
  widths 1 : 2.2. Learn
  reads "Learn" and "Learning…" lit; the other carries the plugin's parameter
  count, "All 259 parameters" (exact below a thousand, then "1.2k", then "12k";
  named "All parameters (N)"; it opens the panel of every parameter). A window
  that stopped answering gives "Not responding" in the warning colour beside a
  filled "End plugin" in their place; a plugin with no window has only the All
  button. A plugin that lists no parameters has no row.
- **The All parameters panel** (the owner's L1): a `Dialog`, "<device>
  parameters", 400 pt wide: a search field, group chips ("All 259", one for each
  of the plugin's groups, "On the strip"), then one list, rows of 28 pt drawn
  only when in view: a star (puts the parameter on the strip), the name with
  its group in `text_hint`, and a bar (a sunk trough filled to the value with
  the plugin's text at its right). Hidden parameters never show; read-only ones
  show no fill movement and cannot be dragged. Up and down move a highlighted
  row and Enter stars it. The strip holds eight: a ninth star says so. A bar
  is typed into the same way as a knob (the field takes the bar's place).
- **The sidechain row** (M6 slice 6c; the owner's B with the light of I1, from
  rendered specimens): a running plugin effect that has a sidechain input has a
  row between the knobs and the Learn row, 22 pt tall in a 28 pt space: the word
  "Sidechain" in `text_secondary` (62 pt) and one raised chooser. The chooser
  shows a 8 pt swatch in the source track's colour, the track's name, " ·
  Post FX" in `text_hint` (the name alone when both do not fit), the light and
  an arrow; "None" in `text_secondary` when there is no source; "▲ Source
  removed" in `warning` when the track was deleted. **The light** is a cable
  port's ring (`border_strong`, `well` inside) with the Beam's dot lit by the
  level of what arrives, and a `warning` hollow ring when the song plays and
  nothing has arrived for two seconds. Its menu (named "Sidechain source, …"
  for the chooser): None, then Tracks, Groups, Returns with a swatch each, a
  track that would feed back greyed with the reason; a rule; "Taken from the
  track" with a three-way switch (Pre FX, Post FX, Post mixer, greyed until a
  source is chosen) and one line saying what the chosen tap is. A card with the
  row takes its 28 pt from the picture, and the Devices panel's least height is
  28 pt more while the shown track has one.
- **The outputs row** (M6 slice 6d; the owner's O1, from rendered specimens): a
  running plugin instrument that has more than one output has the same 22 pt row
  in the same 28 pt space: the word "Outputs" in `text_secondary` and one raised
  chooser saying "Main only" or "Main + N", lit (`Tone::On`) when N is more than
  none. Its menu (named "Outputs, ..." for the chooser, 250 pt wide, never wider):
  the title, one line "Main plays on the instrument's own track", then a list
  that scrolls (224 pt, with a solid scroll bar that takes its own room) with a
  switch for each other output in the plugin's own words, "All on" and "All off".
  A switch on makes an audio track under the instrument's track in its colour and
  group, named "Instrument · Output"; the track's own name shows beside the
  switch only if it was renamed. A switch off removes an empty track at once; a
  track that holds clips or devices asks "Remove track, or keep it with no
  input?" in the menu. A track that plays an output shows a warning triangle in
  its header, with the reason, when its instrument is not running. A switch
  changed while its row is scrolled out of view does not glide when it comes back.
- **Compact cards** (D2, the owner's A): when the tray is too short for a
  whole card without its picture (with 8 pt of hysteresis each way, so
  dragging the panel's edge never makes them flicker), every card turns
  compact: as wide as a full card and 116 pt tall. A 36 pt header holds the
  power and window buttons at `CONTROL_SMALL` and the name, which opens the
  card's menu (it also holds the preset, Learn and All parameters); one row
  of the strip's first four knobs at 18 pt (`KNOB_SMALL`); a 24 pt footer
  with the signal and latency and "All N ▸". A sidechain or outputs chooser
  moves into the footer at `CONTROL_SMALL`, its word into the tooltip. A
  card that is not running says what happened in the knobs' place, whole
  (never cut), with its buttons in a footer that gives them 6 pt above and
  below and `space::M` at the sides; such a card is as tall as its words
  need. The Devices tab's least height is the tallest compact card in its
  tray (212 pt for a plain card), and picking Devices grows the panel back
  only to the height last chosen for it.
- **Chosen:** a 1.5 pt accent edge. Under the pointer: the Ember wash.
- **Cables:** 2 pt `border_strong` when unlit; the Beam (accent line, `core`
  inside, halo and cast) while the song plays. Dotted for notes. A port is a
  ring with the light's dot at the card edge.
- **Faces:** loading shows three empty rows and "starting…"; failed shows
  "Stopped responding" in `warning`, what happened and a **Reload** button;
  missing shows "Not installed", the size of the settings kept and **Rescan
  plugins**. Both keep the cables lit.
- **Safe mode** (M6 slice 5e, the owner's I2 and F2): a strip under the
  transport bar (the notice card with a shield in the warning colour: "Safe
  mode: plugins are not loaded" and a raised **Leave safe mode**). A device
  that is not loaded keeps its card's shape: the artwork, header and footer as
  for a missing device, four knob rings greyed (`control_press` at 50%) where
  the strip stands, then "Not loaded · safe mode", "Its settings (N KB) are
  kept." and a raised **Load this plugin**. After a start that did not finish
  loading plugins the strip is the offer: "Last time Lumen didn't finish loading
  plugins. Start in safe mode to open this project without them.", with the
  primary **Start in safe mode** and **Load plugins**; nothing loads until it is
  answered.
- **Dragging a device from the browser:** an accent line across the tray and a
  plus on the cable at the gap it will land in; neighbours stay put.
- **Empty** (the interface audit's D1): the tray keeps its two chips and
  cable, and above them the shared empty state says "No devices on Bass",
  "Double-click a device in the browser, or drag one here", with **Browse
  devices**, which shows the browser on its Devices tab. No track selected:
  "Select a track to see its devices".

### The browser's Devices tab (the interface audit's B1 to B6)

Everything lines up on three edges, 12 pt in from the pane (the search
field, the type filter, the chips' words and the scan line), 28 pt (the
magnifier, the list's icons) and 44 pt (the search text and the device
names).

- **Search:** a 16 pt magnifier inside the field, at its left.
- **Type filter:** All, Instruments and Effects fill the width
  (`widgets::segmented_full`): in equal thirds while the longest word fits
  a third with 4 pt each side, or, in a narrow pane, each its own width
  with an equal share of the room left.
- **Chips:** Favorites, Recent and the categories are browser chips
  (`chip_toggle`, 8 pt each side of the words) in one flow that fills each
  line, 4 pt apart; their words start on the field's left edge and end by
  its right one, so a lit chip's fill hangs 8 pt outside. The chips' clip
  reaches 4 pt past the pane's own, so that fill is never cut (B8). "More ▾" is a
  bare word with the chips' room and a down arrow (`chip_button`), so the
  row reads as one flow of words.
- **Rows:** the star shows on a favourite, and on the hovered or selected
  row; elsewhere the column is empty. A hovered or selected row also shows
  its category as a tag when the pane is wide enough. A flat list (Recent)
  has no vendor headers, so the tag there is the row's vendor, on every row
  and at any width. On a selected row the tag steps up to `text_secondary`
  so it stays readable on the highlight.
- **Group headers:** the count sits centred on the star column.
- **Scan line:** under the filters and a hairline rule, above the list: the
  count, and "Rescan" (or "Cancel" while a scan runs) as a bare chip word.

## 7g. The block budget (ADR-0075, the owner's B)

In the audio panel, under the test tone: where an average audio block's time
went over the last second.

- **Headline:** "Each block" and, in the monospace, "2.56 of 2.90 ms · 88%",
  `warning` when the mean passes the plugins' deadline.
- **Bar** (`size::BUDGET_BAR`, 18 pt, a `well` with radius `SMALL`): Lumen's
  own time in `text_secondary`, then each of the five costliest plugins in
  `budget[0..5]`, solid for the part of the wait it was working and hatched
  (its colour over `accent_muted`, 1.5 pt lines 5 pt apart) for the rest of
  the wait; every other plugin shares one `text_disabled` segment. A 2 pt
  `warning` line marks the deadline, its time under it at `size::SCALE`.
  Hovering a row leaves its segment lit and dims the rest to 30%.
- **Legend:** work, waiting on the trip, plugins answer by.
- **Missed blocks:** a `warning` box (fill at 10%, hairline at 35%, radius
  `MEDIUM`) with ▲ and "N plugins missed blocks in the last minute…"; the
  rows that missed carry "▲ n".
- **List:** "PLUGINS · N" and "by time · work + trip, ms" in captions; a
  Lumen row; then every plugin, costliest first, as a two-line row (name in
  `text`, track and slot in `text_hint`, figures in the monospace at the
  right, a `size::BUDGET_SWATCH` swatch at the left). It scrolls past
  `size::BUDGET_LIST`; in a short window it gives way first, down to two
  rows, and then the whole panel scrolls.
- Mock-ups of it use generic device names (the owner, 2026-10-17).

## 7i. Tempo and meter lanes (M7 slice A6, ADR-0089)

Two slim lanes sit between the bar ruler and the tracks, a tempo lane (`size::SONG_TEMPO`,
44 pt) over a meter lane (`size::SONG_METER`, 20 pt), shown with **T**; the band glides open and
shut and adds no track row. They are tinted like automation lanes (an accent wash at 5% and
3%, accent hairlines), with a faint line at each bar. The **tempo lane** draws a step line in
the accent at `stroke::LIT`, a point (`draw_point`, as in automation lanes) at each change with
its value in the monospace beside it, hollow at rest, larger under the pointer, solid with a ring
when picked. Its scale fits the song's own tempos with room above and below and at least a 30 BPM
span, and is held for the length of a drag. The **meter lane** draws a chip per change, starting
at its bar line and as wide as its text: `control` with a hairline, `control_hover` under the
pointer, `accent_muted` with an accent hairline when picked; the meter in force at the left edge
is a quiet hint when its chip has scrolled away. The **name column** over the track headers reads
"Tempo" and "Meter" with the tempo and meter in force at the playhead. A value is typed in a
small overlay box beside the change (monospace, `size::SONG_TYPE` wide): Enter or leaving applies,
Esc cancels. While a change is dragged a `draw_hint` chip says the bar and the value. The info
line says what a click, drag and double-click do over each lane, and each lane has an accessible
name ("Tempo lane: click to add a tempo change, drag a point to move it, double-click a point to
type its tempo").

A **ramp** (a change that glides to the next) is drawn as a slope between the two points, in the same
line; a step holds and jumps. The tempo and the name column show the tempo playing at the playhead.
The lanes are found with the tempo-lanes piece (`Icon::TempoLanes`) at the right of the header
corner's well (§7c), lit with `Tone::On` while shown, and with a toggle in the
bar ruler's right-click menu, as well as T.

## 7h. Automation lanes (M7 slice A1, ADR-0082, the owner's L1 B)

A lane is drawn over its track's clip lane, not in a row of its own. While a track shows
one: the clips behind it step back to 45% (`UNDER_LANE`, names too); the lane takes a faint
accent wash with accent hairlines at its top and bottom and a hairline where the control
rests (0 dB, centre); the line is the accent at `stroke::LIT`, and a lane that is off is
`text_hint`. Points are `size::AUTO_POINT` circles: hollow at rest (the canvas colour
inside, an accent ring), a point larger under the pointer, and the picked one solid with a
ring round it so it never rests on colour alone. Values are drawn under the chip, which
sits at the top-left, a raised combo naming the parameter ("Volume", "Send to Reverb",
"Pan · off") with its menu. A track with a lane is at least six steps tall. The lane is
shown and hidden with the A key, the track's menu or the command palette (the header's
**A** button left in M9 R1b, ADR-0101). Sizes are `size::AUTO_*`.

Lane editing (M7 slice A2, ADR-0083). To the right of the chip, a **tool strip** (a
`widgets::segmented` with the MIDI lane's pointer, pencil and curve glyphs: Pointer, Pencil,
Shape), and with a shape tool in hand a raised combo naming the shape (Ramp, Sine, Triangle,
Saw, Square). It is `size::CONTROL_SMALL` tall like the chip, so it never covers a point, and
it is left out when the lane is narrower than 150 points. Picked points are solid with a ring;
a box is a rounded accent wash at 12% with a hairline at 70%; the **bend dot** of a picked
point's segment is a solid accent dot (`size::AUTO_BEND`) in a 22% halo, solid where points
are hollow, larger when held. A point resting on a control's rest value draws a hairline across
the lane in the accent at 50%. The live value is `widgets::draw_hint` beside the point, on the
tooltip layer, below the point when above it would cover the strip.

### A clip's envelope (M7 slice A7, ADR-0090, the owner's E1 A)

A clip's gain or pan envelope is drawn over its clip and nothing else, with the look of a lane (§7h): the
wash, the accent line, hollow points, the chip at the top-left naming it ("Clip gain · name") with its menu.
The track's clips do not step back, so the waveform shows through for placing points. A looping clip shows the
envelope again on each repeat as a dashed line at 55% accent, which the pointer ignores. The E key and the clip's
menu show and hide it; the chip's menu switches between gain and pan and goes back to the track's lanes.

## 8. Key flows

### Join crossfades (M7b E2, accepted X1)

Reuse the incoming audio clip's
fade-in handle and bend dot; a crossing shows both equal-power curves in
the existing fade material. The displayed duration is bounded by the
outgoing source tail and incoming clip. The info line names Crossfade and
shows the live duration during a drag. A custom duration dragged to zero
disables that join's crossfade. Broken joins and missing tails show no
crossing. Automatic crossfades is a switch in Project settings (and a
palette command), not a menu row. A crossfade with the clip before is in
the clip menu and the palette. Controls, curve strokes, focus, spacing and
text use the established theme tokens and shared widgets. This introduces no new palette,
material, typeface or handle geometry.
The app menu scrolls within the viewport at the minimum window height, so
the project setting does not push later controls beyond reach.

### Arrangement tools (M7b E0, owner's K1)

The tool switch is the flexible piece of the header corner's well, between
the grid value and the tempo-lanes icon (§7c): the tool's icon, its name and
a ▾, lit in `accent` while the tool is not Pointer; a narrow column drops the
name and keeps the icon, the accessible name ("Arrangement tool: Draw") and
the tooltip. Its menu lists Pointer, Draw, Erase, Split, Mute and Zoom, each
with its icon (`widgets::menu_toggle_icon`, `size::MENU_ICON`) and keys:
V, B, D, C and U by default (the audit's M6), Zoom none since Z and X zoom
already; the palette lists each tool too.

B toggles Pointer/Draw in the arrangement and retains the active editor's
existing behavior. Escape cancels a cut preview and returns to Pointer.
Split shows a snapped guide; a press and aim is a preview until release,
and Alt bypasses snapping. Ctrl+E cuts at the stopped edit position or
moving playhead, or at both edges of the selected range. Locked clips show
the unavailable cursor. Erase and Mute act on the clicked unlocked clip;
Draw adds a MIDI clip in an empty MIDI lane. Zoom clicks in (Alt clicks out)
and drags to fit a span; X returns to the previous view.

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
5-notify. **A notification is always read in full** (owner rule,
   2026-10-23): toasts, notice cards, banners and error messages wrap onto
   more lines and never end in "…", whatever their length. Truncation is
   for labels (names, paths, status values), with the full text on hover.
   A test lays out a long toast and checks it wraps.
5a. Windows are `widgets::Dialog`; controls on a selected surface use
   `theme::controls(true)`. Circles are snapped to pixels along with the
   rectangles beside them.
5b. Typed values apply on Enter or leaving the field, never per keystroke.
5c. Surfaces come from `widgets::raised` (§5a): controls raised, wells
   sunk, the one primary filled; hover is Ember, never another fill; the
   radius follows the height; no shadows but on windows and popovers.
6. Motion follows §6. With Reduce motion on, it still works and simply does
   not travel.
7. Every control has an accessible name.
7a. Hovering anything that can be dragged or clicked sets the info line
   (§2); nothing floats over content to explain it.
