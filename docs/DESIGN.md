# Lumen design system and key flows

**Status:** v1, M3 slice 2 (2026-10-07). Code: `crates/lumen-app/src/theme.rs`
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
  They draw their own title row (title on the left, ✕ on the right; Esc
  closes) on one flat surface, rather than egui's tinted title bar.
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
  same palette in both themes (content keeps its identity).

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
  - the waveform drawn bright in the track colour (slice 4c).
- **Hints:**
  - a soloed track's lane gets a 6% `accent` wash;
  - clips on tracks that cannot be heard (muted, or not soloed while
    another track is) are drawn at 42%, with their names in
    `text_secondary` so they stay readable in both themes.

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
