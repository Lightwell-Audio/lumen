use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::App;
use crate::commands::Command;

#[derive(Clone, Default)]
struct Drawn(Arc<Mutex<Vec<(String, &'static str)>>>);

fn key() -> egui::Id {
    egui::Id::new("coverage: drawn commands")
}

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

#[derive(Clone, Default)]
struct Rows(Arc<Mutex<Vec<&'static str>>>);

fn rows_key() -> egui::Id {
    egui::Id::new("coverage: drawn preference rows")
}

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
    pub fn record_drawn_rows(ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(rows_key(), Rows::default()));
    }

    pub fn drawn_rows(ctx: &egui::Context) -> Vec<&'static str> {
        ctx.data(|d| d.get_temp::<Rows>(rows_key()))
            .and_then(|rows| rows.0.lock().ok().map(|list| list.clone()))
            .unwrap_or_default()
    }

    pub fn record_drawn_commands(ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(key(), Drawn::default()));
    }

    pub fn drawn_commands(ctx: &egui::Context) -> Vec<(String, &'static str)> {
        ctx.data(|d| d.get_temp::<Drawn>(key()))
            .and_then(|drawn| drawn.0.lock().ok().map(|list| list.clone()))
            .unwrap_or_default()
    }
}
