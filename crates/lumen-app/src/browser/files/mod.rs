//! The browser's file tabs, Samples, Projects and Places (M9 R7c,
//! ADR-0116): finding a sound or a project by name, hearing a file before
//! it is added, and dragging it onto the arrangement.
//!
//! - `walk` reads folders and walks trees, with an index in the cache;
//! - `disk` runs that work on two threads, never on the UI's;
//! - `thumbs` makes the rows' pictures (the owner's R B);
//! - `preview` plays a file through the engine's preview voice (P A, O A);
//! - `list` draws the rows and `strip` the preview strip; `head` draws what
//!   sits above them;
//! - this file holds what each tab shows; what the app does with what was
//!   done is `app::browser::files` (audit M12).

pub(crate) mod disk;
mod head;
mod list;
pub(crate) mod preview;
mod row;
mod strip;
pub(crate) mod thumbs;
pub(crate) mod walk;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use self::disk::{Answer, Disk, Job, ProjectAbout};
use self::thumbs::Thumb;
use self::walk::{Entry, Outcome, Wanted};
use super::Tab;

/// What the browser keeps in the settings, by its old name here.
pub(crate) use crate::settings::browser as saved;
pub(crate) use list::Dragged;
pub(crate) use saved::Place;

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
    pub(crate) generation: u64,
    /// Rows came since the last sort: they are sorted once a frame.
    unsorted: bool,
    /// What the rows are of: the folders and kind asked, so a change reads
    /// again.
    pub(crate) of: Option<(Vec<PathBuf>, Wanted, bool)>,
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
    pub(crate) fn start(&mut self, disk: &mut Disk, job: Job, of: (Vec<PathBuf>, Wanted, bool)) {
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
    pub(crate) fn settle(&mut self) {
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
    pub(crate) fn take(&mut self, answer: &mut Option<Answer>) {
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
    pub(crate) disk: Option<Disk>,
    pub preview: Option<preview::Preview>,
    pub samples: Listing,
    pub projects: Listing,
    /// Places' folder, and its search.
    pub places: Listing,
    pub found: Listing,
    pub at: At,
    /// The top level's places that are there, by path (asked of a worker).
    pub present: HashMap<PathBuf, bool>,
    pub(crate) present_ticket: u64,
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
    pub(crate) asked: HashSet<PathBuf>,
    pub abouts: HashMap<PathBuf, Result<ProjectAbout, String>>,
    pub(crate) asked_about: HashSet<PathBuf>,
    /// The last ticket asked before the last Refresh: a picture or a
    /// project's details of that ticket or older is not kept.
    pub(crate) fresh_after: u64,
    /// The folder to focus after going up (it is where the person came
    /// from).
    pub came_from: Option<PathBuf>,
    /// A read of everything again was asked (Refresh).
    pub(crate) refresh: bool,
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

/// The sample folders: the chosen ones, or the system's Music folder.
pub(crate) fn sample_folders(chosen: &[String]) -> Vec<PathBuf> {
    if chosen.is_empty() {
        walk::music().into_iter().collect()
    } else {
        chosen.iter().map(PathBuf::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_folders_kept_are_the_ones_read() {
        let chosen = ["C:/Samples".to_owned(), "C:/Drums".to_owned()];
        assert_eq!(
            sample_folders(&chosen),
            [PathBuf::from("C:/Samples"), PathBuf::from("C:/Drums")]
        );
    }
}
