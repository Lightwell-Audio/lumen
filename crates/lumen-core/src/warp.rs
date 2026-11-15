use serde::{Deserialize, Serialize};

use crate::time::{PPQ, Ticks};

pub const MAX_MARKERS: usize = 4_096;

pub const MIN_RATIO: f64 = 0.25;

pub const MAX_RATIO: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub source: f64,
    pub at: Ticks,
}

impl Marker {
    pub fn new(source: f64, at: Ticks) -> Self {
        Marker { source, at }
    }
}

pub fn seconds_per_tick(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

pub fn is_sound(markers: &[Marker]) -> bool {
    markers.len() <= MAX_MARKERS
        && markers.iter().all(|m| m.source.is_finite())
        && markers
            .windows(2)
            .all(|w| w[0].at < w[1].at && w[0].source < w[1].source)
}

pub fn ratio(a: Marker, b: Marker, original_bpm: f64) -> f64 {
    (b.source - a.source) / ((b.at.0 - a.at.0) as f64 * seconds_per_tick(original_bpm))
}

pub fn source_at(markers: &[Marker], original_bpm: f64, at: Ticks) -> f64 {
    let outside = seconds_per_tick(original_bpm);
    let (Some(first), Some(last)) = (markers.first(), markers.last()) else {
        return at.0 as f64 * outside;
    };
    if at <= first.at {
        return first.source - (first.at.0 - at.0) as f64 * outside;
    }
    if at >= last.at {
        return last.source + (at.0 - last.at.0) as f64 * outside;
    }
    let after = markers.partition_point(|m| m.at <= at);
    let (a, b) = (markers[after - 1], markers[after]);
    let along = (at.0 - a.at.0) as f64 / (b.at.0 - a.at.0) as f64;
    a.source + (b.source - a.source) * along
}

pub fn source_at_f64(markers: &[Marker], original_bpm: f64, at: f64) -> f64 {
    let outside = seconds_per_tick(original_bpm);
    let (Some(first), Some(last)) = (markers.first(), markers.last()) else {
        return at * outside;
    };
    if at <= first.at.0 as f64 {
        return first.source - (first.at.0 as f64 - at) * outside;
    }
    if at >= last.at.0 as f64 {
        return last.source + (at - last.at.0 as f64) * outside;
    }
    let after = markers.partition_point(|m| m.at.0 as f64 <= at);
    let (a, b) = (markers[after - 1], markers[after]);
    let along = (at - a.at.0 as f64) / (b.at.0 - a.at.0) as f64;
    a.source + (b.source - a.source) * along
}

pub fn tick_at_source(markers: &[Marker], original_bpm: f64, source: f64) -> Ticks {
    let outside = seconds_per_tick(original_bpm);
    let round = |ticks: f64| Ticks(ticks.round().clamp(-1.0e18, 1.0e18) as i64);
    let (Some(first), Some(last)) = (markers.first(), markers.last()) else {
        return round(source / outside);
    };
    if source <= first.source {
        return round(first.at.0 as f64 - (first.source - source) / outside);
    }
    if source >= last.source {
        return round(last.at.0 as f64 + (source - last.source) / outside);
    }
    let after = markers.partition_point(|m| m.source <= source);
    let (a, b) = (markers[after - 1], markers[after]);
    let along = (source - a.source) / (b.source - a.source);
    round(a.at.0 as f64 + (b.at.0 - a.at.0) as f64 * along)
}

pub fn shift(markers: &mut [Marker], by: Ticks) {
    for m in markers {
        m.at = m.at + by;
    }
}

pub fn mirror(markers: &mut [Marker], sum: Ticks, total: f64) {
    for m in markers.iter_mut() {
        *m = Marker::new(total - m.source, Ticks(sum.0 - m.at.0));
    }
    markers.reverse();
}

pub fn marker_at(markers: &[Marker], original_bpm: f64, at: Ticks) -> Marker {
    Marker::new(source_at(markers, original_bpm, at), at)
}

pub fn marker_on(markers: &[Marker], original_bpm: f64, source: f64) -> Marker {
    Marker::new(source, tick_at_source(markers, original_bpm, source))
}

pub fn with_marker(markers: &[Marker], marker: Marker) -> Option<(Vec<Marker>, usize)> {
    if markers.len() >= MAX_MARKERS {
        return None;
    }
    let index = markers.partition_point(|m| m.at < marker.at);
    let mut out = markers.to_vec();
    out.insert(index, marker);
    is_sound(&out).then_some((out, index))
}

pub fn without_marker(markers: &[Marker], index: usize) -> Option<Vec<Marker>> {
    if markers.len() < 2 || index >= markers.len() {
        return None;
    }
    let mut out = markers.to_vec();
    out.remove(index);
    Some(out)
}

pub fn ratios_beside(
    markers: &[Marker],
    original_bpm: f64,
    index: usize,
) -> (Option<f64>, Option<f64>) {
    let Some(&m) = markers.get(index) else {
        return (None, None);
    };
    let before = index
        .checked_sub(1)
        .and_then(|i| markers.get(i))
        .map(|a| ratio(*a, m, original_bpm));
    let after = markers.get(index + 1).map(|b| ratio(m, *b, original_bpm));
    (before, after)
}

const FREE: i64 = 1 << 50;

const SLACK: f64 = 1e-6;

pub fn tick_limits(markers: &[Marker], original_bpm: f64, index: usize) -> (Ticks, Ticks) {
    let Some(&m) = markers.get(index) else {
        return (Ticks::ZERO, Ticks::ZERO);
    };
    let per_tick = seconds_per_tick(original_bpm);
    let (mut low, mut high) = (m.at.0 - FREE, m.at.0 + FREE);
    if let Some(a) = index.checked_sub(1).and_then(|i| markers.get(i)) {
        let ticks = (m.source - a.source) / per_tick;
        low = low.max(a.at.0 + ((ticks / MAX_RATIO - SLACK).ceil() as i64).max(1));
        high = high.min(a.at.0 + (ticks / MIN_RATIO + SLACK).floor() as i64);
    }
    if let Some(b) = markers.get(index + 1) {
        let ticks = (b.source - m.source) / per_tick;
        low = low.max(b.at.0 - (ticks / MIN_RATIO + SLACK).floor() as i64);
        high = high.min(b.at.0 - ((ticks / MAX_RATIO - SLACK).ceil() as i64).max(1));
    }
    (Ticks(low.min(m.at.0)), Ticks(high.max(m.at.0)))
}

pub fn source_limits(markers: &[Marker], original_bpm: f64, index: usize) -> (f64, f64) {
    let Some(&m) = markers.get(index) else {
        return (0.0, 0.0);
    };
    let per_tick = seconds_per_tick(original_bpm);
    let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
    if let Some(a) = index.checked_sub(1).and_then(|i| markers.get(i)) {
        let seconds = (m.at.0 - a.at.0) as f64 * per_tick;
        low = low.max(a.source + seconds * MIN_RATIO);
        high = high.min(a.source + seconds * MAX_RATIO);
    }
    if let Some(b) = markers.get(index + 1) {
        let seconds = (b.at.0 - m.at.0) as f64 * per_tick;
        low = low.max(b.source - seconds * MAX_RATIO);
        high = high.min(b.source - seconds * MIN_RATIO);
    }
    (low.min(m.source), high.max(m.source))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn map() -> Vec<Marker> {
        vec![Marker::new(1.0, q(2)), Marker::new(2.0, q(4))]
    }

    #[test]
    fn between_markers_the_file_follows_the_song_linearly() {
        assert_eq!(source_at(&map(), 100.0, q(2)), 1.0);
        assert_eq!(source_at(&map(), 100.0, q(3)), 1.5);
        assert_eq!(source_at(&map(), 100.0, q(4)), 2.0);
    }

    #[test]
    fn outside_the_markers_the_original_tempo_applies() {
        assert!((source_at(&map(), 120.0, q(5)) - 2.5).abs() < 1e-12);
        assert!((source_at(&map(), 120.0, q(0)) - 0.0).abs() < 1e-12);
        assert!((source_at(&map(), 60.0, q(5)) - 3.0).abs() < 1e-12);
        assert!((source_at(&map(), 60.0, q(1)) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn the_fractional_map_agrees_on_whole_ticks_and_runs_between_them() {
        for bpm in [60.0, 120.0, 187.5] {
            for t in [
                -3 * PPQ,
                0,
                PPQ / 3,
                q(2).0,
                q(3).0 + 17,
                q(4).0,
                q(9).0 + 1,
            ] {
                let whole = source_at(&map(), bpm, Ticks(t));
                let fractional = source_at_f64(&map(), bpm, t as f64);
                assert!((whole - fractional).abs() < 1e-9, "{bpm} BPM, tick {t}");
            }
        }
        let a = source_at_f64(&map(), 120.0, q(3).0 as f64);
        let b = source_at_f64(&map(), 120.0, q(3).0 as f64 + 0.5);
        assert!(b > a && b - a < 1e-6);
        assert!((source_at_f64(&[], 120.0, q(4).0 as f64) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn an_empty_map_pins_second_zero_to_tick_zero() {
        assert!((source_at(&[], 120.0, q(4)) - 2.0).abs() < 1e-12);
        assert_eq!(tick_at_source(&[], 120.0, 2.0), q(4));
    }

    #[test]
    fn tick_at_source_inverts_source_at() {
        for bpm in [60.0, 92.0, 120.0, 187.5] {
            for t in [
                -3 * PPQ,
                0,
                PPQ / 3,
                q(2).0,
                q(3).0 + 17,
                q(4).0,
                q(9).0 + 1,
            ] {
                let s = source_at(&map(), bpm, Ticks(t));
                let back = tick_at_source(&map(), bpm, s);
                assert!((back.0 - t).abs() <= 1, "{bpm} BPM, tick {t}: {back:?}");
            }
        }
    }

    #[test]
    fn a_map_must_move_forward() {
        assert!(is_sound(&map()));
        assert!(is_sound(&[]));
        let flat = [Marker::new(1.0, q(2)), Marker::new(1.0, q(4))];
        assert!(!is_sound(&flat));
        let back = [Marker::new(2.0, q(2)), Marker::new(1.0, q(4))];
        assert!(!is_sound(&back));
        let same_tick = [Marker::new(1.0, q(2)), Marker::new(2.0, q(2))];
        assert!(!is_sound(&same_tick));
        assert!(!is_sound(&[Marker::new(f64::NAN, q(0))]));
    }

    #[test]
    fn ratio_is_the_speed_against_the_original_tempo() {
        let [a, b] = [Marker::new(0.0, q(0)), Marker::new(2.0, q(4))];
        assert!((ratio(a, b, 120.0) - 1.0).abs() < 1e-12);
        assert!((ratio(a, b, 240.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn mirroring_twice_returns_the_map() {
        let mut markers = map();
        mirror(&mut markers, q(8), 10.0);
        assert!(is_sound(&markers));
        assert_eq!(markers[0], Marker::new(8.0, q(4)));
        assert_eq!(markers[1], Marker::new(9.0, q(6)));
        mirror(&mut markers, q(8), 10.0);
        assert_eq!(markers, map());
    }

    #[test]
    fn a_marker_added_where_the_audio_plays_changes_nothing_heard() {
        let before = map();
        for at in [q(1), q(3), q(6)] {
            let (after, index) =
                with_marker(&before, marker_at(&before, 120.0, at)).expect("a free tick");
            assert_eq!(after[index].at, at);
            assert!(is_sound(&after));
            for t in [-PPQ, 0, PPQ / 2, 2 * PPQ + 7, 5 * PPQ, 9 * PPQ] {
                let (was, now) = (
                    source_at(&before, 120.0, Ticks(t)),
                    source_at(&after, 120.0, Ticks(t)),
                );
                assert!((was - now).abs() < 1e-9, "tick {t} with a marker at {at:?}");
            }
        }
        let on = marker_on(&before, 120.0, 1.5);
        assert_eq!(on, Marker::new(1.5, q(3)));
        assert!(with_marker(&before, marker_at(&before, 120.0, q(2))).is_none());
    }

    #[test]
    fn the_only_marker_stays_and_another_goes() {
        assert!(without_marker(&map()[..1], 0).is_none());
        assert!(without_marker(&map(), 2).is_none());
        assert_eq!(without_marker(&map(), 0), Some(vec![map()[1]]));
    }

    #[test]
    fn a_marker_moves_between_a_quarter_and_four_times_the_speed() {
        let three = [
            Marker::new(0.0, q(0)),
            Marker::new(0.5, q(1)),
            Marker::new(1.0, q(2)),
        ];
        assert_eq!(ratios_beside(&three, 120.0, 1), (Some(1.0), Some(1.0)));
        let (low, high) = tick_limits(&three, 120.0, 1);
        assert_eq!(low, Ticks(PPQ / 4));
        assert_eq!(high, Ticks(2 * PPQ - PPQ / 4));
        for at in [low, high] {
            let mut moved = three;
            moved[1].at = at;
            assert!(is_sound(&moved));
            let (before, after) = ratios_beside(&moved, 120.0, 1);
            for r in [before, after].into_iter().flatten() {
                assert!((MIN_RATIO - 1e-9..=MAX_RATIO + 1e-9).contains(&r), "{r}");
            }
        }
        let (low, high) = tick_limits(&three, 120.0, 2);
        assert_eq!(low, Ticks(PPQ + PPQ / 4));
        assert_eq!(high, Ticks(PPQ + 4 * PPQ));
        let (low, high) = tick_limits(&three[..1], 120.0, 0);
        assert!(low < q(-1_000) && high > q(1_000));
    }

    #[test]
    fn audio_slides_under_a_marker_within_the_same_speeds() {
        let three = [
            Marker::new(0.0, q(0)),
            Marker::new(0.5, q(1)),
            Marker::new(1.0, q(2)),
        ];
        let (low, high) = source_limits(&three, 120.0, 1);
        assert!((low - 0.125).abs() < 1e-12, "{low}");
        assert!((high - 0.875).abs() < 1e-12, "{high}");
        let fast = [Marker::new(0.0, q(0)), Marker::new(5.0, q(1))];
        let (low, high) = source_limits(&fast, 120.0, 1);
        assert!(low <= 5.0 && high >= 5.0);
        let (low, high) = tick_limits(&fast, 120.0, 1);
        assert!(low <= q(1) && high >= q(1));
    }

    #[test]
    fn a_mirrored_map_plays_the_stretch_the_other_way_round() {
        let before = map();
        let mut after = before.clone();
        mirror(&mut after, q(8), 10.0);
        for t in [0, PPQ, 3 * PPQ + 5, q(8).0] {
            let was = source_at(&before, 120.0, Ticks(q(8).0 - t));
            let now = source_at(&after, 120.0, Ticks(t));
            assert!((now - (10.0 - was)).abs() < 1e-9, "tick {t}");
        }
    }
}
