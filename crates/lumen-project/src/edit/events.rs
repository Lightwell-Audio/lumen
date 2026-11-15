use lumen_core::curve::Curve;
use lumen_core::time::{PPQ, Ticks};

use super::notes::picked;
use crate::model::{ClipEvent, EventKind};

pub const FINE: Ticks = Ticks(PPQ / 16);

pub const GRID: Ticks = Ticks(PPQ / 4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Controller(u8),
    PitchBend,
    Pressure,
}

impl Lane {
    pub fn of(kind: EventKind) -> Option<Lane> {
        match kind {
            EventKind::Controller { number, .. } => Some(Lane::Controller(number)),
            EventKind::PitchBend { .. } => Some(Lane::PitchBend),
            EventKind::Pressure { .. } => Some(Lane::Pressure),
            EventKind::Program { .. } => None,
        }
    }

    pub fn range(self) -> (i32, i32) {
        match self {
            Lane::PitchBend => (-8192, 8191),
            Lane::Controller(_) | Lane::Pressure => (0, 127),
        }
    }

    pub fn rest(self) -> i32 {
        0
    }

    pub fn event(self, value: i32) -> EventKind {
        let (low, high) = self.range();
        let value = value.clamp(low, high);
        match self {
            Lane::Controller(number) => EventKind::Controller {
                number: number.min(127),
                value: value as u8,
            },
            Lane::PitchBend => EventKind::PitchBend {
                value: value as i16,
            },
            Lane::Pressure => EventKind::Pressure { value: value as u8 },
        }
    }

    pub fn value_of(self, kind: EventKind) -> Option<i32> {
        match (self, kind) {
            (Lane::Controller(lane), EventKind::Controller { number, value }) if lane == number => {
                Some(i32::from(value))
            }
            (Lane::PitchBend, EventKind::PitchBend { value }) => Some(i32::from(value)),
            (Lane::Pressure, EventKind::Pressure { value }) => Some(i32::from(value)),
            _ => None,
        }
    }
}

fn on(lane: Lane, event: &ClipEvent) -> bool {
    lane.value_of(event.kind).is_some()
}

pub fn in_lane(events: &[ClipEvent], lane: Lane) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter(|(_, e)| on(lane, e))
        .map(|(i, _)| i)
        .collect()
}

pub fn set_point(
    events: &mut Vec<ClipEvent>,
    lane: Lane,
    at: Ticks,
    value: i32,
    length: Ticks,
) -> Vec<usize> {
    if length <= Ticks::ZERO {
        return Vec::new();
    }
    let at = at.max(Ticks::ZERO).min(length - Ticks(1));
    let lo = events.partition_point(|e| e.at < at);
    let hi = events.partition_point(|e| e.at <= at);
    let same: Vec<usize> = (lo..hi).filter(|&i| on(lane, &events[i])).collect();
    let event = ClipEvent {
        at,
        kind: lane.event(value),
    };
    match same.split_first() {
        Some((&first, duplicates)) => {
            events[first] = event;
            for &i in duplicates.iter().rev() {
                events.remove(i);
            }
            vec![first]
        }
        None => {
            events.insert(hi, event);
            vec![hi]
        }
    }
}

pub fn delete_points(events: &mut Vec<ClipEvent>, selection: &[usize]) -> Vec<usize> {
    let chosen = picked(selection, events.len());
    let mut at = 0;
    events.retain(|_| {
        let keep = chosen.binary_search(&at).is_err();
        at += 1;
        keep
    });
    Vec::new()
}

pub fn move_points(
    events: &mut Vec<ClipEvent>,
    lane: Lane,
    selection: &[usize],
    time: Ticks,
    value: i32,
    length: Ticks,
) -> Vec<usize> {
    if length <= Ticks::ZERO {
        return Vec::new();
    }
    let chosen: Vec<usize> = picked(selection, events.len())
        .into_iter()
        .filter(|&i| on(lane, &events[i]))
        .collect();
    let Some((time, value)) = clamp_move(events, lane, &chosen, time, value, length) else {
        return Vec::new();
    };
    let moved: Vec<ClipEvent> = chosen
        .iter()
        .map(|&i| shifted(events[i], lane, time, value))
        .collect();
    let mut landing: Vec<Ticks> = moved.iter().map(|e| e.at).collect();
    landing.sort_unstable();
    let mut tagged: Vec<(ClipEvent, bool)> = events
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            chosen.binary_search(i).is_err()
                && !(on(lane, e) && landing.binary_search(&e.at).is_ok())
        })
        .map(|(_, &e)| (e, false))
        .collect();
    tagged.extend(moved.into_iter().map(|e| (e, true)));
    tagged.sort_by_key(|(e, _)| e.at);
    write_tagged(events, tagged)
}

