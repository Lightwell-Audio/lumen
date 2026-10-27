//! The media pool in the app (M9 slice R4b, ADR-0106): the state of the
//! browser's Project media tab, relinking missing files, removing the ones
//! nothing plays, and clearing files that belong to no project. The rules are
//! `crate::pool`, `crate::relink` and `crate::strays`; the tab is drawn by
//! `browser::media_tab`.
//!
//! Everything that reads the disk runs on a worker and is answered through a
//! channel polled once a frame.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use lumen_project::model::{Id, Media};

use crate::app::commands::wait;
use crate::app::projects::FileDialog;
use crate::files::Control;
use crate::pool::{self, Folders, Row};
use crate::relink::{self, Found, Report, Stopped, Verdict};
use crate::strays::{self, Cleared, Strays};
use crate::widgets::raised;
use crate::{App, media};

/// Which rows the tab lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Filter {
    #[default]
    All,
    Missing,
    Unused,
}

/// What the disk said about the project's files and folders.
#[derive(Debug, Default)]
struct Scan {
    strays: Strays,
    /// Files the document knows that are no longer on disk.
    absent: Vec<Id>,
}

/// A folder search under way.
pub(crate) struct Search {
    rx: Receiver<Result<Report, Stopped>>,
    pub(crate) control: Control,
    pub(crate) folder: PathBuf,
}

/// A file the user chose for a missing item, being measured.
struct Checking {
    id: Id,
    rx: Receiver<(PathBuf, Verdict)>,
}

/// What the rows were made from, to know when to make them again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Made {
    session: u64,
    mark: u64,
    files: usize,
    scans: u64,
}

#[derive(Default)]
pub(crate) struct MediaPool {
    pub(crate) text: String,
    pub(crate) filter: Filter,
    pub(crate) selected: Option<Id>,
    pub(crate) focus_search: bool,
    rows: Vec<Row>,
    made: Option<Made>,
    scan: Scan,
    scanning: Option<Receiver<Scan>>,
    scans: u64,
    scanned: Option<Made>,
    /// The item a file dialog was opened for.
    locating: Option<Id>,
    checking: Option<Checking>,
    /// A file of another length, waiting for the user to say it will do.
    pub(crate) confirm: Option<(Id, PathBuf, Option<u64>)>,
    pub(crate) search: Option<Search>,
    /// What the last folder search found, waiting to be applied.
    pub(crate) report: Option<Report>,
    clearing: Option<Receiver<Cleared>>,
    /// Clear was pressed once: the next press moves the files.
    pub(crate) armed: bool,
    /// The banner about missing files was dismissed for this document.
    banner_off: Option<u64>,
    /// The last thing that happened, in a sentence, until the next.
    pub(crate) note: Option<String>,
}

impl MediaPool {
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(crate) fn strays(&self) -> &Strays {
        &self.scan.strays
    }

    /// Rows the filter and the words leave.
    pub(crate) fn shown(&self) -> Vec<&Row> {
        let words = self.text.trim().to_lowercase();
        self.rows
            .iter()
            .filter(|r| match self.filter {
                Filter::All => true,
                Filter::Missing => r.missing,
                Filter::Unused => r.unused(),
            })
            .filter(|r| words.is_empty() || r.name.to_lowercase().contains(&words))
            .collect()
    }

    pub(crate) fn missing(&self) -> usize {
        self.rows.iter().filter(|r| r.missing).count()
    }

    pub(crate) fn unused(&self) -> usize {
        self.rows.iter().filter(|r| r.unused()).count()
    }

    pub(crate) fn busy(&self) -> bool {
        self.search.is_some() || self.checking.is_some() || self.clearing.is_some()
    }

    /// Hides the missing-files banner for the document it is showing for.
    pub(crate) fn dismiss_banner(&mut self, session: u64) {
        self.banner_off = Some(session);
    }

    pub(crate) fn banner_dismissed(&self, session: u64) -> bool {
        self.banner_off == Some(session)
    }

    /// The pool belongs to one document: a new one starts it over.
    pub(crate) fn reset(&mut self) {
        *self = Self {
            text: std::mem::take(&mut self.text),
            filter: self.filter,
            ..Self::default()
        };
    }
}

impl App {
    /// Shows the browser on the Project media tab.
    pub(crate) fn show_media_pool(&mut self) {
        self.show_browser = true;
        self.browser.tab = crate::browser::Tab::Media;
    }

