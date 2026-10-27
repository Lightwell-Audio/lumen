# M9 slice R4b — Media pool, relink and purge: plan

**Status:** Planned 2026-10-26, branch `m9-s4b-media-pool` (cut from `main`
after PR #69). The owner took every recommendation the same day: M1 A, M2 A,
M3 A, M4 A (§3). The defaults in §4 were taken unobjected. Part 1 starts next.

**Goal (M9.md §5, CLAUDE.md §6.10, §7 Browser and media / Project management).**
A project whose audio has moved can be repaired in a few clicks, the owner can
see every file the project holds and which ones nothing plays, and unused files
and Lumen's leftover render and recording files can be cleared away without any
risk to work.

**Not in R4b:** the Samples and Projects tabs of the browser and audition
(R7), the Export window (R5), the peak cache (R6), a trash or restore view of
our own, relinking plugins (the plugin missing state already has Rescan).

## 1. What the code gives us

| Area | Fact (pointer) | Consequence |
|---|---|---|
| Finding media | `lumen_project::locate_media` tries the stored path (relative to the project folder), then `Media.original_path`. A miss is `MediaLocation::Missing { tried }` and `files::open_now` collects the names in `Opened.missing`. | Relink extends this: the search for a moved file is new, the data it needs is already saved (name, `size_bytes`, `frames`, `channels`, `sample_rate`). |
| What the user sees today | One toast on open (`projects.rs` "N audio files are missing, first …"), and a clip chip "file missing" (`ClipState::Missing`, `arrangement/clips.rs`). No way to act on either. | The toast becomes a banner with the action; the chip gets a context-menu entry. |
| The app's file map | `Opened.media_files: HashMap<Id, PathBuf>` is app state; a missing id has no entry. `Library::import_as` loads a file as its media id and ignores an id it already holds. Save writes each media path from that map (relative when inside the project folder). | Relinking a file is: put the path in the map, `import_as` it, and the next Save writes the new path. The project model does not change, so it is **not an undo step** (the R4 rule: where files are, not what the song is). |
| What uses a file | A media id is used by `ClipContent::Audio.media`, by `Takes` passes of a clip (`Clip.takes`, R3), and by `Track.frozen.media` (E5). Reversed twins are derived, not saved. Delete leaves the `Media` entry in `Project.media` (`document/clips.rs`: "the media pool, M9"). | "Used" is a function of the whole project, computed from those two places, with a test that fails when a new place is added without it. |
| Lumen's own folders | Renders go to `Renders` and recordings to `Recordings`, both beside the autosaves (or the system temp folder); Save copies the used ones into `Samples/Rendered` and `Samples/Recorded`. Unused ones stay (KNOWN_ISSUES 2026-10-24 M7b E3; 2026-10-25 M9 R2: discarding a recovered autosave leaves its unfinished takes). | The pool lists them as files not in the project, and clearing them is an explicit action with the size shown. Recordings are the owner's audio: they are never deleted without being asked and are moved to the system's trash, not erased. |
| Browser | Only the Devices tab exists (`browser/panel.rs`); Samples and Projects tabs come with R7. | See question M1. |
| Format | `Media` already holds everything relink needs. | **No format change** unless M3 B is chosen. |
| Dependencies | No trash support in the workspace. | Question M3 decides; the only candidate is the `trash` crate (MIT), flagged for the owner's licence check. |

## 2. Design

### 2.1 The pool

Every `Project.media` entry is a row: name, where it lives (Project, Imported,
Recorded, Rendered or Outside), duration, rate and channels, size, how many
clips play it, and a state: **Ok**, **Missing**, **Unused**. Sorted by name; a
filter chip for Missing and Unused; search by name. Rows reveal the file in the
system's file manager (the entry is hidden when the file is missing).

Selecting a row highlights its clips on the arrangement (one tap shows where it
is used; a second jumps the view to the first). All of it reads the project and
the library; nothing is stored.

### 2.2 Relink

- **Locate…** on a missing row (and **Relink…** in the right-click menu of a
  missing clip, and in the pool): a file dialog on its own thread (as in R4).
  The chosen file is checked against what the project knows: channels and sample
  rate must match; if the length differs by more than a frame the row says "a
  different length" and the relink waits for a second click, so a wrong take is
  never swapped in by accident.
- **Search a folder…**: pick a folder; a background job walks it (and its
  subfolders) and matches every missing item by file name and size, then by name
  and the decoded format when the size differs. It runs with progress in the
  task area and stops on Esc. Matches are shown **before** anything is applied
  (a list of found / not found / more than one match) with an **Apply** button;
  an item with several matches is not applied and stays missing, listed.
- A relinked file is loaded by the library as usual and its clips sound again at
  once (no reopen). `original_path` is refreshed on the next Save.

### 2.3 Purge

- **Remove unused** removes the unused entries from `Project.media`: a named
  `Document` method, **one undo step** (the entries come back with Undo). The
  files are not touched by this step.
- **Files not in the project** (a section below the table, shown only when
  there are any): files in the project's `Samples` folders that no entry names,
  and the `Renders` and `Recordings` leftovers. A line with the count and total
  size and a **Clear…** button (see M3 for what Clear does).
- A file used by another project is not knowable from here, so a file outside
  the project folder is never in the clear list.

### 2.4 When a project opens with missing media

The toast is replaced by a **banner** (the pattern of the autosave banner):
"3 audio files are missing" with **Relink…** (opens the pool filtered to
Missing) and **Dismiss**. The banner stays until dismissed, relinked or the
project is closed, and it also reappears if the project is reverted.

### 2.5 Commands

At the end of `Command` (and `ALL`), with arms in `availability` and `run`:
`media.pool` (show or hide the pool), `media.relink` (Locate on the selected
or first missing item), `media.search_folder`, `media.remove_unused`,
`media.clear_unreferenced`. Tooltips take their keycaps from the keymap; none
has a default key. Each is in the palette.

## 3. Choices for the owner

Answer by label (for example "M1 A, M2 A, M3 A, M4 A").

### M1 — Where the pool lives
- **A (recommended).** A **Project media** tab in the browser, beside Devices,
  where Samples and Projects join it in R7. It is where files are looked for
  already, it shares the search field and the width, and the row for a missing
  file is next to the rest of the browser's actions. Relink opens in the same
  pane.
- **B.** A **floating window** (palette and app menu). More room for the
  columns and the relink report; one more window, off the main surface.
- **C.** A tab in the **detail panel** beside the mixer. Wide rows, but it
  takes the panel from the device chain and clip editors.

### M2 — How relink finds a file
- **A (recommended).** Locate one file **and** Search a folder, as in §2.2:
  matches shown before they are applied, ambiguous ones left alone, a wrong-length
  file needs a second click.
- **B.** Locate one file only. Smallest; a moved project folder means one file
  dialog per file.
- **C.** A search that applies at once, without the preview. Fewest clicks; a
  wrong match is possible, and it is undone only by relinking again.

### M3 — What purge does to files
- **A (recommended).** Remove unused removes **entries only** (undoable). The
  leftovers section's **Clear…** moves the files to the **system trash** (never
  erases them), after a quiet second click in the pane, never a modal. Adds the
  `trash` crate (MIT) for the Recycle Bin, Finder and freedesktop trash; its
  licence is checked when the slice starts (X2 and ASIO already wait on the
  owner).
