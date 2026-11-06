//! The user documentation covers every action (docs/USER_DOCS.md §5.3).
//!
//! The inventory is every command (its id, `midi.split_notes`), every
//! Preferences row (`pref:audio.driver`), every transport bar control and
//! app menu row that runs no command (`bar:Tempo`, `menu:Open recent`; one
//! that runs a command is documented with it), and every gesture in
//! `site/docs/data/gestures.toml` (`gesture.trim_clip_edge`). An action is documented when a guide,
//! recipe or quick-start page lists its id under `[taxonomies] actions`.
//!
//! Until the docs are complete, the actions not yet documented are listed
//! in `site/docs/data/undocumented.txt`, and that list only shrinks: a new
//! action needs a page, and a documented one must leave the list.
//!
//! The pages are checked too: a guide opens with a lead clip, a recipe or
//! the quick start has a walkthrough video, every task page has a
//! screenshot, and every clip and shot a page shows is made by a scene
//! (`tests/shared/scenes.rs`).

#[path = "shared/controls.rs"]
mod controls;
#[path = "shared/scenes.rs"]
mod scenes;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use lumen_app::testing::docs;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository is there")
}

fn site() -> PathBuf {
    repo().join("site/docs")
}

/// A page's path as a reader of the failure finds it: from the repository.
fn shown(path: &Path) -> String {
    path.strip_prefix(repo())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// A gesture: direct manipulation no table holds (§5.2).
#[derive(serde::Deserialize)]
struct Gesture {
    id: String,
    /// Where it happens: "arrangement", "piano roll", "device card".
    place: String,
    /// The input: "drag", "Alt+drag", "double-click".
    input: String,
    /// What it does, in one sentence.
    does: String,
    /// The E2E scenario that covers it: a test function in `tests/e2e/`.
    scenario: String,
    /// The clip that shows it, made by a scene.
    clip: String,
}

#[derive(serde::Deserialize)]
struct Gestures {
    #[serde(default)]
    gesture: Vec<Gesture>,
}

fn gestures() -> Vec<Gesture> {
    let text = fs::read_to_string(site().join("data/gestures.toml")).expect("gestures.toml reads");
    toml::from_str::<Gestures>(&text)
        .expect("gestures.toml is valid")
        .gesture
}

/// Every action's id, with what kind it is.
fn inventory() -> BTreeMap<String, &'static str> {
    let mut all = BTreeMap::new();
    for c in docs::commands() {
        all.insert(c.id.to_owned(), "command");
    }
    // A setting can share its key with the command that changes it
    // (`edit.lock_envelopes`), so settings are listed as `pref:<key>`.
    for p in docs::preferences() {
        all.insert(format!("pref:{}", p.id), "preference");
    }
    for (place, list) in [("bar", controls::BAR), ("menu", controls::MENU)] {
        for (name, has) in list {
            if matches!(has, controls::Has::Without(_)) {
                all.insert(format!("{place}:{name}"), "control");
            }
        }
    }
    for g in gestures() {
        assert!(
            g.id.starts_with("gesture."),
            "gesture ids start with \"gesture.\": {}",
            g.id
        );
        all.insert(g.id, "gesture");
    }
    all
}

/// A page of the docs: where it is, its kind (`[extra] kind`), the actions
/// it lists and the body after its front matter.
struct Page {
    path: PathBuf,
    kind: String,
    actions: Vec<String>,
    body: String,
}

fn pages() -> Vec<Page> {
    let mut files = Vec::new();
    markdown(&site().join("content"), &mut files);
    assert!(!files.is_empty(), "the content folder has pages");
    files
        .into_iter()
        .filter(|f| f.file_name().is_some_and(|n| n != "_index.md"))
        .map(|path| {
            let text = fs::read_to_string(&path).expect("a page reads");
            let rest = text
                .strip_prefix("+++\n")
                .unwrap_or_else(|| panic!("{} starts with +++ front matter", path.display()));
            let (front, body) = rest
                .split_once("\n+++\n")
                .unwrap_or_else(|| panic!("{} closes its front matter", path.display()));
            let front: toml::Table = toml::from_str(front)
                .unwrap_or_else(|e| panic!("{}: front matter: {e}", path.display()));
            let kind = front
                .get("extra")
                .and_then(|e| e.get("kind"))
                .and_then(|k| k.as_str())
                .unwrap_or_else(|| panic!("{} has an [extra] kind", path.display()))
                .to_owned();
            let actions = front
                .get("taxonomies")
                .and_then(|t| t.get("actions"))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().expect("an action is a string").to_owned())
                        .collect()
                })
                .unwrap_or_default();
            Page {
                path,
                kind,
                actions,
                body: body.to_owned(),
            }
        })
        .collect()
}

