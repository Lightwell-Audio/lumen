//! Warp maps (M9.5 S4a, D1): which second of an audio file plays at which
//! tick of a clip. Shared by the model that stores them, the edits that keep
//! them true, and (from S4b) whatever plays them.
//!
//! A map is a list of [`Marker`]s, each pinning a second of the file to a
//! tick measured from the clip's content origin (the origin `loop_phase`
//! counts from). Between two markers the file's time follows the song
//! linearly in ticks, so the audio keeps its place against the bars when the
//! tempo changes. Before the first marker and after the last, the clip's
//! *original tempo* applies: the file advances one beat for every
//! `60 / original_bpm` seconds. An empty list pins second 0 to tick 0.
//!
//! Ticks are positions in music time and seconds are positions in the file,
//! so nothing here knows the project's tempo.

use serde::{Deserialize, Serialize};

use crate::time::{PPQ, Ticks};

/// The most markers a clip holds. A bound for the file format, far above
/// what anyone places by hand.
pub const MAX_MARKERS: usize = 4_096;

/// The slowest a span may play: a quarter of the original speed. The warp
/// editor holds a dragged marker to this (CLAUDE.md §6.2; M9.5 §1).
pub const MIN_RATIO: f64 = 0.25;

/// The fastest a span may play: four times the original speed.
pub const MAX_RATIO: f64 = 4.0;

/// One pin of a warp map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// A position in the file, in seconds from its first frame (from the
    /// first frame of its reversed twin when the clip is reversed).
    pub source: f64,
    /// Where that position plays, in ticks from the clip's content origin.
    /// It can be negative: a clip trimmed later keeps its earlier markers.
    pub at: Ticks,
}

impl Marker {
    pub fn new(source: f64, at: Ticks) -> Self {
        Marker { source, at }
    }
}

/// Seconds of the file that pass in one tick at `bpm`.
pub fn seconds_per_tick(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

/// Whether `markers` form a map: finite, in time order, and moving forward
/// through the file (a span never stands still or plays backwards; reverse
/// is the clip's own flag).
pub fn is_sound(markers: &[Marker]) -> bool {
    markers.len() <= MAX_MARKERS
        && markers.iter().all(|m| m.source.is_finite())
        && markers
            .windows(2)
            .all(|w| w[0].at < w[1].at && w[0].source < w[1].source)
}

/// How fast the file runs between `a` and `b` against its original tempo, in
/// music time: 1 plays it as it was made, 2 twice as fast (it takes half the
/// beats), 0.5 twice as slow. The editor shows it as a percentage and holds
/// it between [`MIN_RATIO`] and [`MAX_RATIO`].
pub fn ratio(a: Marker, b: Marker, original_bpm: f64) -> f64 {
    (b.source - a.source) / ((b.at.0 - a.at.0) as f64 * seconds_per_tick(original_bpm))
}

/// The second of the file that plays at `at`.
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
    // The span `at` falls in: the last marker at or before it.
    let after = markers.partition_point(|m| m.at <= at);
    let (a, b) = (markers[after - 1], markers[after]);
    let along = (at.0 - a.at.0) as f64 / (b.at.0 - a.at.0) as f64;
    a.source + (b.source - a.source) * along
}

/// [`source_at`] for a position between ticks (`at` in ticks, fractional):
/// what the renderer reads at every frame, which a whole tick would move by up
/// to half a tick (a fraction of a sample, but enough to hear as roughness on
/// a high note). Equal to [`source_at`] on a whole tick.
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
    // The span `at` falls in: the last marker at or before it.
    let after = markers.partition_point(|m| m.at.0 as f64 <= at);
    let (a, b) = (markers[after - 1], markers[after]);
    let along = (at - a.at.0 as f64) / (b.at.0 - a.at.0) as f64;
    a.source + (b.source - a.source) * along
}

/// The tick at which second `source` of the file plays, to the nearest tick.
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

/// Moves every marker `by` ticks later (earlier when negative): the audio
/// under them stays the same, only where it plays changes.
pub fn shift(markers: &mut [Marker], by: Ticks) {
    for m in markers {
        m.at = m.at + by;
    }
}

/// Turns the map about the middle of a stretch of the clip so that a reversed
/// clip stays in time (D6). The stretch `[0, sum]` plays backwards: the tick
/// that played second `s` at `at` now plays second `total − s` at
/// `sum − at`, where `total` is the file's length. The markers come back in
/// time order.
pub fn mirror(markers: &mut [Marker], sum: Ticks, total: f64) {
    for m in markers.iter_mut() {
        *m = Marker::new(total - m.source, Ticks(sum.0 - m.at.0));
    }
    markers.reverse();
}

