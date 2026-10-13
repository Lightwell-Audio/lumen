//! MIDI files in the app (M5 slice 7, ADR-0062): dropped or imported files
//! are read in the background and placed as the owner's F1 says (on a MIDI
//! track, one clip; between tracks, a track per part); a file whose tempo
//! or meter differs from the project's offers them in a banner (F2).

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{TimeSignature, TrackKind};
use lumen_project::smf::MidiFile;

use lumen_project::smf::{self, Layout};

use super::projects::{FileDialog, documents_folder};
use crate::App;
use crate::document::{DropInto, Places};
use crate::widgets::raised;
use crate::{midi_export, midi_files};

/// A file's tempo and meter, offered after it came in (F2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TempoOffer {
    /// What the banner says.
    pub(crate) text: String,
    pub(crate) tempo: Vec<TempoPoint>,
    pub(crate) meter: Option<TimeSignature>,
}

/// A tempo as the banner writes it: whole BPM, or to two places.
fn bpm_text(bpm: f64) -> String {
    let text = format!("{bpm:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What `file` (named `name`) offers that the project does not have, if
/// anything.
pub(crate) fn tempo_offer(
    name: &str,
    file: &MidiFile,
    tempo: &[TempoPoint],
    meter: TimeSignature,
) -> Option<TempoOffer> {
    let new_tempo = !file.tempo.is_empty()
        && (file.tempo.len() != tempo.len()
            || file
                .tempo
                .iter()
                .zip(tempo)
                .any(|(a, b)| a.at != b.at || (a.bpm - b.bpm).abs() > 0.005));
    let new_meter = file.meter.filter(|m| *m != meter);
    if !new_tempo && new_meter.is_none() {
        return None;
    }
    let mut text = format!("{name} is");
    if let Some(first) = file.tempo.first().filter(|_| new_tempo) {
        text += &format!(" at {} BPM", bpm_text(first.bpm));
        match file.tempo.len() {
            1 => {}
            2 => text += ", with one tempo change",
            n => text += &format!(", with {} tempo changes", n - 1),
        }
    }
    if let Some(m) = new_meter {
        if new_tempo {
            text += ",";
        }
        text += &format!(" in {}/{}", m.numerator, m.denominator);
    }
    text += ". Its notes follow this project's tempo.";
    Some(TempoOffer {
        text,
        tempo: if new_tempo {
            file.tempo.clone()
        } else {
            Vec::new()
        },
        meter: new_meter,
    })
}

impl App {
    /// Files dropped or chosen in Import: MIDI files are read in the
    /// background, the rest go to the audio loader. `places` says where
    /// each kind lands.
    pub(crate) fn add_files(
        &mut self,
        paths: Vec<PathBuf>,
        at: Ticks,
        places: Places,
        ctx: &egui::Context,
    ) {
        let (midi, other): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| midi_files::is_midi_file(p));
        if !midi.is_empty() {
            self.midi_reader.read(midi, at, places.midi, ctx);
        }
        if !other.is_empty() {
            let rate = self.transport.state().sample_rate;
            self.lanes
                .add_files(other, at, places.audio, rate, ctx, &mut self.document);
        }
    }

    /// Places the MIDI files read since the last frame, each one undo
    /// step, and reports an export that finished.
    pub(crate) fn poll_midi_files(&mut self) {
        if let Some(done) = self.midi_writing.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.midi_writing = None;
            match done {
                Ok(path) => {
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    self.toast = Some((format!("Exported {name}"), Instant::now()));
                }
                Err(why) => self.lanes.say(format!("Not exported: {why}")),
            }
        }
        for read in self.midi_reader.poll() {
            let shown = read
                .path
                .file_name()
                .map_or_else(|| read.name.clone(), |n| n.to_string_lossy().into_owned());
            let file = match read.file {
                Ok(file) => file,
                Err(why) => {
                    self.lanes.say(format!("Not added: {shown} is {why}"));
                    continue;
                }
            };
            if file
                .tracks
                .iter()
                .all(|p| p.notes.is_empty() && p.events.is_empty())
            {
                self.lanes.say(format!("Not added: {shown} holds no notes"));
                continue;
            }
            let project = self.document.project();
            let onto = match read.into {
                DropInto::Track(id) => project
                    .tracks
                    .iter()
                    .find(|t| t.id == id && t.kind == TrackKind::Midi)
                    .map(|t| t.id),
                DropInto::NewTracks { .. } => None,
            };
            let placed = match (onto, read.into) {
                (Some(track), _) => self
                    .document
                    .import_midi_clip(track, read.at, &read.name, &file)
                    .is_some(),
                (None, DropInto::NewTracks { before, group }) => !self
                    .document
                    .import_midi_tracks(before, group, read.at, &read.name, &file)
                    .is_empty(),
                // The track has gone, or is no longer a MIDI track.
                (None, DropInto::Track(_)) => !self
                    .document
                    .import_midi_tracks(None, None, read.at, &read.name, &file)
                    .is_empty(),
            };
            if !placed {
                self.lanes
                    .say(format!("Not added: a locked clip is in the way of {shown}"));
                continue;
            }
            let project = self.document.project();
            if let Some(offer) = tempo_offer(&shown, &file, &project.tempo, project.time_signature)
            {
                self.tempo_offer = Some(offer);
            }
        }
    }

    /// Export MIDI clip… (`clips`, the selected MIDI clips, type 0) or
    /// Export MIDI file… (the project's MIDI tracks, type 1): the bytes
    /// are made now, then the system's save dialog asks where they go.
    pub(crate) fn export_midi(&mut self, ctx: &egui::Context, clips: bool) {
        if self.dialog.is_some() {
            return;
        }
        let project = self.document.project();
        let (file, layout, title) = if clips {
            let ids: Vec<_> = self.arrangement.clips.iter().copied().collect();
            let file = midi_export::clips_file(project, &ids);
            (file, Layout::Single, "Export MIDI clip")
        } else {
            let file = midi_export::project_file(project);
            (file, Layout::Parallel, "Export MIDI file")
        };
        let Some(file) = file else {
            self.lanes
                .say("Not exported: there are no MIDI notes to export".into());
            return;
        };
        let name = match (clips, file.tracks.first()) {
            (true, Some(part)) => part.name.clone().unwrap_or_else(|| "MIDI clip".into()),
            _ => self.document.name(),
        };
        let bytes = smf::write(&file, layout);
        let start = self
            .document
            .path
            .as_deref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .or_else(documents_folder);
        let owner = self.owner;
        let chosen = bytes.clone();
        let show = move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title(title)
                .add_filter("MIDI file", &["mid"])
                .set_file_name(format!("{name}.mid"));
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::ExportMidi {
                path: dialog.save_file(),
                bytes: chosen,
            }
        };
        self.file_dialog(ctx, show, move |paths| FileDialog::ExportMidi {
            path: paths.into_iter().next(),
            bytes,
        });
    }

    /// Writes an exported MIDI file in the background; `poll_midi_files`
    /// says how it went.
    pub(crate) fn write_midi(&mut self, ctx: &egui::Context, mut path: PathBuf, bytes: Vec<u8>) {
        if !midi_files::is_midi_file(&path) {
            path.set_extension("mid");
        }
        self.midi_writing = Some(midi_files::write(path, bytes, ctx));
    }

    /// The banner offering a MIDI file's tempo and meter (F2): "Use them"
    /// takes them as one undo step.
    pub(crate) fn tempo_banner(&mut self, ui: &mut egui::Ui) {
        let Some(offer) = self.tempo_offer.clone() else {
            return;
        };
        egui::Panel::top("tempo banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(&offer.text);
                    let what = match (offer.tempo.is_empty(), offer.meter.is_some()) {
                        (false, true) => "Use its tempo and meter",
                        (false, false) => "Use its tempo",
                        _ => "Use its meter",
                    };
                    if raised::primary_button(ui, what).clicked() {
                        self.document.use_file_tempo(&offer.tempo, offer.meter);
                        self.tempo_offer = None;
                    }
                    if raised::button(ui, "Dismiss").clicked() {
                        self.tempo_offer = None;
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(at: i64, bpm: f64) -> TempoPoint {
        TempoPoint {
            at: Ticks::quarters(at),
            bpm,
        }
    }

    const FOUR: TimeSignature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };

    #[test]
    fn a_file_offers_only_what_differs_from_the_project() {
        let project = [point(0, 120.0)];
        let mut file = MidiFile {
            tempo: vec![point(0, 120.0)],
            meter: Some(FOUR),
            ..MidiFile::default()
        };
        assert_eq!(tempo_offer("a.mid", &file, &project, FOUR), None);
        file.tempo = Vec::new();
        file.meter = None;
        assert_eq!(
            tempo_offer("a.mid", &file, &project, FOUR),
            None,
            "none set"
        );

        file.tempo = vec![point(0, 98.5), point(16, 140.0)];
        let offer = tempo_offer("a.mid", &file, &project, FOUR).unwrap();
        assert_eq!(
            offer.text,
            "a.mid is at 98.5 BPM, with one tempo change. Its notes follow this project's tempo."
        );
        assert_eq!((offer.tempo.len(), offer.meter), (2, None));

        file.tempo = vec![point(0, 120.0)];
        file.meter = Some(TimeSignature {
            numerator: 6,
            denominator: 8,
        });
        let offer = tempo_offer("b.mid", &file, &project, FOUR).unwrap();
        assert_eq!(
            offer.text,
            "b.mid is in 6/8. Its notes follow this project's tempo."
        );
        assert!(offer.tempo.is_empty(), "the same tempo is not offered");

        file.tempo = vec![point(0, 100.0)];
        let offer = tempo_offer("c.mid", &file, &project, FOUR).unwrap();
        assert_eq!(
            offer.text,
            "c.mid is at 100 BPM, in 6/8. Its notes follow this project's tempo."
        );
    }
}
