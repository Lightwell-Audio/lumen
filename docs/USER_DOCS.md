# Lumen user documentation: the strategy

**Status:** Proposed, 2026-11-05 (ADR-0119). The choices in §12 wait for the
owner. Nothing here is built yet.

This is the plan for the user documentation published on the website: what
it covers, how it is organised and searched, how every page gets its
screenshots, and how it stays current. Slice by slice and milestone by
milestone for now, then version by version once Lumen has versions.

It is separate from the engineering documents in `docs/` (plans, decisions,
manual tests). Those stay as they are and are not published.

---

## 1. What the documentation must do

The owner's requirements (2026-11-05), as testable rules:

1. **Every action in Lumen is documented**: every command, every control on
   the transport bar and in the app menu, every preference, and every direct
   manipulation (drags, handles, double-clicks, modifiers). "Documented" means
   a page says how to do it, with the mouse and with the keyboard.
2. **Every piece of functionality has a screenshot**, and every task page
   has a worked example.
3. **Easy to navigate and search.** Any action can be found from the
   sidebar in three clicks or fewer, and from the search box by its name, by
   its shortcut, or by a common synonym.
4. **A quick start and a recipes section.**
5. **The docs follow the work.** Every slice adds or updates the pages for
   what it delivers, in the same branch. Each milestone closes with its docs
   complete. Once every current milestone is finished and a versioning
   strategy exists, the docs move to one set per version (§10.3).

Two more rules follow from Lumen's own principles:

6. **Only what is built is documented.** No page for a feature that is not
   on `main` (no fake completeness). Known limits from
   `docs/KNOWN_ISSUES.md` are stated on the pages they affect.
7. **The docs are checked by CI, not by memory.** Coverage, screenshots,
   links, shortcuts and product-neutral wording are enforced by tests (§5,
   §8), the way commands and menus already are.

## 2. What exists today

| What | Where | Count (2026-11-05) |
|---|---|---|
| Commands, each with an id, name, area, default keys and a one-line "about" | `crates/lumen-app/src/commands/mod.rs` (`commands::ALL`) | 253, in 11 areas |
| Searchable preference rows, each with an id, page, label and search words | `crates/lumen-app/src/preferences/mod.rs` (`ROWS`) | 45, on 8 pages |
| Transport bar controls and app menu rows, each tied to a command or a reason it has none | `crates/lumen-app/tests/e2e/coverage.rs` | about 60 |
| Direct manipulation (drags, handles, double-clicks, modifiers) | Spread across `docs/DESIGN.md`, the milestone plans and the E2E scenarios | no list |
| A screenshot harness that drives the real app headless on the GPU and writes PNGs, in both themes and at any scale | `crates/lumen-app/tests/shots.rs` (`egui_kittest`, wgpu) | 73 scenes, for design checks |
| The public website: one static page, deployed by GitHub Pages | `site/`, `.github/workflows/pages.yml` | no user docs |

Three of the four kinds of action are already in tables the code owns. That
decides the approach: the reference pages are **generated** from those
tables, and a test checks that each entry is also explained on a
hand-written task page. Gestures get a fourth table, kept by hand (§5.2).

## 3. The shape of the documentation

The sections follow the four kinds of reader need (learn, do, look up,
understand), plus troubleshooting and change notes.

| Section | Reader's question | Kind | Written or generated |
|---|---|---|---|
| **Quick start** | "I just installed it. How do I make something?" | Tutorial, one path | Written |
| **Guides** | "How do I do X?" | Task pages, one task each | Written |
| **Recipes** | "How do I get this result?" | Goal-led workflows across features | Written |
| **Reference** | "What does this command, control or setting do?" | Lookup tables and entries | Generated, with written notes |
| **Concepts** | "Why does it work this way?" | Short explanations | Written |
| **Troubleshooting** | "Something is wrong. What do I do?" | One page per designed error state | Written |
| **What's new** | "What changed?" | Change notes per milestone, later per version | Written per slice |

### 3.1 Site map

