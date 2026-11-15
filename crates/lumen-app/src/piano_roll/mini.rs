use eframe::egui::Color32;
use lumen_render::{BoxInstance, RowSlot, beats, spanning};

use super::notes::{ClipNotes, pack, velocity_alpha};

pub(crate) const BANDS: u32 = 128;
const MIN_RANGE: u8 = 48;

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

pub(crate) fn union(a: Option<(u8, u8)>, b: Option<(u8, u8)>) -> Option<(u8, u8)> {
    match (a, b) {
        (Some((a0, a1)), Some((b0, b1))) => Some((a0.min(b0), a1.max(b1))),
        (a, None) => a,
        (None, b) => b,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fit {
    first: f32,
    rows: f32,
}

impl Fit {
    pub(crate) fn new(lo: u8, hi: u8) -> Self {
        let (lo, hi) = (lo.min(hi), hi.max(lo));
        let span = f32::from(hi - lo) + 1.0;
        let rows = span.max(f32::from(MIN_RANGE));
        Self {
            first: f32::from(lo) - (rows - span) / 2.0,
            rows,
        }
    }

    pub(crate) fn bands(&self, pitch: u8) -> (u32, u32) {
        let top = self.first + self.rows;
        let edge = |p: f32| (((top - p) / self.rows) * BANDS as f32).round();
        let p = f32::from(pitch.min(127));
        let a = edge(p + 1.0).clamp(0.0, (BANDS - 1) as f32) as u32;
        let b = (edge(p) as u32).clamp(a + 1, BANDS);
        (a, b - a - 1)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Body {
    pub top: f32,
    pub height: f32,
    pub alpha: f32,
    pub above: f32,
    pub below: f32,
}

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
            loop_phase: Default::default(),
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
                    notes,
                    events: Vec::new(),
                },
            })
        };
        let notes = vec![
            Note::new(q(0), q(1), 64, 90),
            Note::new(q(1), q(1), 48, 90),
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
        for p in [0, 127] {
            let (first, extra) = Fit::new(p, p).bands(p);
            assert!(first + extra < BANDS);
        }
    }

    #[test]
    fn an_octave_up_is_higher_in_a_shared_fit() {
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
