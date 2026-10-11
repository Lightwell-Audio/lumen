//! The notes inside MIDI clips on the arrangement (M5 slice 3b): the
//! project's MIDI clips as mini-note boxes, rebuilt with the clip boxes
//! (`clips::refresh_boxes`), and where each track's bands sit this frame.
//! Tracks without notes get no bands, so the row table stays small.

use lumen_project::model::{ClipContent, Project};
use lumen_render::{BoxInstance, RowSlot};

use super::clips::{INSET, WAVE_GAP};
use super::{TITLE, clip_color};
use crate::piano_roll::mini::{self, BANDS, Body};
use crate::piano_roll::notes::ClipNotes;
use crate::theme;

/// The mini-note boxes, and the tracks (by index) whose bands they use:
/// box row `n * BANDS + band` belongs to `tracks[n]`.
#[derive(Debug, Default, PartialEq)]
pub(super) struct MiniNotes {
    pub boxes: Vec<BoxInstance>,
    pub tracks: Vec<usize>,
}

/// The mini notes of every MIDI clip in `project`.
pub(super) fn build(project: &Project) -> MiniNotes {
    let mut out = MiniNotes::default();
    for (index, track) in project.tracks.iter().enumerate() {
        let mut dense = None;
        for clip in &track.clips {
            let ClipContent::Midi { notes, .. } = &clip.content else {
                continue;
            };
            if notes.is_empty() {
                continue;
            }
            let n = *dense.get_or_insert_with(|| {
                out.tracks.push(index);
                out.tracks.len() - 1
            });
            mini::mini_instances(
                &mut out.boxes,
                &ClipNotes {
                    start: clip.start,
                    length: clip.length,
                    loop_length: clip.loop_length,
                    notes,
                    muted: &[],
                },
                n as u32,
                theme::wave(clip_color(track, clip)),
            );
        }
    }
    out
}

/// The row slots of the bands, from the tracks' row slots (physical
/// pixels): under each clip's title bar, above its bottom inset.
pub(super) fn rows(tracks: &[usize], slots: &[RowSlot], pixels_per_point: f32) -> Vec<RowSlot> {
    let mut out = Vec::with_capacity(tracks.len() * BANDS as usize);
    for &index in tracks {
        let slot = slots.get(index).copied().unwrap_or(RowSlot::HIDDEN);
        out.extend(mini::slots(Body {
            top: slot.top,
            height: slot.height,
            alpha: slot.alpha,
            above: (INSET + TITLE + WAVE_GAP) * pixels_per_point,
            below: (INSET + WAVE_GAP) * pixels_per_point,
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, Id, Note};
    use std::sync::Arc;

    fn clip(notes: Vec<Note>) -> Clip {
        Clip {
            id: Id(1),
            name: "c".into(),
            start: Ticks::quarters(4),
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        }
    }

    #[test]
    fn only_tracks_with_notes_get_bands() {
        let mut project = lumen_project::testing::synthetic(1, 1, 0, 0, 0);
        let mut track = (*project.tracks[0]).clone();
        track.clips = vec![Arc::new(clip(Vec::new()))];
        let mut with = track.clone();
        with.clips = vec![Arc::new(clip(vec![Note(
            Ticks::quarters(1),
            Ticks::quarters(1),
            60,
            100,
        )]))];
        project.tracks = vec![Arc::new(track), Arc::new(with)];
        let built = build(&project);
        assert_eq!(built.tracks, [1]);
        assert_eq!(built.boxes.len(), 1);
        assert!(built.boxes[0].row < BANDS);
        let slots = [
            RowSlot::new(0.0, 100.0, 1.0),
            RowSlot::new(100.0, 100.0, 1.0),
        ];
        let table = rows(&built.tracks, &slots, 1.0);
        assert_eq!(table.len(), BANDS as usize);
        assert!(table[0].top >= 100.0 + INSET + TITLE);
    }
}
