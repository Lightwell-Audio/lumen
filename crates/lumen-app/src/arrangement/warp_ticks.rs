//! A warped clip's markers as ticks on the arrangement (M9.5 S5b, the plan's
//! "shown on the arrangement's clip"): read there, never edited there (the
//! audio editor's warp strip is where they are made and moved).
//!
//! Pure: which x's get a tick. Markers are sorted by `at`, so the ones in view
//! are found by binary search and nothing is sized by a clip's width. Ticks
//! only show from [`FROM_ZOOM`] points a beat, and two never stand closer than
//! [`MIN_GAP`] points, so a dense map never turns into a comb.

use lumen_core::time::Ticks;
use lumen_core::warp::Marker;

use crate::timeview::TimeView;

/// Ticks show from this many points to a beat.
pub(super) const FROM_ZOOM: f64 = 40.0;
/// The least distance between two ticks, in points.
pub(super) const MIN_GAP: f32 = 8.0;
/// The most ticks one clip draws.
pub(super) const MOST: usize = 256;

/// The x of each tick of `markers` (counted from `origin`, the clip's content
/// origin) that lies inside `clip` (its left and right edges) and `visible`
/// (the canvas's), left to right.
pub(super) fn xs(
    view: TimeView,
    left: f32,
    origin: Ticks,
    markers: &[Marker],
    clip: (f32, f32),
    visible: (f32, f32),
) -> Vec<f32> {
    if view.zoom < FROM_ZOOM || markers.is_empty() {
        return Vec::new();
    }
    let (from, to) = (clip.0.max(visible.0), clip.1.min(visible.1));
    if from >= to {
        return Vec::new();
    }
    // The ticks of the window, as the marker's own clock reads them.
    let tick_of = |x: f32| view.beat_at(left, x) * lumen_core::time::PPQ as f64;
    let first = tick_of(from) - origin.0 as f64;
    let last = tick_of(to) - origin.0 as f64;
    let start = markers.partition_point(|m| (m.at.0 as f64) < first);
    let mut out: Vec<f32> = Vec::new();
    for marker in &markers[start..] {
        if (marker.at.0 as f64) > last || out.len() >= MOST {
            break;
        }
        let x = view.x(left, (origin + marker.at).as_quarters());
        if out.last().is_none_or(|prev| x - prev >= MIN_GAP) {
            out.push(x);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::PPQ;

    fn marks(ats: &[i64]) -> Vec<Marker> {
        ats.iter()
            .map(|q| Marker::new(*q as f64, Ticks::quarters(*q)))
            .collect()
    }

    fn view(zoom: f64) -> TimeView {
        TimeView { left: 0.0, zoom }
    }

    #[test]
    fn ticks_stand_at_their_ticks_from_the_content_origin() {
        // Origin at beat 2, zoom 50: a marker at 3 beats is at beat 5.
        let xs = xs(
            view(50.0),
            10.0,
            Ticks::quarters(2),
            &marks(&[0, 3]),
            (0.0, 1_000.0),
            (0.0, 1_000.0),
        );
        assert_eq!(xs, vec![110.0, 260.0]);
    }

    #[test]
    fn nothing_shows_zoomed_out() {
        let ms = marks(&[0, 1, 2]);
        assert!(
            xs(
                view(FROM_ZOOM - 1.0),
                0.0,
                Ticks::ZERO,
                &ms,
                (0.0, 900.0),
                (0.0, 900.0)
            )
            .is_empty()
        );
        assert_eq!(
            xs(
                view(FROM_ZOOM),
                0.0,
                Ticks::ZERO,
                &ms,
                (0.0, 900.0),
                (0.0, 900.0)
            )
            .len(),
            3
        );
    }

    #[test]
    fn only_the_markers_in_view_and_in_the_clip_show() {
        let ms = marks(&(0..1_000).collect::<Vec<_>>());
        // The clip runs beats 100 to 900, the view shows beats 200 to 210.
        let v = TimeView {
            left: 200.0,
            zoom: 60.0,
        };
        let got = xs(v, 0.0, Ticks::ZERO, &ms, (-6_000.0, 42_000.0), (0.0, 600.0));
        assert_eq!(got.len(), 11, "beats 200 to 210, both ends");
        assert!(got.iter().all(|x| (0.0..=600.0).contains(x)));
        // A clip that ends before the view shows none.
        assert!(xs(v, 0.0, Ticks::ZERO, &ms, (-6_000.0, -1.0), (0.0, 600.0)).is_empty());
    }

    #[test]
    fn a_dense_map_is_thinned_and_capped() {
        // A marker every sixteenth at 60 points a beat: 3.75 points apart.
        let dense: Vec<Marker> = (0..4_000)
            .map(|i| Marker::new(f64::from(i), Ticks(i64::from(i) * PPQ / 4)))
            .collect();
        let got = xs(view(60.0), 0.0, Ticks::ZERO, &dense, (0.0, 1e6), (0.0, 1e6));
        assert!(got.windows(2).all(|w| w[1] - w[0] >= MIN_GAP));
        assert!(got.len() <= MOST);
    }
}
