use lumen_core::meter::{Meter, MeterMap, MeterPoint, tidy};
use lumen_core::time::{MAX_BPM, MIN_BPM, TempoPoint, Ticks, bpm_at};

use crate::validate::LONGEST;

pub const MOST_CHANGES: usize = 4_096;

fn segment<T>(points: &[T], at: Ticks, place: impl Fn(&T) -> Ticks) -> usize {
    points.partition_point(|p| place(p) <= at).saturating_sub(1)
}

pub fn tempo_in(points: &[TempoPoint], at: Ticks) -> f64 {
    bpm_at(points, at)
}

fn settle(points: &mut [TempoPoint]) {
    if let Some(last) = points.last_mut() {
        last.ramp = false;
    }
}

fn clamp_bpm(bpm: f64) -> f64 {
    if bpm.is_nan() {
        120.0
    } else {
        bpm.clamp(MIN_BPM, MAX_BPM)
    }
}

pub fn set_tempo_in(points: &mut [TempoPoint], at: Ticks, bpm: f64) {
    let index = segment(points, at, |p| p.at);
    if let Some(point) = points.get_mut(index) {
        point.bpm = clamp_bpm(bpm);
    }
}

pub fn put_tempo(points: &mut Vec<TempoPoint>, at: Ticks, bpm: f64) -> bool {
    let (at, bpm) = (at.clamp(Ticks::ZERO, LONGEST), clamp_bpm(bpm));
    match points.binary_search_by_key(&at, |p| p.at) {
        Ok(index) => points[index].bpm = bpm,
        Err(index) => {
            if points.len() >= MOST_CHANGES {
                return false;
            }
            let ramp = index > 0 && points[index - 1].ramp;
            points.insert(index, TempoPoint { at, bpm, ramp });
        }
    }
    settle(points);
    true
}

pub fn set_ramp(points: &mut [TempoPoint], index: usize, ramp: bool) -> bool {
    if index + 1 >= points.len() {
        return false;
    }
    points[index].ramp = ramp;
    true
}

pub fn move_tempo(points: &mut [TempoPoint], index: usize, at: Ticks, bpm: f64) -> bool {
    if index >= points.len() {
        return false;
    }
    if index > 0 {
        let earliest = points[index - 1].at + Ticks(1);
        let latest = points
            .get(index + 1)
            .map_or(LONGEST, |next| next.at - Ticks(1));
        points[index].at = at.clamp(earliest, latest);
    }
    points[index].bpm = clamp_bpm(bpm);
    true
}

pub fn remove_tempo(points: &mut Vec<TempoPoint>, index: usize) -> bool {
    if index == 0 || index >= points.len() {
        return false;
    }
    points.remove(index);
    settle(points);
    true
}

fn map_of(points: &[MeterPoint]) -> MeterMap {
    MeterMap::new(tidy(points)).expect("a tidy list is a valid map")
}

pub fn set_meter_in(points: &mut Vec<MeterPoint>, at: Ticks, meter: Meter) -> bool {
    if !meter.is_valid() {
        return false;
    }
    let index = segment(points, at, |p| p.at);
    set_meter_of(points, index, meter)
}

pub fn set_meter_of(points: &mut Vec<MeterPoint>, index: usize, meter: Meter) -> bool {
    if !meter.is_valid() || index >= points.len() {
        return false;
    }
    points[index].meter = meter;
    *points = tidy(points);
    true
}

pub fn put_meter(points: &mut Vec<MeterPoint>, at: Ticks, meter: Meter) -> bool {
    if !meter.is_valid() {
        return false;
    }
    let at = map_of(points).nearest_bar_start(at.clamp(Ticks::ZERO, LONGEST));
    match points.binary_search_by_key(&at, |p| p.at) {
        Ok(index) => points[index].meter = meter,
        Err(index) => {
            if points.len() >= MOST_CHANGES {
                return false;
            }
            points.insert(index, MeterPoint { at, meter });
        }
    }
    *points = tidy(points);
    true
}

pub fn move_meter(points: &mut Vec<MeterPoint>, index: usize, at: Ticks) -> bool {
    if index == 0 || index >= points.len() {
        return false;
    }
    let previous = points[index - 1];
    let bar = previous.meter.bar().0;
    let mut bars = ((at.0 - previous.at.0) + bar / 2).div_euclid(bar).max(1);
    if let Some(next) = points.get(index + 1) {
        let room = (next.at.0 - previous.at.0 - 1).div_euclid(bar);
        if room < 1 {
            return false;
        }
        bars = bars.min(room);
    }
    let at = Ticks((previous.at.0 + bars * bar).min(LONGEST.0));
    points[index].at = at;
    *points = tidy(points);
    true
}

