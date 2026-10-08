//! The arrangement's horizontal view (M3 slice 4d): the beat at the left
//! edge and how many points a beat takes. In f64, so a sample-level zoom deep
//! into a long song stays exact (the GPU gets positions split the same way,
//! `lumen_render::beats`). Zooming, scrolling, fitting and the zoom limits
//! are pure functions here; the app applies them.

/// Points per sample at the closest zoom (owner: zoom to single samples).
pub const MAX_POINTS_PER_SAMPLE: f64 = 8.0;
/// The widest zoom: an hour at 120 BPM in under a thousand points.
pub const MIN_ZOOM: f64 = 0.1;
/// Room left on each side when fitting.
const FIT_MARGIN: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    /// The beat (quarter note) at the left edge.
    pub left: f64,
    /// Points per beat.
    pub zoom: f64,
}

/// How far the view may zoom, in points per beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
}

impl Limits {
    /// For audio at `frames_per_beat` (at the device rate and the tempo in
    /// view): out to [`MIN_ZOOM`], in to [`MAX_POINTS_PER_SAMPLE`].
    pub fn for_audio(frames_per_beat: f64) -> Self {
        Self {
            min: MIN_ZOOM,
            max: (frames_per_beat * MAX_POINTS_PER_SAMPLE).max(MIN_ZOOM),
        }
    }
}

impl Default for TimeView {
    fn default() -> Self {
        Self {
            left: 0.0,
            zoom: 24.0,
        }
    }
}

impl TimeView {
    /// The x of `beat`, for a view whose left edge is at `origin`.
    pub fn x(self, origin: f32, beat: f64) -> f32 {
        origin + ((beat - self.left) * self.zoom) as f32
    }

    /// The beat at `x`.
    pub fn beat_at(self, origin: f32, x: f32) -> f64 {
        self.left + f64::from(x - origin) / self.zoom
    }

    /// Zoomed by `factor`, keeping the beat `anchor` points from the left
    /// edge where it is (the pointer, or the playhead).
    pub fn zoomed(self, anchor: f32, factor: f64, limits: Limits) -> Self {
        let beat = self.left + f64::from(anchor) / self.zoom;
        let zoom = (self.zoom * factor).clamp(limits.min, limits.max);
        Self {
            left: (beat - f64::from(anchor) / zoom).max(0.0),
            zoom,
        }
    }

    /// Scrolled by `points` (positive moves the content right, as a wheel
    /// delta does). Never before the start.
    pub fn scrolled(self, points: f32) -> Self {
        Self {
            left: (self.left - f64::from(points) / self.zoom).max(0.0),
            ..self
        }
    }

    /// The view showing beats `from..to` across `width` points, with a
    /// small margin either side (W, "fit width").
    pub fn fit(from: f64, to: f64, width: f32, limits: Limits) -> Self {
        let span = (to - from).max(1.0);
        let zoom =
            (f64::from(width) / (span * (1.0 + 2.0 * FIT_MARGIN))).clamp(limits.min, limits.max);
        Self {
            left: (from - span * FIT_MARGIN).max(0.0),
            zoom,
        }
    }

    /// Between `self` (at 0) and `to` (at 1), keeping the beat at `anchor`
    /// still when given: zoom moves geometrically, so a glide feels even.
    pub fn between(self, to: Self, t: f64, anchor: Option<f32>) -> Self {
        let zoom = self.zoom * (to.zoom / self.zoom).powf(t);
        let left = match anchor {
            Some(anchor) => {
                let beat = self.left + f64::from(anchor) / self.zoom;
                beat - f64::from(anchor) / zoom
            }
            None => self.left + (to.left - self.left) * t,
        };
        Self {
            left: left.max(0.0),
            zoom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Limits = Limits {
        min: MIN_ZOOM,
        max: 1e6,
    };

    #[test]
    fn x_and_beat_are_inverse() {
        let view = TimeView {
            left: 10.0,
            zoom: 50.0,
        };
        assert_eq!(view.x(100.0, 12.0), 200.0);
        assert_eq!(view.beat_at(100.0, 200.0), 12.0);
    }

    #[test]
    fn zooming_keeps_the_anchor_still_and_respects_the_limits() {
        let view = TimeView {
            left: 8.0,
            zoom: 24.0,
        };
        let anchor = 240.0; // beat 18
        let closer = view.zoomed(anchor, 4.0, WIDE);
        assert_eq!(closer.zoom, 96.0);
        assert!((closer.beat_at(0.0, anchor) - 18.0).abs() < 1e-9);
        let capped = view.zoomed(
            anchor,
            1e9,
            Limits {
                min: 1.0,
                max: 500.0,
            },
        );
        assert_eq!(capped.zoom, 500.0);
        // Zooming far out around beat 18 would put the left edge before
        // the start; it stops there instead.
        let out = view.zoomed(anchor, 0.01, WIDE);
        assert_eq!(out.left, 0.0);
    }

    #[test]
    fn sample_level_zoom_deep_into_a_song_stays_exact() {
        // An hour in at 120 BPM, zoomed to 8 points per sample at 48 kHz:
        // the beat under the pointer is still exact to a fraction of a
        // sample after zooming there.
        let limits = Limits::for_audio(24_000.0);
        assert_eq!(limits.max, 192_000.0);
        let view = TimeView {
            left: 7_200.0,
            zoom: 24.0,
        }
        .zoomed(500.0, 1e9, limits);
        let one_sample = 1.0 / 24_000.0;
        let beat = view.beat_at(0.0, 500.0);
        assert!((beat - (7_200.0 + 500.0 / 24.0)).abs() < one_sample / 100.0);
        assert!((view.x(0.0, beat + one_sample) - 508.0).abs() < 1e-3);
    }

    #[test]
    fn fit_shows_the_whole_range_with_a_margin() {
        let view = TimeView::fit(0.0, 100.0, 1_000.0, WIDE);
        assert!(view.x(0.0, 0.0) >= 0.0);
        assert!(view.x(0.0, 100.0) <= 1_000.0);
        assert!(view.x(0.0, 100.0) > 950.0, "nearly full width");
        // An empty song still gets a sensible view.
        assert!(TimeView::fit(0.0, 0.0, 1_000.0, WIDE).zoom.is_finite());
    }

    #[test]
    fn scrolling_stops_at_the_start() {
        let view = TimeView {
            left: 1.0,
            zoom: 10.0,
        };
        assert_eq!(view.scrolled(-20.0).left, 3.0);
        assert_eq!(view.scrolled(50.0).left, 0.0);
    }

    #[test]
    fn a_glide_ends_where_it_aims_and_keeps_its_anchor() {
        let from = TimeView {
            left: 0.0,
            zoom: 10.0,
        };
        let to = from.zoomed(300.0, 8.0, WIDE);
        assert_eq!(from.between(to, 0.0, Some(300.0)), from);
        let end = from.between(to, 1.0, Some(300.0));
        assert!((end.zoom - to.zoom).abs() < 1e-9 && (end.left - to.left).abs() < 1e-9);
        let mid = from.between(to, 0.5, Some(300.0));
        assert!((mid.beat_at(0.0, 300.0) - 30.0).abs() < 1e-9);
    }
}
