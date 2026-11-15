pub const MAX_POINTS_PER_SAMPLE: f64 = 8.0;
pub const MIN_ZOOM: f64 = 0.001;
const FIT_MARGIN: f64 = 0.02;
const MIN_SONG: f64 = 128.0;
const ROOM: f64 = 0.1;
const SHOW_MARGIN: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    pub left: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
    pub end: f64,
    pub width: f32,
}

impl Limits {
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
    pub fn x(self, origin: f32, beat: f64) -> f32 {
        origin + ((beat - self.left) * self.zoom) as f32
    }

    pub fn beat_at(self, origin: f32, x: f32) -> f64 {
        self.left + f64::from(x - origin) / self.zoom
    }

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

    pub fn scrolled(self, points: f32, limits: Limits) -> Self {
        let left = self.left - f64::from(points) / self.zoom;
        let last = limits.last_left(self.zoom).max(self.left);
        Self {
            left: left.min(last).max(0.0),
            ..self
        }
    }

    pub fn holding(beat: f64, x: f32, zoom: f64, limits: Limits) -> Self {
        let zoom = zoom.clamp(limits.min, limits.max);
        Self {
            left: (beat - f64::from(x) / zoom)
                .min(limits.last_left(zoom))
                .max(0.0),
            zoom,
        }
    }

    pub fn fit(from: f64, to: f64, width: f32, limits: Limits) -> Self {
        let span = (to - from).max(1.0);
        let zoom =
            (f64::from(width) / (span * (1.0 + 2.0 * FIT_MARGIN))).clamp(limits.min, limits.max);
        Self {
            left: (from - span * FIT_MARGIN).max(0.0),
            zoom,
        }
    }

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
        let anchor = 240.0;
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
        let out = view.zoomed(anchor, 0.01, WIDE);
        assert_eq!(out.left, 0.0);
    }

    #[test]
    fn sample_level_zoom_deep_into_a_song_stays_exact() {
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
        let limits = Limits::new(1_000.0, 1_100.0, 24_000.0);
        assert!((limits.min - 1.0).abs() < 1e-9);
        assert!((limits.end - 1_100.0).abs() < 1e-9);
        let view = TimeView {
            left: 500.0,
            zoom: 4.0,
        };
        let out = view.zoomed(550.0, 1e-6, limits);
        assert!((out.zoom - 1.0).abs() < 1e-9);
        assert_eq!(out.left, 0.0);
        let end = view.scrolled(-1e6, limits);
        assert!((end.left - (1_100.0 - 1_100.0 / 4.0)).abs() < 1e-9);
        let out = end.zoomed(1_000.0, 0.5, limits);
        assert!((out.left - (1_100.0 - 1_100.0 / 2.0)).abs() < 1e-9);
        let past = TimeView {
            left: 5_000.0,
            zoom: 4.0,
        };
        assert_eq!(past.scrolled(-40.0, limits).left, 5_000.0);
        assert_eq!(past.scrolled(40.0, limits).left, 4_990.0);
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
