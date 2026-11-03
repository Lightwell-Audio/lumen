//! The browser's file tabs, Samples, Projects and Places (M9 R7c,
//! ADR-0116): finding a sound or a project by name, hearing a file before
//! it is added, and dragging it onto the arrangement.
//!
//! - `walk` reads folders and walks trees, with an index in the cache;
//! - `disk` runs that work on two threads, never on the UI's;
//! - `thumbs` makes the rows' pictures (the owner's R B);
//! - `preview` plays a file through the engine's preview voice (P A, O A);
//! - `list` draws the rows and `strip` the preview strip; this file holds
//!   what each tab shows and what the app does with what was done.

pub(crate) mod disk;
mod head;
mod list;
pub(crate) mod preview;
mod row;
mod saved;
mod strip;
pub(crate) mod thumbs;
pub(crate) mod walk;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use eframe::egui;

use self::disk::{Answer, Disk, Job, ProjectAbout};
use self::thumbs::Thumb;
use self::walk::{Entry, Kind, Outcome, Wanted};
use super::Tab;

pub(crate) use list::Dragged;
pub(crate) use saved::{Place, Saved};

/// Where Places is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum At {
    /// The person's places, and Computer.
    #[default]
    Top,
    /// The drives and the standard folders.
    Computer,
    Folder(PathBuf),
}

/// Numbers each change of any listing's rows, so a list made of them knows
/// when it is old.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// One tab's list: what was read, what is being read, and what is chosen.
#[derive(Default)]
pub(crate) struct Listing {
    /// Shared with the list drawn from them, so a frame copies no names.
    pub rows: Vec<Arc<Entry>>,
    /// Which change of the rows this is (from [`GENERATION`]).
    generation: u64,
    /// Rows came since the last sort: they are sorted once a frame.
    unsorted: bool,
    /// What the rows are of: the folders and kind asked, so a change reads
    /// again.
    of: Option<(Vec<PathBuf>, Wanted, bool)>,
    ticket: u64,
    cancel: Option<Arc<AtomicBool>>,
    /// Files found so far while a read runs.
    pub reading: bool,
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
    /// The rows the words leave, as indexes into `rows`, and what they were
    /// made for.
    shown: Vec<usize>,
    shown_for: Option<(String, u64, Option<PathBuf>)>,
}

impl Listing {
    /// Starts reading again: the rows are cleared and the old read
    /// abandoned.
    fn start(&mut self, disk: &mut Disk, job: Job, of: (Vec<PathBuf>, Wanted, bool)) {
        self.stop();
        self.rows.clear();
        self.unsorted = false;
        self.changed();
        self.error = None;
        self.outcome = None;
        let (ticket, cancel) = disk.ask(job);
        self.ticket = ticket;
        self.cancel = Some(cancel);
        self.reading = true;
        self.of = Some(of);
    }

    /// The rows changed.
    fn changed(&mut self) {
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        self.shown_for = None;
    }

    /// Sorts the rows that came this frame in with the rest. Each batch is
    /// sorted alone first, so the stable sort meets two runs and merges them
    /// in one pass.
    fn settle(&mut self) {
        if std::mem::take(&mut self.unsorted) {
            self.rows.sort_by(|a, b| walk::order(a, b));
            self.changed();
        }
    }

    /// Stops a read; what it found stays.
    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if self.reading {
            self.reading = false;
            self.outcome = Some(Outcome::Stopped);
        }
    }

    /// Takes an answer that belongs to this listing.
    fn take(&mut self, answer: &mut Option<Answer>) {
        match answer {
            Some(Answer::Rows { ticket, rows }) if *ticket == self.ticket => {
                walk::sort(rows);
                self.rows.extend(rows.drain(..).map(Arc::new));
                self.unsorted = true;
                *answer = None;
            }
            Some(Answer::Done {
                ticket,
                outcome,
                error,
            }) if *ticket == self.ticket => {
                if self.reading {
                    self.outcome = Some(*outcome);
                }
                self.error = error.take();
                self.reading = false;
                self.cancel = None;
                *answer = None;
            }
            _ => {}
        }
    }

    /// The rows the words and `under` leave, by index into `rows`.
    pub(crate) fn shown(&mut self, words: &str, under: Option<&Path>) -> &[usize] {
        let key = (
            words.to_lowercase(),
            self.generation,
            under.map(Path::to_path_buf),
        );
        if self.shown_for.as_ref() != Some(&key) {
            let words: Vec<&str> = key.0.split_whitespace().collect();
            self.shown = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| under.is_none_or(|u| row.path.starts_with(u)))
                .filter(|(_, row)| words.iter().all(|w| row.key().contains(w)))
                .map(|(i, _)| i)
                .collect();
            self.shown_for = Some(key);
        }
        &self.shown
    }
}

