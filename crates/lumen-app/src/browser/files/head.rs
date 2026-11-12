//! What sits above the file list (M9 R7c): the search, Samples' first
//! visit, Places' way back up, the count with Refresh and the folder menu,
//! and the designed empty states. What the app does with what was pressed is
//! `app::browser::files::head` (audit M12).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Rect, Ui, vec2};

use super::walk::{Entry, Outcome};
use crate::browser::Tab;
use crate::browser::files::Place;
use crate::icons::{self, Icon};
use crate::{theme, widgets};

/// What Samples' first-visit card was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MusicAnswer {
    /// Choose a folder….
    Choose,
    /// Keep Music.
    Keep,
}

/// Samples' first visit: it shows the Music folder until one is chosen.
/// Returns what was pressed.
pub(crate) fn music_card(ui: &mut Ui) -> Option<MusicAnswer> {
    let (mut choose, mut keep) = (false, false);
    let palette = *theme::p();
    egui::Frame::NONE
        .fill(palette.raised)
        .stroke(egui::Stroke::new(theme::stroke::HAIRLINE, palette.border))
        .corner_radius(theme::radius::LARGE)
        .inner_margin(egui::Margin::same(theme::space::S as i8))
        .show(ui, |ui| {
        wrapped(
            ui,
            "Samples lists every sound in your Music folder until you choose the folder you keep samples in.",
        );
        ui.horizontal_wrapped(|ui| {
            choose = widgets::raised::primary_button(ui, "Choose a folder\u{2026}").clicked();
            keep = widgets::raised::button(ui, "Keep Music").clicked();
        });
    });
    ui.add_space(theme::space::XS);
    if choose {
        Some(MusicAnswer::Choose)
    } else if keep {
        Some(MusicAnswer::Keep)
    } else {
        None
    }
}

/// "Drums › Kicks": the folder from the place it is in.
pub(crate) fn crumb_of(dir: &Path, places: &[Place]) -> String {
    let place = places
        .iter()
        .filter(|p| dir.starts_with(&p.path))
        .max_by_key(|p| p.path.len());
    match place {
        Some(place) => {
            let rest = dir.strip_prefix(&place.path).unwrap_or(dir);
            std::iter::once(place.name.clone())
                .chain(rest.iter().map(|p| p.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join(" \u{203a} ")
        }
        None => dir.display().to_string(),
    }
}

/// The folders one level inside the sample folders that hold a file read so
/// far, each once.
pub(crate) fn inner_sample_folders(roots: &[PathBuf], rows: &[Arc<Entry>]) -> Vec<PathBuf> {
    let mut inner: Vec<PathBuf> = rows
        .iter()
        .filter_map(|e| {
            let root = roots.iter().find(|r| e.path.starts_with(r))?;
            let first = e.path.strip_prefix(root).ok()?.iter().next()?;
            let path = root.join(first);
            (path != e.path).then_some(path)
        })
        .collect();
    inner.sort();
    inner.dedup();
    inner
}

/// "2 folders", "2 folders, 3 files", or [`count`] when there are no
/// folders.
pub(crate) fn count_with_folders(folders: usize, files: usize, tab: Tab) -> String {
    let folders_said = || {
        let word = if folders == 1 { "folder" } else { "folders" };
        format!("{} {word}", thousands(folders))
    };
    match (folders, files) {
        (0, _) => count(files, tab),
        (_, 0) => folders_said(),
        _ => format!("{}, {}", folders_said(), count(files, tab)),
    }
}

/// "1,284 files" or "12 projects".
pub(crate) fn count(n: usize, tab: Tab) -> String {
    let what = match (tab, n) {
        (Tab::Projects, 1) => "project",
        (Tab::Projects, _) => "projects",
        (_, 1) => "file",
        _ => "files",
    };
    format!("{} {what}", thousands(n))
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn wrapped(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text),
        )
        .wrap()
        .selectable(false),
    );
}

/// The count line's "Reading… 3 files, stopped" words, from what a read is
/// doing.
pub(crate) fn count_said(found: String, reading: bool, outcome: Option<Outcome>) -> String {
    match (reading, outcome) {
        (true, _) => format!("Reading\u{2026} {found}"),
        (false, Some(Outcome::Stopped)) => format!("{found}, stopped"),
        (false, Some(Outcome::Limited)) => {
            format!("{found}, partial: the folder is very large")
        }
        _ => found,
    }
}

