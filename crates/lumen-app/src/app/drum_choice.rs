//! The drum map chooser (M8 slice D0): the list a MIDI track takes a map
//! from, and the list a clip's notes are converted to. D1 builds the map's own
//! editor; this is only what the two commands need.

use eframe::egui::{self, Align2, RichText, pos2};
use lumen_project::drums::presets::Preset;
use lumen_project::model::Id;

use crate::widgets::{Dialog, raised};
use crate::{App, theme};

/// What the chooser is choosing for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    /// The MIDI tracks the map goes to: to attach, or the ones whose clips
    /// are converted (and then switched).
    pub tracks: Vec<Id>,
    /// The clips whose notes are rewritten; empty when attaching.
    pub clips: Vec<Id>,
    /// Keep the notes the target map lacks (converting only).
    pub keep_missing: bool,
}

impl Choice {
    pub(crate) fn attach(tracks: Vec<Id>) -> Self {
        Self {
            tracks,
            clips: Vec::new(),
            keep_missing: false,
        }
    }

    pub(crate) fn convert(tracks: Vec<Id>, clips: Vec<Id>) -> Self {
        Self {
            tracks,
            clips,
            keep_missing: true,
        }
    }

    fn converting(&self) -> bool {
        !self.clips.is_empty()
    }
}

impl App {
    /// Whether the selected clips can be converted: at least one is on a
    /// track with a drum map, and the project holds another map to go to.
    pub(crate) fn can_convert_drum_map(&self) -> bool {
        let clips = &self.arrangement.clips;
        if clips.is_empty() {
            return false;
        }
        let sources: Vec<Id> = self
            .document
            .project()
            .tracks
            .iter()
            .filter(|t| t.clips.iter().any(|c| clips.contains(&c.id)))
            .filter_map(|t| self.document.drum_map_of(t.id).map(|m| m.id))
            .collect();
        !sources.is_empty()
            && self
                .document
                .project()
                .drum_maps
                .iter()
                .any(|m| !sources.contains(&m.id))
    }

    /// Opens the chooser for the selected MIDI tracks.
    pub(crate) fn choose_drum_map(&mut self) {
        let tracks = self.selected_midi_tracks();
        if tracks.is_empty() {
            self.say("Select a MIDI track first".to_owned());
            return;
        }
        self.drum_choice = Some(Choice::attach(tracks));
    }

    /// Opens the chooser for the selected MIDI clips' notes.
    pub(crate) fn choose_drum_map_conversion(&mut self) {
        let clips: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let mut tracks = Vec::new();
        for track in &self.document.project().tracks {
            if track.clips.iter().any(|c| clips.contains(&c.id))
                && self.document.drum_map_of(track.id).is_some()
            {
                tracks.push(track.id);
            }
        }
        if tracks.is_empty() {
            self.say(
                "The selected clips are not on a track with a drum map, so there is nothing to convert"
                    .to_owned(),
            );
            return;
        }
        self.drum_choice = Some(Choice::convert(tracks, clips));
    }

    /// Takes the map off the selected MIDI tracks that have one.
    pub(crate) fn detach_drum_map_command(&mut self) {
        self.detach_drum_maps_of(&self.selected_midi_tracks());
    }

