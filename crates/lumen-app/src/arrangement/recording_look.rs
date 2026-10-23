//! What the arrangement shows of a recording under way (M5 slice 6d): the
//! clips being recorded are drawn in record red (the owner's 3-A), lent by
//! the app each frame. They drop back into their track colour when the
//! recording ends.
//!
//! Replace (and Record): the take's clip is record red throughout: body
//! tinted, outline, notes lighter red, and the "REC" tag in its header.
//! Overdub: the clip already there keeps its body and notes (the new notes
//! are not shown red yet, M5 slice 6d's deferral); it takes the red outline
//! and the tag.

use eframe::egui::Color32;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id};

use crate::theme::Palette;

/// The share of record red in a take's body, over the ground.
const BODY_TINT: f32 = 0.20;
/// The strength of the record red outline of a clip being recorded.
const OUTLINE: f32 = 0.78;

/// The tracks recording, and which of their clips the take is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RecordingLook {
    /// The armed MIDI tracks recording.
    pub tracks: Vec<Id>,
    /// Where the take begins.
    pub from: Ticks,
    /// Overdub: where the take has reached, rounded up to a bar so the look
    /// changes once a bar rather than every frame; clips from here on are
    /// not part of it.
    pub until: Ticks,
    /// Overdub: the clips under the take stay, and take the red outline.
    pub overdub: bool,
    /// Replace: the clip each track's take is (once it exists).
    pub take_clips: Vec<Id>,
}

impl RecordingLook {
    /// Whether `clip`, on `track`, is drawn as being recorded.
    pub(crate) fn is_recording(&self, track: Id, clip: &Clip) -> bool {
        if !self.tracks.contains(&track) {
            return false;
        }
        if self.overdub {
            clip.start < self.until && clip.start + clip.length > self.from
        } else {
            self.take_clips.contains(&clip.id)
        }
    }
}

/// The colours a clip takes from a recording. Each `Some` replaces the
/// clip's own colour; `None` keeps it, so a clip that is not recording is
/// painted exactly as before.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Paint {
    /// The body, at its own strength over the ground.
    pub body: Option<Color32>,
    /// The outline, at its own strength.
    pub outline: Option<Color32>,
    /// The notes inside a MIDI clip.
    pub notes: Option<Color32>,
    /// The header carries the "REC" tag.
    pub tag: bool,
}

/// What a clip is painted with, given the recording (if any), the track it
/// is on and the palette.
pub(crate) fn paint(
    look: Option<&RecordingLook>,
    track: Id,
    clip: &Clip,
    palette: &Palette,
) -> Paint {
    let Some(look) = look.filter(|look| look.is_recording(track, clip)) else {
        return Paint::default();
    };
    let outline = Some(palette.record.gamma_multiply(OUTLINE));
    if look.overdub {
        return Paint {
            body: None,
            outline,
            notes: None,
            tag: true,
        };
    }
    take_paint(palette)
}

/// What a take being recorded is painted with: record red body, outline and
/// notes, and the "REC" tag.
pub(crate) fn take_paint(palette: &Palette) -> Paint {
    Paint {
        body: Some(palette.record.gamma_multiply(BODY_TINT)),
        outline: Some(palette.record.gamma_multiply(OUTLINE)),
        notes: Some(palette.record_note),
        tag: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};
    use lumen_project::model::{ClipContent, Color};

    fn clip(id: u64, start: i64, length: i64) -> Clip {
        Clip {
            id: Id(id),
            name: "take".into(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            content: ClipContent::Midi {
                notes: Vec::new(),
                events: Vec::new(),
            },
        }
    }

    fn replace(clips: Vec<Id>) -> RecordingLook {
        RecordingLook {
            tracks: vec![Id(7)],
            from: Ticks::quarters(4),
            until: Ticks::quarters(8),
            overdub: false,
            take_clips: clips,
        }
    }

    #[test]
    fn without_a_recording_nothing_is_painted_differently() {
        let c = clip(1, 4, 4);
        assert_eq!(paint(None, Id(7), &c, &DARK), Paint::default());
        let look = replace(vec![Id(1)]);
        // Another track's clip, and a clip that is not the take.
        assert_eq!(paint(Some(&look), Id(8), &c, &DARK), Paint::default());
        assert_eq!(
            paint(Some(&look), Id(7), &clip(2, 0, 4), &DARK),
            Paint::default()
        );
    }

    #[test]
    fn a_take_is_record_red_with_notes_and_a_tag() {
        let look = replace(vec![Id(1)]);
        for palette in [&DARK, &LIGHT] {
            let p = paint(Some(&look), Id(7), &clip(1, 4, 4), palette);
            assert_eq!(p.body, Some(palette.record.gamma_multiply(BODY_TINT)));
            assert_eq!(p.outline, Some(palette.record.gamma_multiply(OUTLINE)));
            assert_eq!(p.notes, Some(palette.record_note));
            assert!(p.tag);
        }
    }

    #[test]
    fn an_overdubbed_clip_keeps_its_body_and_notes() {
        let look = RecordingLook {
            tracks: vec![Id(7)],
            from: Ticks::quarters(4),
            until: Ticks::quarters(8),
            overdub: true,
            take_clips: Vec::new(),
        };
        // Under the take: starts before it, ends after it begins.
        let under = paint(Some(&look), Id(7), &clip(1, 0, 8), &DARK);
        assert_eq!(under.body, None);
        assert_eq!(under.notes, None);
        assert_eq!(under.outline, Some(DARK.record.gamma_multiply(OUTLINE)));
        assert!(under.tag);
        // Ends before the take: untouched.
        assert_eq!(
            paint(Some(&look), Id(7), &clip(2, 0, 4), &DARK),
            Paint::default()
        );
        // Starts after the take has reached: not part of it yet.
        assert_eq!(
            paint(Some(&look), Id(7), &clip(3, 12, 4), &DARK),
            Paint::default()
        );
    }

    #[test]
    fn a_color_is_kept_out_of_the_paint() {
        // The paint never names the track colour: a take's outline is the
        // record red whatever the track's colour is.
        let mut c = clip(1, 4, 4);
        c.color = Some(Color(0x00FF00));
        let p = paint(Some(&replace(vec![Id(1)])), Id(7), &c, &LIGHT);
        assert_eq!(p.outline, Some(LIGHT.record.gamma_multiply(OUTLINE)));
    }
}