fn markdown(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the folder reads") {
        let path = entry.expect("an entry reads").path();
        if path.is_dir() {
            markdown(&path, into);
        } else if path.extension().is_some_and(|e| e == "md") {
            into.push(path);
        }
    }
}

/// The arguments of each of the body's calls to the shortcode `name`: from
/// `name(` to the `)` that closes it, outside quoted text.
fn calls<'a>(body: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("{name}(");
    let mut found = Vec::new();
    for (at, _) in body.match_indices(&open) {
        // A shortcode, not a word ending in the name: "{{ clip(" or "{% tryit(".
        let before = body[..at].trim_end();
        if !(before.ends_with("{{") || before.ends_with("{%")) {
            continue;
        }
        let args = &body[at + open.len()..];
        let mut quoted = false;
        let end = args
            .char_indices()
            .find(|&(_, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                c == ')' && !quoted
            })
            .map_or(args.len(), |(i, _)| i);
        found.push(&args[..end]);
    }
    found
}

/// The value of `key="…"` in a call's arguments.
fn arg(call: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let mut from = 0;
    // A key, not the end of a longer one (`id` in `grid="…"`).
    while let Some(start) = call[from..].find(&needle).map(|i| i + from) {
        let before = call[..start].chars().next_back();
        if before.is_none_or(|c| c == ' ' || c == ',' || c == '(') {
            let value = &call[start + needle.len()..];
            return Some(value[..value.find('"').unwrap_or(value.len())].to_owned());
        }
        from = start + needle.len();
    }
    None
}

/// The values of `key="…"` in the body's calls to the shortcode `name`.
fn shortcode_args(body: &str, name: &str, key: &str) -> Vec<String> {
    calls(body, name)
        .into_iter()
        .filter_map(|call| arg(call, key))
        .collect()
}

/// Whether the body has a call to the shortcode `name` with `arg` in it.
fn has_call_with(body: &str, name: &str, arg: &str) -> bool {
    calls(body, name).iter().any(|call| call.contains(arg))
}

