//! Where tempo values and time-signature changes sit in their lanes (M7
//! slice A6): pure, so every hit test is tested here. The tempo lane's
//! scale fits the song's own tempos, with room above and below; a time
//! signature's chip is as wide as its text.

use eframe::egui::{Pos2, Rect, pos2};
use lumen_core::meter::MeterPoint;
use lumen_core::time::{MAX_BPM, MIN_BPM, TempoPoint, Ticks};

use crate::theme;
use crate::timeview::TimeView;

/// The least span of tempos the lane shows, so a song at one tempo reads
/// as a level line with room to move it.
const LEAST_SPAN: f64 = 30.0;
/// Room above and below the extremes, as a share of the span.
const MARGIN: f64 = 0.25;

/// The tempos the lane's height spans: `lo` at the bottom, `hi` at the top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scale {
    pub lo: f64,
    pub hi: f64,
}

impl Scale {
    /// The scale that holds every tempo in `points`.
    pub fn fit(points: &[TempoPoint]) -> Self {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in points {
            lo = lo.min(p.bpm);
            hi = hi.max(p.bpm);
        }
        if lo > hi {
            (lo, hi) = (120.0, 120.0);
        }
        let span = (hi - lo).max(LEAST_SPAN);
        let middle = (lo + hi) / 2.0;
        let half = span / 2.0 * (1.0 + 2.0 * MARGIN);
        Scale {
            lo: middle - half,
            hi: middle + half,
        }
    }

    /// Where `bpm` sits, 0 at the bottom and 1 at the top.
    pub fn height(self, bpm: f64) -> f32 {
        ((bpm - self.lo) / (self.hi - self.lo)) as f32
    }

    /// The tempo at `height`, clamped to what a song may have.
    pub fn bpm(self, height: f32) -> f64 {
        (self.lo + f64::from(height) * (self.hi - self.lo)).clamp(MIN_BPM, MAX_BPM)
    }
}

/// A tempo in the lane's words: whole BPM, or to a tenth.
pub(crate) fn bpm_text(bpm: f64) -> String {
    let text = format!("{bpm:.1}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A tempo dragged to: whole BPM, or to a tenth when `fine` (Alt).
pub(crate) fn snap_bpm(bpm: f64, fine: bool) -> f64 {
    let bpm = if fine {
        (bpm * 10.0).round() / 10.0
    } else {
        bpm.round()
    };
    bpm.clamp(MIN_BPM, MAX_BPM)
}

/// Reads a typed tempo: "96", "96.5" or "96 BPM".
pub(crate) fn parse_bpm(text: &str) -> Option<f64> {
    let text = text
        .trim()
        .trim_end_matches(|c: char| c.is_alphabetic())
        .trim();
    let bpm: f64 = text.parse().ok()?;
    (bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm)).then_some(bpm)
}

/// A lane's rectangle on the screen and the view that maps time to x.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Plane {
    pub rect: Rect,
    /// Where beat 0 of the view's left edge sits (the lanes' left).
    pub left: f32,
    pub view: TimeView,
}

impl Plane {
    pub fn x(&self, at: Ticks) -> f32 {
        self.view.x(self.left, at.as_quarters())
    }

    pub fn ticks_at(&self, x: f32) -> Ticks {
        crate::ruler::ticks_at(self.view.beat_at(self.left, x))
    }

    /// The vertical room a value has: the lane less its padding.
    fn inner(&self) -> (f32, f32) {
        (
            self.rect.top() + theme::size::AUTO_PAD,
            self.rect.bottom() - theme::size::AUTO_PAD,
        )
    }

    pub fn y(&self, scale: Scale, bpm: f64) -> f32 {
        let (top, bottom) = self.inner();
        bottom - scale.height(bpm) * (bottom - top)
    }

    pub fn bpm_at(&self, scale: Scale, y: f32) -> f64 {
        let (top, bottom) = self.inner();
        scale.bpm(((bottom - y) / (bottom - top).max(1.0)).clamp(0.0, 1.0))
    }

    pub fn pos(&self, scale: Scale, point: &TempoPoint) -> Pos2 {
        pos2(self.x(point.at), self.y(scale, point.bpm))
    }
}

