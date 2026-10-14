//! Notes inside arrangement clips (M5 slice 3b): a mini piano roll fitted
//! to its track's pitch range, drawn on the GPU beside the clip bodies.
//!
//! The box layer places a box vertically only by its row slots, so a track
//! gets [`BANDS`] slots, thin horizontal bands under its clips' title bars,
//! divided evenly among the pitches its MIDI clips play ([`Fit`],
//! [`track_range`]). As in Ableton's arrangement, every clip on a track
//! shares that layout, so a pitch is at the same height in all of them and
//! a part an octave up sits higher; every pitch is a row of the same height,
//! and neighbouring pitches touch, as in the piano roll. A note covers its
//! pitch's bands. The notes are rebuilt only when the clips change (with
//! the clip boxes), and panning and zooming only move the view.
//!
//! While recording, the arrangement draws the project with the take placed
//! as far as it has gone (`Arrangement::preview`), so a note is drawn by
//! this same layout, where it will be once the take is committed.

use eframe::egui::Color32;
use lumen_render::{BoxInstance, RowSlot, beats, spanning};

use super::notes::{ClipNotes, pack, velocity_alpha};

/// Bands a track's clips are divided into, top to bottom: fine enough that
/// every one of the 128 pitches gets at least one, and that rows of any
/// range are even to within a band.
pub(crate) const BANDS: u32 = 128;
/// A track's pitch range is shown at least this many semitones tall (four
/// octaves, centred on its notes), so a few notes are thin rows with room
/// to show where they move, as in Ableton, instead of filling the clips.
const MIN_RANGE: u8 = 48;

/// The lowest and highest pitch of every note the MIDI clips in `clips`
/// hold (a track's clips: they share one [`Fit`]). Notes hidden outside a
/// clip's window count too, so trimming an edge over them never moves the
/// track's layout while the edge is dragged.
pub(crate) fn track_range(
    clips: &[std::sync::Arc<lumen_project::model::Clip>],
) -> Option<(u8, u8)> {
    clips
        .iter()
        .filter_map(|clip| match &clip.content {
            lumen_project::model::ClipContent::Midi { notes, .. } => Some(notes),
            _ => None,
        })
        .flatten()
        .map(|n| n.pitch().min(127))
        .fold(None, |range, p| union(range, Some((p, p))))
}

/// The union of two pitch ranges.
pub(crate) fn union(a: Option<(u8, u8)>, b: Option<(u8, u8)>) -> Option<(u8, u8)> {
    match (a, b) {
        (Some((a0, a1)), Some((b0, b1))) => Some((a0.min(b0), a1.max(b1))),
        (a, None) => a,
        (None, b) => b,
    }
}

/// How a track's pitch range sits in its clips' bodies: `rows` equal rows (the range,
/// padded to [`MIN_RANGE`] and centred), the bottom one at pitch `first`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fit {
    first: f32,
    rows: f32,
}

impl Fit {
    /// The fit of notes spanning `lo` to `hi`.
    pub(crate) fn new(lo: u8, hi: u8) -> Self {
        let (lo, hi) = (lo.min(hi), hi.max(lo));
        let span = f32::from(hi - lo) + 1.0;
        let rows = span.max(f32::from(MIN_RANGE));
        Self {
            first: f32::from(lo) - (rows - span) / 2.0,
            rows,
        }
    }

    /// The bands (0 is the top) `pitch` covers: the first, and how many more
    /// below it. Higher pitches are higher up.
    pub(crate) fn bands(&self, pitch: u8) -> (u32, u32) {
        let top = self.first + self.rows;
        let edge = |p: f32| (((top - p) / self.rows) * BANDS as f32).round();
        let p = f32::from(pitch.min(127));
        let a = edge(p + 1.0).clamp(0.0, (BANDS - 1) as f32) as u32;
        let b = (edge(p) as u32).clamp(a + 1, BANDS);
        (a, b - a - 1)
    }
}

/// The boxes of a clip's notes on `track` (a dense number among the tracks
/// with notes: its bands are slots `track * BANDS ..`), laid out by the
/// track's `fit`, in `color`.
pub(crate) fn mini_instances(
    out: &mut Vec<BoxInstance>,
    clip: &ClipNotes<'_>,
    (track, fit): (u32, Fit),
    color: Color32,
) {
    clip.each_placed(|placed, note| {
        let ghost = if placed.rep > 0 { 0.6 } else { 1.0 };
        let c = pack(
            color,
            (0.45 + 0.55 * velocity_alpha(note.velocity())) * ghost,
        );
        out.push(BoxInstance {
            start: beats(placed.start),
            end: beats(placed.end),
            row: {
                let (band, extra) = fit.bands(note.pitch());
                spanning(track * BANDS + band, extra)
            },
            body: c,
            bar: c,
            border: 0,
            fill: 0,
            progress: 0.0,
        });
    });
}

/// Where the clips' bodies are in one track row, in physical pixels: the
/// bands lie between the title bar and the bottom edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Body {
    /// The row's top and height.
    pub top: f32,
    pub height: f32,
    /// The row's opacity (dimmed tracks).
    pub alpha: f32,
    /// Space above the bands (inset, title bar and a gap) and below them.
    pub above: f32,
    pub below: f32,
}

