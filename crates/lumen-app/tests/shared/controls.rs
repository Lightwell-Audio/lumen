//! The transport bar's controls and the app menu's rows, each with the
//! command it runs or why it has none (M9 R7). The E2E coverage scenario
//! (`tests/e2e/coverage.rs`) checks the app against these lists, and the
//! docs exporter (`tests/docs_data.rs`) publishes them for the user
//! documentation's reference (docs/USER_DOCS.md §5.1).

#![allow(
    dead_code,
    reason = "each test target that includes this file uses part of it"
)]

/// What a control is in the lists: the command it runs, by name, or why it
/// has none. `Each` is one row of a list the user fills (a template, a
/// recent file), drawn here for the one the test makes: it runs no command
/// and is documented with its submenu.
pub enum Has {
    Command(&'static str),
    Without(&'static str),
    Each(&'static str),
}
pub use Has::{Command, Each, Without};

/// The transport bar, by each control's accessible name. A name that ends
/// in a colon or " …" goes on with a value ("Position 1.1.1", "Audio: On").
pub const BAR: &[(&str, Has)] = &[
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
    ("Tap tempo", Command("Tap tempo")),
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
pub const MENU: &[(&str, Has)] = &[
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
    ("History…", Command("History")),
    ("Appearance", Without("a submenu")),
    ("Preferences…", Command("Preferences…")),
    ("Set up Lumen…", Command("Set up Lumen…")),
    ("Keyboard shortcuts…", Command("Keyboard shortcuts")),
    // New from template, with the template the test saves.
    (
        "Beat",
        Each("a template: the palette lists each one as New from template"),
    ),
    ("Open templates folder", Command("Open templates folder")),
    // Open recent, with the project the test saves.
    ("Song", Each("a recent file, opened by its path")),
    // More ways to save.
    ("Save a copy…", Command("Save a copy…")),
    ("Collect and save", Command("Collect and save")),
    ("Save as template…", Command("Save as template…")),
    ("Revert to saved…", Command("Revert to saved…")),
    // Appearance: a quick change of preferences the Appearance page of
    // Preferences holds too.
    ("Dark", Without("a preference")),
    ("Light", Without("a preference")),
    ("Match system", Without("a preference")),
    ("100%", Without("a preference")),
    ("125%", Without("a preference")),
    ("150%", Without("a preference")),
    ("175%", Without("a preference")),
    ("200%", Without("a preference")),
    ("Reduce motion", Without("a preference")),
];