/// The tempo point nearest `p` within reach, nearest first in x.
pub(crate) fn tempo_hit(
    plane: &Plane,
    scale: Scale,
    points: &[TempoPoint],
    p: Pos2,
) -> Option<usize> {
    let reach = theme::size::AUTO_GRAB;
    points
        .iter()
        .enumerate()
        .map(|(i, point)| (i, plane.pos(scale, point).distance(p)))
        .filter(|&(_, d)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The width a time-signature chip takes: the text, with room round it. The
/// monospace's characters are about six tenths of the size wide.
pub(crate) fn chip_width(text: &str) -> f32 {
    text.chars().count() as f32 * theme::size::MONO_SMALL * 0.62 + 2.0 * theme::size::SONG_CHIP_PAD
}

/// The chip of change `point`: it begins at the change's bar line.
pub(crate) fn chip(plane: &Plane, point: &MeterPoint) -> Rect {
    let width = chip_width(&point.meter.to_string());
    let x = plane.x(point.at);
    Rect::from_x_y_ranges(
        x..=x + width,
        plane.rect.top() + 2.0..=plane.rect.bottom() - 2.0,
    )
}

/// The time-signature change under `p`, if there is one.
pub(crate) fn meter_hit(plane: &Plane, points: &[MeterPoint], p: Pos2) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .rev()
        .find(|(_, point)| chip(plane, point).contains(p))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::meter::Meter;

    fn tempo(bpm: f64) -> TempoPoint {
        TempoPoint {
            at: Ticks::ZERO,
            bpm,
            ramp: false,
        }
    }

    fn plane() -> Plane {
        // 10 points a beat, 600 wide, 44 tall, from x 100.
        Plane {
            rect: Rect::from_min_size(pos2(100.0, 0.0), eframe::egui::vec2(600.0, 44.0)),
            left: 100.0,
            view: TimeView {
                left: 0.0,
                zoom: 10.0,
            },
        }
    }

    #[test]
    fn the_scale_holds_every_tempo_with_room_and_a_least_span() {
        let level = Scale::fit(&[tempo(120.0)]);
        assert!(level.hi - level.lo >= LEAST_SPAN);
        assert!(
            (level.height(120.0) - 0.5).abs() < 1e-6,
            "level is centered"
        );
        let wide = Scale::fit(&[tempo(60.0), tempo(180.0)]);
        assert!(wide.height(60.0) > 0.0 && wide.height(180.0) < 1.0);
        assert!(wide.height(60.0) < wide.height(180.0));
        assert_eq!(Scale::fit(&[]).height(120.0), 0.5);
    }

    #[test]
    fn height_and_bpm_are_inverses_within_the_range() {
        let s = Scale::fit(&[tempo(90.0), tempo(150.0)]);
        for bpm in [60.0, 90.0, 117.5, 150.0, 200.0] {
            assert!((s.bpm(s.height(bpm)) - bpm).abs() < 1e-3, "{bpm}");
        }
        assert_eq!(s.bpm(-5.0), MIN_BPM, "never below what a song may have");
        assert_eq!(
            Scale {
                lo: 0.0,
                hi: 5000.0
            }
            .bpm(1.0),
            MAX_BPM
        );
    }

    #[test]
    fn a_dragged_tempo_snaps_to_whole_bpm_or_a_tenth() {
        assert_eq!(snap_bpm(96.4, false), 96.0);
        assert_eq!(snap_bpm(96.46, true), 96.5);
        assert_eq!(snap_bpm(5.0, false), MIN_BPM);
        assert_eq!(bpm_text(96.0), "96");
        assert_eq!(bpm_text(96.5), "96.5");
        assert_eq!(bpm_text(96.04), "96");
    }

    #[test]
    fn typed_tempos_are_understood() {
        assert_eq!(parse_bpm("96"), Some(96.0));
        assert_eq!(parse_bpm(" 96.5 BPM"), Some(96.5));
        assert_eq!(parse_bpm("12"), None, "below 20");
        assert_eq!(parse_bpm("1000"), None, "above 999");
        assert_eq!(parse_bpm("fast"), None);
        assert_eq!(parse_bpm("NaN"), None);
    }

    #[test]
    fn a_point_is_grabbed_within_reach_and_the_nearest_wins() {
        let plane = plane();
        let scale = Scale::fit(&[tempo(120.0)]);
        let points = [
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(1),
                bpm: 120.0,
                ramp: false,
            },
        ];
        let at = plane.pos(scale, &points[1]);
        assert_eq!(tempo_hit(&plane, scale, &points, at), Some(1));
        assert_eq!(
            tempo_hit(&plane, scale, &points, at + eframe::egui::vec2(3.0, 2.0)),
            Some(1)
        );
        assert_eq!(
            tempo_hit(&plane, scale, &points, at + eframe::egui::vec2(40.0, 0.0)),
            None
        );
    }

    #[test]
    fn a_chip_begins_at_its_bar_line_and_is_hit_inside_its_box() {
        let plane = plane();
        let points = [
            MeterPoint {
                at: Ticks::ZERO,
                meter: Meter::FOUR_FOUR,
            },
            MeterPoint {
                at: Ticks::quarters(8),
                meter: Meter {
                    numerator: 7,
                    denominator: 8,
                },
            },
        ];
        let second = chip(&plane, &points[1]);
        assert!((second.left() - (100.0 + 80.0)).abs() < 1e-3);
        assert_eq!(meter_hit(&plane, &points, second.center()), Some(1));
        assert_eq!(
            meter_hit(
                &plane,
                &points,
                pos2(second.right() + 20.0, second.center().y)
            ),
            None
        );
        assert_eq!(
            meter_hit(&plane, &points, chip(&plane, &points[0]).center()),
            Some(0)
        );
    }
}