/// The [`BANDS`] row slots of one track.
pub(crate) fn slots(body: Body) -> [RowSlot; BANDS as usize] {
    let room = body.height - body.above - body.below;
    let unit = room / BANDS as f32;
    if unit <= 0.0 {
        return [RowSlot::HIDDEN; BANDS as usize];
    }
    std::array::from_fn(|b| RowSlot::new(body.top + body.above + b as f32 * unit, unit, body.alpha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::model::Note;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(notes: &[Note]) -> ClipNotes<'_> {
        ClipNotes {
            start: q(0),
            length: q(4),
            loop_length: None,
            notes,
            carried: &[],
            outside: false,
        }
    }

    #[test]
    fn the_range_is_every_pitch_the_track_holds_hidden_notes_too() {
        use lumen_project::model::{Clip, ClipContent, Id};
        let midi = |notes: Vec<Note>| {
            std::sync::Arc::new(Clip {
                id: Id(1),
                name: "c".into(),
                start: q(0),
                length: q(4),
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
            })
        };
        let notes = vec![
            Note::new(q(0), q(1), 64, 90),
            Note::new(q(1), q(1), 48, 90),
            // Past the clip's end, hidden: a trim over it keeps the layout.
            Note::new(q(9), q(1), 100, 90),
        ];
        assert_eq!(track_range(&[midi(notes)]), Some((48, 100)));
        assert_eq!(track_range(&[midi(Vec::new())]), None);
    }

    #[test]
    fn a_wide_range_fills_the_bands_high_notes_on_top() {
        let fit = Fit::new(36, 84);
        assert_eq!(fit.bands(84).0, 0);
        let (first, extra) = fit.bands(36);
        assert_eq!(first + extra, BANDS - 1);
        assert!(fit.bands(60).0 > fit.bands(72).0);
    }

    #[test]
    fn every_pitch_is_an_even_row_and_neighbours_touch() {
        // The piano roll's proportions: one row per semitone, no gaps.
        for (lo, hi) in [(60, 60), (60, 67), (36, 84), (0, 127), (10, 100)] {
            let fit = Fit::new(lo, hi);
            let heights: Vec<u32> = (lo..=hi).map(|p| fit.bands(p).1 + 1).collect();
            let (min, max) = (heights.iter().min(), heights.iter().max());
            assert!(max.unwrap() - min.unwrap() <= 1, "{lo}..{hi}: {heights:?}");
            for p in lo..hi {
                let (above, _) = fit.bands(p + 1);
                let (first, extra) = fit.bands(p);
                assert_eq!(
                    above + fit.bands(p + 1).1 + 1,
                    first,
                    "{p} touches {}",
                    p + 1
                );
                assert!(first + extra < BANDS);
            }
        }
    }

    #[test]
    fn a_narrow_range_is_centred() {
        let (only, extra) = Fit::new(60, 60).bands(60);
        let middle = only + extra / 2;
        assert!(
            (60..=68).contains(&middle),
            "one note sits mid-clip: {middle}"
        );
        assert!(Fit::new(60, 62).bands(62).0 < Fit::new(60, 62).bands(60).0);
        // Still in bounds at the ends of the keyboard.
        for p in [0, 127] {
            let (first, extra) = Fit::new(p, p).bands(p);
            assert!(first + extra < BANDS);
        }
    }

    #[test]
    fn an_octave_up_is_higher_in_a_shared_fit() {
        // Two clips on a track, one note each, an octave apart (the owner's
        // finding): the track's fit puts them at different heights, 12 rows
        // apart, where a fit per clip centred both.
        let fit = Fit::new(48, 60);
        let (low, high) = (fit.bands(48), fit.bands(60));
        assert!(high.0 < low.0);
        let row = BANDS as f32 / f32::from(MIN_RANGE);
        let apart = (low.0 - high.0) as f32;
        assert!((apart - 12.0 * row).abs() <= 1.0, "{apart} bands apart");
    }

    #[test]
    fn instances_follow_the_clip_and_its_track() {
        let notes = [
            Note::new(q(1), q(1), 60, 100),
            Note::new(q(2), q(1), 72, 100),
        ];
        let mut out = Vec::new();
        mini_instances(&mut out, &clip(&notes), (3, Fit::new(60, 72)), Color32::RED);
        assert_eq!(out.len(), 2);
        let slot = |b: &BoxInstance| b.row & 0xFF_FFFF;
        assert!(out.iter().all(|b| slot(b) / BANDS == 3));
        assert!(
            slot(&out[0]) > slot(&out[1]),
            "the higher note is higher up"
        );
    }

    #[test]
    fn slots_divide_the_body_and_hide_in_a_tiny_row() {
        let body = Body {
            top: 100.0,
            height: 80.0,
            alpha: 1.0,
            above: 20.0,
            below: 4.0,
        };
        let s = slots(body);
        assert_eq!(s[0].top, 120.0);
        assert!((s[0].height * BANDS as f32 - 56.0).abs() < 1e-3);
        let tiny = slots(Body {
            height: 20.0,
            ..body
        });
        assert_eq!(tiny[0], RowSlot::HIDDEN);
    }
}