```text
/docs/
├── Quick start
│   ├── Install and first launch          (the first-run window, test sound)
│   ├── Your first song in 15 minutes     (one continuous path, below)
│   └── The window at a glance            (annotated map of every area)
├── Guides
│   ├── Transport and timing              (play, loop, tempo, metronome, count-in, locators)
│   ├── Tracks                            (add, arm, monitor, fold, group, reorder)
│   ├── Clips and selection               (move, trim, fades, time ranges, snapping)
│   ├── Editing                           (split, Join, duplicate, delete time, crossfades)
│   ├── Recording audio                   (inputs, monitoring, punch, latency, takes)
│   ├── MIDI and the piano roll           (draw, quantize, controllers, computer keyboard, SMF)
│   ├── Automation and envelopes          (lanes, modes, overrides, clip envelopes)
│   ├── Mixer and routing                 (faders, pan law, sends, returns, solo, meters)
│   ├── Plugins and devices               (scan, device chain, editors, presets, bypass)
│   ├── Browser and media                 (samples, projects, places, preview, media pool)
│   ├── Projects and saving               (save, autosave, recovery, backups, collect)
│   ├── Rendering and export              (bounce, freeze, export, stems, loudness)
│   ├── Audio and MIDI setup              (drivers, devices, buffer, latency, ports)
│   └── Preferences                       (one page per Preferences page)
├── Recipes
├── Reference
│   ├── Commands and shortcuts            (generated from commands::ALL)
│   ├── Preferences                       (generated from preferences::ROWS)
│   ├── Transport bar and menus           (generated from the coverage lists)
│   ├── Mouse and gestures                (generated from the gesture list, §5.2)
│   ├── Interface map                     (every window, panel and card, annotated)
│   ├── Files and folders                 (project format, settings, autosaves, logs)
│   └── Glossary
├── Concepts
├── Troubleshooting
└── What's new
```

Guides follow the areas the app itself uses (the command areas and the
Preferences pages), so the word a reader sees in the app is the word in the
sidebar.

### 3.2 Quick start

One page a new user follows start to finish, about 15 minutes, ending with
an exported file. Each step names the command, shows the shortcut, and has a
screenshot of what the screen looks like after it.

1. Pass the first-run window: choose the device, Play test sound, find the
   plugins.
2. Add a MIDI track with the built-in Keys sound; play it from the computer
   keyboard.
3. Record four bars over a count-in; quantize them.
4. Add an audio track; drag a sample from the browser; trim and fade it.
5. Loop the section, duplicate it, split and Join a clip.
6. Balance the mix: fader, pan, a send to a return with the built-in reverb.
7. Automate a parameter.
8. Save, then export to WAV.

A small starter project (made only of Lumen's own sounds, so it has no
licensing questions) is offered for download for the readers who want to
skip to step 4.

### 3.3 Recipes

A recipe is a goal, not a feature: "Duck the bass under the kick". It lists
what it needs, gives numbered steps across several features, shows the
result (a screenshot, and where it helps a short before/after audio
example), and links to the guide page for each step. Recipes cover only what
`main` can do. The first set:

| Recipe | Draws on |
|---|---|
| Record a vocal with punch-in and a count-in | recording, locators, metronome |
| Measure and set your interface's recording latency | loopback helper, recording offset |
| Tighten a loose MIDI part and keep its feel | quantize strength and swing, velocity lane |
| Duck the bass under the kick | sidechain, compressor, routing |
| Build a parallel reverb on a return | returns, sends, pre/post fader |
| Fade between two takes smoothly | crossfades, fade shapes |
| Turn a synth part into audio to save CPU | freeze, flatten, bounce in place |
| Export stems for someone else to mix | export scope, stems, tails, loudness report |
| Get back work after a crash | autosave, recovery, take recovery |
| Move a project to another computer | collect and save, relink, media pool |
| Play with low latency from a USB interface | driver, buffer size, monitoring |
| Map a hardware knob to any parameter | MIDI learn, mapping list |