fn clamp_move(
    events: &[ClipEvent],
    lane: Lane,
    chosen: &[usize],
    time: Ticks,
    value: i32,
    length: Ticks,
) -> Option<(Ticks, i32)> {
    let at: Vec<Ticks> = chosen.iter().map(|&i| events[i].at).collect();
    let values: Vec<i32> = chosen
        .iter()
        .filter_map(|&i| lane.value_of(events[i].kind))
        .collect();
    let earliest = Ticks::ZERO - *at.iter().min()?;
    let latest = (length - Ticks(1) - *at.iter().max()?).max(earliest);
    let (lowest, highest) = lane.range();
    let lowest_offset = lowest - *values.iter().min()?;
    let highest_offset = (highest - *values.iter().max()?).max(lowest_offset);
    Some((
        time.min(latest).max(earliest),
        value.min(highest_offset).max(lowest_offset),
    ))
}

fn shifted(event: ClipEvent, lane: Lane, time: Ticks, value: i32) -> ClipEvent {
    let now = lane.value_of(event.kind).unwrap_or(lane.rest());
    ClipEvent {
        at: event.at + time,
        kind: lane.event(now + value),
    }
}

fn write_tagged(events: &mut Vec<ClipEvent>, tagged: Vec<(ClipEvent, bool)>) -> Vec<usize> {
    events.clear();
    let mut chosen = Vec::new();
    for (i, (event, mark)) in tagged.into_iter().enumerate() {
        events.push(event);
        if mark {
            chosen.push(i);
        }
    }
    chosen
}

pub fn in_box(
    events: &[ClipEvent],
    lane: Lane,
    from: Ticks,
    to: Ticks,
    low: i32,
    high: i32,
) -> Vec<usize> {
    let (from, to) = (from.min(to), from.max(to));
    let (low, high) = (low.min(high), low.max(high));
    events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            (from..=to).contains(&e.at)
                && lane
                    .value_of(e.kind)
                    .is_some_and(|v| (low..=high).contains(&v))
        })
        .map(|(i, _)| i)
        .collect()
}

pub fn ramp(
    lane: Lane,
    from: (Ticks, i32),
    to: (Ticks, i32),
    curve: &Curve,
    step: Ticks,
) -> Vec<ClipEvent> {
    let (start, end) = if to.0 < from.0 {
        (to, from)
    } else {
        (from, to)
    };
    let span = end.0 - start.0;
    if span <= Ticks::ZERO {
        return vec![ClipEvent {
            at: start.0,
            kind: lane.event(start.1),
        }];
    }
    let step = if step <= Ticks::ZERO { Ticks(1) } else { step };
    let (lowest, highest) = lane.range();
    let (lowest, highest) = (f64::from(lowest), f64::from(highest));
    let (from_value, to_value) = (f64::from(start.1), f64::from(end.1));
    let mut out = Vec::new();
    let mut previous = None;
    let mut at = start.0;
    loop {
        let here = at.min(end.0);
        let x = ((here - start.0).0 as f64 / span.0 as f64) as f32;
        let level = f64::from(curve.at(x));
        let value = (from_value + (to_value - from_value) * level)
            .round()
            .clamp(lowest, highest) as i32;
        let last = here == end.0;
        if last || previous != Some(value) {
            out.push(ClipEvent {
                at: here,
                kind: lane.event(value),
            });
        }
        previous = Some(value);
        if last {
            break;
        }
        at = Ticks(here.0.saturating_add(step.0));
    }
    out
}

