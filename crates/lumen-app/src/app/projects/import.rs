//! Importing audio and MIDI files: the system's file dialog, and where the
//! chosen files land.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::dialogs::FileDialog;
use crate::document::Places;
use crate::midi::files as midi_files;
use crate::{App, document, media};

impl App {
    /// Import audio or MIDI (M3 review H1, M5 slice 7): the system's file
    /// dialog, then the chosen files load as dropped files do, at the
    /// playhead, onto the selected track (or the track of the selected
    /// clip) when it takes them, else onto new tracks after the selection.
    /// Where imported files land: at the playhead, on the selected track
    /// (or the track of the first selected clip) when it is of their kind,
    /// else on new tracks.
    pub(crate) fn import_places(&self) -> (Ticks, Places) {
        let at = self.transport.state().ticks;
        let project = self.document.project();
        let chosen = self.arrangement.selected.last().copied().or_else(|| {
            let clip = self.first_selected_clip()?;
            let track = project
                .tracks
                .iter()
                .find(|t| t.clips.iter().any(|c| c.id == clip))?;
            Some(track.id)
        });
        let into = Places {
            audio: document::import_place(project, chosen, TrackKind::Audio),
            midi: document::import_place(project, chosen, TrackKind::Midi),
        };
        (at, into)
    }

    pub(crate) fn import_files(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let (at, into) = self.import_places();
        let owner = self.owner;
        let show = move || {
            let both: Vec<&str> = media::EXTENSIONS
                .iter()
                .chain(midi_files::EXTENSIONS)
                .copied()
                .collect();
            let mut dialog = rfd::FileDialog::new()
                .set_title("Import audio or MIDI")
                .add_filter("Audio and MIDI", &both)
                .add_filter("Audio", media::EXTENSIONS)
                .add_filter("MIDI", midi_files::EXTENSIONS);
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            let paths = dialog.pick_files().unwrap_or_default();
            FileDialog::Import { paths, at, into }
        };
        self.file_dialog(ctx, show, move |paths| FileDialog::Import {
            paths,
            at,
            into,
        });
    }
}