The list grows as features land; a recipe is proposed in the slice plan
when a slice makes a new workflow possible.

### 3.4 Reference

Generated pages are always complete, because they are built from the same
tables the app runs on. Each **command entry** shows the name, the area,
the default shortcut on Windows and on macOS, the one-line "about" text from
the table, where it is reached (palette, menu, context menu, bar), and a
link to the guide page that explains it with a screenshot. The table's
stable `id` is the entry's anchor (`/docs/reference/commands/#edit.split`),
so links never break when a command is renamed.

The written part of the reference is the **interface map**: one annotated
screenshot per window, panel and card, with every control numbered and
named by its accessible name.

### 3.5 Concepts and troubleshooting

Concepts are short pages for the ideas a reader needs to predict Lumen:
the arrangement as the whole product; clip selection versus time-range
selection; what one undo step is; how solo keeps return and group paths;
delay compensation; how autosave, backups and take recovery protect work;
plugin isolation and safe mode.

Troubleshooting has one page per **designed error or warning state**:
audio device lost, driver busy, plugin crashed, plugin blocked, missing
media, a recovered take, a project from a newer version. Each shows the
state as Lumen draws it, says what it means, and gives the way out.

## 4. Page types

Every page is a Markdown file with front matter. The front matter is what
the build and the tests read.

### 4.1 A task page (Guides)

```markdown
+++
title = "Split a clip"
section = "editing"
also = ["cut a clip", "slice", "razor"]        # synonyms, shown and searched
actions = ["edit.split", "gesture.split_tool_click"]   # what this page covers
shots = ["split-before", "split-after", "split-range"]
+++

Split cuts a clip in two at the playhead, or at both edges of a time range.
The two halves play exactly as the whole did.

{{ shot(id="split-before", alt="An audio clip selected, the playhead across its middle") }}

## With the keyboard
1. Select the clip.
2. Move the playhead to where the cut goes.
3. Press {{ keys(id="edit.split") }}.

## With the mouse
...

## On a time range
...

## Good to know
- Split never clicks: each new edge gets a short fade.
- {{ keys(id="edit.undo") }} puts the clip back in one step.

Related: [Join clips](...), [Time-range selection](...)
```

The rules for a task page:

- **One task per page**, with a title that is an action ("Split a clip").
- **The mouse way and the keyboard way**, both, as the UX checklist asks of
  the app itself.
- **Keys are never written by hand.** `{{ keys(id=…) }}` reads the default
  keymap from the generated data, as menus and tooltips do in the app. A
  changed default changes every page.
- **At least one screenshot**, and one of the result.
- **A worked example**: concrete values ("trim the clip to bar 9"), not
  placeholders.
- **"Good to know"** for limits, undo behaviour and known issues.

### 4.2 Recipe, reference and concept pages

- **Recipe:** goal, what you need, steps, the result, related guides.
- **Reference entry:** generated; a written note can be added beside an
  entry in a small notes file keyed by id.
- **Concept:** an explanation with at most one diagram or screenshot, and
  links to the tasks that depend on it.

## 5. Covering every action

### 5.1 One inventory, four sources

| Kind | Source | Key |
|---|---|---|
| Command | `commands::ALL` | its `id` (`edit.split`) |
| Preference | `preferences::ROWS` | its id (`audio.driver`) |
| Bar control and menu row | the lists in `tests/e2e/coverage.rs` | its accessible name |
| Gesture | a new `site/docs/data/gestures.toml` (§5.2) | `gesture.<name>` |

A small exporter writes the first three to JSON for the site build
(`commands.json`, `preferences.json`, `controls.json`). It runs as an
ignored test target in `lumen-app` behind an environment variable, the way
`shots` does, so nothing is added to the shipped binary.

### 5.2 Gestures

