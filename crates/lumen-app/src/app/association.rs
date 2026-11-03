//! The "Open .lumen files" row (Preferences › Files, and the first-run
//! Ready step) and its command (M9 R8, FA A).

use std::time::Instant;

use eframe::egui::Ui;
use lumen_core::product::PRODUCT_NAME;

use crate::association::{Association, State};
use crate::widgets::{hint, raised};
use crate::{App, widgets};

impl App {
    /// Registers this copy or removes its registration, and remembers the
    /// wish.
    pub(crate) fn set_association(&mut self, on: bool) {
        self.association.set(on);
        self.settings.projects.associate_files = on;
        self.settings_changed = Some(Instant::now());
    }

    /// A normal start (owner, 2026-11-03): registers this copy when no
    /// copy of the app is registered and the setting wants it, as it does
    /// by default. Another copy that owns it, the setting turned off and
    /// the person's own "Open with" choice are left as they are.
    pub(crate) fn associate_at_start(&mut self) {
        if !Association::supported()
            || !self.settings.projects.associate_files
            || self.association.status().state != State::Off
        {
            return;
        }
        self.association.set(true);
        let error = self.association.error.clone();
        crate::trace::log(|| match error {
            None => "association: registered this copy at start".to_owned(),
            Some(e) => format!("association: not registered at start ({e})"),
        });
    }

    /// The command: on when this copy is not the one registered.
    pub(crate) fn toggle_association(&mut self) {
        let on = self.association.status().state != State::This;
        self.set_association(on);
        let words = match (&self.association.error, on) {
            (Some(e), _) => e.clone(),
            (None, true) => format!("Projects now open with {PRODUCT_NAME} from File Explorer"),
            (None, false) => format!("Projects no longer open with this {PRODUCT_NAME}"),
        };
        self.say(words);
    }

    /// The switch and what it says about who opens projects now.
    pub(crate) fn association_row(&mut self, ui: &mut Ui) {
        let supported = Association::supported();
        let status = self.association.status();
        let mut on = status.state == State::This;
        let label = format!("Open .lumen files with {PRODUCT_NAME}");
        let changed = widgets::switch(ui, &mut on, &label, supported)
            .on_hover_text("Projects open here from File Explorer, each with the project icon.")
            .changed();
        if changed {
            self.set_association(on);
        }
        if !supported {
            hint(
                ui,
                "On this system, project files are set up when the app is installed.",
            );
            return;
        }
        if let Some(error) = self.association.error.clone() {
            hint(ui, &error);
        }
        match &status.state {
            State::Other(path) => {
                hint(
                    ui,
                    &format!(
                        "Another copy of {PRODUCT_NAME} opens them now: {}.",
                        path.display()
                    ),
                );
                if raised::button(ui, "Use this copy").clicked() {
                    self.set_association(true);
                }
            }
            State::This if status.chosen_elsewhere.is_none() => hint(
                ui,
                "Double-click a project in File Explorer to open it here.",
            ),
            _ => {}
        }
        if status.chosen_elsewhere.is_some() {
            hint(
                ui,
                "You chose another program for .lumen files in Windows, and Windows keeps that \
                 choice. Change it in Default apps.",
            );
            if raised::button(ui, "Open Default apps").clicked() {
                self.association.open_default_apps();
            }
        } else if status.will_ask {
            hint(
                ui,
                "Windows will ask which program to use the next time a project is opened.",
            );
        }
    }
}
