//! Command coverage (M9 R7, the 2026-10-24 audit's M4): every control on
//! the transport bar and every row of the app menu either runs a command,
//! so it can be found in the palette and given keys, or is listed here with
//! the reason it has none. The controls are read from what is drawn: their
//! names from the accessibility tree, and the command each was drawn for
//! from the app's own note of it (`App::drawn_commands`). A new control in
//! neither list fails, and so does one wired to another command than its
//! entry says.

use eframe::egui::Rect;
use eframe::egui::accesskit::{Action, Role};
use egui_kittest::kittest::{NodeT, Queryable, by};
use lumen_app::App;

use super::Lumen;

/// What a control is in the lists: the command it runs, by name, or why it
/// has none.
enum Has {
    Command(&'static str),
    Without(&'static str),
}
use Has::{Command, Without};

/// The transport bar, by each control's accessible name. A name that ends
/// in a colon or " …" goes on with a value ("Position 1.1.1", "Audio: On").
const BAR: &[(&str, Has)] = &[
    ("Lumen", Without("opens the app menu, listed below")),
    ("Go to start", Command("Go to start")),
    ("Stop", Command("Play / Stop")),
    ("Play from the start marker", Command("Play / Stop")),
    ("Record", Command("Record")),
    ("Overdub", Command("Overdub")),
    ("Loop", Command("Loop on or off")),
    ("Metronome", Command("Metronome on or off")),
    ("Follow playhead", Command("Follow playhead")),
    (
        "Computer keyboard plays notes",
        Command("Computer keyboard"),
    ),
    ("Clear all solos", Command("Clear all solos")),
    (
        "Tempo",
        Without("a field typed into; the palette takes a typed tempo too"),
    ),
    ("Time signature:", Without("a field typed into")),
    (
        "Position …",
        Without("a field typed into; the palette takes a typed bar too"),
    ),
    (
        "Audio:",
        Without("opens the audio panel, whose switch is Audio on or off"),
    ),
    (
        "MIDI:",
        Without("opens the MIDI panel of ports and activity"),
    ),
    ("Command palette", Command("Command palette")),
    ("Browser", Command("Show or hide the browser")),
    ("Detail", Command("Show / hide detail panel")),
];

/// The app menu's rows, then the rows of its submenus.
const MENU: &[(&str, Has)] = &[
    ("New", Command("New project")),
    (
        "New from template",
        Without("a submenu of templates, each its own palette entry"),
    ),
    ("Open…", Command("Open…")),
    ("Open recent", Without("a submenu of recent files")),
    ("Save", Command("Save")),
    ("Save as…", Command("Save as…")),
    ("More ways to save", Without("a submenu")),
    ("Project settings…", Command("Project settings…")),
    ("Project media", Command("Project media")),
    ("Import audio or MIDI…", Command("Import audio or MIDI…")),
    ("Export MIDI file…", Command("Export MIDI file…")),
    ("Export audio…", Command("Export audio…")),
    ("Undo", Command("Undo")),
    ("Redo", Command("Redo")),
    (
        "History…",
        Without("gains a command with Preferences, R7 part 2"),
    ),
    ("Appearance", Without("a submenu")),
    ("Mixing", Without("a submenu")),
    (
        "Audio and MIDI settings…",
        Without("gains a command with Preferences, R7 part 2"),
    ),
    ("Keyboard shortcuts…", Command("Keyboard shortcuts")),
    // New from template, with the template the test saves.
    (
        "Beat",
        Without("a template: the palette lists each one as New from template"),
    ),
    ("Open templates folder", Command("Open templates folder")),
    // Open recent, with the project the test saves.
    ("Song", Without("a recent file, opened by its path")),
    // More ways to save.
    ("Save a copy…", Command("Save a copy…")),
    ("Collect and save", Command("Collect and save")),
    ("Save as template…", Command("Save as template…")),
    ("Revert to saved…", Command("Revert to saved…")),
    (
        "Copy imported audio into the project",
        Without("a preference: it moves to Preferences, R7 part 2"),
    ),
    // Appearance: preferences, which move to Preferences in R7 part 2.
    ("Dark", Without("a preference")),
    ("Light", Without("a preference")),
    ("Match system", Without("a preference")),
    ("100%", Without("a preference")),
    ("125%", Without("a preference")),
    ("150%", Without("a preference")),
    ("175%", Without("a preference")),
    ("200%", Without("a preference")),
    ("Reduce motion", Without("a preference")),
    // Mixing: switches for preferences, which the palette's commands of
    // the same names flip too; they move to Preferences in R7 part 2.
    ("Additive solo", Without("a preference switch")),
    ("Meters in track colors", Without("a preference switch")),
];

/// How tall the transport bar is: the controls above this line are its.
const BAR_HEIGHT: f32 = 44.0;

/// The submenus whose rows the menu list holds.
const SUBMENUS: [&str; 5] = [
    "New from template",
    "Open recent",
    "More ways to save",
    "Appearance",
    "Mixing",
];

/// A control a user can act on, greyed or not: its node's id, so a control is
/// told apart from another of the same name, and its accessible name.
struct Control {
    id: String,
    name: String,
}

/// The controls drawn now where `keep` says, by their rectangle. A control with no
/// name is named by its role and place, so it fails the check rather than
/// being skipped. Other nodes that take a click (a menu's own area, a
/// switch's caption) count only when named.
fn controls(lumen: &Lumen, keep: impl Fn(Rect) -> bool) -> Vec<Control> {
    lumen
        .harness
        .query_all(by().predicate(|node| {
            let control = matches!(
                node.role(),
                Role::Button
                    | Role::CheckBox
                    | Role::Switch
                    | Role::RadioButton
                    | Role::TextInput
                    | Role::Slider
                    | Role::SpinButton
            );
            let clicked = node.data().supports_action(Action::Click) && node.label().is_some();
            control || clicked
        }))
        .filter(|node| keep(node.rect()))
        .map(|node| {
            let at = node.rect().center();
            let node = node.accesskit_node();
            let name = node.label().unwrap_or_else(|| {
                format!(
                    "a {:?} with no name at {:.0}, {:.0}",
                    node.role(),
                    at.x,
                    at.y
                )
            });
            Control {
                id: format!("{:?}", node.id()),
                name,
            }
        })
        .collect()
}

/// The name a control is drawn with: its accessible name without the
/// keycaps a menu row adds ("Save Ctrl+S" is "Save"). `chords` are every
/// command's shortcuts, as shown.
fn drawn_name<'a>(name: &'a str, chords: &[String]) -> &'a str {
    chords
        .iter()
        .find_map(|chord| name.strip_suffix(chord.as_str())?.strip_suffix(' '))
        .unwrap_or(name)
}

