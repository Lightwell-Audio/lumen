//! The drum map chooser (M8 slice D0, narrowed by D1): the list a clip's
//! notes are converted to. Attaching a map, and editing one, is D1's map
//! editor (`app/drum_map_window`); this window is only the destructive
//! conversion's choice of a target, which the clip's menu opens.

use eframe::egui::{self, Align2, RichText, pos2};
use lumen_project::model::Id;

use crate::widgets::{Dialog, raised};
use crate::{App, theme};

/// What the chooser is choosing for: the clips whose notes are rewritten,
/// the tracks they sit on (switched to the target), and whether the notes
/// the target lacks are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub tracks: Vec<Id>,
    pub clips: Vec<Id>,
    pub keep_missing: bool,
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
        self.drum_choice = Some(Choice {
            tracks,
            clips,
            keep_missing: true,
        });
    }

    /// The chooser, while it is open.
    pub(crate) fn drum_choice_window(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.drum_choice.clone() else {
            return;
        };
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
        // A map every chosen track already plays through is not offered.
        let used = |id: Id| !current.is_empty() && current.iter().all(|c| *c == Some(id));
        let mut convert = None;
        let mut keep = choice.keep_missing;
        let close = Dialog::new("drum map", "Convert to a drum map", 400.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Rewrite the notes of {whom} into another map's notes."
                    ))
                    .small()
                    .weak(),
                );
                ui.add_space(theme::space::S);
                ui.label(RichText::new("This project's maps").small().weak());
                for (id, name, sounds) in &maps {
                    if used(*id) {
                        continue;
                    }
                    if raised::button(ui, &format!("{name} ({sounds} sounds)")).clicked() {
                        convert = Some(*id);
                    }
                }
                if maps.len() < 2 {
                    ui.label(
                        RichText::new(
                            "This project has one map: convert needs another one to go to.",
                        )
                        .small()
                        .weak(),
                    );
                }
                ui.add_space(theme::space::S);
                ui.checkbox(&mut keep, "Keep the notes the target map lacks");
            });
        if close {
            self.drum_choice = None;
            return;
        }
        let Some(target) = convert else {
            if keep != choice.keep_missing
                && let Some(open) = &mut self.drum_choice
            {
                open.keep_missing = keep;
            }
            return;
        };
        self.drum_choice = None;
        let done = self.document.convert_drum_map(&choice.clips, target, keep);
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
    }
}