/// The file tabs' state.
#[derive(Default)]
pub(crate) struct Files {
    disk: Option<Disk>,
    pub preview: Option<preview::Preview>,
    pub samples: Listing,
    pub projects: Listing,
    /// Places' folder, and its search.
    pub places: Listing,
    pub found: Listing,
    pub at: At,
    /// The top level's places that are there, by path (asked of a worker).
    pub present: HashMap<PathBuf, bool>,
    present_ticket: u64,
    /// The rows last drawn, and what they were made from.
    items: Option<(list::ItemsKey, Arc<[list::Item]>)>,
    /// The chosen row of each tab.
    pub selected: HashMap<Tab, PathBuf>,
    /// Each tab's words.
    pub text: HashMap<Tab, String>,
    pub focus_search: bool,
    /// The folder Samples is narrowed to.
    pub narrow: Option<PathBuf>,
    /// Row pictures known, and asked for.
    pub thumbs: HashMap<PathBuf, Result<Thumb, String>>,
    asked: HashSet<PathBuf>,
    pub abouts: HashMap<PathBuf, Result<ProjectAbout, String>>,
    asked_about: HashSet<PathBuf>,
    /// The last ticket asked before the last Refresh: a picture or a
    /// project's details of that ticket or older is not kept.
    fresh_after: u64,
    /// The folder to focus after going up (it is where the person came
    /// from).
    pub came_from: Option<PathBuf>,
    /// A read of everything again was asked (Refresh).
    refresh: bool,
    /// A missing place whose new folder is being chosen (Find it…).
    pub finding: Option<PathBuf>,
    /// The row the keys should bring into view.
    pub scroll_to: bool,
    /// The pane had the keyboard the frame before (Esc clears it first).
    pub had_keys: bool,
}

impl Files {
    /// The words typed in `tab`'s search.
    pub(crate) fn words(&self, tab: Tab) -> &str {
        self.text.get(&tab).map_or("", String::as_str)
    }

    /// Asks for `path`'s row picture once.
    pub(crate) fn want_picture(&mut self, path: &Path) {
        if self.thumbs.contains_key(path) || !self.asked.insert(path.to_owned()) {
            return;
        }
        if let Some(disk) = &mut self.disk {
            disk.ask(Job::Picture(path.to_owned()));
        }
    }

    /// Asks for a project's details once.
    pub(crate) fn want_about(&mut self, path: &Path) {
        if self.abouts.contains_key(path) || !self.asked_about.insert(path.to_owned()) {
            return;
        }
        if let Some(disk) = &mut self.disk {
            disk.ask(Job::Project(path.to_owned()));
        }
    }

    /// Reads everything again.
    pub(crate) fn ask_refresh(&mut self) {
        self.refresh = true;
        // A picture or a project's details asked before now may answer
        // after what is asked again: it is not kept.
        if let Some(disk) = &self.disk {
            self.fresh_after = disk.last_ticket();
        }
        self.thumbs.clear();
        self.asked.clear();
        self.abouts.clear();
        self.asked_about.clear();
        self.present.clear();
    }

    /// The listing `tab` shows.
    pub(crate) fn listing(&mut self, tab: Tab) -> &mut Listing {
        match tab {
            Tab::Projects => &mut self.projects,
            Tab::Places if !self.words(Tab::Places).trim().is_empty() => &mut self.found,
            Tab::Places => &mut self.places,
            _ => &mut self.samples,
        }
    }
}

impl crate::App {
    /// The sample folders: the chosen ones, or the system's Music folder.
    pub(crate) fn sample_folders(&self) -> Vec<PathBuf> {
        let chosen = &self.settings.browser.sample_folders;
        if chosen.is_empty() {
            walk::music().into_iter().collect()
        } else {
            chosen.iter().map(PathBuf::from).collect()
        }
    }

