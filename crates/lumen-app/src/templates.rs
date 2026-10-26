//! Templates (M9 slice R4, ADR-0105): projects kept to start new ones from.
//! A template is a whole project in `<Templates>/<Name>/<Name>.lumen`, audio
//! and all, beside the settings. Listing the folder runs on a worker; the app
//! reads the last answer each frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::files::EXTENSION;
use crate::preset_files::{NAME_LIMIT, safe_name};

/// How long a listing is trusted before opening the menu asks again (a
/// template can be added or removed in the file manager).
const FRESH: Duration = Duration::from_secs(2);

/// A template found in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub file: PathBuf,
}

/// The longest name the window takes.
pub const LONGEST_NAME: usize = NAME_LIMIT;

/// `name` as a template's folder and file name.
pub fn safe(name: &str) -> String {
    safe_name(name, "Template")
}

/// Where the template called `name` is kept in `dir`.
pub fn file_for(dir: &Path, name: &str) -> PathBuf {
    let name = safe(name);
    dir.join(&name).join(format!("{name}.{EXTENSION}"))
}

/// The templates in `dir`, by name; none when the folder is not there.
pub fn list(dir: &Path) -> Vec<Template> {
    let mut found: Vec<Template> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let folder = entry.ok()?.path();
            let name = folder.file_name()?.to_str()?.to_owned();
            let file = folder.join(format!("{name}.{EXTENSION}"));
            file.is_file().then_some(Template { name, file })
        })
        .collect();
    found.sort_by_key(|t| t.name.to_lowercase());
    found
}

/// The templates last found, and the worker finding them again.
#[derive(Default)]
pub struct Listing {
    found: Vec<Template>,
    pending: Option<Receiver<Vec<Template>>>,
    asked: Option<Instant>,
}

impl Listing {
    /// The templates found at the last answer.
    pub fn found(&self) -> &[Template] {
        &self.found
    }

    /// Asks the worker to list `dir` again, unless it just did (`force`
    /// asks anyway) or is still at it.
    pub fn ask(&mut self, ctx: &egui::Context, dir: PathBuf, force: bool) {
        if self.pending.is_some() || (!force && self.asked.is_some_and(|t| t.elapsed() < FRESH)) {
            return;
        }
        self.asked = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("lumen-templates".into())
            .spawn(move || {
                let _ = tx.send(list(&dir));
                ctx.request_repaint();
            })
            .expect("spawn template listing");
        self.pending = Some(rx);
    }

    /// Takes the worker's answer when it is in. True when the list changed.
    pub fn poll(&mut self) -> bool {
        let Some(found) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.pending = None;
        let changed = found != self.found;
        self.found = found;
        changed
    }

    /// Whether the folder has been asked about at all yet.
    pub fn asked_once(&self) -> bool {
        self.asked.is_some()
    }

    /// Whether a listing is under way (tests wait for it).
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether a template of this name (as its folder is named) is listed.
    pub fn has(&self, name: &str) -> bool {
        let wanted = safe(name).to_lowercase();
        self.found.iter().any(|t| t.name.to_lowercase() == wanted)
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::testing::TempDir;

    use super::*;

    fn make(dir: &Path, name: &str, with_file: bool) {
        let folder = dir.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        if with_file {
            std::fs::write(folder.join(format!("{name}.{EXTENSION}")), "{}").unwrap();
        }
    }

    #[test]
    fn templates_are_the_folders_that_hold_their_own_project_file_by_name() {
        let tmp = TempDir::new("templates-list");
        make(tmp.path(), "synth Pop", true);
        make(tmp.path(), "Beat", true);
        make(tmp.path(), "Empty folder", false);
        std::fs::write(tmp.path().join("stray.lumen"), "{}").unwrap();
        // A folder whose project file has another name is not a template.
        std::fs::create_dir_all(tmp.path().join("Odd")).unwrap();
        std::fs::write(tmp.path().join("Odd/other.lumen"), "{}").unwrap();

        let names: Vec<String> = list(tmp.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Beat", "synth Pop"], "by name, ignoring case");
        assert!(list(&tmp.path().join("nowhere")).is_empty());
    }

    #[test]
    fn a_name_becomes_a_safe_folder_and_file_name() {
        let dir = Path::new("T");
        assert_eq!(
            file_for(dir, "Song: take 2?"),
            Path::new("T/Song take 2/Song take 2.lumen")
        );
        assert_eq!(
            file_for(dir, "  ..  "),
            Path::new("T/Template/Template.lumen")
        );
    }

    #[test]
    fn a_name_that_is_listed_is_found_whatever_its_case_or_punctuation() {
        let listing = Listing {
            found: vec![Template {
                name: "Song take 2".into(),
                file: PathBuf::from("x"),
            }],
            ..Listing::default()
        };
        assert!(listing.has("song TAKE 2"));
        assert!(listing.has("Song: take 2?"));
        assert!(!listing.has("Song take 3"));
    }
}
