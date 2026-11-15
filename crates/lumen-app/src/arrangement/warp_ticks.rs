use lumen_core::time::Ticks;
use lumen_core::warp::Marker;

use crate::timeview::TimeView;

pub(super) const FROM_ZOOM: f64 = 40.0;
pub(super) const MIN_GAP: f32 = 8.0;
pub(super) const MOST: usize = 256;

const MOST_TURNS: i64 = 4_096;

pub(super) struct Map<'a> {
    pub origin: Ticks,
    pub markers: &'a [Marker],
    pub period: Option<Ticks>,
    pub clip: (f32, f32),
}

pub(super) fn xs(view: TimeView, left: f32, map: &Map<'_>, visible: (f32, f32)) -> Vec<f32> {
    if view.zoom < FROM_ZOOM || map.markers.is_empty() {
        return Vec::new();
    }
    let (from, to) = (map.clip.0.max(visible.0), map.clip.1.min(visible.1));
    if from >= to {
        return Vec::new();
    }
    let tick_of = |x: f32| view.beat_at(left, x) * lumen_core::time::PPQ as f64;
    let first = tick_of(from) - map.origin.0 as f64;
    let last = tick_of(to) - map.origin.0 as f64;
    let mut out: Vec<f32> = Vec::new();
    let mut turn = |markers: &[Marker], offset: i64| {
        let start = markers.partition_point(|m| ((m.at.0 + offset) as f64) < first);
        for marker in &markers[start..] {
            if ((marker.at.0 + offset) as f64) > last || out.len() >= MOST {
                break;
            }
            let x = view.x(
                left,
                (map.origin + Ticks(marker.at.0 + offset)).as_quarters(),
            );
            if out.last().is_none_or(|prev| x - prev >= MIN_GAP) {
                out.push(x);
            }
        }
    };
    match map.period.filter(|p| p.0 > 0) {
        None => turn(map.markers, 0),
        Some(p) => {
            let ms = map.markers;
            let inside =
                &ms[ms.partition_point(|m| m.at.0 < 0)..ms.partition_point(|m| m.at.0 < p.0)];
            let (n0, n1) = (
                (first / p.0 as f64).floor() as i64,
                (last / p.0 as f64).floor() as i64,
            );
            if n1 - n0 > MOST_TURNS {
                return Vec::new();
            }
            for n in n0..=n1 {
                turn(inside, n * p.0);
            }
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

    fn map(markers: &[Marker], origin: Ticks, clip: (f32, f32)) -> Map<'_> {
        Map {
            origin,
            markers,
            period: None,
            clip,
        }
    }

    #[test]
    fn ticks_stand_at_their_ticks_from_the_content_origin() {
        let ms = marks(&[0, 3]);
        let got = xs(
            view(50.0),
            10.0,
            &map(&ms, Ticks::quarters(2), (0.0, 1_000.0)),
            (0.0, 1_000.0),
        );
        assert_eq!(got, vec![110.0, 260.0]);
    }

    #[test]
    fn nothing_shows_zoomed_out() {
        let ms = marks(&[0, 1, 2]);
        let m = map(&ms, Ticks::ZERO, (0.0, 900.0));
        assert!(xs(view(FROM_ZOOM - 1.0), 0.0, &m, (0.0, 900.0)).is_empty());
        assert_eq!(xs(view(FROM_ZOOM), 0.0, &m, (0.0, 900.0)).len(), 3);
    }

    #[test]
    fn only_the_markers_in_view_and_in_the_clip_show() {
        let ms = marks(&(0..1_000).collect::<Vec<_>>());
        let v = TimeView {
            left: 200.0,
            zoom: 60.0,
        };
        let got = xs(
            v,
            0.0,
            &map(&ms, Ticks::ZERO, (-6_000.0, 42_000.0)),
            (0.0, 600.0),
        );
        assert_eq!(got.len(), 11, "beats 200 to 210, both ends");
        assert!(got.iter().all(|x| (0.0..=600.0).contains(x)));
        assert!(
            xs(
                v,
                0.0,
                &map(&ms, Ticks::ZERO, (-6_000.0, -1.0)),
                (0.0, 600.0)
            )
            .is_empty()
        );
    }

    #[test]
    fn a_dense_map_is_thinned_and_capped() {
        let dense: Vec<Marker> = (0..4_000)
            .map(|i| Marker::new(f64::from(i), Ticks(i64::from(i) * PPQ / 4)))
            .collect();
        let got = xs(
            view(60.0),
            0.0,
            &map(&dense, Ticks::ZERO, (0.0, 1e6)),
            (0.0, 1e6),
        );
        assert!(got.windows(2).all(|w| w[1] - w[0] >= MIN_GAP));
        assert!(got.len() <= MOST);
    }

    #[test]
    fn a_loops_markers_repeat_in_every_turn() {
        let ms = marks(&[1]);
        let m = Map {
            period: Some(Ticks::quarters(4)),
            ..map(&ms, Ticks::quarters(2), (100.0, 1_000.0))
        };
        let got = xs(view(50.0), 0.0, &m, (0.0, 1_000.0));
        assert_eq!(got, vec![150.0, 350.0, 550.0, 750.0, 950.0]);
        let outside = marks(&[-1, 4, 5]);
        let m = Map {
            period: Some(Ticks::quarters(4)),
            ..map(&outside, Ticks::ZERO, (0.0, 1_000.0))
        };
        assert!(xs(view(50.0), 0.0, &m, (0.0, 1_000.0)).is_empty());
        let tiny = Map {
            period: Some(Ticks(1)),
            ..map(&ms, Ticks::ZERO, (0.0, 1_000.0))
        };
        assert!(xs(view(50.0), 0.0, &tiny, (0.0, 1_000.0)).is_empty());
    }
}