Direct manipulation is not in any table today. The docs add one:
`gestures.toml`, one entry per gesture, with an id, where it happens
(arrangement, piano roll, device card…), the input (drag, Shift+drag,
double-click), what it does, and the E2E scenario that covers it. It is
seeded from `docs/DESIGN.md`, the milestone plans and the E2E scenarios,
and reviewed in every milestone review. This is the one list kept by hand;
the review is what keeps it honest, and a test checks that each entry
names an E2E scenario that exists.

### 5.3 The coverage test, with a ratchet

`docs_coverage` (a test in `lumen-app`) fails when:

- an id from the inventory is not listed in the `actions` of any guide,
  recipe or quick-start page;
- a page lists an id that no longer exists;
- a page has no screenshot, or names a shot that no scene produces.

On the first day almost nothing is documented, so the test starts with a
**baseline file of known gaps** (`site/docs/data/undocumented.txt`). The
rule is that the file **only shrinks**: an id not in the inventory fails,
and an id in it that is now documented must be taken out. So a slice that
adds a command, a setting or a gesture without a page fails CI from the
first day, while the backlog is cleared in the order of §11. The docs are
complete when the file is empty.

## 6. Screenshots and examples

### 6.1 Generated, not captured

Screenshots are rendered by the real app in the existing headless harness,
from scripted **scenes**, so they are exact, identical in both themes, and
redrawn whenever the interface changes. A new target,
`crates/lumen-app/tests/docs_shots.rs`, holds one function per scene, named
after the shot id the page uses.

- **A scene drives the app the way the page tells the reader to**: it opens
  the demo project, selects, presses the shortcut, drags. A scene that
  cannot reach its state fails, so a page whose steps no longer work is
  caught when the docs are built.
- **Same content everywhere.** Scenes start from one short demo song built
  from Lumen's own synth and test sounds, with musical names, so the
  pictures look like music rather than test fixtures. No third-party
  samples, plugins or plugin windows appear; plugin windows use the test
  plugin under an anonymous name.
- **Fake devices only**, as the E2E rules require: `test:fake` audio,
  virtual MIDI ports, never the machine's folders.
- **Cropped by accessible name.** A scene names the control or panel the
  shot is about, and the harness crops to that node's bounds plus a margin.
  A layout change moves the crop with the control.
- **Callouts are drawn by the page, not into the image.** The harness
  writes a small JSON file beside each shot with the bounds of the named
  controls; the page draws numbered markers over the image at those places.
  The markers stay sharp, follow the control, and can be restyled without
  rendering again.

### 6.2 Format and sizes

- Rendered at 2× from a 1280 × 800 point window, saved as lossless WebP.
- **Both themes for every shot.** The page shows the one that matches the
  reader's theme (`<picture>` with `prefers-color-scheme`) and lets the
  reader switch.
- **Short loops for gestures**: drags and handle moves are rendered as
  frames by the same harness and encoded to a muted MP4 of six seconds or
  less, with a still as the poster. They play when clicked, and the still
  is all that shows when the reader asks for reduced motion.
- **Alt text is required**: the `shot` shortcode will not build without
  it.

Expected size at completion: about 200 pages, 400 to 600 shots in two
themes, roughly 80 to 120 MB of images. GitHub Pages allows 1 GB.

### 6.3 What cannot be generated

