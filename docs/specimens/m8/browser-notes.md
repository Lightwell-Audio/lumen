# The browser's built-ins (OR-1, option A): specimens and build notes

Pitch for the owner's M8 review finding OR-1, chosen as option A on 2026-11-14
(`docs/milestones/M8.md` §10). Nothing here is built and no Rust file was
changed. The pictures are in `browser.html` (self-contained; the toolbar has
Theme, Scale 100/150/200 %, the direction whose states are shown, and Reduce
motion; query `?theme=light&scale=1.5&dir=A`) with two stills,
`browser-dark.png` and `browser-light.png` (100 %, direction C). The
plugin rows are anonymous labels (RP-n), per the naming rule.

Contents: 1 The three directions and the recommendation · 2 What the code does
today · 3 The build, piece by piece · 4 Keyboard flow · 5 Accessible names ·
6 Favorites and recents · 7 The removed menu rows · 8 Commands that change ·
9 Tests the build should add · 10 Questions for the owner · 11 Not done, risks

---

## 1. The three directions, and the recommendation

All three keep what OR-1 A asks for: a **Lumen** one-click filter at the top, the
built-ins listed first and grouped by kind (Instruments, Dynamics, EQ, Time,
Utility), drag to a track or chain, double-click to add to the selected track.
They differ in how a built-in row looks.

| | A · Shelf | B · Index | C · Tiles |
|---|---|---|---|
| Filter | Lumen chip, first in the chip row | A source switch, All · Lumen · Plugins | Lumen chip (as A) |
| Row | 44 pt, two lines: name and kind tag, then the description; a 3 pt personality bar | 28 pt (today's row): an 8 pt colour dot, the name, the kind in grey; the description is the tooltip and the info line | 48 pt, as A with a 36 pt still of the device's face in place of the bar |
| Groups | Foldable kind headers (as vendor headers) | Thin non-foldable dividers | Foldable kind headers |
| Shelf height (7 devices) | about 440 pt | about 300 pt | about 466 pt |
| Cost | none beyond the data | none | seven still painters (reduced faces) |
| OR-1 A's "one-line description on the row" | yes | **no** (tooltip) | yes |

**Recommended: C, Tiles.** It is the only one that gives each device a face to
recognise before it is added, drawn from the same pictures the cards use, so
the shelf teaches what the card will look like (the owner asked for personality
over generic choices). It keeps the whole OR-1 list. It costs seven small still
painters (static at rest; they move on hover only, never at rest, and not at
all with Reduce motion). Below 232 pt of pane width the tile gives way to A's
3 pt bar, so A is its fallback, and C can ship as A first and gain the tiles
later with one row-height change. B is the cheapest, but it moves the
description off the row, which the owner's spec asks for on it.

Common to all three, found while drawing them:

- **A built-in's name does not say what it is** (Gravity, Horizon, Prism), so
  the **kind is always visible** on the row (a small outlined tag, as plugin
  rows show their category, but not only on hover). It is also what a search
  for "comp" finds Gravity by.
- **Lumen is a separate source filter, not one of the one-at-a-time chips.** It
  composes with the type switch (All/Instruments/Effects), a category chip and
  the search. In B it is the middle of a three-way source switch.
- Sections: a label row **Lumen** (with seven colour dots) over the shelf, and
  **Plugins** (with a count) over the vendor groups. They are labels, not
  folds.
- The narrow pane (220 pt, `BROWSER_MIN`) cuts descriptions with "…"; the
  tooltip carries the whole line.

## 2. What the code does today (so the build knows its start)

- The catalog lists **only** `builtin:synth` (`Source::Builtin`, vendor "Lumen",
  name "Synth"): `lumen_plugin_catalog::catalog::builtin_devices`. The six
  effects are **not in the catalog**; they are added from the track menu, the
  chain tail's menu and the palette (`Command::AddUtility`, `AddEq`, `AddDelay`,
  `AddReverb`, `AddCompressor`, `AddLimiter`). `chain::actions::plugin_ref`
  refuses every `Source::Builtin` id but the synth.
