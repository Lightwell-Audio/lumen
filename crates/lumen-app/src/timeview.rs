//! The arrangement's horizontal view (M3 slice 4d): the beat at the left
//! edge and how many points a beat takes. In f64, so a sample-level zoom deep
//! into a long song stays exact (the GPU gets positions split the same way,
//! `lumen_render::beats`). Zooming, scrolling, fitting and the zoom limits
//! are pure functions here; the app applies them.

/// Points per sample at the closest zoom (owner: zoom to single samples).
pub const MAX_POINTS_PER_SAMPLE: f64 = 8.0;
/// The widest zoom there can ever be (a safety floor: the song's length
/// sets the real one, [`Limits::new`]).
pub const MIN_ZOOM: f64 = 0.001;
/// Room left on each side when fitting.
const FIT_MARGIN: f64 = 0.02;
/// The view reaches at least this far (beats: 32 bars of 4/4), so an empty
/// or short song still has room to work in.
const MIN_SONG: f64 = 128.0;
/// Room after the song: the view reaches this fraction past its end.
const ROOM: f64 = 0.1;
/// Room kept between a span scrolled into view and the view's edge, as a
/// fraction of the view's width.
const SHOW_MARGIN: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    /// The beat (quarter note) at the left edge.
    pub left: f64,
    /// Points per beat.
    pub zoom: f64,
}

/// How far the view may zoom (points per beat) and scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
    /// The last beat the view scrolls to: the song with some room after it.
    pub end: f64,
    /// The view's width in points.
    pub width: f32,
}

impl Limits {
    /// The song sets how far out the view goes: `song` is
    /// the furthest beat in use (clips, playhead, loop), shown `width`
    /// points wide with [`ROOM`] after it. In, it goes to
    /// [`MAX_POINTS_PER_SAMPLE`] for audio at `frames_per_beat` (the device
    /// rate at the tempo in view).
    pub fn new(song: f64, width: f32, frames_per_beat: f64) -> Self {
        let end = song.max(MIN_SONG) * (1.0 + ROOM);
        let min = (f64::from(width) / end).max(MIN_ZOOM);
        Self {
            min,
            max: (frames_per_beat * MAX_POINTS_PER_SAMPLE).max(min),
            end,
            width,
        }
    }

