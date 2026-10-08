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
└──────────────────────────────────────────────────────────────────────┘
```

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
- **Hack** (egui's bundled monospace) for numbers that must not jitter.

egui's bundled fonts stay behind Inter as fallbacks for symbols (⚠, ⟳).
Licences: SIL Open Font License, see `THIRD_PARTY_NOTICES.md`.

| Style | Face | Size (pt) | Use |
|---|---|---|---|
| `caption` | Inter | 11 | Hints, secondary facts, sample counts |
| `body` | Inter | 13 | Labels, values, menu items, buttons |
| section heading | Inter medium | 13 | "Device", "Format" in windows |
| `wordmark` | Unbounded | 14 | "Lumen" in the transport bar |
| `title` | Unbounded | 15 | Window titles |
| `heading` | Unbounded | 16 | Panel titles (Browser) |
| `mono` | Hack | 13 | Position, tempo, CPU: anything that changes while you read it | Sentence case for every label ("Audio settings", not
"Audio Settings").

## 5. Spacing and shape

- **4 pt grid:** `XS 4 · S 8 · M 12`. Controls sit 8 apart. Sections in a
  window are divided by a separator with 10 above each heading. Content
  insets in windows are 12.
- **Control height:** 24 pt (transport icons 28).
- **Radius:** `SMALL 4` (buttons, inputs, clips), `MEDIUM 6` (popovers),
  `LARGE 8` (windows).
- **Elevation:** only windows and popovers float. They get `overlay` fill,
  a `border_strong` outline and one soft shadow. Nothing else has a shadow.
- **Settings rows:** labels in a right-aligned column 96 pt wide, values to
  the right of it, warnings wrapped within the column's width.

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
  - notices and banners appearing (fade and a 6 pt drop);
  - hover and press fills on transport buttons;
  - the playhead and start marker moving after a locate while stopped;
  - clips appearing after an import (fade);
  - the progress fill of a loading clip.
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
| Toggle (Browser, Detail) | as Button | as Button | as Button | `accent_muted` fill, `accent` text | — |
| Field (tempo, meter) | `raised` fill | `border_strong` outline | — | `accent` outline while editing | — |
| Combo / menu | as Field, ▼ | | | selected row `accent_muted` | |
| Popover / window | `overlay`, `border_strong`, shadow | | | | |
| Notice | `warning` text, wrapped, with Dismiss | | | | |

Keyboard focus shows as a 2 pt `accent` ring on every focusable control.

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
  - **volume** fills from the left, with a thin handle line and the value
    in dB;
  - **pan** fills from the centre; its fill moves from neutral grey to the
    accent as the pan nears an edge (strength = |pan|^1.2). The centre is
    marked by short ticks at the edges, clear of the value;
  - both: drag (Shift for fine), double-click to reset, click then type a
    value (Enter applies, Esc cancels), arrow keys when focused.
- **Record arm ("glow and breathe")**, the owner's choice:
  - the rail turns red and glows into the header;
  - the button is a red dot with a ring around it. The ring breathes
    slowly (2.4 s) while armed, its glow reaching at most 4 pt, inside the
    gap to S. Every circle shares one centre;
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
- **Adding tracks:**
  - a **"+ Add track ▾"** button below the last track opens a menu of
    every type with its shortcut (audio Ctrl+T, MIDI Ctrl+Shift+T, group,
    return);
  - the hint underneath says "Or double-click empty space", which adds an
    audio track;
  - the right-click menu offers the same.
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
  - Rows with a 62 pt label column (`text_secondary`): Name (focused when
    it opens), Colour (12 square swatches and a round "Track colour"
    swatch, 14 pt, ringed in `accent` when chosen), Gain (a
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