Operating system dialogs (the installer, the file association prompt, a
driver's own control panel) and real hardware are outside the harness.
These few images are captured by hand into `site/docs/static/captured/`,
each with the date, the build and the platform in a sidecar file. The
milestone review lists any older than the last milestone.

### 6.4 Audio examples

Where a picture cannot show the difference (fade shapes, pan laws, quantize
strength, a ducked bass), a recipe or guide can carry a short before/after
audio example rendered offline by Lumen's own export from a fixture
project, so it is exact and repeatable. Used sparingly; about a dozen in
total.

## 7. Navigation and search

### 7.1 Navigation

- A **sidebar** with the sections of §3.1, folded to the current section;
  a **breadcrumb**; an **"On this page"** list of headings; **previous and
  next** links through a section.
- **Landing cards** on `/docs/`: Quick start, the three most-read guides,
  Recipes, Commands and shortcuts, Troubleshooting.
- **Every page answers "Related"**: the concept behind it, the reference
  entries it uses, and the recipes that use it.
- **The interface map links everywhere.** Each numbered control on an
  annotated shot links to the page for that control.
- **Phone width works**: the sidebar folds into a menu; screenshots scale
  and stay legible to the crop.

### 7.2 Search

- **Client-side search** over the built pages (Pagefind, §8), so it works
  on GitHub Pages with no server.
- **Ctrl/Cmd+K opens it**, as in Lumen, and "/" too. Results are grouped by
  section and can be filtered by section and area.
- **Search finds shortcuts.** Each command entry carries its keys in text
  ("Ctrl+E"), so typing a shortcut finds what it does.
- **Synonyms are visible.** A page's `also` words are shown under its title
  ("Also called: consolidate, glue, merge") and searched with it. This is
  where the app's chosen words meet the reader's: the users' word for
  consolidate is Join, and a search for "consolidate" finds Join.
- **The preference rows' search words** (the `ROWS` table's fourth column)
  feed the search too, so "sound card" finds the Device setting, as it does
  in the Preferences window.
- **No analytics, no cookies**: search runs in the browser and nothing is
  sent anywhere, in line with Lumen's privacy rule.

## 8. Building and publishing

### 8.1 Where the files live

```text
site/
├── index.html                 the existing landing page (unchanged)
└── docs/                      the documentation project
    ├── config.toml
    ├── content/               the Markdown pages, by section
    ├── templates/             page layouts and the shortcodes (keys, shot, callouts)
    ├── static/                styles, fonts (the site's own), captured shots
    └── data/                  gestures.toml, undocumented.txt, notes; generated JSON at build
```

The docs live in the repository beside the code, so a slice's branch
carries its code and its pages together, and the owner reviews both in one
pull request.

### 8.2 The build

1. Export the reference data (§5.1) and render the scenes (§6).
2. Build the Markdown into HTML with the static site generator (§12, G).
3. Index the HTML for search.
4. Publish `site/` with the docs at `/docs/`, by the existing Pages
   workflow, which gains these steps and runs when `site/**` or the app's
   sources change on `main`.

The docs use the landing page's fonts (Inter, Unbounded, Martian Mono) and
the app's design tokens (`docs/design-system/lumen-tokens.json`): dark by
default, a light theme, one accent colour, the muted track colours for
areas, the same keycap style as the command palette. The look is pitched as
rendered page specimens before it is built (§11, D0), as with every visual
choice.

### 8.3 Checks on every pull request

| Check | Fails when |
|---|---|
| `docs_coverage` | an action has no page, a page names a missing action or shot, or `undocumented.txt` grows (§5.3) |
| Scenes, one theme at 1× | a scene cannot reach its state (fast; the full set renders at deploy) |
| Build | a shortcode is wrong, a shot has no alt text, or a key id is unknown |
| Links | an internal link or anchor is broken |
| Product-neutral names | a page names another product (`tests/third_party_names.rs`, extended to `site/docs/`) |

A pull request that changes the docs can attach the built site as a CI
artifact, so the owner can look at the pages and shots before merging.

## 9. Writing style

- **Product-neutral**, by the owner's rule and the `lumen-documentation`
  skill: no other product is named or compared; behaviour is described
  directly. Technical names (Windows, ASIO, VST3, WAV, FLAC) stay.
- **The app's own words.** Labels are quoted exactly as Lumen draws them,
  in bold: **Join**, **Play test sound**. Where the app's word differs from
  a common one, the common one goes in `also`.
- **Plain, short, imperative steps**: one action per step, numbered; the
  result after the step that causes it.
- **Platform keys**: Windows keys by default, with a switch for macOS
  (Ctrl becomes Cmd) that the site remembers. Never written by hand
  (§4.1).
- **Notifications are quoted in full**, as Lumen shows them; never cut off.
- **The product name**: templates (titles, navigation, banners) take it
  from the site's config. Prose writes it plainly, as the landing page and
  the engineering documents do; a rename is one scripted replace across
  `content/`.
