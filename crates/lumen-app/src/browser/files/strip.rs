use std::path::{Path, PathBuf};

use eframe::egui::{self, Ui};

use super::Files;
use super::walk::Kind;
use crate::browser::Tab;
use crate::theme;

pub(crate) const LINE: f32 = 24.0;
const WELL: f32 = 40.0;
pub(crate) const WELL_ID: &str = "browser preview position";
pub(crate) const LEVEL_ID: &str = "browser preview level";

pub(crate) const LEVEL: f32 = 104.0;
pub(crate) const LEVEL_LEAST: f32 = 48.0;
pub(crate) const AUTO: f32 = 76.0;

pub(crate) fn subject(files: &Files, tab: Tab) -> Option<(PathBuf, Kind)> {
    let chosen = files
        .selected
        .get(&tab)
        .filter(|p| Kind::of_file(p).is_some_and(|k| k != Kind::Folder))
        .cloned();
    let path = chosen.or_else(|| {
        let preview = files.preview.as_ref()?;
        preview.playing.as_ref().map(|p| p.path.clone())
    })?;
    let kind = Kind::of_file(&path)?;
    Some((path, kind))
}

pub(crate) fn message(files: &Files, path: &Path, kind: Kind) -> Option<(String, bool)> {
    match kind {
        Kind::Audio => files
            .preview
            .as_ref()
            .and_then(|p| p.failed.as_ref())
            .filter(|(p, _)| p == path)
            .map(|(_, why)| (format!("This file cannot be played: {why}"), true)),
        Kind::Midi => Some((
            "MIDI files are not previewed. Drag it onto a track, or press Enter to add it."
                .to_owned(),
            false,
        )),
        Kind::Project => Some(match files.abouts.get(path) {
            Some(Ok(about)) => (
                format!(
                    "{} BPM \u{b7} {} \u{b7} {} {}",
                    trim_bpm(about.bpm),
                    about.meter,
                    about.tracks,
                    if about.tracks == 1 { "track" } else { "tracks" }
                ),
                false,
            ),
            Some(Err(error)) => (format!("This project cannot be read: {error}"), true),
            None => ("Reading the project\u{2026}".to_owned(), false),
        }),
        Kind::Folder => None,
    }
}

pub(crate) fn height(ui: &Ui, files: &Files, tab: Tab, width: f32) -> f32 {
    let Some((path, kind)) = subject(files, tab) else {
        return 0.0;
    };
    let inner = width - 2.0 * (theme::space::M - theme::space::XS);
    let message = message(files, &path, kind);
    let text = |width: f32| {
        message
            .as_ref()
            .map_or(0.0, |(text, _)| message_height(ui, text, width))
    };
    let edges = theme::space::S * 2.0;
    let gap = ui.spacing().item_spacing.y;
    match kind {
        Kind::Audio => {
            edges
                + LINE * 2.0
                + well_height(text(inner - 2.0 * theme::space::S))
                + theme::space::XS * 2.0
                + gap * 2.0
        }
        Kind::Project => edges + LINE * 2.0 + text(inner) + theme::space::XS + gap * 2.0,
        Kind::Midi => edges + LINE + text(inner) + gap,
        Kind::Folder => 0.0,
    }
}

pub(crate) fn message_height(ui: &Ui, text: &str, width: f32) -> f32 {
    ui.fonts_mut(|f| {
        f.layout(
            text.to_owned(),
            egui::FontId::proportional(theme::size::CAPTION),
            theme::p().text,
            width.max(1.0),
        )
    })
    .size()
    .y
}

pub(crate) fn well_height(text: f32) -> f32 {
    WELL.max(text + 2.0 * theme::space::XS)
}

pub(crate) fn strip_note(ui: &mut Ui, text: &str, bad: bool) {
    let palette = theme::p();
    let colour = if bad {
        palette.warning
    } else {
        palette.text_secondary
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(colour),
        )
        .wrap()
        .selectable(false),
    );
}

fn trim_bpm(bpm: f64) -> String {
    let text = format!("{bpm:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempos_drop_their_zeros() {
        assert_eq!(trim_bpm(120.0), "120");
        assert_eq!(trim_bpm(92.5), "92.5");
    }

    #[test]
    fn the_strip_is_about_the_chosen_file_and_a_folder_has_none() {
        let mut files = Files::default();
        assert_eq!(subject(&files, Tab::Samples), None);
        files
            .selected
            .insert(Tab::Samples, PathBuf::from("/x/Kick.wav"));
        assert_eq!(
            subject(&files, Tab::Samples),
            Some((PathBuf::from("/x/Kick.wav"), Kind::Audio))
        );
        assert_eq!(
            subject(&files, Tab::Projects),
            None,
            "each tab has its own chosen file"
        );
        files
            .selected
            .insert(Tab::Places, PathBuf::from("/x/Drums"));
        assert_eq!(
            subject(&files, Tab::Places),
            None,
            "a folder is not previewed"
        );
    }

    #[test]
    fn a_midi_file_says_it_is_not_previewed_and_a_project_in_read_says_so() {
        let files = Files::default();
        let (text, bad) = message(&files, Path::new("/x/Part.mid"), Kind::Midi)
            .expect("a MIDI file has its own line");
        assert!(text.contains("not previewed"));
        assert!(!bad);
        let (text, bad) = message(&files, Path::new("/x/Song.lumen"), Kind::Project)
            .expect("a project says it is being read");
        assert!(text.contains("Reading"));
        assert!(!bad);
    }
}
