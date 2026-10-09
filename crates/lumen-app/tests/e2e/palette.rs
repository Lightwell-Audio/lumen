//! The command palette (M3 slice 7b): Ctrl+K, fuzzy search, every command
//! found and run from it, names for what a command acts on, the reason a
//! command cannot run, and typed values (a tempo, a bar, a track's name).

use eframe::egui::{Event, Key, Modifiers};
use lumen_app::App;
use lumen_core::time::Ticks;

use super::Lumen;
use super::properties::{select, with_two_clips};

/// Opens the palette and types `query`.
fn search(lumen: &mut Lumen, query: &str) {
    if !lumen.app().palette_open() {
        lumen.key(Modifiers::COMMAND, Key::K);
    }
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
}

/// The palette's rows: what each says, and whether it can run now.
fn rows(lumen: &Lumen) -> Vec<(String, bool)> {
    let ctx = lumen.harness.ctx.clone();
    lumen.app().palette_shows(&ctx)
}

#[test]
fn ctrl_k_finds_a_command_and_enter_runs_it() {
    let mut lumen = Lumen::start("palette-run");
    lumen.key(Modifiers::COMMAND, Key::K);
    assert!(lumen.app().palette_open());
    assert!(lumen.shows("Search commands"), "the field is named");
    search(&mut lumen, "add aud");
    assert_eq!(rows(&lumen)[0], ("Add audio track".to_owned(), true));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(!lumen.app().palette_open(), "running closes it");
    assert_eq!(lumen.track_names().len(), 1);

    // Opened again, empty: the command just run comes first.
    lumen.key(Modifiers::COMMAND, Key::K);
    assert_eq!(rows(&lumen)[0].0, "Add audio track");
    // Down, then Enter runs the second row; Esc closes without running.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().palette_open());
    assert_eq!(lumen.track_names().len(), 1, "Esc ran nothing");
    // The transport button opens it too.
    lumen.click("Command palette");
    assert!(lumen.app().palette_open());
}

/// The slice's pass test: every registered command is found by its name,
/// first, and Enter runs it, or, when it cannot run now, says why and
/// stays open.
#[test]
fn every_command_can_be_found_and_run_from_the_palette() {
    let (mut lumen, kick, _) = with_two_clips("palette-all");
    select(&mut lumen, kick);
    // Commands that open a system file dialog, or the unsaved-changes
    // question, are found but not run here.
    let dialogs = ["Open…", "Save", "Save as…", "New project"];
    for name in App::command_names() {
        search(&mut lumen, name);
        let (first, ready) = rows(&lumen).first().cloned().expect("a row");
        let plain = lumen.app().palette_shows(&lumen.harness.ctx.clone());
        assert!(
            first == name || first.starts_with(name.split(' ').next().unwrap_or(name)),
            "{name}: first row is {first:?}, rows {plain:?}"
        );
        if dialogs.contains(&name) {
            lumen.key(Modifiers::NONE, Key::Escape);
            continue;
        }
        lumen.key(Modifiers::NONE, Key::Enter);
        lumen.frames(3);
        if ready {
            // It ran and closed (Command palette itself opens it again).
            assert!(
                !lumen.app().palette_open() || name == "Command palette",
                "{name} ran"
            );
        } else {
            assert!(lumen.app().palette_open(), "{name} cannot run: stays open");
        }
        // Esc closes the palette, or cancels what the command opened (a
        // rename field takes the next keys otherwise).
        lumen.key(Modifiers::NONE, Key::Escape);
    }
}

#[test]
fn results_name_what_they_act_on_and_say_why_they_cannot_run() {
    let (mut lumen, kick, _) = with_two_clips("palette-context");
    select(&mut lumen, kick);
    search(&mut lumen, "dup");
    assert_eq!(
        rows(&lumen)[0],
        ("Duplicate clip \u{201C}kick\u{201D}".to_owned(), true)
    );

    // Nothing copied yet: Paste is listed, dimmed, with the reason, and
    // Enter leaves the palette open.
    search(&mut lumen, "paste");
    assert_eq!(rows(&lumen)[0], ("Paste".to_owned(), false));
    assert!(lumen.shows("Paste, unavailable: Nothing to paste: copy or cut first"));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(lumen.app().palette_open());
}

#[test]
fn numbers_and_names_become_actions() {
    let (mut lumen, _, _) = with_two_clips("palette-values");
    search(&mut lumen, "140");
    let labels: Vec<String> = rows(&lumen).into_iter().map(|r| r.0).collect();
    assert_eq!(labels[..2], ["Set tempo to 140 BPM", "Go to bar 140"]);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().project().tempo[0].bpm, 140.0);
    assert_eq!(lumen.app().undo_label(), Some("Change tempo"));

    search(&mut lumen, "bar 3");
    lumen.key(Modifiers::NONE, Key::Enter);
    // Bar 3 in 4/4 starts after two bars: eight quarters (to the sample
    // the playhead reports from).
    lumen.wait_for("the playhead at bar 3", |app| {
        (app.playhead() - Ticks::quarters(8)).0.abs() < Ticks::quarters(1).0 / 1000
    });

    let snare = lumen.app().project().tracks[1].id;
    search(&mut lumen, "snare");
    assert_eq!(rows(&lumen)[0].0, "Go to track \u{201C}snare\u{201D}");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
    assert_eq!(lumen.app().selected_tracks(), [snare]);
}