- `browser::rows` already sorts a vendor that has a built-in first, so "Lumen"
  is today a vendor header above the plugins.
- A device row's double-click and Enter call `App::choose` →
  `add_listed_device`, which places by the selected track, as the menus do.
- Identity (name, kind, colour) is `builtin::identity`; the descriptions
  below are new data.

## 3. The build, piece by piece

1. **Catalog.** `builtin_devices()` returns all seven. Ids: Flare keeps
   **`builtin:synth`** (it is in people's saved favorites and recents; only the
   shown name changes, "Synth" → "Flare"); the six effects use
   `builtin:<the project's built-in name>` (`utility::NAME`, `eq::NAME`, …).
   Fields: `kind` (Instrument/Effect), `categories` = `[group]`, plus two new
   ones on `Device`: `group` (Instruments, Dynamics, EQ, Time, Utility) and
   `blurb` (the one-line description). Better as data in `builtin::INSTRUMENTS`
   and `IDENTITIES` (`Identity` gains `group`, `blurb`, `tip`) so the catalog
   builds from the one table. `plugin_ref` maps each id to
   `PluginRef::Builtin { name }`.
2. **View.** `View.lumen_only: bool` (not persisted, as the chip is not);
   `is_filtering` includes it; `clear_filters` clears it. Safe mode: plugins
   are not listed; Lumen's devices still are (they are not plugins).