- English only for 1.0; the folder layout leaves room for
  `/docs/<language>/` later.

## 10. Keeping the docs current

### 10.1 Per slice (now)

Every slice's definition of done gains one line: **its user docs are on the
branch**. In practice:

1. **The slice plan names its docs**: the pages it adds or changes, the
   recipes it makes possible, and the shots it needs. This is a short
   "Docs" list in the slice's section of `docs/milestones/M<n>.md`.
2. **The branch carries them**: pages, scenes, gesture entries, and one
   line in What's new under the milestone.
3. **CI proves it**: `docs_coverage` fails if a new command, setting,
   control or gesture has no page; the scenes fail if a changed interface
   breaks a page's steps.
4. **The slice report says what was documented**, beside what was
   verified.

A bug fix or a chore updates any page it makes wrong, and adds no What's
new line unless users would notice.

### 10.2 Per milestone (now)

- The milestone's What's new section is finished: one paragraph of what
  the milestone gives users, and its slices' lines.
- The milestone review (`lumen-review`) gains a docs pass: read the
  milestone's pages against the app, refresh hand-captured shots, check the
  gesture list against the milestone's E2E scenarios, and check that the
  pages describe only what shipped.
- Until Lumen has versions, the published docs follow `main`. Every page
  carries a line under its title: "Describes Lumen as built on <date>
  (milestone <n>). Lumen has no released version yet."

### 10.3 Per version (later)

The switch happens when both are true: every milestone now planned is
finished, and a versioning strategy is accepted (its own ADR, with M10's
release work). Then:

- **Docs by version**: `/docs/1.0/`, `/docs/1.1/` and so on are built from
  the release tags; `/docs/latest/` is the newest release; `/docs/dev/` is
  `main`, marked as unreleased. Patch releases update their minor version's
  docs and get no set of their own.
- **A version picker** on every page; search is scoped to the version being
  read; older versions carry a banner linking to the same page in the
  latest version.
- **"Added in" and "Changed in" badges** come from a `since` field in the
  front matter.
- **What's new becomes release notes**: the per-slice lines of an
  unreleased version are gathered into that version's notes when it is
  tagged.
- The per-slice rule (§10.1) does not change: slices still document on
  their branch, and their pages land in `dev` until the next release.

Nothing in §10.1 or §10.2 has to be rebuilt for the switch: the same pages
and scenes are built once per tag instead of once for `main`.

## 11. Getting there: the documentation slices

The backlog is everything built in M1 to M9. It is written in seven slices
on `docs/…` branches. They can run alongside milestone 9.5 or before it
(§12, T); once D0 is merged, 9.5's slices follow §10.1.

