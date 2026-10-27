//! The project's own settings (M9 slice R4): its sample rate, its notes and
//! the colour new tracks take. Each is one undo step; typing in the notes
//! merges into one. The pan law and automatic crossfades, which the settings
//! window shows too, have their setters beside the controls that first needed
//! them (`mixer`, `crossfades`).

use lumen_project::model::{Color, MOST_NOTES_BYTES};

use super::Document;

/// The rates the settings window offers. The model accepts any rate from
/// 8 kHz to 384 kHz; these are the ones interfaces run at.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// What the model accepts (`lumen_project::validate`).
const LOWEST_RATE: u32 = 8_000;
const HIGHEST_RATE: u32 = 384_000;

impl Document {
    /// Sets the rate of the files Lumen makes for this project and the
    /// default of Export. A rate outside what the model accepts changes
    /// nothing. Returns whether the project changed.
    pub fn set_sample_rate(&mut self, rate: u32) -> bool {
        if !(LOWEST_RATE..=HIGHEST_RATE).contains(&rate) {
            return false;
        }
        self.history.apply("Change project rate", None, |project| {
            project.sample_rate = rate;
        })
    }

    /// Sets the project's notes. Text past the limit is cut at a character
    /// boundary. Typing is one step (a merge key), however many keys.
    pub fn set_notes(&mut self, text: &str) {
        let mut end = text.len().min(MOST_NOTES_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = &text[..end];
        self.history
            .apply("Edit project notes", Some("project notes"), |project| {
                project.notes = text.to_owned();
            });
    }

    /// Chooses the colour new tracks take, or none to cycle the palette.
    pub fn set_new_track_color(&mut self, color: Option<Color>) {
        self.history
            .apply("Change new track colour", None, |project| {
                project.new_track_color = color;
            });
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::TrackKind;

    use super::*;

    fn document() -> Document {
        Document::new(crate::empty_project())
    }

    #[test]
    fn a_new_project_starts_at_the_device_rate_when_the_model_accepts_it() {
        use crate::empty_project_at;
        assert_eq!(empty_project_at(Some(44_100)).sample_rate, 44_100);
        assert_eq!(empty_project_at(Some(192_000)).sample_rate, 192_000);
        // Not known yet (no device open), or not a rate a project can hold.
        for odd in [None, Some(0), Some(7_999), Some(384_001)] {
            assert_eq!(empty_project_at(odd).sample_rate, 48_000, "{odd:?}");
        }
    }

    #[test]
    fn the_rate_changes_in_one_step_and_undo_restores_it() {
        let mut doc = document();
        assert!(doc.set_sample_rate(44_100));
        assert_eq!(doc.project().sample_rate, 44_100);
        assert_eq!(doc.undo().as_deref(), Some("Change project rate"));
        assert_eq!(doc.project().sample_rate, 48_000);
    }

    #[test]
    fn a_rate_the_model_cannot_hold_is_refused_and_the_same_rate_is_no_step() {
        let mut doc = document();
        assert!(!doc.set_sample_rate(7_999));
        assert!(!doc.set_sample_rate(384_001));
        assert!(
            !doc.set_sample_rate(48_000),
            "the same rate changes nothing"
        );
        assert_eq!(doc.project().sample_rate, 48_000);
        assert!(!doc.history().can_undo(), "nothing was recorded");
    }

    #[test]
    fn every_offered_rate_is_one_the_model_accepts() {
        for rate in RATES {
            let mut doc = document();
            doc.set_sample_rate(rate);
            assert!(
                lumen_project::validate(doc.project()).is_ok(),
                "{rate} is offered but refused"
            );
        }
    }

    #[test]
    fn typing_the_notes_is_one_step() {
        let mut doc = document();
        for typed in ["V", "Ve", "Ver", "Verse"] {
            doc.set_notes(typed);
        }
        assert_eq!(doc.project().notes, "Verse");
        assert_eq!(doc.undo().as_deref(), Some("Edit project notes"));
        assert_eq!(
            doc.project().notes,
            "",
            "one undo takes all the typing back"
        );
        assert!(!doc.history().can_undo());
    }

    #[test]
    fn notes_past_the_limit_are_cut_on_a_character_boundary() {
        let mut doc = document();
        // Two-byte letters: the limit falls inside one.
        let long = "é".repeat(MOST_NOTES_BYTES);
        doc.set_notes(&long);
        let kept = &doc.project().notes;
        assert!(kept.len() <= MOST_NOTES_BYTES);
        assert!(kept.chars().all(|c| c == 'é'));
        assert!(lumen_project::validate(doc.project()).is_ok());
    }

    #[test]
    fn new_tracks_take_the_chosen_colour_and_undo_gives_back_the_cycle() {
        let mut doc = document();
        let first = doc.add_track(TrackKind::Audio, None);
        let cycled = doc.project().tracks[0].color;
        doc.set_new_track_color(Some(Color(0x123456)));
        let second = doc.add_track(TrackKind::Midi, None);
        let third = doc.add_track(TrackKind::Audio, None);
        let color_of = |doc: &Document, id| {
            doc.project()
                .tracks
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.color)
        };
        assert_eq!(
            color_of(&doc, first),
            Some(cycled),
            "an earlier track stays"
        );
        assert_eq!(color_of(&doc, second), Some(Color(0x123456)));
        assert_eq!(color_of(&doc, third), Some(Color(0x123456)));
        doc.set_new_track_color(None);
        let fourth = doc.add_track(TrackKind::Audio, None);
        assert_ne!(color_of(&doc, fourth), Some(Color(0x123456)));
    }
}