pub fn remove_meter(points: &mut Vec<MeterPoint>, index: usize) -> bool {
    if index == 0 || index >= points.len() {
        return false;
    }
    points.remove(index);
    *points = tidy(points);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::{PPQ, TempoMap};

    fn tempo(at: i64, bpm: f64) -> TempoPoint {
        TempoPoint {
            at: Ticks::quarters(at),
            bpm,
            ramp: false,
        }
    }

    fn meter(at: i64, numerator: u32, denominator: u32) -> MeterPoint {
        MeterPoint {
            at: Ticks::quarters(at),
            meter: Meter {
                numerator,
                denominator,
            },
        }
    }

    fn m(numerator: u32, denominator: u32) -> Meter {
        Meter {
            numerator,
            denominator,
        }
    }

    #[test]
    fn a_tempo_changes_the_segment_it_is_in() {
        let mut p = vec![tempo(0, 120.0), tempo(8, 90.0)];
        set_tempo_in(&mut p, Ticks::quarters(3), 100.0);
        set_tempo_in(&mut p, Ticks::quarters(8), 2000.0);
        assert_eq!((p[0].bpm, p[1].bpm), (100.0, 999.0), "999 at most");
        set_tempo_in(&mut p, Ticks::quarters(-4), 1.0);
        assert_eq!(p[0].bpm, 20.0, "before the start is the first segment");
    }

    #[test]
    fn a_tempo_point_is_added_in_order_or_changes_the_one_there() {
        let mut p = vec![tempo(0, 120.0), tempo(8, 90.0)];
        assert!(put_tempo(&mut p, Ticks::quarters(4), 100.0));
        assert_eq!(
            p.iter().map(|x| x.bpm).collect::<Vec<_>>(),
            [120.0, 100.0, 90.0]
        );
        assert!(put_tempo(&mut p, Ticks::quarters(4), 111.0));
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].bpm, 111.0);
        assert!(put_tempo(&mut p, Ticks::quarters(-2), 130.0));
        assert_eq!((p.len(), p[0].bpm), (3, 130.0), "before zero is point 0");
        TempoMap::new(p).unwrap();
    }

    #[test]
    fn a_tempo_point_stays_between_its_neighbours_and_the_first_at_zero() {
        let mut p = vec![tempo(0, 120.0), tempo(4, 100.0), tempo(8, 90.0)];
        assert!(move_tempo(&mut p, 1, Ticks::quarters(20), 105.0));
        assert_eq!(p[1].at, Ticks::quarters(8) - Ticks(1));
        assert!(move_tempo(&mut p, 1, Ticks::quarters(-5), 105.0));
        assert_eq!(p[1].at, Ticks(1));
        assert!(move_tempo(&mut p, 0, Ticks::quarters(5), 77.0));
        assert_eq!((p[0].at, p[0].bpm), (Ticks::ZERO, 77.0));
        assert!(!move_tempo(&mut p, 3, Ticks::ZERO, 100.0));
        TempoMap::new(p).unwrap();
    }

    #[test]
    fn the_first_tempo_point_cannot_be_removed() {
        let mut p = vec![tempo(0, 120.0), tempo(4, 100.0)];
        assert!(!remove_tempo(&mut p, 0));
        assert!(remove_tempo(&mut p, 1));
        assert_eq!(p.len(), 1);
        assert!(!remove_tempo(&mut p, 1));
    }

    #[test]
    fn a_tempo_ramps_to_the_next_point_and_never_the_last() {
        let mut p = vec![tempo(0, 120.0), tempo(8, 60.0)];
        assert!(set_ramp(&mut p, 0, true));
        assert!(p[0].ramp);
        assert!(!set_ramp(&mut p, 1, true), "nothing to ramp to");
        assert!(!set_ramp(&mut p, 5, true), "no such point");
        assert_eq!(tempo_in(&p, Ticks::quarters(4)), 90.0, "half way down");
        assert!(set_ramp(&mut p, 0, false));
        assert_eq!(tempo_in(&p, Ticks::quarters(4)), 120.0);
    }

    #[test]
    fn a_point_added_inside_a_ramp_splits_it_and_the_rest_still_ramps() {
        let mut p = vec![tempo(0, 120.0), tempo(8, 60.0)];
        set_ramp(&mut p, 0, true);
        assert!(put_tempo(&mut p, Ticks::quarters(4), 100.0));
        assert_eq!(p.len(), 3);
        assert!(p[0].ramp && p[1].ramp, "both halves ramp");
        assert!(!p[2].ramp);
        assert!(put_tempo(&mut p, Ticks::quarters(12), 80.0));
        assert!(!p[3].ramp && !p[2].ramp);
        TempoMap::new(p).unwrap();
    }

    #[test]
    fn removing_the_last_point_ends_the_ramp_before_it() {
        let mut p = vec![tempo(0, 120.0), tempo(4, 100.0), tempo(8, 60.0)];
        set_ramp(&mut p, 0, true);
        set_ramp(&mut p, 1, true);
        assert!(remove_tempo(&mut p, 2));
        assert!(p[0].ramp && !p[1].ramp);
        let mut p = vec![tempo(0, 120.0), tempo(4, 100.0), tempo(8, 60.0)];
        set_ramp(&mut p, 0, true);
        assert!(remove_tempo(&mut p, 1));
        assert_eq!(tempo_in(&p, Ticks::quarters(4)), 90.0);
    }

    #[test]
    fn a_full_tempo_map_refuses_another_point() {
        let mut p: Vec<TempoPoint> = (0..MOST_CHANGES as i64).map(|i| tempo(i, 120.0)).collect();
        assert!(!put_tempo(&mut p, Ticks(1), 100.0));
        assert!(
            put_tempo(&mut p, Ticks::quarters(5), 100.0),
            "a change in place is fine"
        );
    }

    #[test]
    fn a_meter_is_put_on_the_nearest_bar_line() {
        let mut p = vec![meter(0, 4, 4)];
        assert!(put_meter(
            &mut p,
            Ticks::quarters(5) + Ticks(PPQ / 2),
            m(7, 8)
        ));
        assert_eq!(p[1].at, Ticks::quarters(4));
        assert!(put_meter(&mut p, Ticks::quarters(4), m(3, 4)));
        assert_eq!((p.len(), p[1].meter), (2, m(3, 4)));
        MeterMap::new(p).unwrap();
    }

    #[test]
    fn a_meter_put_at_the_start_replaces_the_first() {
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4)];
        assert!(put_meter(&mut p, Ticks::ZERO, m(5, 4)));
        assert_eq!(p[0].meter, m(5, 4));
        assert_eq!(p[1].at, Ticks::quarters(10));
        MeterMap::new(p).unwrap();
    }

    #[test]
    fn changing_a_meter_moves_later_changes_to_bar_lines_and_drops_collisions() {
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4), meter(11, 7, 8)];
        assert!(set_meter_in(&mut p, Ticks::quarters(1), m(2, 4)));
        assert_eq!(p.len(), 3);
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4), meter(11, 7, 8)];
        assert!(set_meter_of(&mut p, 0, m(5, 4)));
        assert_eq!(p[1].at, Ticks::quarters(10));
        MeterMap::new(p).unwrap();
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4), meter(11, 7, 8)];
        assert!(set_meter_of(&mut p, 0, m(16, 4)));
        assert_eq!(p.len(), 2);
        assert_eq!((p[1].at, p[1].meter), (Ticks::quarters(16), m(3, 4)));
        MeterMap::new(p).unwrap();
    }

    #[test]
    fn a_meter_change_moves_between_its_neighbours_on_bar_lines() {
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4), meter(20, 4, 4)];
        assert!(move_meter(&mut p, 1, Ticks::quarters(100)));
        assert_eq!(p[1].at, Ticks::quarters(16));
        assert!(move_meter(&mut p, 1, Ticks::quarters(-9)));
        assert_eq!(p[1].at, Ticks::quarters(4));
        assert!(
            !move_meter(&mut p, 0, Ticks::quarters(4)),
            "the first stays at zero"
        );
        MeterMap::new(p).unwrap();
    }

    #[test]
    fn a_meter_change_with_no_bar_line_to_move_to_stays_put() {
        let mut p = vec![meter(0, 4, 4), meter(4, 3, 4), meter(7, 4, 4)];
        assert!(move_meter(&mut p, 1, Ticks::quarters(6)));
        assert_eq!(p[1].at, Ticks::quarters(4));
    }

    #[test]
    fn removing_a_meter_change_keeps_the_first_and_leaves_a_valid_map() {
        let mut p = vec![meter(0, 4, 4), meter(8, 3, 4), meter(17, 7, 8)];
        assert!(!remove_meter(&mut p, 0));
        assert!(remove_meter(&mut p, 1));
        assert_eq!(p[1].at, Ticks::quarters(16));
        MeterMap::new(p).unwrap();
    }

    #[test]
    fn every_edit_of_a_random_walk_leaves_a_valid_map() {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut meters = vec![meter(0, 4, 4)];
        let mut tempos = vec![tempo(0, 120.0)];
        for _ in 0..2_000 {
            let at = Ticks((next() % (PPQ as u64 * 64)) as i64);
            let (n, d) = (1 + (next() % 12) as u32, 1_u32 << (next() % 7));
            match next() % 9 {
                0 => {
                    put_meter(&mut meters, at, m(n, d));
                }
                1 => {
                    set_meter_in(&mut meters, at, m(n, d));
                }
                2 => {
                    move_meter(&mut meters, (next() % 6) as usize, at);
                }
                3 => {
                    remove_meter(&mut meters, (next() % 6) as usize);
                }
                4 => {
                    put_tempo(&mut tempos, at, 20.0 + (next() % 980) as f64);
                }
                5 => {
                    move_tempo(&mut tempos, (next() % 6) as usize, at, 50.0);
                }
                6 => {
                    remove_tempo(&mut tempos, (next() % 6) as usize);
                }
                7 => {
                    set_ramp(&mut tempos, (next() % 6) as usize, next() % 2 == 0);
                }
                _ => set_tempo_in(&mut tempos, at, 100.0),
            }
            assert!(
                tempos.last().is_some_and(|p| !p.ramp),
                "the last point never ramps"
            );
            MeterMap::new(meters.clone()).expect("the meter map stays valid");
            TempoMap::new(tempos.clone()).expect("the tempo map stays valid");
        }
    }
}