pub fn replace_span(
    events: &mut Vec<ClipEvent>,
    lane: Lane,
    from: Ticks,
    to: Ticks,
    with: Vec<ClipEvent>,
    length: Ticks,
) -> Vec<usize> {
    let (from, to) = (from.min(to), from.max(to));
    events.retain(|e| !(on(lane, e) && (from..=to).contains(&e.at)));
    let mut incoming: Vec<ClipEvent> = with
        .into_iter()
        .filter(|e| (Ticks::ZERO..length).contains(&e.at))
        .filter_map(|e| {
            lane.value_of(e.kind).map(|v| ClipEvent {
                at: e.at,
                kind: lane.event(v),
            })
        })
        .collect();
    incoming.sort_by_key(|e| e.at);
    let mut tagged: Vec<(ClipEvent, bool)> = events.iter().map(|&e| (e, false)).collect();
    tagged.extend(incoming.into_iter().map(|e| (e, true)));
    tagged.sort_by_key(|(e, _)| e.at);
    write_tagged(events, tagged)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: i64 = PPQ;
    const LEN: Ticks = Ticks(1000);
    const CC7: Lane = Lane::Controller(7);

    fn t(n: i64) -> Ticks {
        Ticks(n)
    }

    fn cc(at: i64, number: u8, value: u8) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::Controller { number, value },
        }
    }

    fn bend(at: i64, value: i16) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::PitchBend { value },
        }
    }

    fn press(at: i64, value: u8) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::Pressure { value },
        }
    }

    fn program(at: i64) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::Program {
                program: 1,
                bank: None,
            },
        }
    }

    fn assert_valid(events: &[ClipEvent]) {
        assert!(events.is_sorted_by_key(|e| e.at), "unsorted: {events:?}");
        assert!(
            events.iter().all(|e| e.kind.in_range()),
            "out of range: {events:?}"
        );
    }

    fn points(events: &[ClipEvent], lane: Lane) -> Vec<(i64, i32)> {
        events
            .iter()
            .filter_map(|e| lane.value_of(e.kind).map(|v| (e.at.0, v)))
            .collect()
    }

    fn held(events: &[ClipEvent], lane: Lane, at: Ticks) -> Option<i32> {
        events
            .iter()
            .filter(|e| e.at <= at)
            .filter_map(|e| lane.value_of(e.kind))
            .next_back()
    }

    #[test]
    fn lanes_say_what_they_are_and_what_they_hold() {
        assert_eq!(
            Lane::of(EventKind::Controller {
                number: 1,
                value: 0
            }),
            Some(Lane::Controller(1))
        );
        assert_eq!(
            Lane::of(EventKind::PitchBend { value: 0 }),
            Some(Lane::PitchBend)
        );
        assert_eq!(
            Lane::of(EventKind::Pressure { value: 0 }),
            Some(Lane::Pressure)
        );
        assert_eq!(
            Lane::of(EventKind::Program {
                program: 0,
                bank: None
            }),
            None
        );
        assert_eq!(CC7.range(), (0, 127));
        assert_eq!(Lane::PitchBend.range(), (-8192, 8191));
        assert_eq!(Lane::Pressure.range(), (0, 127));
        assert_eq!(
            [CC7.rest(), Lane::PitchBend.rest(), Lane::Pressure.rest()],
            [0, 0, 0]
        );
        assert_eq!(
            CC7.event(200),
            EventKind::Controller {
                number: 7,
                value: 127
            }
        );
        assert_eq!(
            CC7.event(-5),
            EventKind::Controller {
                number: 7,
                value: 0
            }
        );
        assert_eq!(
            Lane::PitchBend.event(-99_999),
            EventKind::PitchBend { value: -8192 }
        );
        assert_eq!(
            Lane::PitchBend.event(99_999),
            EventKind::PitchBend { value: 8191 }
        );
        assert_eq!(
            Lane::Pressure.event(300),
            EventKind::Pressure { value: 127 }
        );
        assert_eq!(
            Lane::Controller(200).event(5),
            EventKind::Controller {
                number: 127,
                value: 5
            },
            "a number past 127 is held to 127"
        );
        for lane in [CC7, Lane::PitchBend, Lane::Pressure] {
            for value in [i32::MIN, -1, 0, 64, 127, 128, i32::MAX] {
                assert!(lane.event(value).in_range(), "{lane:?} {value}");
            }
        }
        let kind = EventKind::Controller {
            number: 7,
            value: 9,
        };
        assert_eq!(CC7.value_of(kind), Some(9));
        assert_eq!(Lane::Controller(1).value_of(kind), None);
        assert_eq!(CC7.value_of(EventKind::PitchBend { value: 9 }), None);
        assert_eq!(
            Lane::PitchBend.value_of(EventKind::PitchBend { value: -3 }),
            Some(-3)
        );
        assert_eq!(
            Lane::Pressure.value_of(EventKind::Pressure { value: 4 }),
            Some(4)
        );
    }

    #[test]
    fn in_lane_finds_only_that_lane_in_time_order() {
        let events = vec![
            cc(0, 1, 10),
            cc(0, 7, 20),
            bend(5, 100),
            press(6, 3),
            program(7),
            cc(9, 7, 30),
        ];
        assert_eq!(in_lane(&events, CC7), [1, 5]);
        assert_eq!(in_lane(&events, Lane::Controller(1)), [0]);
        assert_eq!(in_lane(&events, Lane::PitchBend), [2]);
        assert_eq!(in_lane(&events, Lane::Pressure), [3]);
        assert!(in_lane(&[], CC7).is_empty());
    }

    #[test]
    fn a_point_is_inserted_in_time_order() {
        let mut events = vec![cc(100, 7, 10), cc(300, 7, 30)];
        assert_eq!(set_point(&mut events, CC7, t(200), 20, LEN), [1]);
        assert_eq!(points(&events, CC7), [(100, 10), (200, 20), (300, 30)]);
        assert_valid(&events);
    }

    #[test]
    fn a_point_on_an_existing_tick_replaces_it() {
        let mut events = vec![cc(100, 7, 10), cc(300, 7, 30)];
        assert_eq!(set_point(&mut events, CC7, t(300), 90, LEN), [1]);
        assert_eq!(events, [cc(100, 7, 10), cc(300, 7, 90)]);
    }

    #[test]
    fn a_point_replaces_duplicates_on_its_tick() {
        let mut events = vec![cc(100, 7, 1), cc(100, 7, 2)];
        assert_eq!(set_point(&mut events, CC7, t(100), 5, LEN), [0]);
        assert_eq!(events, [cc(100, 7, 5)]);
    }

    #[test]
    fn a_point_is_clamped_into_the_clip_and_the_lane() {
        let mut events = Vec::new();
        assert_eq!(set_point(&mut events, CC7, t(-5), 300, LEN), [0]);
        assert_eq!(events, [cc(0, 7, 127)]);
        assert_eq!(set_point(&mut events, CC7, LEN, 0, LEN), [1]);
        assert_eq!(events[1].at, t(999), "the last tick inside the clip");
        assert_eq!(set_point(&mut events, CC7, t(9_000), 1, LEN), [1]);
        assert_eq!(events, [cc(0, 7, 127), cc(999, 7, 1)]);
        assert_eq!(
            set_point(&mut events, Lane::PitchBend, t(5), -20_000, LEN),
            [1]
        );
        assert_eq!(events[1], bend(5, -8192));
        assert_valid(&events);
    }

    #[test]
    fn a_clip_with_no_length_takes_no_point() {
        let mut events = vec![cc(0, 7, 1)];
        assert!(set_point(&mut events, CC7, t(0), 5, Ticks::ZERO).is_empty());
        assert_eq!(events, [cc(0, 7, 1)]);
    }

    #[test]
    fn other_lanes_on_the_same_tick_are_left_alone() {
        let mut events = vec![cc(100, 1, 40), bend(100, 500), cc(100, 7, 9)];
        assert_eq!(set_point(&mut events, CC7, t(100), 60, LEN), [2]);
        assert_eq!(events, [cc(100, 1, 40), bend(100, 500), cc(100, 7, 60)]);
        assert_eq!(set_point(&mut events, Lane::PitchBend, t(100), 7, LEN), [1]);
        assert_eq!(events, [cc(100, 1, 40), bend(100, 7), cc(100, 7, 60)]);
        let mut events = vec![cc(100, 1, 40), bend(100, 500)];
        assert_eq!(set_point(&mut events, CC7, t(100), 60, LEN), [2]);
        assert_eq!(events[2], cc(100, 7, 60));
    }

    #[test]
    fn deleting_removes_the_picked_points_and_ignores_bad_indices() {
        let mut events = vec![cc(0, 7, 1), cc(10, 1, 2), cc(20, 7, 3), bend(30, 4)];
        assert!(delete_points(&mut events, &[2, 0, 0, 9, usize::MAX]).is_empty());
        assert_eq!(events, [cc(10, 1, 2), bend(30, 4)]);
        assert!(delete_points(&mut events, &[]).is_empty());
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn a_group_moves_in_time_and_value_keeping_its_offsets() {
        let mut events = vec![cc(100, 7, 10), cc(150, 1, 5), cc(200, 7, 20)];
        let sel = move_points(&mut events, CC7, &[0, 2], t(50), 5, LEN);
        assert_eq!(sel, [1, 2]);
        assert_eq!(events, [cc(150, 1, 5), cc(150, 7, 15), cc(250, 7, 25)]);
        assert_valid(&events);
    }

    #[test]
    fn a_group_clamps_at_the_start_as_a_whole() {
        let mut events = vec![cc(50, 7, 10), cc(200, 7, 20)];
        let sel = move_points(&mut events, CC7, &[0, 1], t(-500), 0, LEN);
        assert_eq!(sel, [0, 1]);
        assert_eq!(events, [cc(0, 7, 10), cc(150, 7, 20)]);
    }

    #[test]
    fn a_group_clamps_at_the_end_as_a_whole() {
        let mut events = vec![cc(800, 7, 10), cc(900, 7, 20)];
        let sel = move_points(&mut events, CC7, &[0, 1], t(500), 0, LEN);
        assert_eq!(sel, [0, 1]);
        assert_eq!(events, [cc(899, 7, 10), cc(999, 7, 20)]);
    }

    #[test]
    fn a_group_clamps_in_value_as_a_whole() {
        let mut events = vec![cc(10, 7, 10), cc(20, 7, 120)];
        move_points(&mut events, CC7, &[0, 1], Ticks::ZERO, 50, LEN);
        assert_eq!(events, [cc(10, 7, 17), cc(20, 7, 127)]);
        move_points(&mut events, CC7, &[0, 1], Ticks::ZERO, -50, LEN);
        assert_eq!(events, [cc(10, 7, 0), cc(20, 7, 110)]);
        let mut bends = vec![bend(10, -8000), bend(20, 8000)];
        move_points(&mut bends, Lane::PitchBend, &[0, 1], Ticks::ZERO, 1000, LEN);
        assert_eq!(bends, [bend(10, -7809), bend(20, 8191)]);
    }

    #[test]
    fn a_moved_point_takes_the_place_of_an_unpicked_one_on_its_tick() {
        let mut events = vec![cc(100, 7, 10), cc(300, 7, 5), cc(300, 1, 9)];
        let sel = move_points(&mut events, CC7, &[0], t(200), 0, LEN);
        assert_eq!(sel, [1]);
        assert_eq!(events, [cc(300, 1, 9), cc(300, 7, 10)]);
        let mut bends = vec![bend(100, 1), bend(300, 2), cc(300, 7, 5)];
        move_points(&mut bends, Lane::PitchBend, &[0], t(200), 0, LEN);
        assert_eq!(bends, [cc(300, 7, 5), bend(300, 1)]);
    }

    #[test]
    fn picks_of_other_lanes_and_bad_indices_are_ignored() {
        let mut events = vec![cc(10, 1, 4), cc(20, 7, 8), bend(30, 100)];
        let sel = move_points(&mut events, CC7, &[0, 1, 2, 2, 99], t(100), 10, LEN);
        assert_eq!(sel, [2]);
        assert_eq!(events, [cc(10, 1, 4), bend(30, 100), cc(120, 7, 18)]);
        let before = events.clone();
        assert!(move_points(&mut events, CC7, &[0, 1], t(100), 10, LEN).is_empty());
        assert_eq!(events, before);
        assert!(move_points(&mut events, CC7, &[1], t(1), 1, Ticks::ZERO).is_empty());
        assert_eq!(events, before);
    }

    #[test]
    fn a_box_takes_the_points_of_its_lane_inside_its_edges() {
        let events = vec![
            cc(10, 7, 20),
            cc(20, 7, 60),
            bend(20, 50),
            cc(30, 7, 100),
            cc(30, 1, 50),
            cc(40, 7, 50),
        ];
        assert_eq!(in_box(&events, CC7, t(40), t(10), 60, 20), [0, 1, 5]);
        assert_eq!(in_box(&events, CC7, t(20), t(20), 0, 127), [1]);
        assert_eq!(
            in_box(&events, Lane::PitchBend, t(0), t(100), -100, 100),
            [2]
        );
        assert_eq!(
            in_box(&events, Lane::Controller(1), t(0), t(100), 0, 127),
            [4]
        );
        assert!(in_box(&events, CC7, t(0), t(5), 0, 127).is_empty());
    }

    #[test]
    fn a_ramp_lands_exactly_on_both_ends_and_rises_steadily() {
        let line = ramp(CC7, (t(0), 0), (t(2 * Q), 127), &Curve::LINEAR, GRID);
        assert_valid(&line);
        let found = points(&line, CC7);
        let ticks: Vec<i64> = found.iter().map(|p| p.0).collect();
        assert_eq!(ticks, (0..=8).map(|k| k * GRID.0).collect::<Vec<_>>());
        assert_eq!(found.first(), Some(&(0, 0)));
        assert_eq!(found.last(), Some(&(2 * Q, 127)));
        assert!(found.windows(2).all(|w| w[0].1 <= w[1].1), "{found:?}");
        for (k, &(_, value)) in (0..).zip(&found) {
            let straight = (127.0 * f64::from(k) / 8.0).round() as i32;
            assert!((value - straight).abs() <= 1, "{k}: {value} vs {straight}");
        }
        let back = ramp(CC7, (t(2 * Q), 127), (t(0), 0), &Curve::LINEAR, GRID);
        assert_eq!(back, line, "a ramp to an earlier tick runs the other way");
    }

    #[test]
    fn a_slow_curve_sits_below_the_straight_line_at_the_middle() {
        let line = ramp(CC7, (t(0), 0), (t(2 * Q), 127), &Curve::LINEAR, FINE);
        let slow = ramp(CC7, (t(0), 0), (t(2 * Q), 127), &Curve::SLOW, FINE);
        assert_valid(&slow);
        let mid = t(Q);
        assert!(
            held(&slow, CC7, mid) < held(&line, CC7, mid),
            "slow {:?} line {:?}",
            held(&slow, CC7, mid),
            held(&line, CC7, mid)
        );
        assert_eq!(points(&slow, CC7).last(), Some(&(2 * Q, 127)));
        assert!(points(&slow, CC7).windows(2).all(|w| w[0].1 <= w[1].1));
    }

    #[test]
    fn a_flat_ramp_collapses_to_its_first_and_last_points() {
        let flat = ramp(CC7, (t(0), 5), (t(Q), 5), &Curve::LINEAR, GRID);
        assert_eq!(flat, [cc(0, 7, 5), cc(Q, 7, 5)]);
    }

    #[test]
    fn a_ramp_of_no_span_is_one_point_at_its_start() {
        assert_eq!(
            ramp(CC7, (t(100), 9), (t(100), 40), &Curve::LINEAR, GRID),
            [cc(100, 7, 9)]
        );
    }

    #[test]
    fn a_zero_or_negative_step_is_one_tick() {
        let fine = ramp(CC7, (t(0), 0), (t(3), 3), &Curve::LINEAR, Ticks::ZERO);
        assert_eq!(fine, [cc(0, 7, 0), cc(1, 7, 1), cc(2, 7, 2), cc(3, 7, 3)]);
        assert_eq!(
            ramp(CC7, (t(0), 0), (t(3), 3), &Curve::LINEAR, Ticks(-4)),
            fine
        );
    }

    #[test]
    fn the_end_lands_exactly_even_off_the_step() {
        let odd = ramp(CC7, (t(0), 0), (t(10), 100), &Curve::LINEAR, t(4));
        assert_eq!(
            odd,
            [cc(0, 7, 0), cc(4, 7, 40), cc(8, 7, 80), cc(10, 7, 100)]
        );
    }

    #[test]
    fn repeated_values_are_dropped_but_the_end_is_kept() {
        let r = ramp(CC7, (t(0), 0), (t(90), 1), &Curve::LINEAR, t(10));
        assert_eq!(points(&r, CC7), [(0, 0), (50, 1), (90, 1)]);
    }

    #[test]
    fn ramp_values_are_held_to_the_lane_range() {
        let bend_ramp = ramp(
            Lane::PitchBend,
            (t(0), -10_000),
            (t(Q), 10_000),
            &Curve::LINEAR,
            GRID,
        );
        assert_valid(&bend_ramp);
        assert_eq!(bend_ramp.first(), Some(&bend(0, -8192)));
        assert_eq!(bend_ramp.last(), Some(&bend(Q, 8191)));
    }

    #[test]
    fn a_span_is_replaced_by_the_new_points_and_the_rest_is_kept() {
        let mut events = vec![
            cc(0, 7, 1),
            cc(100, 7, 2),
            cc(150, 1, 9),
            cc(200, 7, 3),
            cc(300, 7, 4),
        ];
        let added = replace_span(
            &mut events,
            CC7,
            t(100),
            t(200),
            vec![cc(120, 7, 50), cc(200, 7, 60)],
            LEN,
        );
        assert_eq!(
            events,
            [
                cc(0, 7, 1),
                cc(120, 7, 50),
                cc(150, 1, 9),
                cc(200, 7, 60),
                cc(300, 7, 4)
            ]
        );
        assert_eq!(added, [1, 3]);
        assert_valid(&events);
    }

    #[test]
    fn a_span_takes_its_edges_in_either_order() {
        let mut events = vec![cc(90, 7, 1), cc(100, 7, 2), cc(200, 7, 3), cc(210, 7, 4)];
        let added = replace_span(&mut events, CC7, t(200), t(100), vec![], LEN);
        assert!(added.is_empty());
        assert_eq!(events, [cc(90, 7, 1), cc(210, 7, 4)]);
    }

    #[test]
    fn an_added_point_goes_after_the_events_on_its_tick() {
        let mut events = vec![cc(120, 1, 9)];
        let added = replace_span(&mut events, CC7, t(0), t(0), vec![cc(120, 7, 5)], LEN);
        assert_eq!(added, [1]);
        assert_eq!(events, [cc(120, 1, 9), cc(120, 7, 5)]);
    }

    #[test]
    fn added_points_are_kept_only_inside_the_clip_and_in_range() {
        let mut events = Vec::new();
        let added = replace_span(
            &mut events,
            CC7,
            t(0),
            t(LEN.0),
            vec![
                cc(10, 1, 3),
                bend(20, 5),
                cc(1000, 7, 5),
                cc(-1, 7, 5),
                cc(40, 7, 200),
                cc(30, 7, 127),
            ],
            LEN,
        );
        assert_eq!(events, [cc(30, 7, 127), cc(40, 7, 127)]);
        assert_eq!(added, [0, 1]);
        assert_valid(&events);
    }

    #[test]
    fn replacing_with_nothing_only_removes() {
        let mut events = vec![press(5, 1), cc(10, 7, 2)];
        assert!(replace_span(&mut events, CC7, t(0), t(50), vec![], LEN).is_empty());
        assert_eq!(events, [press(5, 1)]);
    }

    #[test]
    fn any_sequence_of_edits_keeps_the_list_sorted_and_in_range() {
        let lanes = [CC7, Lane::Controller(1), Lane::PitchBend, Lane::Pressure];
        let mut events = Vec::new();
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _ in 0..2_000 {
            let lane = lanes[next(4) as usize];
            let at = next(1_200) as i64 - 100;
            let value = next(400) as i32 - 200;
            match next(4) {
                0 => {
                    set_point(&mut events, lane, t(at), value, LEN);
                }
                1 => {
                    let mut sel = Vec::new();
                    for _ in 0..next(5) {
                        sel.push(next(events.len().max(1) as u64) as usize);
                    }
                    move_points(&mut events, lane, &sel, t(at - 500), value, LEN);
                }
                2 => {
                    let sel = in_box(&events, lane, t(at), t(at + 200), value - 50, value + 50);
                    delete_points(&mut events, &sel);
                }
                _ => {
                    let with = ramp(
                        lane,
                        (t(at), value),
                        (t(at + 300), -value),
                        &Curve::SLOW,
                        t(37),
                    );
                    replace_span(&mut events, lane, t(at), t(at + 300), with, LEN);
                }
            }
            assert_valid(&events);
        }
    }
}