    /// Starts the workers and the preview the first time a file tab shows,
    /// and reads what the open tab needs.
    pub(crate) fn files_ready(&mut self, ctx: &egui::Context) {
        let files = &mut self.browser.files;
        if files.disk.is_none() {
            files.disk = Some(Disk::start(Some(self.cache.dir.clone()), ctx));
        }
        if files.preview.is_none() {
            let feed = self.transport.audio.take_preview_feed();
            files.preview = Some(preview::Preview::new(
                self.transport.audio.preview.clone(),
                feed,
            ));
        }
        let refresh = std::mem::take(&mut self.browser.files.refresh);
        match self.browser.tab {
            Tab::Samples => {
                let roots = self.sample_folders();
                self.read_tree(Tab::Samples, roots, Wanted::Sounds, refresh);
            }
            Tab::Projects => {
                let roots: Vec<PathBuf> = self.projects_folder().into_iter().collect();
                self.read_tree(Tab::Projects, roots, Wanted::Projects, refresh);
            }
            Tab::Places => self.read_places(refresh),
            Tab::Devices | Tab::Media => {}
        }
    }

    /// Walks `roots` for `tab` unless their rows are read already.
    fn read_tree(&mut self, tab: Tab, roots: Vec<PathBuf>, wanted: Wanted, refresh: bool) {
        let files = &mut self.browser.files;
        let Some(disk) = files.disk.as_mut() else {
            return;
        };
        let listing = match tab {
            Tab::Projects => &mut files.projects,
            Tab::Places => &mut files.found,
            _ => &mut files.samples,
        };
        let of = (roots.clone(), wanted, false);
        if !refresh && listing.of.as_ref() == Some(&of) {
            return;
        }
        listing.start(disk, Job::Walk { roots, wanted }, of);
    }

    /// Reads the folder Places is in, or its search.
    fn read_places(&mut self, refresh: bool) {
        let words = self.browser.files.words(Tab::Places).trim().to_owned();
        if !words.is_empty() {
            let roots = match &self.browser.files.at {
                At::Folder(dir) => vec![dir.clone()],
                At::Top | At::Computer => self
                    .settings
                    .browser
                    .places
                    .iter()
                    .map(|p| PathBuf::from(&p.path))
                    .collect(),
            };
            self.read_tree(Tab::Places, roots, Wanted::Sounds, refresh);
            return;
        }
        let files = &mut self.browser.files;
        let Some(disk) = files.disk.as_mut() else {
            return;
        };
        match files.at.clone() {
            At::Top => {
                let paths: Vec<PathBuf> = self
                    .settings
                    .browser
                    .places
                    .iter()
                    .map(|p| PathBuf::from(&p.path))
                    .collect();
                if refresh || paths.iter().any(|p| !files.present.contains_key(p)) {
                    let (ticket, _) = disk.ask(Job::Present(paths.clone()));
                    files.present_ticket = ticket;
                    for path in paths {
                        files.present.entry(path).or_insert(true);
                    }
                }
            }
            At::Computer => {
                let of = (Vec::new(), Wanted::Sounds, true);
                if refresh || files.places.of.as_ref() != Some(&of) {
                    files.places.start(disk, Job::Computer, of);
                }
            }
            At::Folder(dir) => {
                let of = (vec![dir.clone()], Wanted::Sounds, false);
                if refresh || files.places.of.as_ref() != Some(&of) {
                    files.places.start(
                        disk,
                        Job::Folder {
                            dir,
                            wanted: Wanted::Sounds,
                        },
                        of,
                    );
                }
            }
        }
    }

