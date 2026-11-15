use eframe::egui::Rect;
use eframe::egui::accesskit::{Action, Role};
use egui_kittest::kittest::{NodeT, Queryable, by};
use lumen_app::App;

use super::Lumen;

use crate::controls::{BAR, Command, Each, Has, MENU, Without};

const BAR_HEIGHT: f32 = 44.0;

const SUBMENUS: [&str; 4] = [
    "New from template",
    "Open recent",
    "More ways to save",
    "Appearance",
];

struct Control {
    id: String,
    name: String,
}

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

fn drawn_name<'a>(name: &'a str, chords: &[String]) -> &'a str {
    chords
        .iter()
        .find_map(|chord| name.strip_suffix(chord.as_str())?.strip_suffix(' '))
        .unwrap_or(name)
}

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
            Without(why) | Each(why) if !runs.is_empty() => problems.push(format!(
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
            lumen.run_until("the template listed", |l| l.shows("Beat"));
        }
        take(&lumen);
    }
    let drawn = App::drawn_commands(&ctx);
    check(MENU, &menu, &drawn, &chords, "app menu", &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");
}
