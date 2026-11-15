use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;

use super::dialogs::{FileDialog, made};
use crate::App;
use crate::{arrangement, document, empty_project_at, files, lanes, trace};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Next {
    New,
    Open(Option<PathBuf>),
    Template(PathBuf),
    Quit,
}

impl App {
    pub(crate) fn begin_opening(&mut self, file: PathBuf) {
        self.opening_mark = self.document.change_mark();
        self.opening = Some(files::open(file));
    }

    pub(crate) fn request(&mut self, ctx: &egui::Context, next: Next) {
        if self.holds(&next, false) || self.copy_blocks(&next) {
            return;
        }
        if self.document.is_dirty() {
            self.prompt = Some(next);
        } else {
            self.proceed(ctx, next);
        }
    }

    fn copy_blocks(&mut self, next: &Next) -> bool {
        let blocks = self.collecting.is_some() && *next != Next::Quit;
        if blocks {
            self.say("A copy of the project is under way. Esc stops it".to_owned());
        }
        blocks
    }

    pub(crate) fn proceed(&mut self, ctx: &egui::Context, next: Next) {
        if self.holds(&next, true) || self.copy_blocks(&next) {
            return;
        }
        match next {
            Next::New => {
                let rate = Some(self.lanes.rate());
                self.replace_document(document::Document::new(empty_project_at(rate)));
                self.toast = Some(("New project".into(), Instant::now()));
            }
            Next::Open(Some(path)) => {
                if self.opening.is_none() {
                    trace::log(|| format!("files: opening {}", path.display()));
                    self.begin_opening(path);
                }
            }
            Next::Template(file) => {
                if self.opening.is_none() {
                    trace::log(|| format!("files: opening the template {}", file.display()));
                    self.templates.opening = true;
                    self.begin_opening(file);
                }
            }
            Next::Open(None) => {
                if self.dialog.is_some() {
                    return;
                }
                let start = self.dialog_start();
                let owner = self.owner;
                let show = move || {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Open project")
                        .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION]);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(made(dir));
                    }
                    if let Some(owner) = &owner {
                        dialog = dialog.set_parent(owner);
                    }
                    FileDialog::Open(dialog.pick_file())
                };
                self.file_dialog(ctx, show, |paths| {
                    FileDialog::Open(paths.into_iter().next())
                });
            }
            Next::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub(crate) fn replace_document(&mut self, document: document::Document) {
        self.transport.stop();
        self.document = document;
        self.document
            .set_undo_limit(self.settings.editing.undo_steps);
        self.lanes = lanes::Lanes::default();
        self.follow_jobs = Default::default();
        self.arrangement = arrangement::Arrangement::default();
        self.chain.folded.clear();
        self.chain.shown.clear();
        self.chain.deferred.clear();
        self.chain.panel = None;
        self.chain.selected = None;
        self.synced = None;
        self.normalizing = None;
        self.consolidating = None;
        self.bounce_waiting = None;
        self.bouncing = None;
        self.show_bounce = false;
        self.freeze_waiting = None;
        self.freezing = None;
        self.show_history = false;
        self.tap.end();
        self.media_pool.reset();
        self.revert.kept = None;
        self.forget_passes();
        self.transport.locate(Ticks::ZERO);
    }
}
