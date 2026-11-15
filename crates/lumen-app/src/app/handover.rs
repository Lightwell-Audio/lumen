use std::path::{Path, PathBuf};

use eframe::egui;

use super::projects::Next;
use crate::instance::{Incoming, Reply, Request};
use crate::widgets::raised;
use crate::{App, files};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Holding {
    pub(crate) next: Next,
    choice_made: bool,
    mark: u64,
}

pub(crate) struct Checked {
    incoming: Option<Incoming>,
    compared: Option<PathBuf>,
    result: Result<(PathBuf, bool), String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    Recording,
    Export,
    Saving,
    Opening,
}

impl App {
    pub(crate) fn busy_for_open(&self) -> Option<Busy> {
        if self.is_recording() || self.audio_takes_finishing() {
            Some(Busy::Recording)
        } else if self.export.busy() {
            Some(Busy::Export)
        } else if self.saving.is_some()
            || self.save_waiting.is_some()
            || self.save_dialog
            || self.collecting.is_some()
        {
            Some(Busy::Saving)
        } else if self.opening.is_some() {
            Some(Busy::Opening)
        } else {
            None
        }
    }

    pub(crate) fn holds(&mut self, next: &Next, choice_made: bool) -> bool {
        let mark = self.document.change_mark();
        self.hold(next, choice_made, mark)
    }

    pub(crate) fn hold(&mut self, next: &Next, choice_made: bool, mark: u64) -> bool {
        let replaces = matches!(next, Next::New | Next::Open(Some(_)) | Next::Template(_));
        if !replaces || self.busy_for_open().is_none() {
            return false;
        }
        crate::trace::log(|| format!("files: {next:?} waits for the work under way"));
        self.holding = Some(Holding {
            next: next.clone(),
            choice_made,
            mark,
        });
        true
    }

    pub(crate) fn release_holding(&mut self, ctx: &egui::Context) {
        if self.holding.is_none() || self.busy_for_open().is_some() {
            return;
        }
        let Some(held) = self.holding.take() else {
            return;
        };
        if held.choice_made && held.mark == self.document.change_mark() {
            self.proceed(ctx, held.next);
        } else {
            self.request(ctx, held.next);
        }
    }

    pub(crate) fn poll_instance(&mut self, ctx: &egui::Context) {
        while let Some(incoming) = self.instance.as_ref().and_then(|i| i.next()) {
            self.take_request(ctx, incoming);
        }
        let checked: Vec<Checked> = self
            .checks
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for checked in checked {
            self.take_checked(ctx, checked);
        }
    }

    fn take_request(&mut self, ctx: &egui::Context, incoming: Incoming) {
        let request = incoming.request.clone();
        crate::trace::log(|| format!("instance: {request:?}"));
        match request {
            Request::Show => {
                if incoming.answer(Reply::Accepted) {
                    come_to_front(ctx);
                }
            }
            Request::Open(path) => self.check(ctx, path, Some(incoming)),
        }
    }

    pub(crate) fn open_handed(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.check(ctx, path, None);
    }

    fn check(&mut self, ctx: &egui::Context, path: PathBuf, incoming: Option<Incoming>) {
        let (tx, _) = self.checks.get_or_insert_with(std::sync::mpsc::channel);
        let tx = tx.clone();
        let open = self.document.path.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("lumen-handover-check".into())
            .spawn(move || {
                let result = project_file(&path).map(|path| {
                    let same = open.as_deref().is_some_and(|o| same_file(o, &path));
                    (path, same)
                });
                let _ = tx.send(Checked {
                    incoming,
                    compared: open,
                    result,
                });
                ctx.request_repaint();
            });
        if let Err(e) = spawned {
            crate::trace::log(|| format!("instance: the file could not be checked ({e})"));
        }
    }

    fn take_checked(&mut self, ctx: &egui::Context, checked: Checked) {
        let (path, same) = match checked.result {
            Ok(found) => found,
            Err(reason) => {
                match checked.incoming {
                    Some(incoming) => {
                        incoming.answer(Reply::Rejected(reason));
                    }
                    None => self.say(reason),
                }
                return;
            }
        };
        if let Some(incoming) = checked.incoming {
            if !incoming.answer(Reply::Accepted) {
                return;
            }
            come_to_front(ctx);
        }
        if !same || self.document.path != checked.compared {
            self.request(ctx, Next::Open(Some(path)));
        }
    }

