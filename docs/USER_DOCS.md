# Lumen user documentation: the strategy

**Status:** Accepted, 2026-11-05 (ADR-0119). Owner, 2026-11-05: G A, S A,
L A, C A, H A, T A; videos and animations throughout ("show, don't tell",
§6); and docs that look inviting and are fun to read and learn from,
matching the landing page and the design system (§9). The look was chosen
from specimens at the start of D0: Playbook, with Stage's rays and
shimmering title on the docs home (§9.5). D0 is being built.

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
2. **Show, don't tell.** Every action is shown in motion, and every state
   it reaches is shown as a screenshot, before the page explains it in
   words. Every task page has a worked example.
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
an exported file. It opens with a walkthrough video of the whole path, with
sound, chaptered by step (§6.1). Each step names the command, shows the
shortcut, and has a clip of the step and a screenshot of what the screen
looks like after it.

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
what it needs, opens with a walkthrough video of the whole recipe, gives
numbered steps across several features (each a chapter of the video), shows
the result (a screenshot, and where it helps a short before/after audio
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
stable `id` is the entry's anchor (`/docs/reference/commands/#midi.split_notes`),
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
actions = ["midi.split_notes", "gesture.split_tool_click"]   # what this page covers
clips = ["split-lead", "split-tool-click", "split-range"]
shots = ["split-before", "split-after"]
+++

{{ clip(id="split-lead", alt="The playhead stops in a drum clip; Ctrl+E cuts it in two and the halves play on") }}

Split cuts a clip in two at the playhead, or at both edges of a time range.
The two halves play exactly as the whole did.

## With the keyboard
1. Select the clip.
   {{ shot(id="split-before", alt="An audio clip selected, the playhead across its middle") }}
2. Move the playhead to where the cut goes.
3. Press {{ keys(id="midi.split_notes") }}.
   {{ shot(id="split-after", alt="Two clips where there was one, the second selected") }}

## With the mouse
{{ clip(id="split-tool-click", alt="The Split tool cuts a clip where it is clicked, on the grid") }}
...

## On a time range
...

## Good to know
- Splitting never changes the sound: the halves play back to back exactly
  as the clip did.
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
- **It opens with a lead clip**: the task done once, start to finish, before
  any words (§6.1). Each way of doing it (keyboard, mouse, on a range) can
  have its own clip.
- **A screenshot for every state a step reaches**, and one of the result.
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
| Command | `commands::ALL` | its `id` (`midi.split_notes`) |
| Preference | `preferences::ROWS` | `pref:` and its key (`pref:audio.driver`; a key can equal a command's id) |
| Bar control and menu row that runs no command | `tests/shared/controls.rs` (the E2E coverage lists) | `bar:` or `menu:` and its accessible name (`bar:Tempo`); one that runs a command is documented with it |
| Gesture | `site/docs/data/gestures.toml` (§5.2) | `gesture.<name>` |

A small exporter writes the first three to JSON for the site build
(`commands.json`, `preferences.json`, `controls.json`). It runs as an
ignored test target in `lumen-app` behind an environment variable, the way
`shots` does (`tests/docs_data.rs`, `DOCS_DATA`). A page lists the ids it
explains under `[taxonomies] actions`, which also links each reference
entry to its guides.

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
- a guide page has no lead clip, a gesture has no clip, or the quick start
  or a recipe has no walkthrough;
- a page has no screenshot, or names a clip or shot that no scene produces.

On the first day almost nothing is documented, so the test starts with a
**baseline file of known gaps** (`site/docs/data/undocumented.txt`). The
rule is that the file **only shrinks**: an id not in the inventory fails,
and an id in it that is now documented must be taken out. So a slice that
adds a command, a setting or a gesture without a page fails CI from the
first day, while the backlog is cleared in the order of §11. The docs are
complete when the file is empty.

## 6. Show, don't tell: videos, screenshots and examples

### 6.1 The rule

Lumen is built for direct manipulation, so its documentation shows the
action before it describes it (owner, 2026-11-05). A reader should be able
to learn most tasks from the motion alone, and read the words for the
details.

| Medium | Where | What it shows | Length |
|---|---|---|---|
| **Lead clip** | Top of every guide page | The task done once, start to finish, at human speed | 3 to 10 s, loops |
| **Gesture clip** | Every entry in the gesture list (§5.2), on its guide page and in the Mouse and gestures reference | One drag, handle or modifier and what it changes | 2 to 6 s, loops |
| **Walkthrough video** | The quick start and every recipe | The whole workflow, with sound, chaptered by step | 1 to 4 min |
| **Screenshot** | Each step's resulting state; the interface map | A state to compare against, with numbered callouts | still |
| **Animated diagram** | Concept pages, where a mechanism moves (solo paths, delay compensation, autosave) | The idea, not the interface | short, loops |
| **Before/after audio** | Where the difference is heard (fade shapes, pan laws, quantize strength, a ducked bass) | The result, rendered by Lumen | a few seconds |

How they behave on the page:

- **Loops are silent and play while in view**, stop when scrolled away, and
  carry a pause button (anything that moves for more than five seconds can
  be stopped). The poster is the clip's last frame, the result, so the page
  reads correctly before it plays and when printed.
- **Walkthroughs never play on their own.** They have sound, captions, and
  chapters that match the page's step headings: clicking a step's heading
  seeks the video to it, and the playing chapter is highlighted in the
  steps.
- **Reduced motion is respected**: with the reader's "reduce motion"
  setting, nothing plays on its own; the poster and a play button show
  instead.
- **Every clip and video has a text equivalent**: alt text for loops (the
  build fails without it), captions for walkthroughs, and the written steps
  as the transcript.

### 6.2 Rendered by the real app, not captured

Videos and screenshots come from the same place: the real app, in the
existing headless harness, driven by scripted **scenes**. So they are exact,
identical in both themes, and redrawn whenever the interface changes. A new
target, `crates/lumen-app/tests/docs_media/`, holds one function per
scene; `tests/shared/scenes.rs` lists the clips, shots and walkthroughs
each one makes, and the scene checks it made exactly those.

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
  virtual MIDI ports, never the machine's folders. A walkthrough plays on
  the test device with inputs, whose blocks the recorder lets through only
  while it takes a frame's sound (§6.3).
- **Cropped by accessible name.** A scene names the control or panel it is
  about, and the harness crops to that node's bounds plus a margin (D0's
  scenes crop to the arrangement around the song's clips, found by the
  clips' ids). A layout
  change moves the crop with the control. A clip can pan between two crops
  when the action crosses the window (a drag from the browser to a track).
- **Screenshot callouts are drawn by the page, not into the image.** The
  harness writes a small JSON file beside each shot with the bounds of the
  named controls; the page draws numbered markers over the image at those
  places, so they stay sharp and follow the control.

### 6.3 How a clip is recorded

The harness already steps the app at a fixed 1/60 s. The recorder saves
every other step, 30 frames a second, and adds what a screen recording
would have:

- **A pointer.** The render has no system cursor, so the recorder draws
  one, in the shape Lumen asks for at that moment (egui reports it each
  frame): the resize arrows on a clip's edge, the hand on a fade handle.
- **A click mark and the keys pressed.** A small ring where the pointer
  presses, and the keys of each shortcut shown as keycaps in the corner, in
  the command palette's keycap style, so a reader sees that Ctrl+E was
  pressed and not just that the clip split.
- **Human pacing.** A scene gives the pointer's path and the recorder
  eases it at a hand's speed, holds about 400 ms before and after each
  change, and holds the result for at least a second before the loop
  starts again. These timings live in one place, like `motion.rs` in the
  app.
- **The video's clock.** The fake audio device runs the real engine, and
  during a take it runs a video frame's time for each frame and nothing in
  between: the blocks that 1/30 s holds (12 or 13 of 128 samples), then the
  app draws the frame. The engine answers a click or a key in the same
  frame whichever machine renders it and however slowly, so a scene makes
  the same frames on every run. The meters fall by egui's input clock for
  the same reason (the wall clock in the app, the harness's steps here).
- **Sound for walkthroughs.** The recorder takes what that device played,
  its whole output as a listener hears it (`App::heard` in the test API),
  and lays it under the video, so the song moves on
  exactly as much as is heard. Each walkthrough checks that its first
  sound comes with the first playing frame and that the playhead keeps to
  the video's time, both within two frames, and that every caption shows
  for at least 1.5 s.

Frames are encoded with ffmpeg in CI (a build tool of the deploy, never
shipped with Lumen) to H.264 MP4, which every browser plays, with a WebP
poster. Equal frames give an equal file: each clip is cached by the hash of
its frames, sound and keycaps, and only changed clips are encoded again (a
second run of D0's scenes took every clip from the cache). Rendering the
frames, not encoding them, is most of the time (§6.4), so the cache saves
little time; it keeps the published files unchanged when nothing changed.

### 6.4 Formats and sizes

- **Screenshots:** 2× from a 960 × 420 point window, cut to what the page
  is about (D0's are the arrangement: 1920 × 302 px), lossless WebP.
- **Clips and walkthroughs:** the same window and scale, 30 frames a
  second, H.264 at CRF 24 (walkthroughs with AAC at 128 kb/s), with a WebP
  poster of the last frame. D0 planned 1.5× and 1600 px, the landing
  page's format (`site/assets/video/`), and built 2× instead: a cut of the
  window is narrow and short, so the files stay small (below) and text in
  the clip stays as sharp as in the screenshot beside it.
- **Themes:** every screenshot, lead clip and gesture clip in both themes;
  the page shows the one matching the reader's theme (`<picture>` and
  `<source media="(prefers-color-scheme: …)">`) and lets the reader switch.
  Walkthroughs are in the dark theme only, Lumen's default, to keep their
  size down.
- **Loading:** nothing is fetched until it is near the view
  (`preload="none"` and lazy images), so a long page stays light.

Measured in D0 (a Linux container, 4 cores, lavapipe), for its three
scenes:

| What | Each | Size |
|---|---|---|
| Screenshot (arrangement cut) | 1920 × 302 px | 18 to 20 KB |
| Lead or gesture clip, 5.2 s | 155 frames | 67 to 72 KB, poster 29 to 31 KB |
| Walkthrough, 11 s with sound | 332 frames | 285 KB, poster 30 KB |
| Rendering | | about 0.3 s a frame; 300 s for the three scenes (952 frames, five launches) |
| Checking only (no `DOCS_MEDIA`) | | 56 s for the three scenes |

Projected for the whole set, from those numbers (the counts are §6.4's
earlier estimate; full-window screenshots are larger than D0's cuts):

| What | Count | Size |
|---|---|---|
| Screenshots, two themes | 400 to 600 | 10 to 40 MB |
| Lead and gesture clips, two themes, with posters | about 600 | about 60 MB |
| Walkthroughs of about a minute, dark only | about 15 | about 25 MB |
| **Total** | | **about 100 to 130 MB** |

That is a quarter of the earlier estimate (400 to 600 MB). GitHub Pages
allows a 1 GB site and has a soft limit of 100 GB of traffic a month, so
several versions fit before hosting needs a decision (§10.3, §13). The
time is the problem: about 600 clips of 155 frames and 15 walkthroughs of
1,800 frames come to about 10 hours of rendering on one runner (§13).

### 6.5 What cannot be generated

Operating system dialogs (the installer, the file association prompt, a
driver's own control panel) and real hardware are outside the harness.
These few images are captured by hand into `site/docs/static/captured/`,
each with the date, the build and the platform in a sidecar file. The
milestone review lists any older than the last milestone. There are no
hand-recorded videos.

Animated diagrams on concept pages are not the interface, so they are drawn
by hand as SVG with CSS animation, in the site's tokens for both themes,
and stand still under reduced motion.

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

1. Export the reference data (§5.1), render the scenes whose own source,
   pages or gestures changed since the published site to screenshots and
   frames, encode the changed clips (§6), and take every other scene's
   media from the published site (§13, R).
2. Build the Markdown into HTML with the static site generator (§12, G).
3. Index the HTML for search.
4. Publish `site/` with the docs at `/docs/`, by the existing Pages
   workflow, which gains these steps and runs when `site/**` or the app's
   sources change on `main` (`.github/workflows/pages.yml`, D0; the
   encoded clips are kept in the Actions cache between deploys, outside
   `target/`, which the Rust cache cleans).

The docs share the landing page's stylesheet and fonts, and take their
tokens from `docs/design-system/lumen-tokens.json` at build time (§9.1).
The look is pitched as rendered specimens before it is built (§9.5).

### 8.3 Checks on every pull request

| Check | Fails when |
|---|---|
| `docs_coverage` | an action has no page, a page names a missing action or shot, or marks a control its scene does not record |
| The undocumented list | `undocumented.txt` gains a line against the base branch (§5.3; a step in `docs.yml`) |
| Scenes, once in the dark theme, without rendering | a scene cannot reach its state, or would write or mark other than `scenes.rs` lists (fast; the full set renders and encodes at deploy) |
| Build | a shortcode is wrong, a shot has no alt text, or a key id is unknown |
| Links | an internal link or anchor is broken |
| Product-neutral names | a page names another product (`tests/third_party_names.rs`, extended to `site/docs/`) |

These run in `.github/workflows/docs.yml` (D0) on every pull request that
touches `site/` or the app, since CI skips changes to Markdown alone. The
scenes check runs each scene once in the dark theme and writes nothing
(no `DOCS_MEDIA`); Zola's build checks internal links and their anchors.
The built pages, with their media (§13) and the landing page beside
them, are kept as the run's `docs-site` artifact for the owner to look at
before merging. They are built for `http://localhost:8123/docs`: unzip
the artifact and serve the folder on that port (`site/docs/README.md`).

## 9. Look and voice

The owner (2026-11-05): the docs must **look inviting and be fun to read
and learn from**. Fun has to fit the job each section does, so it lives
mostly in the Quick start, Guides and Recipes. The Reference stays crisp and
dense, because a reader looking something up wants the answer, not a
smile.

### 9.1 The look: the landing page and the design system, one source

- **One stylesheet for the landing page and the docs.** The landing page's
  inline styles move into a shared `site/assets/lumen.css` that both use,
  so the docs and the front page cannot drift apart.
- **Tokens from the design system, not copied.** A Zola template reads
  `docs/design-system/lumen-tokens.json` (colour, type, spacing, radius,
  size, duration, stroke) at build time and writes it out as CSS custom
  properties. A token changed for the app changes the docs on the next
  build.
- **The same type**: Unbounded for page titles and section heads, Inter for
  reading, Martian Mono for keys, values and paths. Dark by default, the
  light theme one click away, one accent colour.
- **Each guide section has a colour**: the track colour the app already
  gives that command area (`Area::light`, exported with the commands; the light under a keycap in the
  palette). Transport pages carry the Transport colour on their sidebar
  mark, keycaps and section card, so a reader learns the app's colours
  while reading.
- **Keycaps that look like the app's**, lit in the area's colour.
- **The landing page's motion**, kept restrained: headings and cards
  reveal as they scroll in; clips play in view; nothing moves under reduced
  motion.
- **Comfortable reading**: a measure of about 70 characters, generous line
  height, large step numbers, clips and shots wider than the text column.

### 9.2 Inviting: the front door and Lumi

- **The docs home asks what you want to make**, not which chapter you want:
  big cards such as "Make your first song", "Record yourself", "Shape a
  beat", "Mix it", "Share it", each with a looping clip, beside a search
  box that says "Search or press Ctrl+K".
- **Lumi is the guide, sparingly.** The mascot already has approved poses
  (`docs/brand/`: waving, curious, focused, celebrating). Lumi welcomes on
  the docs home, holds the "Lumi's tip" boxes (one at most per page),
  celebrates at the end of the quick start and of each recipe, looks
  curious on Troubleshooting pages and on a search with no results. Never on
  Reference pages. New poses are made from
  the approved design, as `docs/brand/lumi-expressions.md` describes, and
  pitched first.
- **Recipe cards** show the result in motion, how long it takes and what
  it needs, so choosing one feels like picking a track to learn.

### 9.3 Fun to learn from

- **"You'll learn" and "About 5 minutes"** at the top of every tutorial and
  recipe, so the reader knows the size of what they start.
- **"Try it" at the end of a guide**: a small challenge in the demo song
  ("split the drum clip at bar 5 and fade the second half in"), with the
  answer as a clip behind a "Show me" button.
- **Progress through the quick start**: steps tick off as the reader
  marks them, remembered in the browser only (no account, nothing sent),
  with Lumi celebrating at the end.
- **Small wins said out loud**: after the step that makes the first sound,
  the page says so ("That's your first sound in Lumen").
- **Hover for a term**: a glossary word is underlined lightly and explains
  itself on hover or focus, so no reader is left behind and no page stops
  to define things.

### 9.4 The voice

Warm, direct and short: a friend who knows the app well and wants you to
get to the music. Second person, present tense, active verbs. A light
touch of wit in introductions and tips, never inside steps and never at
the reader's expense.

> Instead of: "The Split command divides the selected clip at the playhead
> position."
>
> Write: "Split cuts a clip in two right at the playhead. Both halves keep
> playing exactly as before, so nothing you hear changes until you move
> them."

The rules that keep it clear:

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

### 9.5 Chosen from specimens

D0 opens by rendering the docs home and one guide page (with a lead clip
playing, two shots, keys, a tip and "Try it") in three directions, in both
themes, at desktop and phone width:

- **A. Studio**: close to the app's own chrome; quiet, dark, the clips
  large; Lumi only on the home page and in tips.
- **B. Playbook**: warmer and more playful; rounder cards, section colours
  stronger, Lumi in tips, wins and "Try it".
- **C. Stage**: the landing page's big Unbounded headlines and scroll
  reveals carried into the docs; bold section openers.

**Chosen (owner, 2026-11-05): B, Playbook**, with one part of C: the
docs home's hero takes Stage's slowly turning light rays (around Lumi)
and its shimmering gradient title. Guide pages keep Playbook's calmer
style: the sun glow behind the page, round display titles, colour per
area, Lumi in tips and "Try it". The other two directions were removed
from the stylesheet once the choice was made.

## 10. Keeping the docs current

### 10.1 Per slice (now)

Every slice's definition of done gains one line: **its user docs are on the
branch**. In practice:

1. **The slice plan names its docs**: the pages it adds or changes, the
   recipes it makes possible, and the clips and shots it needs. This is a short
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
- **Versions share their media.** Clips and screenshots are stored once,
  under names made from their content's hash, in one folder all versions
  use. A video that did not change between 1.0 and 1.1 is one file, so the
  site grows by what changed, not by a full copy per version.
- **What's new becomes release notes**: the per-slice lines of an
  unreleased version are gathered into that version's notes when it is
  tagged.
- The per-slice rule (§10.1) does not change: slices still document on
  their branch, and their pages land in `dev` until the next release.

Nothing in §10.1 or §10.2 has to be rebuilt for the switch: the same pages
and scenes are built once per tag instead of once for `main`.

## 11. Getting there: the documentation slices

The backlog is everything built in M1 to M9. It is written in eight slices
on `docs/…` branches. They can run alongside milestone 9.5 or before it
(§12, T); once D0 is merged, 9.5's slices follow §10.1.

| Slice | Delivers | Done when |
|---|---|---|
| **D0 Foundations** | Specimens of the docs home and a guide page in three directions, pitched first (§9.5); the landing page's styles moved to a shared stylesheet; tokens generated from the design system; the docs project, theme and search; the data exporter; `keys`, `shot`, `clip`, `video` and callout shortcodes; the scene target with the demo song; the clip recorder (pointer, keycaps, pacing, encoding, cache) and the sound for walkthroughs proven; three scenes as a lead clip, a gesture clip and a screenshot; measured render time and size; `docs_coverage` with the full baseline; the extended name tests; the Pages workflow; the generated Commands, Preferences and Controls reference | `/docs/` is live with the full generated reference; CI runs the checks |
| **D1 Quick start** | Install and first launch, Your first song with its walkthrough video, The window at a glance (the interface map's first shots); the starter project | A new user can follow it to an exported WAV; every step has a clip and a shot |
| **D2 Arranging and editing** | Transport, Tracks, Clips and selection, Editing guides; gestures for the arrangement | The Transport, Edit, Clip, Track, View, Grid and Locators areas are out of the baseline |
| **D3 Audio, recording and export** | Recording audio, Rendering and export, Projects and saving, Browser and media | File-area commands and Recording and Files preferences are out of the baseline |
| **D4 MIDI** | MIDI and the piano roll, the computer keyboard, SMF, MIDI learn; piano-roll gestures | The MIDI area is out of the baseline |
| **D5 Mixing and automation** | Mixer and routing, Automation and envelopes; the lane and fader gestures | The Mixer area and the Mixing preferences are out of the baseline |
| **D6 Plugins, setup, troubleshooting** | Plugins and devices, Audio and MIDI setup, Preferences, every Troubleshooting page | The Devices area and the remaining preferences are out of the baseline |
| **D7 Recipes and concepts** | The first recipes (§3.3) with their walkthroughs, Concepts with their animated diagrams, Glossary; a full read-through | `undocumented.txt` is empty; the docs pass of §10.2 is done |

Each slice closes the usual way: the checks pass, the owner looks at the
built pages in both themes and at phone width, and the slice report says
what is covered.

**D0 report (2026-11-05, branch `docs/d0-foundations`).**

- **Built:**
  - the Playbook look, with Stage's rays and shimmering title on the
    docs home only (§9.5), and one stylesheet shared with the landing page;
  - the Zola project (`site/docs/`) with search;
  - the generated reference: Commands, Preferences, and Bar and menus,
    exported from the app;
  - `docs_coverage` with the full baseline, and the name tests over the
    docs;
  - the `keys`, `shot`, `clip` and `video` shortcodes, and callouts;
  - the scene target (`tests/docs_media/`) with the demo song;
  - the recorder, which adds the pointer, the click ring, keycaps, pacing,
    the video's clock, sound, captions and chapters, and the encode cache;
  - three scenes for Split a clip: a lead clip, a gesture clip, before and
    after shots, and a walkthrough with sound;
  - the checks on pull requests (`docs.yml`) and the deploy
    (`pages.yml`).
- **Proven:**
  - sound and picture stay in sync, and every caption shows long enough to
    read;
  - a second run makes the same frames and takes every clip from the
    cache;
  - a broken internal link or anchor stops the build.
- **Changed from the plan:**
  - clips are at 2× rather than 1.5×, at 30 frames a second (§6.4);
  - the media is rendered at each deploy instead of being committed;
  - the meters now fall by egui's input clock (§6.3). In the app this
    behaves as before.
- **Measured:** sizes and times are in §6.4. The whole set comes to about
  100 to 130 MB, and about 10 hours to render on one runner.
- **After the report (2026-11-05, on Windows):**
  - the owner chose A for the render time; the deploy is built that way
    (§13);
  - the branch was checked on Windows for the first time: fmt, clippy, the
    docs tests and the three scenes (checking, dark theme: 19 s on the
    machine's GPU) pass. Two meter E2E tests failed there every time: with
    the meters on egui's clock, a frame drawn straight after another sees
    no new audio on Windows and reads a step (0.33 dB) low. The tests now
    read levels on a frame drawn just after the engine played
    (`tests/e2e/meters.rs`, `listen`); the app is unchanged;
  - the landing page's top menu links to the docs ("Docs", after
    Download; owner's go-ahead).
- **Open:**
  - the side-by-side with a render made on Windows (needs ffmpeg on the
    owner's machine);
  - the owner's look at the built pages, in both themes and at phone
    width;
  - the Preferences reference is an index (each setting, its page, the
    words that find it, and a link to its guide). Saying what each
    setting does needs an `about` on `preferences::Row`; that comes with
    the Preferences guides in D1.
- **Live:** `/docs/` goes live when the owner merges this branch to
  `main`. The repository's Pages source must be set to "GitHub Actions"
  once (§8.2).

## 12. Choices

### 12.1 Decided (owner, 2026-11-05)

| Choice | Decision |
|---|---|
| **G. Site generator** | **A**: Zola, with Pagefind, on the condition that it matches the landing page and the design system (owner); §9.1 says how. |
| **S. Screenshots** | **A**: rendered from scenes at deploy, not committed. Extended by the owner to videos and animations ("show, don't tell", §6); clips and walkthroughs are rendered the same way. |
| **L. Where the pages live** | **A**: `site/docs/`, in this repository. |
| **C. Coverage enforcement** | **A**: the test with a shrinking baseline, which also checks the clips and walkthroughs (§5.3). |
| **H. Help inside Lumen** | **A**: later, as its own slice after D2: "Learn more" in the palette's detail strip and F1 "Help for this", by stable ids, pitched with specimens. |
| **T. When the backlog is written** | **A**: D0 before milestone 9.5, D1 to D7 alongside it. |
| **R. Render time** | **A**: render only what changed; a full render by hand at each milestone's docs pass (§13). |

### 12.2 Why Zola (the comparison behind G)

The owner asked why Zola (2026-11-05), then chose it. Search is Pagefind in every option:
it indexes the built pages whatever made them, so G is only about turning
Markdown into pages.

What these docs need from the generator, in order of weight:

1. **Our own components.** The heart of these docs is a handful of
   components: `clip` (a themed, lazy, pausable loop with a poster), `video`
   (chapters tied to the step headings), `shot` (a themed picture with
   callouts), `keys` (keycaps from the keymap). They need full control of
   the HTML each one writes.
2. **Pages from data.** The reference is built from `commands.json`,
   `preferences.json` and `controls.json` (§5.1).
3. **The Lumen look.** The landing page's fonts and the app's tokens, not a
   stock documentation theme.
4. **Little to maintain.** Lumen has no JavaScript toolchain; the site has
   no build step today.
5. **Docs chrome**: sidebar, previous and next, "On this page", later a
   version picker.

| | A. Zola | B. mdBook | C. Astro Starlight |
|---|---|---|---|
| 1. Own components | Shortcodes are templates we write, any HTML | Through a preprocessor written in Rust, or raw HTML in pages | Components in MDX, any HTML; the most capable |
| 2. Pages from data | Built in (`load_data` reads JSON in a template) | Needs a preprocessor | Built in (content collections) |
| 3. Lumen look | Templates are ours from the first line | A book theme to restyle; its layout shows through | A strong stock theme to restyle; it shows through less with work |
| 4. Upkeep | One binary, pinned in CI | One binary, pinned in CI | Node and a tree of npm packages to keep updated |
| 5. Docs chrome | **We write it**: a few hundred lines of templates and CSS | Built in (sidebar from `SUMMARY.md`) | **Built in and best**: sidebar, on-this-page, accessible defaults; versioning through a plugin |
| If we move later | Pages are Markdown; the shortcodes are rewritten | Same | Pages are MDX; more to undo |

**Why A.** It is the only option that is strong on 1 to 4 at once: our
components are plain templates, the reference reads JSON with no plugin,
the look is ours, and it adds one pinned binary rather than a toolchain.
Its cost is item 5: we build the docs chrome ourselves, once, in D0. The
pages stay Markdown, so the choice is cheap to undo.

**When C is the better choice.** If you would rather have the most
polished navigation and accessibility on day one, and accept a Node
toolchain in the deploy (not in the app), C is the strong alternative; it
also handles the video components well. B is the quickest start but the
weakest on 1 to 3.

- **A (rec.) Zola**, with Pagefind.
- B. mdBook, with Pagefind.
- C. Astro Starlight, with Pagefind.
- D. Decide from specimens: D0 renders the same guide page (a lead clip,
  two shots, keys) in A and C, both themes and phone width, before either
  is set up. Costs about a day.

## 13. Risks and what cannot be checked

- **Scene upkeep.** Scenes that drive the interface break when it changes.
  That is intended (a broken scene is a stale page), but it costs time in
  slices that change much of the interface. Mitigation: scenes go through
  accessible names and commands, the same way the E2E tests do, never
  through coordinates.
- **Rendering on CI.** The deploy renders on Linux with a software Vulkan
  driver. Text and colour are close to Windows but not identical, and some
  Windows-only panels (the ASIO driver choice) show test devices. The
  time is measured (§6.4); the side-by-side with a Windows render waits
  for the owner's machine (D0 was built in a Linux container).
- **Gestures** are the one list kept by hand (§5.2). The milestone review
  is the check; a gesture added without an entry is caught there, not by
  CI.
- **Accuracy of prose.** Tests prove that a page exists, that its shots
  render and that its keys are right; they cannot prove the words are
  right. The owner's look at each docs slice and the milestone docs pass
  are the check.
- **Hosting size and traffic.** About 100 to 130 MB for one set of docs
  (measured in D0, §6.4), against GitHub Pages' 1 GB and its soft 100 GB
  a month of traffic. Shared media keeps versions small (§10.3), but a popular site
  or many versions can outgrow it. When the site passes 700 MB, or traffic
  nears the limit, moving the media to another host is a decision for the
  owner (it may cost money). Lazy loading keeps traffic to what is watched.
- **Render time (owner: A, 2026-11-05).** Measured in D0: about 0.3 s
  a frame on the software renderer, so the whole set would take about 10
  hours on one runner, where today's three scenes take 5 minutes. The
  cache saves encoding, not rendering: a frame has to be drawn before it
  can be compared. Only 11% of a walkthrough's frames repeat the one
  before (38 of 332), so skipping still frames would save little there;
  the silent clips are another matter (see "Frames at rest", below). The
  choices were:
  - **A. Render when the docs change (chosen).** The deploy renders
    only the scenes whose page or scene changed in the push, and reuses
    the last published media for the rest; a full render runs by hand
    (`workflow_dispatch`) at each milestone's docs pass (§10.2), split
    into parallel jobs. Between passes, a page can show the interface as
    it was at the last pass; the milestone pass is the check.
  - **B. Render everything on each deploy, in parallel.** Ten or more jobs
    of about an hour each. The media is always current, but each deploy
    costs about 10 hours of Actions minutes, which have run out before.
  - **C. Render at 1×.** A quarter of the pixels, likely faster (not measured), but
    text in clips is blurred on high-density screens next to 2×
    screenshots. This can be combined with A or B.

  **How A is built** (`.github/workflows/pages.yml`; the plan is
  `plan_the_render` in `tests/docs_coverage.rs`):
  - Each scene has a fingerprint of what its media is made from, short of
    the app: its own function in `tests/docs_media/main.rs`, the code the
    scenes share (the rest of that folder), its `scenes.rs` entry, the
    pages that show its media and the gestures that name its clips.
    Editing the recorder or the demo song renders every scene;
    `RENDER_EPOCH` forces it.
  - The published site keeps `docs/media/manifest.json`: each scene's
    fingerprint, the commit it was rendered at, how long it took and its
    files. A deploy renders the scenes whose fingerprint differs from the
    manifest (or that are not in it) and fetches the others' files from the
    site as they are. With no manifest, everything renders. A file missing
    from the site stops the deploy, which asks for a full render.
  - "Run workflow" on the Site workflow renders every scene (full, the
    default) among up to 8 jobs, longest scenes first, by the times in the
    manifest. Do it at each milestone's docs pass (§10.2), and after a
    change to the app that the pictures should show at once.
  - The run's summary lists each scene: rendered and why, or reused and
    the commit it shows.
  - **The pull-request check renders as the deploy does** (2026-11-08;
    `.github/workflows/docs-render.yml`, called by `pages.yml` and
    `docs.yml`). It used to render its scenes one after another in a
    single job: the five audio-editor scenes alone took about 22 minutes,
    and the check 37. The plan and the render are now one reusable
    workflow, so a pull request shares its scenes among 6 jobs, beside the
    checks. The final job, still named `docs`, builds the pages from the
    media.
  - **The app is built once on the way** (ADR-0129). There is no job
    that plans alone: every render job builds the app, makes the same plan
    from the same published manifest and renders its own share of it, so
    the plan does not sit in front of the render. The last job also exports the
    data the pages are made from (the commands, the settings), so the job
    that builds the pages has no Rust. The jobs' plans are compared
    (`docs-media.sh plans`): if a deploy landed between two jobs'
    fetches, they differ, the run fails with that message and is run
    again.
  - **Frames at rest are not drawn again** (ADR-0129). Drawing a frame is
    the cost of a clip (the software renderer is slow by design: the same
    pictures everywhere). A frame of a hold after the first, in which the
    app asked for no immediate repaint after this frame and the one before,
    is the frame before it, so the recorder keeps that picture and draws
    the pointer on it. `DOCS_VERIFY_REUSE=1` draws those frames anyway and
    counts the ones that came out different (the scene's last line, `PHASES`,
    says `verified` or `MISMATCH`); the numbers are in ADR-0129.
  - **A scene with two themes is two pieces.** A scene with clips or shots
    is planned as `<test>:dark` and `<test>:light` (`DOCS_THEME` selects the
    one a job draws), each a unit of half the scene's time, so the longest
    scene no longer bounds the run. The folders of its pieces together hold
    what the scene writes, and their seconds are added up for the manifest.
  - **The target is ten minutes** (owner, 2026-11-08) from the push to the
    built pages: 6 jobs, each about 8 minutes (set-up, one build, its share of
    the render), then the page job. This is an estimate until a run on
    GitHub has measured it; if it comes out longer, the levers are more jobs,
    splitting the heaviest scenes, and a larger runner (ADR-0129).
  - **Rendered scenes are kept by fingerprint.** The last job of a run
    (the deploy, or the check's `docs` job) saves every scene it rendered
    or reused into one cache entry (`docs-media.sh keep`), before it builds
    the pages, so a build that fails does not lose the render. Each render
    job restores the latest entry (`docs-media.sh render`,
    `DOCS_SCENE_CACHE`): a scene whose fingerprint is there is copied, not
    drawn again, whichever job drew it last time, so the next push to the
    branch renders only the scenes it changed. The entry holds only the
    plan's render list, so it never grows.
  - **A full render starts a new generation.** The manifest carries a
    `generation` (a hash unique to the last full render run; `0` before
    one), and the cache's key starts with it. A full render does not read the cache,
    and entries kept before it are never found afterwards, so a pull
    request cannot bring back pictures from before the milestone's
    refresh.
  - **Who can read the cache.** A pull request run reads its own caches
    and `main`'s; `main` cannot read a pull request's. After a merge, the
    first deploy still renders what the branch's pages added (in parallel,
    as before), and saves them for every pull request after it.
  - A scene taken from the cache is listed in the manifest with the commit
    of the run that reused it, not the one that drew it; the fingerprint,
    not the commit, decides what is reused.
- **Size of the backlog.** About 250 commands, 45 settings and an unknown
  number of gestures. Eight slices (D0 to D7) is an estimate, revised after
  D2.