/// The marker that pins tick `at` to the audio that plays there now: adding
/// it changes nothing that is heard.
pub fn marker_at(markers: &[Marker], original_bpm: f64, at: Ticks) -> Marker {
    Marker::new(source_at(markers, original_bpm, at), at)
}

/// The marker that pins second `source` of the file (a transient) to the tick
/// where it plays now, to the nearest tick.
pub fn marker_on(markers: &[Marker], original_bpm: f64, source: f64) -> Marker {
    Marker::new(source, tick_at_source(markers, original_bpm, source))
}

/// `markers` with `marker` put in its place in time, and its index. `None`
/// when the list is full or the result is not a map (a marker is at that tick
/// or on that second already).
pub fn with_marker(markers: &[Marker], marker: Marker) -> Option<(Vec<Marker>, usize)> {
    if markers.len() >= MAX_MARKERS {
        return None;
    }
    let index = markers.partition_point(|m| m.at < marker.at);
    let mut out = markers.to_vec();
    out.insert(index, marker);
    is_sound(&out).then_some((out, index))
}

/// `markers` without the one at `index`. `None` for the only marker (a map
/// keeps one) or an index that is not there.
pub fn without_marker(markers: &[Marker], index: usize) -> Option<Vec<Marker>> {
    if markers.len() < 2 || index >= markers.len() {
        return None;
    }
    let mut out = markers.to_vec();
    out.remove(index);
    Some(out)
}

/// How fast the spans before and after marker `index` play against the
/// original tempo ([`ratio`]); `None` on a side with no marker.
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

/// As far as a marker is taken from where it stands on a side with no
/// neighbour: far past any song, and small enough to add to a tick.
const FREE: i64 = 1 << 50;

/// A span this much of a tick past a limit is on it: the arithmetic of a
/// span that sits exactly on a limit may land a hair either side.
const SLACK: f64 = 1e-6;

/// The ticks marker `index` may be moved between, so that the spans beside it
/// stay between [`MIN_RATIO`] and [`MAX_RATIO`] and it stays between its
/// neighbours. Where it stands is always allowed.
pub fn tick_limits(markers: &[Marker], original_bpm: f64, index: usize) -> (Ticks, Ticks) {
    let Some(&m) = markers.get(index) else {
        return (Ticks::ZERO, Ticks::ZERO);
    };
    let per_tick = seconds_per_tick(original_bpm);
    let (mut low, mut high) = (m.at.0 - FREE, m.at.0 + FREE);
    if let Some(a) = index.checked_sub(1).and_then(|i| markers.get(i)) {
        // The span before holds `m.source − a.source` seconds: it is
        // shortest at the fastest ratio and longest at the slowest.
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

/// The seconds of the file marker `index` may be slid between (the audio
/// moved under it), by the same rule as [`tick_limits`].
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

    /// Two markers: second 1 plays at beat 2 and second 2 at beat 4, so the
    /// file plays at 120 BPM between them.
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
        // 120 BPM: half a second a beat.
        assert!((source_at(&map(), 120.0, q(5)) - 2.5).abs() < 1e-12);
        assert!((source_at(&map(), 120.0, q(0)) - 0.0).abs() < 1e-12);
        // 60 BPM: a second a beat.
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
        // Half a tick along a span is half a tick's seconds along.
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
        // Two seconds over four beats is 120 BPM: as made at 120, it plays as
        // made; a file made at 240 BPM runs twice as fast.
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
        // On a second of the file: at the tick it plays.
        let on = marker_on(&before, 120.0, 1.5);
        assert_eq!(on, Marker::new(1.5, q(3)));
        // A tick that has a marker takes no other.
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
        // Three markers a beat apart, each pinning half a second on: the file
        // plays as made at 120 BPM.
        let three = [
            Marker::new(0.0, q(0)),
            Marker::new(0.5, q(1)),
            Marker::new(1.0, q(2)),
        ];
        assert_eq!(ratios_beside(&three, 120.0, 1), (Some(1.0), Some(1.0)));
        let (low, high) = tick_limits(&three, 120.0, 1);
        // Before it, half a second fits a quarter of a beat at 400%; after
        // it, the same half second must not be squeezed under a quarter beat.
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
        // The last marker has no span after it: it is held on one side only.
        let (low, high) = tick_limits(&three, 120.0, 2);
        assert_eq!(low, Ticks(PPQ + PPQ / 4));
        assert_eq!(high, Ticks(PPQ + 4 * PPQ));
        // A lone marker goes anywhere.
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
        // A beat is half a second: before it at least an eighth of a second
        // (25%), after it at least an eighth too.
        assert!((low - 0.125).abs() < 1e-12, "{low}");
        assert!((high - 0.875).abs() < 1e-12, "{high}");
        // A span already past the limit still allows where the marker is.
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