    /// The furthest left edge at `zoom`: the view ends at [`Self::end`].
    fn last_left(self, zoom: f64) -> f64 {
        (self.end - f64::from(self.width) / zoom).max(0.0)
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
    /// edge where it is (the pointer, or the playhead), within the start
    /// and the song's end.
    pub fn zoomed(self, anchor: f32, factor: f64, limits: Limits) -> Self {
        let beat = self.left + f64::from(anchor) / self.zoom;
        let zoom = (self.zoom * factor).clamp(limits.min, limits.max);
        Self {
            left: (beat - f64::from(anchor) / zoom)
                .min(limits.last_left(zoom))
                .max(0.0),
            zoom,
        }
    }

    /// Scrolled by `points` (positive moves the content right, as a wheel
    /// delta does). Never before the start, nor further than the song's
    /// end (a view already past it stays, and can scroll back).
    pub fn scrolled(self, points: f32, limits: Limits) -> Self {
        let left = self.left - f64::from(points) / self.zoom;
        let last = limits.last_left(self.zoom).max(self.left);
        Self {
            left: left.min(last).max(0.0),
            ..self
        }
    }

    /// The view at `zoom` with `beat` held `x` points from the left edge
    /// (dragging the ruler: the grabbed beat stays under the pointer),
    /// within the limits.
    pub fn holding(beat: f64, x: f32, zoom: f64, limits: Limits) -> Self {
        let zoom = zoom.clamp(limits.min, limits.max);
        Self {
            left: (beat - f64::from(x) / zoom)
                .min(limits.last_left(zoom))
                .max(0.0),
            zoom,
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

    /// Scrolled the least, at the same zoom, to show beats `from..to` (a
    /// clip chosen from the keyboard) with a little room. A span wider than
    /// the view shows from its start.
    pub fn showing(self, from: f64, to: f64, limits: Limits) -> Self {
        let width = f64::from(limits.width) / self.zoom;
        let margin = width * SHOW_MARGIN;
        let left = if from < self.left + margin || to - from > width - 2.0 * margin {
            from - margin
        } else if to > self.left + width - margin {
            to + margin - width
        } else {
            return self;
        };
        let last = limits.last_left(self.zoom).max(self.left);
        Self {
            left: left.min(last).max(0.0),
            ..self
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

    #[test]
    fn showing_a_span_scrolls_the_least() {
        // 10 points a beat across 1000 points: beats 0 to 100 in view.
        let limits = Limits::new(1000.0, 1000.0, 24_000.0);
        let view = TimeView {
            left: 0.0,
            zoom: 10.0,
        };
        assert_eq!(view.showing(20.0, 30.0, limits), view, "already in view");
        let right = view.showing(120.0, 130.0, limits);
        assert_eq!(right.zoom, 10.0);
        assert!((right.left - 35.0).abs() < 1e-9, "its end 5% in: {right:?}");
        let back = right.showing(10.0, 20.0, limits);
        assert!((back.left - 5.0).abs() < 1e-9, "its start 5% in: {back:?}");
        let wide = view.showing(200.0, 500.0, limits);
        assert!((wide.left - 195.0).abs() < 1e-9, "wider: from its start");
        assert_eq!(view.showing(1.0, 2.0, limits).left, 0.0, "never before 0");
    }

    const WIDE: Limits = Limits {
        min: MIN_ZOOM,
        max: 1e6,
        end: 1e9,
        width: 1_000.0,
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
                ..WIDE
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
        let limits = Limits::new(10_000.0, 1_000.0, 24_000.0);
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
    fn a_held_beat_stays_under_the_pointer() {
        let view = TimeView::holding(40.0, 200.0, 10.0, WIDE);
        assert_eq!(
            view,
            TimeView {
                left: 20.0,
                zoom: 10.0
            }
        );
        assert!((view.beat_at(0.0, 200.0) - 40.0).abs() < 1e-9);
        // Near the start it stops at the start; the zoom is limited.
        let near = TimeView::holding(1.0, 500.0, 1e9, Limits { max: 100.0, ..WIDE });
        assert_eq!(
            near,
            TimeView {
                left: 0.0,
                zoom: 100.0
            }
        );
    }

    #[test]
    fn scrolling_stops_at_the_start() {
        let view = TimeView {
            left: 1.0,
            zoom: 10.0,
        };
        assert_eq!(view.scrolled(-20.0, WIDE).left, 3.0);
        assert_eq!(view.scrolled(50.0, WIDE).left, 0.0);
    }

    #[test]
    fn the_song_sets_how_far_out_and_along_the_view_goes() {
        // A 1,000-beat song, 1,100 points wide: out to the song and its
        // 10% of room, 1 point per beat.
        let limits = Limits::new(1_000.0, 1_100.0, 24_000.0);
        assert!((limits.min - 1.0).abs() < 1e-9);
        assert!((limits.end - 1_100.0).abs() < 1e-9);
        let view = TimeView {
            left: 500.0,
            zoom: 4.0,
        };
        let out = view.zoomed(550.0, 1e-6, limits);
        assert!((out.zoom - 1.0).abs() < 1e-9);
        // All the way out, the whole song is in view from the start.
        assert_eq!(out.left, 0.0);
        // Scrolling stops where the view's right edge meets the end...
        let end = view.scrolled(-1e6, limits);
        assert!((end.left - (1_100.0 - 1_100.0 / 4.0)).abs() < 1e-9);
        // ...and zooming out near the end keeps the end at the right edge.
        let out = end.zoomed(1_000.0, 0.5, limits);
        assert!((out.left - (1_100.0 - 1_100.0 / 2.0)).abs() < 1e-9);
        // A view already past the end stays, and can scroll back.
        let past = TimeView {
            left: 5_000.0,
            zoom: 4.0,
        };
        assert_eq!(past.scrolled(-40.0, limits).left, 5_000.0);
        assert_eq!(past.scrolled(40.0, limits).left, 4_990.0);
        // A short or empty song still has 32 bars and room to work in.
        let empty = Limits::new(0.0, 1_000.0, 24_000.0);
        assert!((empty.end - 128.0 * 1.1).abs() < 1e-9);
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