/// The names in a call's `callouts=["…", "…"]`.
fn callout_names(call: &str) -> Vec<String> {
    let Some(start) = call.find("callouts=[") else {
        return Vec::new();
    };
    let list = &call[start + "callouts=[".len()..];
    let list = &list[..list.find(']').unwrap_or(list.len())];
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The page kinds that teach a task, and so need pictures.
const TASKS: [&str; 3] = ["Guide", "Recipe", "Quick start"];

fn documented(pages: &[Page]) -> BTreeSet<String> {
    pages
        .iter()
        .filter(|p| TASKS.contains(&p.kind.as_str()))
        .flat_map(|p| p.actions.iter().cloned())
        .collect()
}

fn baseline() -> Vec<String> {
    fs::read_to_string(site().join("data/undocumented.txt"))
        .expect("undocumented.txt reads")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_action_is_documented_or_in_the_shrinking_list() {
    let inventory = inventory();
    let pages = pages();
    let documented = documented(&pages);
    let baseline = baseline();
    let listed: BTreeSet<_> = baseline.iter().cloned().collect();
    let mut problems = Vec::new();

    for page in &pages {
        for id in &page.actions {
            if !inventory.contains_key(id) {
                problems.push(format!(
                    "{} lists {id}, which is no command, setting, control or gesture",
                    shown(&page.path)
                ));
            }
        }
    }
    if listed.len() != baseline.len() {
        problems.push("undocumented.txt lists an id twice".to_owned());
    }
    for id in &baseline {
        if !inventory.contains_key(id) {
            problems.push(format!(
                "undocumented.txt lists {id}, which no longer exists: take it out"
            ));
        } else if documented.contains(id) {
            problems.push(format!(
                "{id} is documented now: take it out of undocumented.txt"
            ));
        }
    }
    for (id, kind) in &inventory {
        if !documented.contains(id) && !listed.contains(id) {
            problems.push(format!(
                "the {kind} {id} has no page: list it in the actions of the guide that \
                 explains it (undocumented.txt only shrinks)"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_task_page_shows_the_move_and_every_picture_has_a_scene() {
    let mut problems = Vec::new();
    for page in pages() {
        let at = shown(&page.path);
        let clips: Vec<_> = shortcode_args(&page.body, "clip", "id")
            .into_iter()
            .chain(shortcode_args(&page.body, "tryit", "answer"))
            .collect();
        let walks = shortcode_args(&page.body, "video", "id");
        let shots = shortcode_args(&page.body, "shot", "id");
        for walk in &walks {
            if !scenes::has_walk(walk) {
                problems.push(format!(
                    "{at} shows the walkthrough {walk}, which no scene makes"
                ));
            }
        }
        for clip in &clips {
            if !scenes::has_clip(clip) {
                problems.push(format!("{at} shows the clip {clip}, which no scene makes"));
            }
        }
        for shot in &shots {
            if !scenes::has_shot(shot) {
                problems.push(format!("{at} shows the shot {shot}, which no scene makes"));
            }
        }
        for call in calls(&page.body, "shot") {
            let id = arg(call, "id").unwrap_or_default();
            let recorded = scenes::callouts(&id);
            for name in callout_names(call) {
                if !recorded.contains(&name.as_str()) {
                    problems.push(format!(
                        "{at} marks \"{name}\" on the shot {id}, which its scene does not \
                         record (tests/shared/scenes.rs)"
                    ));
                }
            }
        }
        match page.kind.as_str() {
            "Guide" if !has_call_with(&page.body, "clip", "lead=true") => {
                problems.push(format!("{at}: a guide opens with a lead clip"));
            }
            "Recipe" | "Quick start" if walks.is_empty() => {
                problems.push(format!("{at}: a {} has a walkthrough video", page.kind));
            }
            _ => {}
        }
        if TASKS.contains(&page.kind.as_str()) && shots.is_empty() {
            problems.push(format!("{at}: a task page has at least one screenshot"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_gesture_names_its_scenario_and_its_clip() {
    let mut scenarios = String::new();
    let e2e = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/e2e");
    for entry in fs::read_dir(e2e).expect("the E2E folder reads") {
        scenarios += &fs::read_to_string(entry.expect("an entry reads").path()).unwrap_or_default();
    }
    let mut problems = Vec::new();
    for g in gestures() {
        if !scenarios.contains(&format!("fn {}(", g.scenario)) {
            problems.push(format!(
                "{}: no E2E scenario is called {}",
                g.id, g.scenario
            ));
        }
        if !scenes::has_clip(&g.clip) {
            problems.push(format!("{}: no scene makes the clip {}", g.id, g.clip));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The reader's words for commands (`data/also.toml`) name real commands,
/// so a renamed or removed command does not leave its words behind.
#[test]
fn every_synonym_names_a_command() {
    let text = fs::read_to_string(site().join("data/also.toml")).expect("also.toml reads");
    let also: BTreeMap<String, Vec<String>> = toml::from_str(&text).expect("also.toml is valid");
    let ids: BTreeSet<&str> = docs::commands().iter().map(|c| c.id).collect();
    let unknown: Vec<&String> = also
        .keys()
        .filter(|id| !ids.contains(id.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "also.toml names no command: {unknown:?}"
    );
    assert!(
        also.values().all(|words| !words.is_empty()),
        "an also.toml entry has no words"
    );
}

#[test]
fn shortcode_arguments_are_read_from_calls_only() {
    let body = "A clip(id=\"no\") in prose.\n{{ clip(id=\"lead\", lead=true) }}\n\
                {% tryit(answer=\"answer\") %}x{% end %}";
    assert_eq!(shortcode_args(body, "clip", "id"), ["lead"]);
    assert_eq!(shortcode_args(body, "tryit", "answer"), ["answer"]);
    assert!(has_call_with(body, "clip", "lead=true"));
}

#[test]
fn a_bracket_in_quoted_text_does_not_end_a_call() {
    let body = "{{ shot(alt=\"Two clips (cut)\", id=\"after\", callouts=[\"First\", \"Second\"]) }}\n\
                {{ clip(alt=\"A (lead)\", lead=true, grid=\"x\", id=\"lead\") }}";
    assert_eq!(shortcode_args(body, "shot", "id"), ["after"]);
    assert_eq!(shortcode_args(body, "clip", "id"), ["lead"]);
    assert!(has_call_with(body, "clip", "lead=true"));
    assert_eq!(callout_names(calls(body, "shot")[0]), ["First", "Second"]);
}

// ---- What the deploy renders (USER_DOCS.md §13, the owner's choice A) ----
//
// The deploy renders a scene again only when what it is made from changed
// since the published site: its own function in `tests/docs_media/main.rs`,
// the code every scene shares (the rest of that folder), its entry in
// `scenes.rs`, the pages that show its media, and the gestures that name its
// clips. The app's own code is left out on purpose: between milestones a
// picture may show the interface as it was, and the full render at each
// milestone's docs pass (§10.2) brings every picture up to date.

/// Raised to render every scene again at the next deploy.
const RENDER_EPOCH: u64 = 1;

/// FNV-1a, 64 bits: the same on every machine and toolchain, unlike
/// `DefaultHasher`, so the deploy compares fingerprints made on other runs.
#[derive(Clone)]
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    /// Adds `text`, ending it so that "ab" + "c" differs from "a" + "bc".
    /// Line ends count as `\n` (a Windows checkout writes `\r\n`).
    fn text(&mut self, text: &str) {
        for byte in text.bytes().filter(|&b| b != b'\r').chain([0xff]) {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Where the scene `test` is in the scene target's `main.rs`: its doc
/// comment, attributes and function.
fn scene_span(main: &str, test: &str) -> Option<std::ops::Range<usize>> {
    let head = format!("\nfn {test}() {{\n");
    let fn_at = main.find(&head)? + 1;
    let end = fn_at + main[fn_at..].find("\n}\n")? + "\n}\n".len();
    let mut start = fn_at;
    while let Some(line_start) = main[..start - 1].rfind('\n').map(|i| i + 1) {
        let line = main[line_start..start - 1].trim_start();
        if !(line.starts_with("///") || line.starts_with("#[")) {
            break;
        }
        start = line_start;
    }
    Some(start..end)
}

/// The scene target's sources with line ends as `\n`: `main.rs`, and every
/// other file in the folder by name.
fn scene_target() -> (String, BTreeMap<String, String>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/docs_media");
    let mut main = String::new();
    let mut others = BTreeMap::new();
    for entry in fs::read_dir(&dir).expect("the scene target reads") {
        let path = entry.expect("an entry reads").path();
        let text = fs::read_to_string(&path)
            .expect("a scene source reads")
            .replace('\r', "");
        let name = path
            .file_name()
            .expect("a file")
            .to_string_lossy()
            .into_owned();
        if name == "main.rs" {
            main = text;
        } else {
            others.insert(name, text);
        }
    }
    (main, others)
}

/// Each scene's fingerprint: what its media is made from, short of the app.
fn fingerprints() -> BTreeMap<&'static str, String> {
    let (main, others) = scene_target();
    let mut spans: Vec<_> = scenes::SCENES
        .iter()
        .map(|s| {
            let span = scene_span(&main, s.test)
                .unwrap_or_else(|| panic!("tests/docs_media/main.rs has `fn {}() {{`", s.test));
            (s, span)
        })
        .collect();
    spans.sort_by_key(|(_, span)| span.start);
    // Everything that is not a scene's own source is shared by all of them.
    let mut shared = Fnv::new();
    shared.text(&RENDER_EPOCH.to_string());
    let mut at = 0;
    for (_, span) in &spans {
        assert!(span.start >= at, "scene sources do not overlap");
        shared.text(&main[at..span.start]);
        at = span.end;
    }
    shared.text(&main[at..]);
    for (name, text) in &others {
        shared.text(name);
        shared.text(text);
    }
    let pages = pages();
    let gestures = gestures();
    spans
        .iter()
        .map(|(scene, span)| {
            let mut hash = shared.clone();
            hash.text(&main[span.clone()]);
            hash.text(&format!(
                "{:?} {:?} {:?} {:?}",
                scene.clips, scene.shots, scene.walks, scene.callouts
            ));
            for page in &pages {
                let shows = |name: &str, key: &str, has: &[&str]| {
                    shortcode_args(&page.body, name, key)
                        .iter()
                        .any(|id| has.contains(&id.as_str()))
                };
                if shows("clip", "id", scene.clips)
                    || shows("tryit", "answer", scene.clips)
                    || shows("shot", "id", scene.shots)
                    || shows("video", "id", scene.walks)
                {
                    let path = page.path.strip_prefix(site()).unwrap_or(&page.path);
                    hash.text(&path.to_string_lossy().replace('\\', "/"));
                    hash.text(&page.body);
                }
            }
            for g in gestures
                .iter()
                .filter(|g| scene.clips.contains(&g.clip.as_str()))
            {
                for field in [&g.id, &g.place, &g.input, &g.does, &g.scenario, &g.clip] {
                    hash.text(field);
                }
            }
            (scene.test, hash.finish())
        })
        .collect()
}

/// A scene as the published site records it in `media/manifest.json`.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct Published {
    fingerprint: String,
    /// The commit it was last rendered at.
    commit: String,
    /// How long it took to render, for sharing scenes out among the jobs.
    seconds: f64,
    files: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
struct Manifest {
    #[serde(default)]
    scenes: BTreeMap<String, Published>,
}

/// Writes the deploy's render plan to `DOCS_PLAN` (a JSON file), against
/// the published `media/manifest.json` at `DOCS_PLAN_PUBLISHED` (when it is
/// absent or unreadable, everything renders). `DOCS_PLAN_FULL=true` renders
/// every scene (the milestone pass); `DOCS_PLAN_JOBS` is how many jobs may
/// share the work (4 by default). Run by `.github/workflows/pages.yml`.
#[test]
#[ignore = "writes the deploy's render plan; run by pages.yml"]
fn plan_the_render() {
    let out = std::env::var("DOCS_PLAN").expect("DOCS_PLAN names the plan's file");
    let full = std::env::var("DOCS_PLAN_FULL").is_ok_and(|v| v == "true");
    let jobs: usize = std::env::var("DOCS_PLAN_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
        .max(1);
    let published = std::env::var("DOCS_PLAN_PUBLISHED")
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Manifest>(&text).ok())
        .unwrap_or_default();
    let fingerprints = fingerprints();
    let mut render = Vec::new();
    let mut reuse = serde_json::Map::new();
    let mut why = serde_json::Map::new();
    for scene in scenes::SCENES {
        let fingerprint = &fingerprints[scene.test];
        let before = published.scenes.get(scene.test);
        let reason = match before {
            _ if full => "full render",
            None => "not published yet",
            Some(p) if &p.fingerprint != fingerprint => "changed",
            Some(p) => {
                // Fetched from the site as they are; the file names come
                // from scenes.rs, whose entry is in the fingerprint.
                let mut entry = p.clone();
                entry.files = scene.files(scenes::THEMES).into_iter().collect();
                reuse.insert(
                    scene.test.into(),
                    serde_json::to_value(entry).expect("an entry"),
                );
                continue;
            }
        };
        render.push((scene.test, before.map_or(60.0, |p| p.seconds)));
        why.insert(scene.test.into(), reason.into());
    }
    // Longest first, each to the job with the least so far.
    render.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut shards: Vec<(f64, Vec<&str>)> = vec![(0.0, Vec::new()); jobs.min(render.len())];
    for (test, seconds) in &render {
        let lightest = shards
            .iter_mut()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("a job for each scene to render");
        lightest.0 += seconds;
        lightest.1.push(test);
    }
    let shards: Vec<_> = shards
        .iter()
        .enumerate()
        .map(|(index, (_, tests))| serde_json::json!({ "index": index, "tests": tests.join(" ") }))
        .collect();
    let plan = serde_json::json!({
        "full": full,
        "fingerprints": fingerprints,
        "render": render.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        "why": why,
        "reuse": reuse,
        "shards": shards,
    });
    fs::write(
        &out,
        serde_json::to_string_pretty(&plan).expect("the plan") + "\n",
    )
    .expect("write the plan");
}

#[test]
fn every_scene_has_its_own_source_and_fingerprint() {
    let prints = fingerprints();
    assert_eq!(prints.len(), scenes::SCENES.len());
    let distinct: BTreeSet<_> = prints.values().collect();
    assert_eq!(
        distinct.len(),
        prints.len(),
        "each scene's fingerprint is its own"
    );
}

#[test]
fn a_scene_span_takes_its_comment_and_attributes_and_nothing_else() {
    let main = "use x;\n\n/// Before.\n#[test]\nfn one() {\n    a();\n}\n\nfn helper() {\n}\n";
    let span = scene_span(main, "one").expect("found");
    assert_eq!(
        &main[span],
        "/// Before.\n#[test]\nfn one() {\n    a();\n}\n"
    );
    assert!(scene_span(main, "two").is_none());
}

#[test]
fn fingerprints_ignore_windows_line_ends() {
    let mut unix = Fnv::new();
    unix.text("a\nb\n");
    let mut windows = Fnv::new();
    windows.text("a\r\nb\r\n");
    assert_eq!(unix.finish(), windows.finish());
    let (mut ab, mut a_b) = (Fnv::new(), Fnv::new());
    ab.text("ab");
    a_b.text("a");
    a_b.text("b");
    assert_ne!(ab.finish(), a_b.finish());
}
