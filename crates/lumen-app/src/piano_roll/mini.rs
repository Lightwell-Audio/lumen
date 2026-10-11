//! Notes inside arrangement clips (M5 slice 3b): a mini piano roll fitted
//! to each clip's pitch range, drawn on the GPU beside the clip bodies.
//!
//! The box layer places a box vertically only by its row slot, so a track
//! gets [`BANDS`] slots, thin horizontal bands under its clips' title bars,
//! and a clip maps its own pitch range onto them (`band`). The notes are
//! rebuilt only when the clips change (with the clip boxes), and panning
//! and zooming only move the view.

use eframe::egui::Color32;
use lumen_render::{BoxInstance, RowSlot, beats};

use super::notes::{ClipNotes, pack, velocity_alpha};

/// Bands a track's clips are divided into, top to bottom.
pub(crate) const BANDS: u32 = 32;
/// A clip's pitch range is shown at least this many semitones tall, so one
/// or two notes sit mid-clip instead of filling it.
const MIN_RANGE: u8 = 16;

/// The lowest and highest pitch of the notes a clip plays, if any.
pub(crate) fn pitch_range(clip: &ClipNotes<'_>) -> Option<(u8, u8)> {
    let span = clip
        .loop_length
        .map(|p| p.0)
        .filter(|&p| p > 0)
        .unwrap_or(clip.length.0)
        .min(clip.length.0);
    clip.notes
        .iter()
        .filter(|n| n.start().0 >= 0 && n.start().0 < span)
        .map(|n| n.pitch().min(127))
        .fold(None, |range, p| match range {
            None => Some((p, p)),
            Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
        })
}

/// The band (0 is the top) of `pitch` in a clip whose notes span `lo` to
/// `hi`: the range is centred when narrower than [`MIN_RANGE`], and
/// squeezed into the bands when wider than they are.
pub(crate) fn band(pitch: u8, lo: u8, hi: u8) -> u32 {
    let span = f32::from(hi.saturating_sub(lo)) + 1.0;
    let padded = span.max(f32::from(MIN_RANGE));
    let first = f32::from(lo) - (padded - span) / 2.0;
    let up = ((f32::from(pitch) - first) / (padded - 1.0) * (BANDS - 1) as f32).round();
    (BANDS - 1) - (up.clamp(0.0, (BANDS - 1) as f32) as u32)
}

/// The boxes of a clip's notes on `track` (a dense number among the tracks
/// with notes: its bands are slots `track * BANDS ..`), in `color`.
pub(crate) fn mini_instances(
    out: &mut Vec<BoxInstance>,
    clip: &ClipNotes<'_>,
    track: u32,
    color: Color32,
) {
    let Some((lo, hi)) = pitch_range(clip) else {
        return;
    };
    clip.each_placed(|placed, note| {
        let ghost = if placed.rep > 0 { 0.6 } else { 1.0 };
        let c = pack(
            color,
            (0.45 + 0.55 * velocity_alpha(note.velocity())) * ghost,
        );
        out.push(BoxInstance {
            start: beats(placed.start),
            end: beats(placed.end),
            row: track * BANDS + band(note.pitch().min(127), lo, hi),
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
            muted: &[],
        }
    }

    #[test]
    fn the_range_is_the_pitches_that_play() {
        let notes = [
            Note(q(0), q(1), 64, 90),
            Note(q(1), q(1), 48, 90),
            Note(q(9), q(1), 100, 90),
        ];
        assert_eq!(pitch_range(&clip(&notes)), Some((48, 64)));
        assert_eq!(pitch_range(&clip(&[])), None);
    }

    #[test]
    fn a_wide_range_fills_the_bands_high_notes_on_top() {
        assert_eq!(band(84, 36, 84), 0);
        assert_eq!(band(36, 36, 84), BANDS - 1);
        assert!(band(60, 36, 84) > band(72, 36, 84));
    }

    #[test]
    fn a_narrow_range_is_centred() {
        let only = band(60, 60, 60);
        assert!((14..=17).contains(&only), "one note sits mid-clip: {only}");
        assert!(band(62, 60, 62) < band(60, 60, 62));
        // Still in bounds at the ends of the keyboard.
        assert!(band(0, 0, 0) < BANDS && band(127, 127, 127) < BANDS);
    }

    #[test]
    fn instances_follow_the_clip_and_its_track() {
        let notes = [Note(q(1), q(1), 60, 100), Note(q(2), q(1), 72, 100)];
        let mut out = Vec::new();
        mini_instances(&mut out, &clip(&notes), 3, Color32::RED);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|b| b.row / BANDS == 3));
        assert!(out[0].row > out[1].row, "the higher note is higher up");
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