    /// Takes the workers' answers and follows the preview. Every frame.
    pub(crate) fn poll_browser_files(&mut self, ctx: &egui::Context) {
        let on_file_tab = self.show_browser
            && matches!(self.browser.tab, Tab::Samples | Tab::Projects | Tab::Places);
        let files = &mut self.browser.files;
        if let Some(disk) = &files.disk {
            for answer in disk.answers() {
                let mut answer = Some(answer);
                files.samples.take(&mut answer);
                files.projects.take(&mut answer);
                files.places.take(&mut answer);
                files.found.take(&mut answer);
                match answer {
                    Some(Answer::Picture {
                        ticket,
                        path,
                        thumb,
                    }) if ticket > files.fresh_after => {
                        files.asked.remove(&path);
                        files.thumbs.insert(path, thumb);
                    }
                    Some(Answer::Project {
                        ticket,
                        path,
                        about,
                    }) if ticket > files.fresh_after => {
                        files.asked_about.remove(&path);
                        files.abouts.insert(path, about);
                    }
                    Some(Answer::Present { ticket, present }) if ticket == files.present_ticket => {
                        files.present.extend(present);
                    }
                    _ => {}
                }
            }
            // A row back in view asks again for a picture that gave way.
            for path in disk.dropped_pictures() {
                files.asked.remove(&path);
            }
            for listing in [
                &mut files.samples,
                &mut files.projects,
                &mut files.places,
                &mut files.found,
            ] {
                listing.settle();
            }
            if !on_file_tab {
                disk.forget_pictures();
                files.asked.clear();
            }
        }
        // The preview: heard on the cue pair when there is one (O A), and
        // ended by leaving the file tabs or by the device stopping.
        let running = matches!(
            self.transport.audio.status,
            crate::audio::Status::Running { .. }
        );
        let channel = self.transport.audio.preview_channel();
        let db = self.settings.browser.preview_db;
        if let Some(preview) = &mut files.preview {
            preview.set_output(db, channel);
            if !on_file_tab || !running {
                preview.stop();
            }
            if preview.poll().is_some() {
                ctx.request_repaint();
            }
        }
    }

    /// Plays `path` from `from` seconds (an audio file; MIDI files are not
    /// previewed, the owner's M A).
    pub(crate) fn preview_file(&mut self, path: &Path, from: f64) {
        if Kind::of_file(path) != Some(Kind::Audio) {
            return;
        }
        if let Some(preview) = &mut self.browser.files.preview {
            preview.play(path, from);
        }
    }

    /// Stops the preview.
    pub(crate) fn stop_preview(&mut self) {
        if let Some(preview) = &mut self.browser.files.preview {
            preview.stop();
        }
    }

    /// Whether a preview is playing.
    pub(crate) fn previewing(&self) -> bool {
        self.browser
            .files
            .preview
            .as_ref()
            .is_some_and(|p| p.playing.is_some())
    }

    /// The selected row of the open file tab.
    pub(crate) fn selected_file(&self) -> Option<PathBuf> {
        self.browser.files.selected.get(&self.browser.tab).cloned()
    }

    /// Adds `paths` where Import would: the selected track at the playhead,
    /// or new tracks (the rule devices follow, D3 of M6 slice 2), as one
    /// undo step each.
    pub(crate) fn add_browser_files(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let (at, places) = self.import_places();
        self.add_files(paths, at, places, ctx);
    }

    /// Enter or a double-click on a row: a folder opens, a project opens
    /// (asking about unsaved changes), a sound is added.
    pub(crate) fn open_row(&mut self, entry: &Entry, ctx: &egui::Context) {
        match entry.kind {
            Kind::Folder => self.enter_folder(entry.path.clone()),
            Kind::Project => self.request(
                ctx,
                crate::app::projects::Next::Open(Some(entry.path.clone())),
            ),
            Kind::Audio | Kind::Midi => self.add_browser_files(vec![entry.path.clone()], ctx),
        }
    }

    /// Opens `dir` in Places.
    pub(crate) fn enter_folder(&mut self, dir: PathBuf) {
        self.browser.tab = Tab::Places;
        let files = &mut self.browser.files;
        files.at = At::Folder(dir);
        files.selected.remove(&Tab::Places);
        files.text.remove(&Tab::Places);
        files.scroll_to = true;
        self.remember_place_folder();
    }

    /// Goes up a folder in Places, to the top after a place's own folder.
    pub(crate) fn folder_up(&mut self) {
        let files = &mut self.browser.files;
        let from = match &files.at {
            At::Folder(dir) => dir.clone(),
            At::Computer => {
                files.at = At::Top;
                self.remember_place_folder();
                return;
            }
            At::Top => return,
        };
        let is_root = |p: &Path| {
            self.settings
                .browser
                .places
                .iter()
                .any(|place| Path::new(&place.path) == p)
        };
        files.at = match from.parent() {
            Some(parent) if !is_root(&from) && !parent.as_os_str().is_empty() => {
                At::Folder(parent.to_owned())
            }
            _ if is_root(&from) => At::Top,
            _ => At::Computer,
        };
        files.selected.insert(Tab::Places, from.clone());
        files.came_from = Some(from);
        files.scroll_to = true;
        self.remember_place_folder();
    }

