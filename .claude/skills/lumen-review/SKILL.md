---
name: lumen-review
description: Full repository review of Lumen against its standards. Use at the end of every milestone (before the milestone report), or when the owner asks for an audit or review of the codebase.
---

# Lumen repository review

The owner wants a full review at the end of every milestone, so problems
are fixed before the next one builds on them (owner decision, 2026-10-07).
The first audit, `docs/audits/2026-10-07-architecture.md`, is the model
for the report.

## 1. Load the standards

Load the `lumen-architecture` and `lumen-design` skills, and reread
`CLAUDE.md` §2–§5 and §10. The review measures the code against those, plus
the milestone's plan in `docs/milestones/`.

## 2. Run the gates

- fmt;
- workspace clippy `-D warnings`;
- tests;
- the E2E suite and the real-binary smoke tests (`docs/DEVELOPMENT.md`,
  "End-to-end tests"); also check that every workflow the milestone
  delivered has an E2E scenario;
- the WSL mirror (`clippy.sh`, `perf.sh`);
- the macOS cross-check (see `docs/DEVELOPMENT.md`).

Record anything red.

## 3. Sweep the code

Read every crate, not only what changed. Run these mechanical scans first,
then judge each hit:

| Look for | Why |
|---|---|
| Files over about 800 lines, or that mix concerns | Split before the next milestone grows them |
| `allow(` attributes | Each one still justified? `too_many_arguments` means a parameter struct |
| `expect(` / `unwrap(` outside tests | A failure the user can meet should be an error message |
| `Color32::from_rgb`, `from_rgba`, literal font sizes, `Duration::from_millis` outside `theme`, `fonts` and `motion` | Design tokens only |
| `egui::Window::new` outside `widgets::Dialog` | One dialog component |
| `DragValue` without `update_while_editing(false)` | Typed values apply on Enter |
| `"Lumen"` in user-visible strings | `PRODUCT_NAME` |
| `std::fs`, `File::`, `write`, `remove_file` in UI-thread code | Disk work off the UI thread |
| `doc.apply(` outside `document.rs` | Edits are named, tested `Document` methods |
| `TODO`, `FIXME`, `unimplemented!`, `todo!` | Tracked in PROGRESS, or removed |
| `pub` items used in one module | Narrow the visibility |

Then judge by reading:
- **Real-time paths:** everything the engine and device callbacks reach;
  tests cover them under the allocation guard.
- **Persistence:** format version, migrations, fixtures and settings
  versioning match what is saved.
- **Undo:** every user-visible edit is one step; drags merge.
- **Tests:** logic without unit tests, especially model-to-engine
  translation and editing operations.
- **UX:** each feature shipped this milestone against the UX acceptance
  checklist (CLAUDE.md §5): keyboard-only, discoverable, undoable,
  responsive, feedback, empty and error states, both themes at 100%, 150%
  and 200%, accessible names. Take screenshots (see `lumen-design`).
- **Docs drift:** whether `ARCHITECTURE.md` module tables, `PROGRESS.md`
  statuses, `DESIGN.md` tokens, `MANUAL_TESTS.md`, `KNOWN_ISSUES.md` and
  `CLAUDE.md` §12 match the code.
- **Known issues:** every entry in `docs/KNOWN_ISSUES.md` is still real
  and still has an owner (a slice or milestone); fixed ones are removed.
  Anything the review finds but does not fix goes there.

## 4. Report, then fix

Write `docs/audits/YYYY-MM-DD-<milestone>.md` with:
- **What holds:** a table of standards and their evidence;
- **Findings:**
  - **High:** breaks a stated rule (real-time safety, losing work, undo,
    blocking the UI, keyboard or accessibility requirements);
  - **Medium:** architecture and maintainability;
  - **Low:** consistency and polish.
  Each finding names the file, the rule it breaks and the consequence;
- **Recommended order.**

Show the owner the findings and wait for their go-ahead before fixing.
Fix in small commits, one per group, each passing the gates. Then add a
**Status** table to the report (finding, fix, commit), and update
`ARCHITECTURE.md`, `PROGRESS.md` and `CLAUDE.md` §12.