    /// The project's media that are not on this machine, as the project knows
    /// them: what a search looks for.
    pub(crate) fn missing_media(&self) -> Vec<Media> {
        let absent = &self.media_pool.scan.absent;
        self.document
            .project()
            .media
            .iter()
            .filter(|m| !self.document.media_files.contains_key(&m.id) || absent.contains(&m.id))
            .cloned()
            .collect()
    }

    /// Called every frame: answers from the workers, and the next scan when
    /// the project or its files changed.
    pub(crate) fn poll_media_pool(&mut self, ctx: &egui::Context) {
        if let Some(scan) = self
            .media_pool
            .scanning
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.media_pool.scanning = None;
            self.media_pool.scan = scan;
            self.media_pool.scans += 1;
        }
        self.poll_checked(ctx);
        self.poll_search(ctx);
        if let Some(cleared) = self
            .media_pool
            .clearing
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.media_pool.clearing = None;
            self.media_pool.armed = false;
            self.media_pool.scanned = None;
            self.media_pool.note = Some(cleared_note(&cleared));
        }
        // The rows are cheap, and kept for the frames the project does not
        // change; the disk is read only while the tab is on screen or a
        // banner needs the answer.
        let now = self.media_pool_key();
        if self.media_pool.made != Some(now) {
            let folders = Folders {
                renders: Some(self.renders_dir()),
                recordings: Some(self.recordings_dir()),
            };
            let files: HashMap<_, _> = self
                .document
                .media_files
                .iter()
                .filter(|(id, _)| !self.media_pool.scan.absent.contains(id))
                .map(|(id, path)| (*id, path.clone()))
                .collect();
            self.media_pool.rows = pool::rows(self.document.project(), &files, &folders);
            self.media_pool.made = Some(now);
            if self
                .media_pool
                .selected
                .is_some_and(|id| !self.media_pool.rows.iter().any(|r| r.id == id))
            {
                self.media_pool.selected = None;
            }
        }
        let watching = self.show_browser && self.browser.tab == crate::browser::Tab::Media;
        let wanted = Made { scans: 0, ..now };
        if watching
            && self.media_pool.scanning.is_none()
            && self.media_pool.scanned != Some(wanted)
            && !self.is_working_in_files()
        {
            self.media_pool.scanned = Some(wanted);
            self.media_pool.scanning = Some(self.start_scan());
            ctx.request_repaint();
        }
    }

    fn media_pool_key(&self) -> Made {
        Made {
            session: self.document.session(),
            mark: self.document.change_mark(),
            files: self.document.media_files.len(),
            scans: self.media_pool.scans,
        }
    }

    /// Whether something is writing audio into the work folders, so a file
    /// there that no entry names yet may be one being made.
    pub(crate) fn is_working_in_files(&self) -> bool {
        self.consolidating.is_some()
            || self.bouncing.is_some()
            || self.freezing.is_some()
            || self.normalizing.is_some()
            || self.is_recording()
    }

    fn start_scan(&self) -> Receiver<Scan> {
        let project = self.document.project();
        let inputs = strays::Inputs {
            project_dir: self
                .document
                .path
                .as_deref()
                .and_then(|p| p.parent())
                .map(PathBuf::from),
            media: project.media.clone(),
            files: self
                .document
                .media_files
                .iter()
                .map(|(id, path)| (*id, path.clone()))
                .collect(),
            renders: Some(self.renders_dir()),
            recordings: Some(self.recordings_dir()),
            autosaves: self
                .autosave
                .as_ref()
                .and_then(|a| a.dir())
                .map(PathBuf::from),
            session: self.autosave.as_ref().map(|a| a.session().to_owned()),
        };
        let files = self.document.media_files.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("lumen-pool-scan".into())
            .spawn(move || {
                let _ = tx.send(Scan {
                    absent: pool::absent(&files),
                    strays: strays::scan(&inputs),
                });
            })
            .expect("spawn media pool scan");
        rx
    }

    /// Asks which file stands in for the missing item `id`.
    pub(crate) fn locate_media(&mut self, ctx: &egui::Context, id: Id) {
        if self.dialog.is_some() || self.media_pool.checking.is_some() {
            return;
        }
        let name = self
            .document
            .project()
            .media
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.path.rsplit('/').next().unwrap_or(&m.path).to_owned());
        let Some(name) = name else { return };
        self.media_pool.locating = Some(id);
        let owner = self.owner;
        let show = move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title(format!("Locate {name}"))
                .add_filter("Audio", media::EXTENSIONS);
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::Locate(dialog.pick_file())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::Locate(paths.into_iter().next())
        });
    }

    /// The file dialog for Locate answered.
    pub(crate) fn located(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        let id = self.media_pool.locating.take();
        let (Some(id), Some(path)) = (id, path) else {
            return;
        };
        let Some(media) = self
            .document
            .project()
            .media
            .iter()
            .find(|m| m.id == id)
            .cloned()
        else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("lumen-relink-check".into())
            .spawn(move || {
                let verdict = relink::verdict(&media, &path);
                let _ = tx.send((path, verdict));
                ctx.request_repaint();
            })
            .expect("spawn relink check");
        self.media_pool.checking = Some(Checking { id, rx });
    }

    fn poll_checked(&mut self, _ctx: &egui::Context) {
        let Some(checking) = &self.media_pool.checking else {
            return;
        };
        let Ok((path, verdict)) = checking.rx.try_recv() else {
            return;
        };
        let id = checking.id;
        self.media_pool.checking = None;
        match verdict {
            Verdict::Exact | Verdict::SameAudio => self.relink_media(id, path),
            Verdict::DifferentLength(frames) => {
                self.media_pool.confirm = Some((id, path, frames));
            }
            Verdict::Refused(why) => {
                self.media_pool.note = Some(format!("That file will not do. {why}"));
            }
        }
    }

    /// Puts a file where the project lost one, and says so.
    pub(crate) fn relink_media(&mut self, id: Id, path: PathBuf) {
        let name = file_name(&path);
        if self.document.relink(id, path) {
            self.media_pool.confirm = None;
            self.media_pool.note = Some(format!("Relinked {name}. Save to keep it."));
            self.media_pool.selected = Some(id);
        }
    }

    /// The user said the file of another length will do.
    pub(crate) fn accept_confirmed(&mut self) {
        if let Some((id, path, _)) = self.media_pool.confirm.take() {
            self.relink_media(id, path);
        }
    }

    /// Asks which folder to search for the missing files.
    pub(crate) fn search_folder_for_media(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some()
            || self.media_pool.search.is_some()
            || self.missing_media().is_empty()
        {
            return;
        }
        let start = self.dialog_start();
        let owner = self.owner;
        let show = move || {
            let mut dialog = rfd::FileDialog::new().set_title("Search a folder for missing audio");
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::RelinkFolder(dialog.pick_folder())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::RelinkFolder(paths.into_iter().next())
        });
    }

    /// The folder dialog answered: the search begins.
    pub(crate) fn searching_folder(&mut self, ctx: &egui::Context, folder: PathBuf) {
        let wanted = self.missing_media();
        if wanted.is_empty() {
            return;
        }
        let control = Control::default();
        let rx = relink::start(folder.clone(), wanted, control.clone());
        self.media_pool.report = None;
        self.media_pool.search = Some(Search {
            rx,
            control,
            folder,
        });
        self.show_media_pool();
        ctx.request_repaint();
    }

    fn poll_search(&mut self, ctx: &egui::Context) {
        let Some(search) = &self.media_pool.search else {
            return;
        };
        let Ok(result) = search.rx.try_recv() else {
            // Moves the progress bar.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.media_pool.search = None;
        match result {
            Ok(report) => {
                self.media_pool.note = None;
                self.media_pool.report = Some(report);
            }
            Err(Stopped) => {
                self.media_pool.note = Some("Search stopped. Nothing was changed.".into())
            }
        }
    }

    /// Esc, or the Stop button.
    pub(crate) fn stop_search(&mut self) {
        if let Some(search) = &self.media_pool.search {
            search.control.stop();
        }
    }

    /// Relinks everything the last search found that is safe to use.
    pub(crate) fn apply_report(&mut self) {
        let Some(report) = self.media_pool.report.take() else {
            return;
        };
        let mut done = 0;
        for Found { id, path, .. } in report.found {
            if self.document.relink(id, path) {
                done += 1;
            }
        }
        let left = report.different.len() + report.ambiguous.len() + report.not_found.len();
        let mut note = format!(
            "Relinked {done} {}.",
            if done == 1 { "file" } else { "files" }
        );
        if left > 0 {
            note.push_str(&format!(" {left} still missing. Use Locate for those."));
        }
        if done > 0 {
            note.push_str(" Save to keep it.");
        }
        self.media_pool.note = Some(note);
    }

    /// Drops from the project the entries nothing plays, as one undo step.
    pub(crate) fn remove_unused_media(&mut self) {
        let ids: Vec<Id> = self
            .media_pool
            .rows
            .iter()
            .filter(|r| r.unused())
            .map(|r| r.id)
            .collect();
        let removed = self.document.remove_unused_media(&ids);
        self.media_pool.note = Some(match removed {
            0 => "Every file in the project is used.".to_owned(),
            1 => "Removed 1 unused file from the project. The file itself is untouched. Undo brings it back.".to_owned(),
            n => format!(
                "Removed {n} unused files from the project. The files themselves are untouched. Undo brings them back."
            ),
        });
    }

    /// Clear is pressed: the first press asks, the second moves the files to
    /// the trash on a worker.
    pub(crate) fn clear_stray_files(&mut self, ctx: &egui::Context) {
        let strays = self.media_pool.scan.strays.files.clone();
        if strays.is_empty() || self.media_pool.clearing.is_some() || self.is_working_in_files() {
            return;
        }
        if !self.media_pool.armed {
            self.media_pool.armed = true;
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("lumen-pool-clear".into())
            .spawn(move || {
                let _ = tx.send(strays::clear(&strays));
                ctx.request_repaint();
            })
            .expect("spawn media pool clear");
        self.media_pool.clearing = Some(rx);
    }

    /// The missing file chosen in the tab, which Locate acts on.
    pub(crate) fn selected_missing_media(&self) -> Option<Id> {
        let id = self.media_pool.selected?;
        self.media_pool
            .rows
            .iter()
            .any(|r| r.id == id && r.missing)
            .then_some(id)
    }

    /// Selects the clips that play `id` in the arrangement.
    pub(crate) fn select_clips_of(&mut self, id: Id) {
        use lumen_project::model::ClipContent;
        let clips: Vec<Id> = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| matches!(c.content, ClipContent::Audio { media, .. } if media == id))
            .map(|c| c.id)
            .collect();
        self.arrangement.clips = clips.into_iter().collect();
    }

    /// Why the pool's commands cannot run now.
    pub(crate) fn media_pool_unavailable(&self) -> Result<(), &'static str> {
        if self.dialog.is_some() {
            Err(wait::DIALOG)
        } else if self.media_pool.busy() {
            Err(wait::MEDIA_POOL)
        } else {
            Ok(())
        }
    }

    /// The banner for audio the project names that is not here, with the way
    /// to put it back. Called every frame, with the other banners.
    pub(crate) fn missing_media_banner(&mut self, ui: &mut egui::Ui) {
        let Some(missing) = self.missing_banner() else {
            return;
        };
        let words = if missing == 1 {
            "1 audio file is missing. Clips that play it are silent until you relink it.".to_owned()
        } else {
            format!(
                "{missing} audio files are missing. Clips that play them are silent until you relink them."
            )
        };
        let (mut open, mut dismiss) = (false, false);
        egui::Panel::top("missing media banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(words);
                    open = raised::primary_button(ui, "Relink\u{2026}").clicked();
                    dismiss = raised::button(ui, "Dismiss").clicked();
                });
            });
        if open {
            self.show_media_pool();
            self.media_pool.filter = Filter::Missing;
            self.media_pool.dismiss_banner(self.document.session());
        } else if dismiss {
            self.media_pool.dismiss_banner(self.document.session());
        }
    }

    /// Whether the missing-files banner shows.
    pub(crate) fn missing_banner(&self) -> Option<usize> {
        if self.media_pool.banner_dismissed(self.document.session()) {
            return None;
        }
        let project = self.document.project();
        let missing = project
            .media
            .iter()
            .filter(|m| !self.document.media_files.contains_key(&m.id))
            .count();
        (missing > 0).then_some(missing)
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn cleared_note(cleared: &Cleared) -> String {
    let size = crate::files::describe_size(cleared.bytes);
    let mut note = match cleared.moved {
        0 => "Nothing was moved to the trash.".to_owned(),
        1 => format!("Moved 1 file ({size}) to the trash."),
        n => format!("Moved {n} files ({size}) to the trash."),
    };
    if let Some((path, why)) = cleared.failed.first() {
        note.push_str(&format!(
            " {} could not be moved and stays where it is: {why}",
            file_name(path)
        ));
        if cleared.failed.len() > 1 {
            note.push_str(&format!(" ({} more stayed too.)", cleared.failed.len() - 1));
        }
    }
    note
}
