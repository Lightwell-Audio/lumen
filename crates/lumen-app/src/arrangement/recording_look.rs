use eframe::egui::Color32;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id};

use crate::theme::Palette;

const BODY_TINT: f32 = 0.20;
const OUTLINE: f32 = 0.78;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RecordingLook {
    pub tracks: Vec<Id>,
    pub from: Ticks,
    pub until: Ticks,
    pub overdub: bool,
    pub take_clips: Vec<Id>,
}

impl RecordingLook {
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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Paint {
    pub body: Option<Color32>,
    pub outline: Option<Color32>,
    pub notes: Option<Color32>,
    pub tag: bool,
}

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
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
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
        let under = paint(Some(&look), Id(7), &clip(1, 0, 8), &DARK);
        assert_eq!(under.body, None);
        assert_eq!(under.notes, None);
        assert_eq!(under.outline, Some(DARK.record.gamma_multiply(OUTLINE)));
        assert!(under.tag);
        assert_eq!(
            paint(Some(&look), Id(7), &clip(2, 0, 4), &DARK),
            Paint::default()
        );
        assert_eq!(
            paint(Some(&look), Id(7), &clip(3, 12, 4), &DARK),
            Paint::default()
        );
    }

    #[test]
    fn a_color_is_kept_out_of_the_paint() {
        let mut c = clip(1, 4, 4);
        c.color = Some(Color(0x00FF00));
        let p = paint(Some(&replace(vec![Id(1)])), Id(7), &c, &LIGHT);
        assert_eq!(p.outline, Some(LIGHT.record.gamma_multiply(OUTLINE)));
    }
}
