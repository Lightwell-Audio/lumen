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
    #[allow(dead_code, reason = "read by the site, checked for presence")]
    place: String,
    /// The input: "drag", "Alt+drag", "double-click".
    #[allow(dead_code, reason = "read by the site, checked for presence")]
    input: String,
    /// What it does, in one sentence.
    #[allow(dead_code, reason = "read by the site, checked for presence")]
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

/// The values of `key="…"` in the body's calls to the shortcode `name`.
fn shortcode_args(body: &str, name: &str, key: &str) -> Vec<String> {
    let open = format!("{name}(");
    let mut found = Vec::new();
    for (at, _) in body.match_indices(&open) {
        // A shortcode, not a word ending in the name: "{{ clip(" or "{% tryit(".
        let before = body[..at].trim_end();
        if !(before.ends_with("{{") || before.ends_with("{%")) {
            continue;
        }
        let call = &body[at..];
        let call = &call[..call.find(')').unwrap_or(call.len())];
        let needle = format!("{key}=\"");
        if let Some(start) = call.find(&needle) {
            let value = &call[start + needle.len()..];
            found.push(value[..value.find('"').unwrap_or(value.len())].to_owned());
        }
    }
    found
}

/// Whether the body has a call to the shortcode `name` with `arg` in it.
fn has_call_with(body: &str, name: &str, arg: &str) -> bool {
    let open = format!("{name}(");
    body.match_indices(&open).any(|(at, _)| {
        let call = &body[at..];
        call[..call.find(')').unwrap_or(call.len())].contains(arg)
    })
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

#[test]
fn shortcode_arguments_are_read_from_calls_only() {
    let body = "A clip(id=\"no\") in prose.\n{{ clip(id=\"lead\", lead=true) }}\n\
                {% tryit(answer=\"answer\") %}x{% end %}";
    assert_eq!(shortcode_args(body, "clip", "id"), ["lead"]);
    assert_eq!(shortcode_args(body, "tryit", "answer"), ["answer"]);
    assert!(has_call_with(body, "clip", "lead=true"));
}
