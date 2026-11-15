use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui::{self, Ui};
use lumen_core::product::PRODUCT_NAME;

use crate::association::{Association, State, Status};
use crate::widgets::{hint, raised};
use crate::{App, widgets};

#[derive(Default)]
pub(crate) struct Reported {
    status: Option<Status>,
    pending: Option<Receiver<Status>>,
    tried: bool,
    stale: bool,
    shown: bool,
    start_pending: bool,
    failure: Option<String>,
}

impl Reported {
    pub(crate) fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    pub(crate) fn set(&mut self, status: Status) {
        self.status = Some(status);
        self.pending = None;
        self.stale = false;
    }

    pub(crate) fn start_registers(&mut self, yes: bool) {
        self.start_pending = yes;
    }

    fn take_start(&mut self) -> bool {
        std::mem::take(&mut self.start_pending)
    }

    fn wants(&self, shown: bool) -> bool {
        (!self.tried && self.status.is_none()) || self.stale || (shown && !self.shown)
    }

    fn ask(&mut self, ctx: &egui::Context, association: &Association) {
        if self.pending.is_some() {
            return;
        }
        self.stale = false;
        self.tried = true;
        let (tx, rx) = mpsc::channel();
        let (hive, exe, ctx) = (association.hive(), association.exe.clone(), ctx.clone());
        let spawned = crate::spawn::spawn_named("lumen-association", move || {
            let status = match hive.lock() {
                Ok(hive) => crate::association::status(hive.as_ref(), &exe),
                Err(_) => crate::association::unreadable(),
            };
            let _ = tx.send(status);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                self.failure = Some(crate::spawn::start_failed(
                    "the check of which program opens project files",
                    &error,
                ));
            }
        }
    }

    fn poll(&mut self) -> Option<Status> {
        let status = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        self.status = Some(status.clone());
        Some(status)
    }

    fn wrote(&mut self, written: Option<bool>) {
        let Some(on) = written else {
            return;
        };
        let chosen_elsewhere = self
            .status
            .as_ref()
            .and_then(|status| status.chosen_elsewhere.clone());
        self.status = Some(Status {
            state: if on { State::This } else { State::Off },
            chosen_elsewhere,
            will_ask: false,
        });
        self.pending = None;
        self.stale = true;
    }

    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

impl App {
    pub(crate) fn association_status(&self) -> Option<&Status> {
        self.association_status.status()
    }

    pub(crate) fn poll_association(&mut self, ctx: &egui::Context) {
        let shown = self.preferences.open || self.welcome.open;
        if self.association_status.wants(shown) {
            self.association_status.ask(ctx, &self.association);
        }
        self.association_status.shown = shown;
        if self.association_status.poll().is_some() && self.association_status.take_start() {
            self.associate_at_start();
        }
        if let Some(why) = self.association_status.take_failure() {
            self.say(why);
        }
    }

    pub(crate) fn set_association(&mut self, on: bool) {
        self.association.set(on);
        self.association_status
            .wrote(self.association.error.is_none().then_some(on));
        self.settings.projects.associate_files = on;
        self.settings_changed = Some(Instant::now());
    }

    pub(crate) fn associate_at_start(&mut self) {
        let Some(state) = self.association_status().map(|status| status.state.clone()) else {
            return;
        };
        if !Association::supported()
            || !self.settings.projects.associate_files
            || state != State::Off
        {
            return;
        }
        self.set_association(true);
        let error = self.association.error.clone();
        crate::trace::log(|| match error {
            None => "association: registered this copy at start".to_owned(),
            Some(e) => format!("association: not registered at start ({e})"),
        });
    }

    pub(crate) fn toggle_association(&mut self) {
        let on = self
            .association_status()
            .is_none_or(|status| status.state != State::This);
        self.set_association(on);
        let words = match (&self.association.error, on) {
            (Some(e), _) => e.clone(),
            (None, true) => format!("Projects now open with {PRODUCT_NAME} from File Explorer"),
            (None, false) => format!("Projects no longer open with this {PRODUCT_NAME}"),
        };
        self.say(words);
    }

    pub(crate) fn association_row(&mut self, ui: &mut Ui) {
        let supported = Association::supported();
        let status = self.association_status().cloned();
        let mut on = status
            .as_ref()
            .is_some_and(|status| status.state == State::This);
        let label = format!("Open .lumen files with {PRODUCT_NAME}");
        let known = status.is_some();
        let changed = widgets::switch(ui, &mut on, &label, supported && known)
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
        let Some(status) = status else {
            return;
        };
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::association::{MapHive, State};

    use super::*;

    fn association(registered: bool) -> Association {
        let exe = PathBuf::from(r"C:\Apps\Lumen\lumen.exe");
        let mut association = Association::with(Box::new(MapHive::default()), exe);
        if registered {
            association.set(true);
        }
        association
    }

    #[test]
    fn the_status_arrives_from_a_worker_and_until_then_nothing_is_drawn() {
        let ctx = egui::Context::default();
        let association = association(true);
        let mut reported = Reported::default();
        reported.ask(&ctx, &association);
        assert!(reported.status().is_none(), "not read yet");
        let status = lumen_testkit::wait_for("the worker's status", || reported.poll());
        assert_eq!(status.state, State::This);
        assert_eq!(
            reported.status().map(|status| status.state.clone()),
            Some(State::This),
            "and kept for the frames that follow"
        );
    }

    #[test]
    fn a_change_shows_at_once_and_is_confirmed_by_a_read() {
        let mut reported = Reported::default();
        reported.wrote(Some(true));
        assert_eq!(
            reported.status().map(|status| status.state.clone()),
            Some(State::This)
        );
        assert!(reported.stale, "the worker looks again");
        let before = reported.status().cloned();
        reported.wrote(None);
        assert_eq!(reported.status().cloned(), before);
    }

    #[test]
    fn a_read_is_wanted_once_until_a_window_opens_or_the_answer_is_stale() {
        let mut reported = Reported::default();
        assert!(reported.wants(false), "the first look");
        reported.tried = true;
        assert!(!reported.wants(false), "not asked again every frame");
        assert!(reported.wants(true), "a window showing the row opened");
        reported.shown = true;
        assert!(!reported.wants(true), "and it stays open");
        reported.stale = true;
        assert!(reported.wants(true), "a change asks again");
    }

    #[test]
    fn a_seeded_hive_drops_a_read_that_predates_it() {
        let ctx = egui::Context::default();
        let association = association(false);
        let mut reported = Reported::default();
        reported.ask(&ctx, &association);
        reported.set(crate::association::Status {
            state: State::Other(PathBuf::from(r"D:\Old\lumen.exe")),
            chosen_elsewhere: None,
            will_ask: false,
        });
        assert!(!reported.stale);
        assert!(reported.pending.is_none());
        assert!(matches!(
            reported.status().map(|status| &status.state),
            Some(State::Other(_))
        ));
    }
}