/// The list is empty: a designed state for each reason, as its title, its
/// hint and its one action.
pub(crate) fn empty_words(
    tab: Tab,
    reading: bool,
    error: Option<&str>,
    words: &str,
    missing_place: Option<&Path>,
) -> (String, String, Option<(&'static str, &'static str)>) {
    let clear = ("Clear the search", "Shows every file again");
    if reading {
        return (
            "Reading\u{2026}".into(),
            "The files appear as they are found".into(),
            None,
        );
    }
    if let Some(place) = missing_place.filter(|_| words.is_empty()) {
        return (
            format!("{} is not there", place.display()),
            "An unplugged drive, or a folder that moved".into(),
            Some(("Find it\u{2026}", "Choose where the folder is now")),
        );
    }
    if let Some(error) = error {
        let action = (!words.is_empty()).then_some(clear);
        return (
            "This folder cannot be read".into(),
            error.to_owned(),
            action,
        );
    }
    if !words.is_empty() {
        return (
            "No files match".into(),
            "Try other words, or clear the search".into(),
            Some(clear),
        );
    }
    match tab {
        Tab::Samples => (
            "No sounds here yet".into(),
            "Audio and MIDI files in your sample folders show here".into(),
            Some((
                "Add a sample folder\u{2026}",
                "Choose another folder to list",
            )),
        ),
        Tab::Projects => (
            "No projects yet".into(),
            "Projects you save, and the projects folder's, show here".into(),
            None,
        ),
        _ => (
            "Nothing here".into(),
            "This folder holds no folders and no audio or MIDI files".into(),
            None,
        ),
    }
}

/// The file icon an empty state shows.
pub(crate) fn empty_icon(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32) {
    let rect = Rect::from_center_size(at, vec2(theme::space::L, theme::space::L));
    painter.extend(icons::shapes(Icon::Folder, rect, colour));
}

#[cfg(test)]
mod tests {
    use super::super::walk::Kind;
    use super::*;

    fn place(name: &str, path: &str) -> Place {
        Place {
            name: name.to_owned(),
            path: path.to_owned(),
        }
    }

    #[test]
    fn the_count_line_says_what_a_read_is_doing() {
        assert_eq!(
            count_said("3 files".into(), true, None),
            "Reading\u{2026} 3 files"
        );
        assert_eq!(
            count_said("3 files".into(), false, Some(Outcome::Stopped)),
            "3 files, stopped"
        );
        assert_eq!(
            count_said("3 files".into(), false, Some(Outcome::Limited)),
            "3 files, partial: the folder is very large"
        );
        assert_eq!(count_said("3 files".into(), false, None), "3 files");
    }

    #[test]
    fn an_empty_list_says_why_and_what_can_be_done() {
        let (title, _, action) = empty_words(Tab::Samples, false, None, "", None);
        assert_eq!(title, "No sounds here yet");
        assert_eq!(
            action.map(|(label, _)| label),
            Some("Add a sample folder\u{2026}")
        );

        let (title, _, action) = empty_words(Tab::Places, false, None, "kick", None);
        assert_eq!(title, "No files match");
        assert_eq!(action.map(|(label, _)| label), Some("Clear the search"));

        let moved = Path::new("/gone/Samples");
        let (title, _, action) = empty_words(Tab::Places, false, None, "", Some(moved));
        assert!(title.ends_with("is not there"));
        assert_eq!(action.map(|(label, _)| label), Some("Find it\u{2026}"));

        let (title, hint, _) = empty_words(Tab::Samples, false, Some("denied"), "", None);
        assert_eq!(
            (title.as_str(), hint.as_str()),
            ("This folder cannot be read", "denied")
        );
    }

    #[test]
    fn counts_read_as_words() {
        assert_eq!(count(1, Tab::Samples), "1 file");
        assert_eq!(count(1284, Tab::Samples), "1,284 files");
        assert_eq!(count(2, Tab::Projects), "2 projects");
        assert_eq!(count_with_folders(2, 0, Tab::Places), "2 folders");
        assert_eq!(count_with_folders(1, 3, Tab::Places), "1 folder, 3 files");
        assert_eq!(count_with_folders(0, 3, Tab::Places), "3 files");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn a_crumb_names_the_place_the_folder_is_under() {
        let places = [place("Samples", "/x/Samples")];
        assert_eq!(
            crumb_of(Path::new("/x/Samples/Drums/Kicks"), &places),
            "Samples \u{203a} Drums \u{203a} Kicks"
        );
        assert_eq!(
            crumb_of(Path::new("/x/Elsewhere"), &places),
            "/x/Elsewhere",
            "a folder under no place says its path"
        );
    }

    #[test]
    fn the_sample_folders_inside_the_roots_are_listed_once_each() {
        let roots = [PathBuf::from("/x/Samples")];
        let rows: Vec<Arc<Entry>> = [
            "/x/Samples/Drums/Kick.wav",
            "/x/Samples/Drums/Snare.wav",
            "/x/Samples/Bass/Sub.wav",
            "/x/Samples/Root.wav",
        ]
        .into_iter()
        .map(|p| Arc::new(Entry::new(PathBuf::from(p), Kind::Audio)))
        .collect();
        assert_eq!(
            inner_sample_folders(&roots, &rows),
            [
                PathBuf::from("/x/Samples/Bass"),
                PathBuf::from("/x/Samples/Drums")
            ]
        );
    }
}