- **B.** Entries only. No file is ever removed by Lumen: **Show in folder** and
  the file manager do the rest. No new dependency; leftovers accumulate until
  the owner clears them.
- **C.** Clear erases files for good. Not recommended: not reversible, against
  priority 1.

### M4 — The open-time warning
- **A (recommended).** A banner with **Relink…** (§2.4).
- **B.** Keep the toast and add the pool as the only way in.

## 4. Open (defaults taken unless the owner objects)

1. **Relink is not an undo step** (§1); **Remove unused is.**
2. **Used** means a clip, a take pass of a clip or a frozen track plays it. A
   media entry held only by a clip in the undo history is "unused" now and the
   removal is the undo step; redoing the clip's delete works as before.
3. **Search a folder** follows no shortcuts or symbolic links outside the chosen
   folder, and stops at 200,000 entries with a message.
4. **Unfinished recordings** are listed with the time of the take and the
   session's name when the sidecar says it; they are cleared only with the
   rest, never on a timer.
5. **Renders and Recordings are not cleared automatically**, at startup or at
   any age (M9 R2's rule: they are the owner's audio).

## 5. Parts (commits on the branch)

1. **Model and scan:** `used_media` (clips, take passes, frozen tracks) with a
   test that lists the places and fails if the model gains another; the file-state
   scan; no UI. Tests: each use alone; a deleted clip leaves its media unused;
   undo restores it.
2. **Relink:** the map update, the check against `Media`, the library load, the
   folder search worker with progress and stop. Tests: a project folder moved
   with its audio relinks in one search; ambiguous matches are listed; a wrong
   channel count is refused; Esc leaves everything as it was.
3. **Purge:** `Document::remove_unused_media` (one undo step), the leftovers scan,
   the clear (per M3). Tests: undo; the clear list never holds a file outside the
   project's `Samples` folders and Lumen's two work folders.
4. **The pool pane** (per M1): rows, states, filters, search, the leftovers
   section, the missing-clip menu entry, the banner (per M4); commands, menu and
   palette entries; tooltips. Shots at 100, 150 and 200% in both themes.
5. **E2E, MT-PROJ-2, docs:** scenarios by names and keys (open a project whose
   audio was moved, relink by folder, see the clips sound; remove unused and
   undo; clear leftovers); ADR-0106; PROGRESS (the Browser and media and Project
   management rows), ARCHITECTURE, `docs/manual-tests/M9.md`, KNOWN_ISSUES (the
   two R2 and E3 rows move to done), `CLAUDE.md` §12.

## 6. Gates and honesty

The usual gates (fmt, clippy `-D warnings`, workspace tests, E2E, smoke,
macOS cross-clippy). No format change and no real-time code, so the WSL perf
gate is expected to be unchanged; it is run if the model crate is touched.

Verified here: the scan, the matching, the relink on the fake device, purge and
its undo, the trash call against a temporary folder on Windows and in the
Linux mirror. **Not verifiable here:** the real file dialogs (tests answer
them), the system trash on macOS (cross-clippy only) and a large library on a
slow or network drive; those are MT-PROJ-2 on the owner's machine.

## 7. Risks

1. **A false match** relinks the wrong take. Mitigated by the size and format
   check, the preview before apply (M2 A) and the second click for a different
   length.
2. **"Unused" is wrong** if a place that plays a file is missed. Mitigated by
   the test in part 1 that names the places, and by the removal being undoable.
3. **The trash call** behaves differently per platform and can fail on a
   network drive; a failure leaves the file where it is and says why.
4. **A large folder search** on a slow drive: runs on a worker, shows progress,
   stops on Esc and caps its entries.
