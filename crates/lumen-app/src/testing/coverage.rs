//! Which command each control is drawn for (M9 R7, the command coverage
//! test): transport-bar buttons and menu rows note their label and the
//! command whose keys they show while a test records them. Nothing is kept
//! otherwise, so the app pays one look-up per control.

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::App;
use crate::commands::Command;

/// The controls drawn since recording began: each label and the name of
/// its command, once.
#[derive(Clone, Default)]
struct Drawn(Arc<Mutex<Vec<(String, &'static str)>>>);

fn key() -> egui::Id {
    egui::Id::new("coverage: drawn commands")
}

/// Notes that the control `label` was drawn for `command`, if a test is
/// recording.
pub(crate) fn drawn(ctx: &egui::Context, label: &str, command: Command) {
    let Some(drawn) = ctx.data(|d| d.get_temp::<Drawn>(key())) else {
        return;
    };
    let Ok(mut list) = drawn.0.lock() else {
        return;
    };
    let entry = (label.to_owned(), command.name());
    if !list.contains(&entry) {
        list.push(entry);
    }
}

/// The Preferences rows drawn since recording began, by key (search's rows,
/// `preferences::ROWS`).
#[derive(Clone, Default)]
struct Rows(Arc<Mutex<Vec<&'static str>>>);

fn rows_key() -> egui::Id {
    egui::Id::new("coverage: drawn preference rows")
}

/// Notes that the Preferences row `key` was drawn, if a test is recording.
pub(crate) fn row_drawn(ctx: &egui::Context, key: &'static str) {
    let Some(rows) = ctx.data(|d| d.get_temp::<Rows>(rows_key())) else {
        return;
    };
    if let Ok(mut list) = rows.0.lock()
        && !list.contains(&key)
    {
        list.push(key);
    }
}

impl App {
    /// Starts noting which Preferences rows are drawn.
    pub fn record_drawn_rows(ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(rows_key(), Rows::default()));
    }

    /// The Preferences rows drawn since [`Self::record_drawn_rows`], by key.
    pub fn drawn_rows(ctx: &egui::Context) -> Vec<&'static str> {
        ctx.data(|d| d.get_temp::<Rows>(rows_key()))
            .and_then(|rows| rows.0.lock().ok().map(|list| list.clone()))
            .unwrap_or_default()
    }

    /// Starts noting which command each control is drawn for.
    pub fn record_drawn_commands(ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(key(), Drawn::default()));
    }

    /// The controls drawn since [`Self::record_drawn_commands`]: each label
    /// and its command's name.
    pub fn drawn_commands(ctx: &egui::Context) -> Vec<(String, &'static str)> {
        ctx.data(|d| d.get_temp::<Drawn>(key()))
            .and_then(|drawn| drawn.0.lock().ok().map(|list| list.clone()))
            .unwrap_or_default()
    }
}
