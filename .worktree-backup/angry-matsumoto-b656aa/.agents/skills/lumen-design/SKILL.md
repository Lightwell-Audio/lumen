---
name: lumen-design
description: Lumen's design system and UX rules. Use before building or changing any Lumen interface (windows, panels, controls, track headers, clips, menus, colours, type, motion, icons), and when judging or proposing a visual change.
---

# Lumen design

The source of truth is `docs/DESIGN.md`. Its §7a covers tracks and clips;
its §9 is the checklist for new UI. The code that implements it:
- `theme.rs`: palettes, `controls()`, `text_on()`, space, radius and size;
- `motion.rs`: durations, springs, Reduce motion;
- `fonts.rs`: Inter for the interface, Unbounded for titles;
- `widgets.rs`: `Dialog`, `value_slider`, arrows, gradients, glow;
- `icons.rs`.

Product rules from `AGENTS.md` §2 and §5 come first: few steps,
discoverable, keyboard and mouse equal, nothing blocks, everything undoable.

## The owner's taste

- The owner wants **personality, not generic choices**, and judges by eye.
  For any new visual choice (a typeface, an icon set, colours, a layout
  option), **show rendered specimens first** and let the owner pick. Ask
  before downloading assets.
- Choices made so far are recorded in DESIGN.md and in the
  owner-decisions table in `docs/DECISIONS.md`. Don't re-litigate them.

## Rules

1. **Tokens only.** Colours, sizes, radii, spacing and durations come from
   `theme`, `fonts` and `motion`, never literals. A new token is added to
   both palettes and to DESIGN.md §3.
2. **One accent** (blue) for active, selected, playing and focus. Record
   red is the only exception. State is never shown by colour alone; it also
   has a shape, label or position.
3. **Controls follow the surface under them:**
   - colours come from `theme::controls(selected)`. On a selected row the
     plain grey vanishes, so controls step up there;
   - text on a lit fill uses `theme::text_on(fill)`;
   - a test keeps the contrast; extend it for new surfaces.
4. **Every floating window is a `widgets::Dialog`:**
   - fixed width, our own title row (Unbounded title, ✕ at the right
     edge), Esc closes;
   - a question uses `.question()` and is answered by its buttons;
   - content wraps or truncates inside the width and never widens it.
5. **Values are set with `widgets::value_slider`** (the value shown inside;
   drag, Shift for fine, double-click resets, click and type, arrow keys)
   or an equivalent. **Typed values apply on Enter or leaving the field,
   never per keystroke**: for an egui `DragValue`, call
   `.update_while_editing(false)`.
6. **Pixel-perfect alignment:**
   - egui snaps rectangles but not circles or lines;
   - when shapes sit side by side (a round button next to square ones),
     snap their rects together with `round_to_pixels(pixels_per_point)`
     and draw from the snapped rects;
   - keep gaps even.
7. **Motion:**
   - `FAST`, `BASE` and `GLIDE` for transitions; springs for movement;
     `motion::NOTE` for how long a note stays;
   - repaint only while something moves. The armed ring is the one
     deliberate loop;
   - with Reduce motion on, everything still works, with nothing moving.
8. **Accessibility:**
   - every interactive part has an accessible name (`widget_info`);
   - every core flow works from the keyboard alone;
   - focus shows a 2 pt accent ring.
9. **Layout:**
   - nothing overlaps at the 960 × 540 minimum window;
   - text in a header or field is shortened with "…";
   - windows open top-centre;
   - popovers open under the control that opened them.
10. **Designed states:** empty, loading, error and warning states say what
    happened and what to do (Dismiss, Recover, Relink...).

## Verifying a UI change

Look at the result yourself before reporting:
- `target/debug/lumen.exe --screenshot out.png`, with optional `--scale 1.5`
  or `2`, `--appearance light`, `--audio-settings`, or a `.lumen` path to
  open (copy `crates/lumen-project/tests/fixtures/v3/golden.lumen` to a
  scratch folder);
- crop and enlarge with PowerShell `System.Drawing` to check alignment
  pixel by pixel;
- check both themes, and 100%, 150% and 200% scale (AGENTS.md §5,
  checklist item 9);
- never kill the owner's running Lumen: use the debug build for
  screenshots.

Visual problems found and not fixed now go in `docs/KNOWN_ISSUES.md`.

Then run through DESIGN.md §9 and the UX acceptance checklist in AGENTS.md
§5, and add the manual steps the owner will check to the slice's MT-*
test.
