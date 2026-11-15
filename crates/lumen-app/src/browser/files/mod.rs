pub(crate) mod disk;
pub(crate) mod head;
pub(crate) mod list;
pub(crate) mod preview;
pub(crate) mod row;
pub(crate) mod strip;
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

pub(crate) use crate::settings::browser as saved;
pub(crate) use list::Dragged;
pub(crate) use saved::Place;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum At {
    #[default]
    Top,
    Computer,
    Folder(PathBuf),
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct Listing {
    pub rows: Vec<Arc<Entry>>,
    pub(crate) generation: u64,
    unsorted: bool,
    pub(crate) of: Option<(Vec<PathBuf>, Wanted, bool)>,
    ticket: u64,
    cancel: Option<Arc<AtomicBool>>,
    pub reading: bool,
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
    shown: Vec<usize>,
    shown_for: Option<(String, u64, Option<PathBuf>)>,
}

impl Listing {
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

    fn changed(&mut self) {
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        self.shown_for = None;
    }

    pub(crate) fn settle(&mut self) {
        if std::mem::take(&mut self.unsorted) {
            self.rows.sort_by(|a, b| walk::order(a, b));
            self.changed();
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if self.reading {
            self.reading = false;
            self.outcome = Some(Outcome::Stopped);
        }
    }

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

#[derive(Default)]
pub(crate) struct Files {
    pub(crate) disk: Option<Disk>,
    pub preview: Option<preview::Preview>,
    pub samples: Listing,
    pub projects: Listing,
    pub places: Listing,
    pub found: Listing,
    pub at: At,
    pub present: HashMap<PathBuf, bool>,
    pub(crate) present_ticket: u64,
    pub(crate) items: Option<(list::ItemsKey, Arc<[list::Item]>)>,
    pub selected: HashMap<Tab, PathBuf>,
    pub text: HashMap<Tab, String>,
    pub focus_search: bool,
    pub narrow: Option<PathBuf>,
    pub thumbs: HashMap<PathBuf, Result<Thumb, String>>,
    pub(crate) asked: HashSet<PathBuf>,
    pub abouts: HashMap<PathBuf, Result<ProjectAbout, String>>,
    pub(crate) asked_about: HashSet<PathBuf>,
    pub(crate) fresh_after: u64,
    pub came_from: Option<PathBuf>,
    pub(crate) refresh: bool,
    pub finding: Option<PathBuf>,
    pub scroll_to: bool,
    pub had_keys: bool,
}

impl Files {
    pub(crate) fn words(&self, tab: Tab) -> &str {
        self.text.get(&tab).map_or("", String::as_str)
    }

    pub(crate) fn want_picture(&mut self, path: &Path) {
        if self.thumbs.contains_key(path) || !self.asked.insert(path.to_owned()) {
            return;
        }
        if let Some(disk) = &mut self.disk {
            disk.ask(Job::Picture(path.to_owned()));
        }
    }

    pub(crate) fn want_about(&mut self, path: &Path) {
        if self.abouts.contains_key(path) || !self.asked_about.insert(path.to_owned()) {
            return;
        }
        if let Some(disk) = &mut self.disk {
            disk.ask(Job::Project(path.to_owned()));
        }
    }

    pub(crate) fn ask_refresh(&mut self) {
        self.refresh = true;
        if let Some(disk) = &self.disk {
            self.fresh_after = disk.last_ticket();
        }
        self.thumbs.clear();
        self.asked.clear();
        self.abouts.clear();
        self.asked_about.clear();
        self.present.clear();
    }

    pub(crate) fn listing(&mut self, tab: Tab) -> &mut Listing {
        match tab {
            Tab::Projects => &mut self.projects,
            Tab::Places if !self.words(Tab::Places).trim().is_empty() => &mut self.found,
            Tab::Places => &mut self.places,
            _ => &mut self.samples,
        }
    }
}

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