    /// Takes the map off every track in `tracks` that has one, saying what
    /// happened. The chooser names its tracks when it opens, so a click on
    /// another header while it is open detaches nothing of the kind
    /// (M8 D0 review L1).
    pub(crate) fn detach_drum_maps_of(&mut self, tracks: &[Id]) {
        let with: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| self.document.drum_map_of(*id).is_some())
            .collect();
        if with.is_empty() {
            return;
        }
        self.document.detach_drum_maps(&with);
        self.say(if with.len() == 1 {
            "Drum map detached: the notes play as they are".to_owned()
        } else {
            format!(
                "Drum map detached from {} tracks: the notes play as they are",
                with.len()
            )
        });
    }

    /// The chooser, while it is open.
    pub(crate) fn drum_choice_window(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.drum_choice.clone() else {
            return;
        };
        let convert = choice.converting();
        let maps: Vec<(Id, String, usize)> = self
            .document
            .project()
            .drum_maps
            .iter()
            .map(|m| (m.id, m.name.clone(), m.sounds.len()))
            .collect();
        let current: Vec<Option<Id>> = choice
            .tracks
            .iter()
            .map(|t| self.document.drum_map_of(*t).map(|m| m.id))
            .collect();
        let names: Vec<String> = choice
            .tracks
            .iter()
            .filter_map(|id| {
                self.document
                    .project()
                    .tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| t.name.clone())
            })
            .collect();
        let whom = match names.as_slice() {
            [] => "the selected tracks".to_owned(),
            [one] => format!("\u{201c}{one}\u{201d}"),
            [first, rest @ ..] => format!("\u{201c}{first}\u{201d} and {} more", rest.len()),
        };
        let title = if convert {
            "Convert to a drum map"
        } else {
            "Drum map"
        };
        // A map every chosen track already plays through is not offered.
        let used = |id: Id| !current.is_empty() && current.iter().all(|c| *c == Some(id));
        let mut attach: Option<Result<Id, Preset>> = None;
        let mut detach = false;
        let mut keep = choice.keep_missing;
        let can_detach = !convert && current.iter().any(|c| c.is_some());
        let close = Dialog::new("drum map", title, 400.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(if convert {
                        format!("Rewrite the notes of {whom} into another map's notes.")
                    } else {
                        format!("The kit {whom} reads its notes through.")
                    })
                    .small()
                    .weak(),
                );
                if !convert {
                    ui.add_space(theme::space::S);
                    ui.label(
                        RichText::new(concat!(lumen_core::product_name!(), "'s kits"))
                            .small()
                            .weak(),
                    );
                    for preset in Preset::ALL {
                        if raised::button(ui, preset.name).clicked() {
                            attach = Some(Err(preset));
                        }
                    }
                }
                if !maps.is_empty() {
                    ui.add_space(theme::space::S);
                    ui.label(RichText::new("This project's maps").small().weak());
                    for (id, name, sounds) in &maps {
                        if used(*id) {
                            continue;
                        }
                        let label = format!("{name} ({sounds} sounds)");
                        if raised::button(ui, &label).clicked() {
                            attach = Some(Ok(*id));
                        }
                    }
                }
                if maps.is_empty() && convert {
                    ui.label(
                        RichText::new(
                            "This project has one map: convert needs another one to go to.",
                        )
                        .small()
                        .weak(),
                    );
                }
                if convert {
                    ui.add_space(theme::space::S);
                    ui.checkbox(&mut keep, "Keep the notes the target map lacks");
                }
                if can_detach {
                    ui.add_space(theme::space::S);
                    if raised::button(ui, "Detach the drum map").clicked() {
                        detach = true;
                    }
                }
            });
        if close {
            self.drum_choice = None;
            return;
        }
        let Some(picked) = attach else {
            if detach {
                let tracks = choice.tracks.clone();
                self.drum_choice = None;
                self.detach_drum_maps_of(&tracks);
                return;
            }
            // The toggle lives while the window is open.
            if convert
                && keep != choice.keep_missing
                && let Some(open) = &mut self.drum_choice
            {
                open.keep_missing = keep;
            }
            return;
        };
        self.drum_choice = None;
        match picked {
            Err(preset) => {
                self.document
                    .attach_drum_map_preset_to(&choice.tracks, preset);
                self.say(format!(
                    "{} attached: the clips keep their notes and play through it",
                    preset.name
                ));
            }
            Ok(map) => {
                if convert {
                    let done =
                        self.document
                            .convert_drum_map(&choice.clips, map, choice.keep_missing);
                    let mut words = format!("Converted {} notes to the other map", done.moved);
                    if done.kept > 0 {
                        words.push_str(&format!(", kept {} it lacks", done.kept));
                    }
                    if done.dropped > 0 {
                        words.push_str(&format!(", left out {}", done.dropped));
                    }
                    if done.moved + done.kept + done.dropped == 0 {
                        words = "Nothing to convert: the clips play that map already".to_owned();
                    }
                    self.say(words);
                } else {
                    self.document.attach_drum_map_to(&choice.tracks, map);
                    let name = self
                        .document
                        .drum_map(map)
                        .map_or_else(String::new, |m| m.name.clone());
                    self.say(format!(
                        "{name} attached: the clips keep their notes and play through it"
                    ));
                }
            }
        }
    }
}