/// The entry of `list` that a control drawn as `name` is: the entry's name
/// itself, or one that ends in a colon or " …" followed by a value.
fn entry<'a>(list: &'a [(&'a str, Has)], name: &str) -> Option<&'a (&'a str, Has)> {
    list.iter().find(|(entry, _)| {
        if let Some(stem) = entry.strip_suffix(" …") {
            name.strip_prefix(stem)
                .is_some_and(|rest| rest.starts_with(' '))
        } else if entry.ends_with(':') {
            name.starts_with(entry)
        } else {
            name == *entry
        }
    })
}

/// Checks the drawn `controls` against `list`: each is listed and drawn
/// for its entry's command (`drawn`, the app's note of the command each
/// control name was drawn for) or, without one, for none; and every entry
/// was drawn.
fn check(
    list: &[(&str, Has)],
    controls: &[Control],
    drawn: &[(String, &str)],
    chords: &[String],
    place: &str,
    problems: &mut Vec<String>,
) {
    let mut seen = Vec::new();
    for control in controls {
        let name = drawn_name(&control.name, chords);
        let Some((entry, has)) = entry(list, name) else {
            problems.push(format!(
                "{place}: \"{name}\" runs no listed command and has no reason"
            ));
            continue;
        };
        seen.push(*entry);
        let runs: Vec<&str> = drawn
            .iter()
            .filter(|(label, _)| label == name)
            .map(|(_, command)| *command)
            .collect();
        match has {
            Command(command) if runs != [*command] => problems.push(format!(
                "{place}: \"{name}\" should run \"{command}\" but was drawn for {runs:?}"
            )),
            Without(why) if !runs.is_empty() => problems.push(format!(
                "{place}: \"{name}\" runs {runs:?}: list it with its command, not \"{why}\""
            )),
            _ => {}
        }
    }
    for (entry, _) in list {
        if !seen.contains(entry) {
            problems.push(format!("{place}: \"{entry}\" is listed but not drawn"));
        }
    }
}

#[test]
fn a_name_matches_its_entry_whole_or_with_a_value() {
    let chords = vec!["Ctrl+S".to_owned()];
    assert_eq!(drawn_name("Save Ctrl+S", &chords), "Save");
    assert_eq!(
        drawn_name("Save project notes", &chords),
        "Save project notes"
    );
    assert!(entry(MENU, "Save").is_some());
    assert!(
        entry(MENU, "Save project notes").is_none(),
        "not a longer name"
    );
    assert!(entry(BAR, "Position 1.1.1").is_some());
    assert!(entry(BAR, "Positions").is_none());
    assert!(entry(BAR, "Audio: On").is_some());
    assert!(entry(BAR, "Tempo lane").is_none());
}

#[test]
fn every_bar_control_and_app_menu_row_runs_a_command_or_says_why_not() {
    let mut lumen = Lumen::start("coverage");
    // A recent project and a template, so their submenus list a row each.
    let song = lumen.dir().join("Song.lumen");
    lumen.app_mut().save_to(song.clone());
    lumen.wait_for("the project saved", |app| {
        !app.is_busy() && !app.recent_projects().is_empty()
    });
    let template = lumen.app().templates_folder().join("Beat");
    std::fs::create_dir_all(&template).expect("make the template's folder");
    std::fs::copy(&song, template.join("Beat.lumen")).expect("save a template");
    let chords: Vec<String> = App::command_names()
        .into_iter()
        .flat_map(|name| lumen.app().shortcuts_of(name))
        .collect();
    let ctx = lumen.harness.ctx.clone();
    App::record_drawn_commands(&ctx);
    lumen.frames(5);
    let mut problems = Vec::new();
    let bar = controls(&lumen, |r| r.top() < BAR_HEIGHT);
    let drawn = App::drawn_commands(&ctx);
    check(BAR, &bar, &drawn, &chords, "bar", &mut problems);

    // The menu's controls are the nodes below the bar that were not there
    // before it opened, by node rather than by name: a menu row can share a
    // name with another control. The bar is left out: its audio chip is
    // drawn anew as its status changes ("Audio: Dropouts ×3").
    let before: Vec<String> = controls(&lumen, |_| true)
        .into_iter()
        .map(|c| c.id)
        .collect();
    let mut menu: Vec<Control> = Vec::new();
    let mut take = |lumen: &Lumen| {
        for control in controls(lumen, |r| r.center().y > BAR_HEIGHT) {
            if !before.contains(&control.id) && !menu.iter().any(|c| c.id == control.id) {
                menu.push(control);
            }
        }
    };
    lumen.click("Lumen");
    take(&lumen);
    for submenu in SUBMENUS {
        let at = lumen.harness.get_by_label(submenu).rect().center();
        lumen.harness.hover_at(at);
        lumen.frames(30);
        if submenu == "New from template" {
            // The folder is read in the background, again once the first
            // listing (at start, before the template was made) is stale.
            lumen.run_until("the template listed", |l| l.shows("Beat"));
        }
        take(&lumen);
    }
    let drawn = App::drawn_commands(&ctx);
    check(MENU, &menu, &drawn, &chords, "app menu", &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");
}