3. **Rows.** `Row::Section { name, count }` (the Lumen and Plugins labels) and
   `Row::Group` for a built-in kind header, with its own fold key
   (`lumen:<group>`, so it can never collide with a vendor called
   "Instruments"; folds are stored in the same set). A built-in device row is a
   `Row::Device` that carries the built-in's look. Groups with no match are not
   listed; **Analysis** (B6's old list) has no member yet and shows no header.
   Built-in order inside a group: by name. Group order: Instruments, Dynamics,
   EQ, Time, Utility.
4. **Search.** Matches a built-in's name, kind, group, description and
   instrument/effect, and a plugin's name, vendor and category, as today. A
   match is marked (`accent_muted` behind the matched letters). This highlight is
   new for plugin rows as well.
5. **List painting.** `list::draw_device` gains the built-in look: a personality
   bar (A) or tile (C) at the left, a kind tag always shown, the second line, the
   star on hover. Heights: built-in row 44 (A) or 48 (C), header 26, section 24,
   plugin row 28 (as today). `list::height` stays a pure function so only rows in
   view are laid out (rule: draw only what is in view).
6. **Tiles (C).** `browser/tiles.rs`: seven functions drawing a 36 pt still on the
   device's deep screen (`theme::personality::*_DEEP`, `HORIZON_SKY`, …), the same
   in both themes. Cached as one texture or drawn as shapes; at rest they request
   no repaint. Hover plays at most 1.6 s of the face's motion, from
   `design/motion.rs`'s vocabulary (round 4), and nothing with Reduce motion.
7. **Tooltip and info line.** The tooltip: `Name · Kind`, the `tip` sentence, and
   "Double-click to add · drag to a track". The info line (`browser_info`) is
   unchanged in form; it names the target ("add to Bass").
8. **Drag.** `chain::Dragged(id)` already carries a listed device; its ghost is
   `paint_drag_ghost`. The ghost gains the personality dot. Dropping on a track
   adds after its last card; in an empty area makes a new audio track (or a MIDI
   track for Flare), as the spec's acceptance says.
9. **Chain panel.** A dashed **Add device…** slot after the last card (and on the
   cable when the chain is empty) that opens the browser (below); the tail's
   right-click menu is the same one row. The empty state keeps its words and its
   Browse devices button.
10. **Menus.** See 7.
11. **Presets.** Flare's five become factory presets in the card's Preset menu
    (**Factory** heading, then **Your presets**), as values over the table's
    defaults (B6's tables; the B4 presets already are). The popover is the
    existing one.

One-line descriptions (data; checked for the product-neutral rule: no
third-party names), at most 34 characters so the 260 pt row shows them whole:

| Device | Kind | Group | Description (row) | Tip (tooltip) |
|---|---|---|---|---|
| Flare | Polysynth | Instruments | Two oscillators, filter, 16 voices | A warm polysynth: two oscillators, a filter and up to 16 voices. Five factory presets. |
| Gravity | Compressor | Dynamics | Pulls peaks down; sidechain-ready | A compressor that pulls peaks into a well. Sidechain from any track. |
| Horizon | Limiter | Dynamics | A ceiling the peaks cannot pass | A limiter: the ceiling is the horizon. Look-ahead, with its latency reported. |
| Iris | Parametric EQ | EQ | Eight bands and a live analyzer | Eight bands (cuts, shelves, bells, notches) with a live analyzer. |
| Parallax | Stereo delay | Time | Stereo echoes in notes or ms | A stereo delay in notes that follow the tempo, or in milliseconds. Ping-pong. |
| Afterglow | Reverb | Time | A tail that lingers and fades | A reverb: pre-delay, size, and a decay the tail really takes. |
| Prism | Utility | Utility | Gain, pan, width and polarity | Gain, pan, width, mono below a frequency, polarity, swap and a DC block. |

Shutter (the sampler, B5) joins **Instruments** when it lands; nothing in the
layout changes.

## 4. Keyboard flow

The list is **one Tab stop, the pane's own** (F6), and the arrows move through
it, as today (`browser_keys`); built-in rows and headers join the same list.

- **Ctrl+F** (`devices.search`, today's command) opens the browser on Devices and
  puts the keyboard in the search field. This is also what the **Add device…**
  rows do.
- Typing filters live (as today). **Down** in the search field hands the keyboard
  to the list at its first row (new: today only Esc leaves the field). **Esc**
  clears the words; with none, it gives the keyboard back to the pane (as today).
- **Up / Down / Home / End** move through headers and rows, skipping the section
  labels (they are not stops). **Left** on a group header folds it, and on a
  device goes up to its group header; **Right** unfolds or goes to its first row
  (as vendors today).
- **Enter** on a device adds it to the target (the selected track, after the
  chosen card; or a new track when none fits), as `devices.add` does. On a header
  Enter folds or unfolds it.
- **Focus after adding:** the keyboard **stays in the list on the same row**, so
  Enter twice adds twice and Down + Enter adds the next device. The new card is
  selected in the chain (as `add_device_at` does) and the info line says what
  happened ("Added Gravity to Bass"). F6 moves to the chain.
- **F** stars the selected device (`devices.favorite`, unchanged). The Lumen
  filter has a command (below), so it is reachable without the mouse; the chip
  itself is a toggle in the chips' own Tab order.
- Reduce motion: nothing moves at rest or on hover; the lit chip, selection and
  focus ring still show state (never colour alone: the chip is also filled, the
  row has the selection fill, the focus ring is 2 pt).

## 5. Accessible names

| Part | Name |
|---|---|
| Built-in row | "Gravity, Lumen compressor, effect. Pulls peaks down; sidechain-ready" (selectable label, selected state) |
| Plugin row | as today: "RP-1 Bus Compressor, Vendor A, effect" |
| Group header | "Dynamics, 2 devices, open" (as vendor headers: "…, folded") |
| Section label | heading "Lumen devices, 7" and "Plugins, 9" (not a stop) |
| Lumen chip | "Lumen devices only" (toggle, pressed state); in B, a three-way "Source: All, Lumen, Plugins" |
| Tile | decorative (no name: the row's name says it) |
| Star | "Favorite Gravity" (as today) |
| Tooltip | the same words as above, once |
| Dashed slot | "Add a device to the chain" (the tail's current name) |
| Menu rows | "Add device…, Ctrl+F" |
| Empty state | "No devices match", with its Clear filters button |
| Safe mode note | "Plugins are off for this run" (the scan line says it too) |

## 6. Favorites and recents include built-ins

- Favorites and Recents already store catalog ids, so **no new storage**. Flare is
  `builtin:synth` (unchanged, so existing lists keep working); the effects add
  ids as they are chosen. An id the catalog does not know (a build without a
  device) is skipped, never errors.
- **Favorites chip** lists starred built-ins first (the Lumen section), then
  starred plugins. **Recent** is one flat list, most recent first, built-in and
  plugin mixed; a built-in row there keeps its kind tag and shows no vendor caption
  (its tag says it), a plugin shows its vendor as today.
- Choosing a built-in from anywhere (the browser, a menu that remains, the palette)
  `touch`es its id, so the palette's "Add Gravity" also puts it in Recent. That is
  new for the palette path: today the effect commands do not record a recent.
- The star shows on hover and on the selected row, always on a favorite (as today).

## 7. The removed menu rows, and what replaces each

| Removed | Where it was | Replaced by |
|---|---|---|
| Add Prism (utility) | Track menu; chain tail menu | **Add device…** (opens the browser on Devices with the search focused, the clicked track the target); in the palette `devices.add_utility` stays |
| Add Parallax (delay) | same | same; `devices.add_delay` stays |
| Add Afterglow (reverb) | same | same; `devices.add_reverb` stays |
| Add Iris (EQ) | same | same; `devices.add_eq` stays |
| Add Gravity (compressor) | same | same; `devices.add_compressor` stays |
| Add Horizon (limiter) | same | same; `devices.add_limiter` stays |
| Instrument ▸ Poly, Keys, Bass, Pluck, Pad | MIDI track menu | **Flare's Preset menu** (Factory heading), plus Previous/Next preset in the card menu. A track's menu no longer has an instrument picker; the sound of an existing track is not touched |
| Instrument ▸ None | MIDI track menu | **Remove device** on the card (`devices.remove`, undoable, as for any device) |
| (a new instrument on a MIDI track with none) | the Instrument submenu | the browser (drag or double-click **Flare**), or the palette's new `devices.add_synth` |

The track menu's **Add device…** sits where the six Add rows were (after Fold,
before Import). It shows the keycap from the keymap, never written text
(`Ctrl+F` today). Rows that cannot act are left out (rule): a track that cannot
hold a device (a master with its own limits, if any) does not list it.

## 8. Commands that change (for the parent to plan the build)

Add new commands at the **end** of `Command` and `commands::ALL` (the order
test's last variant moves), with arms in `availability` and `run`; each needs a
page or an `@m8` tag in `site/docs/data/undocumented.txt`.

| Command | Change |
|---|---|
| `devices.search` (`DevicesSearch`, Ctrl+F) | **No new command for Add device…**: the rows call this with the target set first (`self.selected = targets`, as the Add rows did). Its description gains "also the Add device… row". No key change |
| `devices.add_utility`, `add_eq`, `add_delay`, `add_reverb`, `add_compressor`, `add_limiter` | **Stay** (palette and keyboard); their menu rows go; they `touch` the recents |
| `devices.add_synth` (new) | "Add Flare (polysynth)": parity with the six, because the Instrument submenu's job is gone. Puts Flare on the selected MIDI track after the chosen card, or on a new MIDI track |
| `devices.filter_lumen` (new) | "Show only Lumen's devices": toggles the filter, opens the Devices tab. Cover it in `tests/e2e/coverage.rs` |
| `devices.add` (`DevicesAdd`) | Unchanged in behaviour; now also adds a built-in the selection names |
| `Document::set_instrument(…, Some(Preset))` | Stays for the Flare preset menu; its caller moves from the track menu to the preset code (`choose_preset` for a built-in) |
| Menu code | `arrangement/menus.rs`: remove the Add rows and the Instrument submenu, add one row. `chain/view/panel.rs`: the tail's menu is one row; the slot is new. `commands/mod.rs`: names stay |
| Preferences | no new setting; the kind headers' folded state is kept as vendors' is |

No project-format change: devices are already `PluginRef::Builtin { name }`;
the catalog is not saved; settings keep the same ids.

## 9. Tests the build should add

- **E2E** (B6's acceptance, widened): search "comp", double-click Gravity, the
  card appears on the selected track; drag Iris to an empty area makes a track;
  the Lumen chip shows only seven built-ins and Escape/Clear turns it off; a
  built-in favorite and a recent survive a restart; **safe mode still lists all
  seven** (E2E starts safe mode through the `safe_mode` module's existing launch
  option); the track menu has an **Add
  device…** row and none of the old Add rows, and the row opens the Devices tab with
  the search focused; the Flare preset menu lists the five and choosing Bass
  changes the sound (the existing preset fixtures).
- **Pure** (`browser/rows.rs`): group order and fold keys do not collide with
  vendor names; the search matches kind and description; Recent mixes.
- **Coverage** (`testing::coverage`): the two new commands and the Lumen chip.
- **Contrast** (theme tests): the kind tag and description on a selected row (the
  `controls(selected)` rule) in both themes, and the personality bar on both.
- **Shots** at 100/150/200 % in both themes, compared with `browser-*.png`; the
  narrow pane (220 pt) and the 960 × 540 minimum.

## 10. Questions for the owner

1. **Direction:** C Tiles (recommended), A Shelf, or B Index. If C, is a still of
   each face the right size (36 pt), or should it be larger (48 pt) with a
   taller row?
2. **A separate Lumen filter that composes** with the chips (this pitch) rather
   than another one-at-a-time chip. Recommended; the other makes "Lumen
   Dynamics" impossible.
3. **Kind visible on every built-in row** (a tag, not only on hover). Recommended,
   because the names do not say it.
4. **`devices.add_synth`** to keep a keyboard path for what the Instrument
   submenu did. Recommended.
5. **The dashed Add device… slot** in the chain, since a right-click on the tail
   is invisible today. Recommended.

## 11. Not done, and risks

- **Not built; not drawn in egui.** The page is HTML with Inter, which is close to
  but not the same as egui's metrics; a row may break a word a point earlier or
  later. The build's shots are the judge.
- **Short windows.** At the minimum height (540 pt window, a 460 pt pane) the list
  has about 250 pt: only Instruments and part of Dynamics are in view. The kind
  headers fold (and stay folded), and the list scrolls, but a person must fold
  or scroll to find the plugins. A is the least costly here (440 pt), B the most
  forgiving (300 pt). A whole-shelf fold (the **Lumen** label folds) would help
  and is cheap; not drawn here.
- **The drag's drop targets** are drawn for a track and for empty space only;
  dropping on a card (replace) is not offered, as for plugins today.
- **The tiles** are static stills of the faces; the faces themselves are still
  being redesigned (round 4: Iris, Gravity, Horizon, Prism, Flare). If a face
  changes, its tile changes with it; build the tiles **after** the faces are
  chosen, or draw from the same code.
- **Search highlights** are new for plugin rows too; trivial, but a change to a
  row everyone sees.
- **Flare's rename.** The shown name moves from "Synth" to "Flare" in the browser
  and in settings only through the catalog; a person who typed "synth" in the
  search still finds it (kind "Polysynth", description). Project files are not
  touched.
- **Assumptions in the mock-up:** the pane background is `canvas` and its corner
  radius 12 (the real browser is a side panel, not a card); the default pane
  body is 760 pt tall (a 1080p window). Colours and the other sizes are
  `theme.rs`'s; the personality colours and deep screens are
  `theme::personality`.