| Slice | Delivers | Done when |
|---|---|---|
| **D0 Foundations** | Page specimens pitched first; the docs project, theme and search; the data exporter; `keys`, `shot` and callout shortcodes; the scene target with the demo song and three scenes; `docs_coverage` with the full baseline; the extended name tests; the Pages workflow; the generated Commands, Preferences and Controls reference | `/docs/` is live with the full generated reference; CI runs the checks |
| **D1 Quick start** | Install and first launch, Your first song, The window at a glance (the interface map's first shots); the starter project | A new user can follow it to an exported WAV; every step has a shot |
| **D2 Arranging and editing** | Transport, Tracks, Clips and selection, Editing guides; gestures for the arrangement | The Transport, Edit, Clip, Track, View, Grid and Locators areas are out of the baseline |
| **D3 Audio, recording and export** | Recording audio, Rendering and export, Projects and saving, Browser and media | File-area commands and Recording and Files preferences are out of the baseline |
| **D4 MIDI** | MIDI and the piano roll, the computer keyboard, SMF, MIDI learn; piano-roll gestures | The MIDI area is out of the baseline |
| **D5 Mixing and automation** | Mixer and routing, Automation and envelopes; the lane and fader gestures | The Mixer area and the Mixing preferences are out of the baseline |
| **D6 Plugins, setup, troubleshooting** | Plugins and devices, Audio and MIDI setup, Preferences, every Troubleshooting page | The Devices area and the remaining preferences are out of the baseline |
| **D7 Recipes and concepts** | The first recipes (§3.3), Concepts, Glossary; a full read-through | `undocumented.txt` is empty; the docs pass of §10.2 is done |

Each slice closes the usual way: the checks pass, the owner looks at the
built pages in both themes and at phone width, and the slice report says
what is covered.

## 12. Choices for the owner

Each has a recommendation, marked **(rec.)**. Reply with the letters, for
example "G A, S A". (The slices of §11 are D0 to D7; these choices use
other letters so the two do not mix.)

**G. Site generator**

- **A (rec.) Zola, with Pagefind for search.** One binary each, no
  JavaScript toolchain, full control of the templates (so the docs can
  share the landing page's look), data files read by templates (the
  generated reference), and search that scales to hundreds of pages with
  section filters, and later version filters.
- B. mdBook. Built for Rust projects and quickest to start; its built-in
  search and its book layout are harder to make look like Lumen, and it has
  no data files for generated pages without a preprocessor.
- C. A documentation framework on Node (Astro Starlight). The richest
  navigation and versioning out of the box, but it adds a Node toolchain
  and dependency updates to a project that has none.

**S. Screenshots**

- **A (rec.) Rendered from scenes at deploy, not committed.** Always
  current; the repository does not grow; pull requests check the scenes
  cheaply (§8.3).
- B. Rendered from scenes but committed. Shots are visible in a pull
  request's diff, but every interface change rewrites hundreds of binary
  files.
- C. Captured by hand. Simple to start; goes stale within a slice, and
  "a screenshot for everything" becomes a chore no test can keep.

**L. Where the pages live**

- **A (rec.) `site/docs/`, in this repository.** Code and docs share a
  branch and a review; the existing Pages workflow publishes both.
- B. A separate documentation repository. Keeps this one smaller, but the
  per-slice rule then needs two pull requests per slice.

**C. Coverage enforcement**

- **A (rec.) The test with a shrinking baseline (§5.3).** New work is
  documented from the first day; the backlog is visible and only goes
  down.
- B. Review only. No test; the milestone review checks coverage by hand.

**H. Help inside Lumen**

- **A (rec.) Later, as its own slice after D2.** A "Learn more" link in
  the command palette's detail strip and an F1 "Help for this" that opens
  the page for the command or panel at hand, using the stable ids. Pitched
  with specimens when it starts.
- B. Not planned.

**T. When the backlog is written**

- **A (rec.) D0 before milestone 9.5, D1 to D7 alongside it.** 9.5's
  slices then document from their first day, and the backlog shrinks in
  parallel.
- B. All of D0 to D7 before 9.5 starts.
- C. After M9.5, before M8.

## 13. Risks and what cannot be checked

- **Scene upkeep.** Scenes that drive the interface break when it changes.
  That is intended (a broken scene is a stale page), but it costs time in
  slices that change much of the interface. Mitigation: scenes go through
  accessible names and commands, the same way the E2E tests do, never
  through coordinates.
- **Rendering on CI.** The deploy renders on Linux with a software Vulkan
  driver. Text and colour are close to Windows but not identical, and some
  Windows-only panels (the ASIO driver choice) show test devices. To be
  measured in D0: the time for the full set, and a side-by-side with a
  Windows render.
- **Gestures** are the one list kept by hand (§5.2). The milestone review
  is the check; a gesture added without an entry is caught there, not by
  CI.
- **Accuracy of prose.** Tests prove that a page exists, that its shots
  render and that its keys are right; they cannot prove the words are
  right. The owner's look at each docs slice and the milestone docs pass
  are the check.
- **Size of the backlog.** About 250 commands, 45 settings and an unknown
  number of gestures. Eight slices (D0 to D7) is an estimate, revised after
  D2.