    pub(crate) fn holding_banner(&mut self, ui: &mut egui::Ui) {
        let Some(held) = self.holding.clone() else {
            return;
        };
        let Some(busy) = self.busy_for_open() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let recording = self.is_recording();
        let mut stop = false;
        let mut cancel = false;
        egui::Panel::top("holding banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(words(&held.next, busy, recording));
                    if busy == Busy::Recording
                        && recording
                        && raised::primary_button(ui, "Stop and open").clicked()
                    {
                        stop = true;
                    }
                    if raised::button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.holding = None;
        } else if stop {
            self.transport.stop();
            ctx.request_repaint();
        }
    }
}

pub(crate) fn words(next: &Next, busy: Busy, recording: bool) -> String {
    let what = match next {
        Next::Open(Some(path)) => format!("{} opens", file_name(path)),
        Next::Template(path) => format!("The project from {} starts", file_name(path)),
        _ => "The new project starts".to_owned(),
    };
    let when = match busy {
        Busy::Recording if recording => "when recording stops",
        Busy::Recording => "once the recorded takes are saved",
        Busy::Export => "when the export is done",
        Busy::Saving => "once the project is saved",
        Busy::Opening => "after the project being opened now",
    };
    format!("{what} {when}. Nothing is replaced before then.")
}

pub(crate) fn left_out_words(left: usize) -> String {
    let (files, were) = if left == 1 {
        ("file", "was")
    } else {
        ("files", "were")
    };
    format!("Only the project opens: the {left} other {files} in the drop {were} not added.")
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn project_file(path: &Path) -> Result<PathBuf, String> {
    if !files::is_project(path) {
        return Err(format!(
            "{} is not a {} project.",
            file_name(path),
            lumen_core::product::PRODUCT_NAME
        ));
    }
    if !path.is_file() {
        return Err(format!("{} was not found.", path.display()));
    }
    Ok(path.to_path_buf())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn come_to_front(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Informational,
    ));
    ctx.request_repaint();
}

pub(crate) fn interrupted(busy: Option<Busy>, what: &str) -> String {
    match busy {
        Some(Busy::Recording) => format!("A recording started while {what} was read"),
        Some(Busy::Export) => format!("An export started while {what} was read"),
        Some(Busy::Saving) => format!("A save started while {what} was read"),
        Some(Busy::Opening) => format!("Another project was asked for while {what} was read"),
        None => format!("The project changed while {what} was read"),
    }
}

pub(crate) fn when_done(busy: Option<Busy>) -> &'static str {
    match busy {
        Some(Busy::Recording) => " when recording stops",
        Some(Busy::Export) => " when the export is done",
        Some(Busy::Saving) => " once the project is saved",
        Some(Busy::Opening) => " once it is open",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_the_file_and_when_it_opens() {
        let next = Next::Open(Some(PathBuf::from("Songs").join("Night Drive.lumen")));
        assert_eq!(
            words(&next, Busy::Recording, true),
            "Night Drive.lumen opens when recording stops. Nothing is replaced before then."
        );
        assert!(words(&next, Busy::Export, false).contains("when the export is done"));
        assert!(words(&Next::New, Busy::Recording, false).starts_with("The new project"));
        assert!(words(&next, Busy::Saving, false).contains("once the project is saved"));
        assert!(words(&next, Busy::Opening, false).contains("after the project being opened"));
        assert_eq!(when_done(Some(Busy::Saving)), " once the project is saved");
    }

    #[test]
    fn only_an_existing_project_file_is_taken() {
        let dir = lumen_project::testing::TempDir::new("handover-file");
        let song = dir.path().join("Song.LUMEN");
        assert!(project_file(&song).unwrap_err().contains("was not found"));
        std::fs::write(&song, "{}").unwrap();
        assert_eq!(project_file(&song).unwrap(), song);
        let wav = dir.path().join("kick.wav");
        std::fs::write(&wav, "").unwrap();
        assert!(project_file(&wav).unwrap_err().contains("is not a"));
    }
}