    /// Keeps Places' folder in the settings.
    fn remember_place_folder(&mut self) {
        let folder = match &self.browser.files.at {
            At::Top => None,
            At::Computer => Some(saved::COMPUTER.to_owned()),
            At::Folder(dir) => Some(dir.to_string_lossy().into_owned()),
        };
        if folder != self.settings.browser.place_folder {
            self.settings.browser.place_folder = folder;
            self.settings_changed = Some(std::time::Instant::now());
        }
    }

    /// Restores Places' folder from the settings (at start).
    pub(crate) fn restore_place_folder(&mut self) {
        self.browser.files.at = match self.settings.browser.place_folder.as_deref() {
            None => At::Top,
            Some(saved::COMPUTER) => At::Computer,
            Some(dir) => At::Folder(PathBuf::from(dir)),
        };
    }

    /// Adds `dir` as a place (a picker, a dropped folder, a row's menu).
    pub(crate) fn add_place(&mut self, dir: &Path) {
        let path = dir.to_string_lossy().into_owned();
        let places = &mut self.settings.browser.places;
        if places.iter().any(|p| p.path == path) {
            return;
        }
        let name = dir
            .file_name()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        places.push(Place { name, path });
        self.browser.files.present.remove(dir);
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// Removes the place at `path`.
    pub(crate) fn remove_place(&mut self, path: &Path) {
        self.settings
            .browser
            .places
            .retain(|p| Path::new(&p.path) != path);
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// Adds a sample folder (Choose a folder…, Add a sample folder…).
    pub(crate) fn add_sample_folder(&mut self, dir: &Path) {
        let path = dir.to_string_lossy().into_owned();
        let saved = &mut self.settings.browser;
        if !saved.sample_folders.contains(&path) {
            saved.sample_folders.push(path);
        }
        saved.music_answered = true;
        self.browser.files.narrow = None;
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// Removes a sample folder.
    pub(crate) fn remove_sample_folder(&mut self, dir: &Path) {
        self.settings
            .browser
            .sample_folders
            .retain(|f| Path::new(f) != dir);
        self.browser.files.narrow = None;
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// Keep Music: the first visit's question is answered.
    pub(crate) fn keep_music(&mut self) {
        self.settings.browser.music_answered = true;
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// Asks for a folder to add to Samples.
    pub(crate) fn pick_sample_folder(&mut self, ctx: &egui::Context) {
        self.pick_folder(ctx, "Choose a sample folder", |path| {
            crate::app::projects::FileDialog::SampleFolder(path)
        });
    }

    /// Asks for a folder to add as a place.
    pub(crate) fn pick_place(&mut self, ctx: &egui::Context) {
        self.pick_folder(ctx, "Add a place", |path| {
            crate::app::projects::FileDialog::Place(path)
        });
    }

    /// Asks for the folder a missing place moved to.
    pub(crate) fn find_place(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.browser.files.finding = Some(path);
        self.pick_folder(ctx, "Find the place", |path| {
            crate::app::projects::FileDialog::Place(path)
        });
    }

    fn pick_folder(
        &mut self,
        ctx: &egui::Context,
        title: &'static str,
        answer: fn(Option<PathBuf>) -> crate::app::projects::FileDialog,
    ) {
        if self.dialog.is_some() {
            return;
        }
        let owner = self.owner;
        let start = walk::home();
        let show = move || {
            let mut dialog = rfd::FileDialog::new().set_title(title);
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            answer(dialog.pick_folder())
        };
        self.file_dialog(ctx, show, move |paths| answer(paths.into_iter().next()));
    }

    /// A place folder was chosen: added, or put in place of a missing one.
    pub(crate) fn place_chosen(&mut self, dir: PathBuf) {
        if let Some(missing) = self.browser.files.finding.take() {
            let path = dir.to_string_lossy().into_owned();
            if let Some(place) = self
                .settings
                .browser
                .places
                .iter_mut()
                .find(|p| Path::new(&p.path) == missing)
            {
                place.path = path;
                self.browser.files.present.remove(&dir);
                self.settings_changed = Some(std::time::Instant::now());
                return;
            }
        }
        self.add_place(&dir);
    }
}
