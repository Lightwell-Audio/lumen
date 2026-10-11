# M5 slice 4 — velocity and controller lanes

**Status (2026-10-11):** Approved. The owner took every recommendation:
**V A** stems and heads, **L A** one lane with a switcher, **T A** tools in
the lane header, **R B** smooth 1/64 for Line, Curve and bend (Pencil
follows the grid). MT-MIDI-4 (slice 3) is still open. Specimens: the private artifact
"Lumen Controller Lane Specimens" (live; drag in the lanes).

## Scope (from M5.md, slice 4)

A velocity lane (drag a stem, draw across stems, box-select) and lanes for
CC, pitch bend and pressure, with pencil, line and curve tools. Curves reuse
`lumen_core::curve` (the fades' free curves, ADR-0031). Controller data is
already in the model since format v11 (`ClipEvent`, `EventKind`), so this
slice should need **no format change**. Also from the known issues: the
clip editor does not auto-scroll at its edges while dragging; it may be
handled here with the lanes.

## Choices for the owner

| # | Question | Options | Recommendation |
|---|---|---|---|
| V | How does a velocity read? | A: stems and round heads, one per note start. B: filled bars as wide as their notes. | **A**: legible when dense; keeps the notes' room |
| L | Lane layout | A: one lane at a time with a switcher (Velocity, Mod wheel, Pitch bend, Pressure, "+ CC" menu). B: a stack of lanes with Add and Remove. | **A**: full height for the lane being edited; a stack can follow later |
| T | Drawing tools | A: Pencil, Line and Curve in the lane's own header (keys 1, 2, 3 with the lane focused; Shift with Pencil draws a line). B: modifiers only, no new buttons. | **A**: discoverable (CLAUDE.md §5); separate from the roll's tools |
| R | Events written by a drawn line | A: follow the grid (1/16). B: smooth, every 1/64 note, only repeated values dropped. | **B** for Line, Curve and pitch bend; the Pencil follows the grid when snapping is on |

A drawn line is stored as plain events because MIDI has no ramps; thinning
drops only repeated values, so a ramp never becomes a staircase the user did
not draw.

## Proposed behaviour (not asked, taken from slice 3)

- Every lane gesture is one undo step; Esc cancels; edits preview and commit
  on release; the document is not mutated each frame.
- Lanes share the roll's horizontal view, grid and snapping.
- Lane edits go through named `Document` methods over pure transforms in
  `lumen_project::edit` (as `edit::notes`), tested before the UI exists.
- Playing: events already reach the engine through the clip player (slice 1);
  the lanes need no new engine path, but pitch bend and CC timing get golden
  renders.

## Sub-slices (after the choices)

1. **4a Pure edits:** velocity set / ramp / scale, event add, move, delete,
   range replace for lines and curves, thinning. Unit tests.
2. **4b Lane drawing:** the lane surface (GPU-drawn like the notes), the
   switcher, selection and box select, the three tools, `Document` methods.
3. **4c Playback and records:** E2E draws each lane and plays it back
   (fake MIDI port sees the events), golden renders for bend and CC timing,
   MT-MIDI-5, ADR-0050, PROGRESS, CLAUDE.md §12.

Work is delegated per `.claude/skills/references/delegation.md`: bounded
Haiku subagents for 4a tests and mechanical edits, the parent for design and
integration.
