//! The notes inside MIDI clips on the arrangement (M5 slice 3b): the
//! project's MIDI clips as mini-note boxes, rebuilt with the clip boxes
//! (`clips::refresh_boxes`), and where each track's bands sit this frame.
//! Tracks without notes get no bands, so the row table stays small.

use lumen_project::model::{ClipContent, Project, Track};
use lumen_render::{BoxInstance, RowSlot};

use super::clips::{INSET, WAVE_GAP};
use super::recording_look::{self, RecordingLook};
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

/// The mini notes of every MIDI clip in `project` on the tracks `keep`
/// takes, each track's clips on one fit. A clip being recorded shows its
/// notes in the lighter record red (3-A).
pub(super) fn build(
    project: &Project,
    recording: Option<&RecordingLook>,
    keep: impl Fn(&Track) -> bool,
) -> MiniNotes {
    let mut out = MiniNotes::default();
    let palette = theme::p();
    for (index, track) in project.tracks.iter().enumerate() {
        if !keep(track) {
            continue;
        }
        let mut dense = None;
        let Some((lo, hi)) = mini::track_range(&track.clips) else {
            continue;
        };
        let fit = mini::Fit::new(lo, hi);
        // Notes held across a seam are drawn on into the next clip, as
        // they sound (ADR-0058).
        let carried = lumen_project::edit::seams::carried(track);
        for (clip, carried) in track.clips.iter().zip(&carried) {
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
            let color = recording_look::paint(recording, track.id, clip, palette)
                .notes
                .unwrap_or_else(|| theme::wave(clip_color(track, clip)));
            mini::mini_instances(
                &mut out.boxes,
                &ClipNotes {
                    start: clip.start,
                    length: clip.length,
                    loop_length: clip.loop_length,
                    loop_phase: clip.loop_phase,
                    notes,
                    carried,
                    outside: false,
                },
                (n as u32, fit),
                color,
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
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
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
        with.clips = vec![Arc::new(clip(vec![Note::new(
            Ticks::quarters(1),
            Ticks::quarters(1),
            60,
            100,
        )]))];
        project.tracks = vec![Arc::new(track), Arc::new(with)];
        let built = build(&project, None, |_| true);
        assert_eq!(built.tracks, [1]);
        assert_eq!(built.boxes.len(), 1);
        assert!(built.boxes[0].row & 0xFF_FFFF < BANDS);
        let slots = [
            RowSlot::new(0.0, 100.0, 1.0),
            RowSlot::new(100.0, 100.0, 1.0),
        ];
        let table = rows(&built.tracks, &slots, 1.0);
        assert_eq!(table.len(), BANDS as usize);
        assert!(table[0].top >= 100.0 + INSET + TITLE);
    }

    #[test]
    fn a_note_held_across_a_seam_is_drawn_on_into_the_next_clip() {
        // A note from beat 6 to 10, split at 8: both pieces keep it, so the
        // second draws its tail from its start (the owner's boundary case).
        let q = Ticks::quarters;
        let mut project = lumen_project::testing::synthetic(1, 1, 0, 0, 0);
        let mut track = (*project.tracks[0]).clone();
        let left = clip(vec![Note::new(q(2), q(4), 60, 100)]);
        let right = Clip {
            id: Id(2),
            start: q(8),
            ..clip(vec![Note::new(q(-2), q(4), 60, 100)])
        };
        track.clips = vec![Arc::new(left), Arc::new(right.clone())];
        project.tracks = vec![Arc::new(track.clone())];
        let built = build(&project, None, |_| true);
        let spans: Vec<(f64, f64)> = built
            .boxes
            .iter()
            .map(|b| {
                let at = |v: [f32; 2]| f64::from(v[0]) + f64::from(v[1]);
                (at(b.start), at(b.end))
            })
            .collect();
        assert_eq!(spans, vec![(6.0, 8.0), (8.0, 10.0)]);
        // Moved away from the first clip, the second keeps its note hidden.
        track.clips[1] = Arc::new(Clip {
            start: q(9),
            ..right
        });
        project.tracks = vec![Arc::new(track)];
        assert_eq!(build(&project, None, |_| true).boxes.len(), 1);
    }

    #[test]
    fn a_recording_clips_notes_are_record_red() {
        let mut project = lumen_project::testing::synthetic(1, 1, 0, 0, 0);
        let mut track = (*project.tracks[0]).clone();
        let mut c = clip(vec![Note::new(
            Ticks::quarters(1),
            Ticks::quarters(1),
            60,
            100,
        )]);
        c.id = Id(5);
        track.clips = vec![Arc::new(c)];
        let track_id = track.id;
        project.tracks = vec![Arc::new(track)];
        let look = RecordingLook {
            tracks: vec![track_id],
            from: Ticks::quarters(4),
            until: Ticks::quarters(8),
            overdub: false,
            take_clips: vec![Id(5)],
        };
        let red = build(&project, Some(&look), |_| true);
        let plain = build(&project, None, |_| true);
        assert_eq!(red.boxes.len(), 1);
        assert_ne!(red.boxes[0].body, plain.boxes[0].body);
        // Not recording: the same boxes as with no recording at all.
        let other = RecordingLook {
            take_clips: vec![Id(9)],
            ..look
        };
        assert_eq!(build(&project, Some(&other), |_| true), plain);
    }
}
