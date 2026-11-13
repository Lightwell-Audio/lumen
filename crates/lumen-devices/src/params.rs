//! What a built-in device's parameters are, and what they are worth as a block
//! plays them (M8 slice B0, ADR-0145).
//!
//! Each device declares a **static table** ([`ParamInfo`]): a stable `u32` id
//! that is never reused, the name a person reads, its unit, range and taper, its
//! default, its steps, and how its value reads as text and is read back. The
//! project saves values by id, normalised, so the table is the only place a
//! device's parameters are defined.
//!
//! A device plays a parameter through [`values`]: the value the project holds,
//! glided when the interface moves it, or the points a lane gives it, on their
//! exact frames and straight between them, with a step moved over a
//! millisecond rather than jumped. It is a pure function of the timeline sample
//! and of where the last block left off, so any block size plays the same
//! curve.

use crate::ParamBlock;

/// How a parameter's normalised value maps to its own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Taper {
    /// Straight from the bottom of the range to the top.
    Linear,
    /// Slower near the bottom: `low + (high - low) * x^power` (a frequency).
    Power(f32),
    /// Equal ratios for equal turns, `low * (high / low)^x`: what a person
    /// hears of a frequency or a Q (M8 slice B1). A range whose bottom is not
    /// above zero reads as [`Taper::Linear`].
    Log,
}

impl Taper {
    /// The value in the parameter's own units.
    pub fn value(self, (low, high): (f32, f32), x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => low + (high - low) * x,
            Self::Power(power) => low + (high - low) * x.powf(power),
            Self::Log if low > 0.0 && high > 0.0 => low * (high / low).powf(x),
            Self::Log => low + (high - low) * x,
        }
    }

    /// The normalised value of a value in the parameter's own units.
    pub fn normalised(self, (low, high): (f32, f32), value: f32) -> f32 {
        let span = high - low;
        if span <= 0.0 {
            return 0.0;
        }
        let x = ((value - low) / span).clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Power(power) => x.powf(1.0 / power),
            Self::Log if low > 0.0 && high > 0.0 => {
                let ratio = (high / low).ln();
                if ratio <= 0.0 {
                    0.0
                } else {
                    ((value.max(low) / low).ln() / ratio).clamp(0.0, 1.0)
                }
            }
            Self::Log => x,
        }
    }
}

/// One parameter of a built-in device.
#[derive(Debug, Clone, Copy)]
pub struct ParamInfo {
    /// Stable, never reused. A new meaning gets a new id.
    pub id: u32,
    /// The name the card, the lane chooser and automation show.
    pub name: &'static str,
    /// What the values are ("dB", "%", "Hz"); empty for a switch.
    pub unit: &'static str,
    /// The lowest and highest values, in the parameter's own units.
    pub low: f32,
    pub high: f32,
    pub taper: Taper,
    /// The value a project that has never saved one plays.
    pub default: f32,
    /// 0 for a value that moves freely; otherwise the number of steps, so a
    /// choice holds `steps + 1` values and a switch holds two (`steps` 1).
    pub steps: i32,
    /// The word for each step of a choice, lowest first; empty otherwise.
    pub choices: &'static [&'static str],
    /// The value as the card shows it.
    pub text: fn(f32) -> String,
    /// A value from what a person typed: a number in the parameter's own
    /// units (with or without the unit) or one of a choice's words.
    pub parse: fn(&str) -> Option<f32>,
}

impl ParamInfo {
    pub fn range(&self) -> (f32, f32) {
        (self.low, self.high)
    }

    /// Whether the value is one of a few named steps.
    pub fn is_choice(&self) -> bool {
        self.steps > 0
    }

    /// The value of normalised `x`.
    pub fn value(&self, x: f32) -> f32 {
        let value = self.taper.value(self.range(), x);
        if self.is_choice() {
            let steps = f32::from(i16::try_from(self.steps).unwrap_or(i16::MAX));
            (value * steps).round() / steps
        } else {
            value
        }
    }

    /// The normalised value of `value`, at the nearest step of a choice.
    pub fn normalised(&self, value: f32) -> f32 {
        let x = self.taper.normalised(self.range(), value);
        if self.is_choice() {
            let steps = f32::from(i16::try_from(self.steps).unwrap_or(i16::MAX));
            (x * steps).round() / steps
        } else {
            x
        }
    }

    /// The words for normalised `x`.
    pub fn text_of(&self, x: f32) -> String {
        (self.text)(self.value(x))
    }

    /// `text` read back as a normalised value; `None` when it says nothing
    /// this parameter understands.
    pub fn value_of(&self, text: &str) -> Option<f32> {
        (self.parse)(text).map(|value| self.normalised(value))
    }

    /// The word of step `index` of a choice.
    pub fn choice(&self, index: usize) -> Option<&'static str> {
        self.choices.get(index).copied()
    }
}

/// The table's parameter `id`.
pub fn find(table: &'static [ParamInfo], id: u32) -> Option<&'static ParamInfo> {
    table.iter().find(|p| p.id == id)
}

/// The parameter `id` of `table` from a project's saved values, in its own
/// units: what it holds, or its default.
pub fn value_of_param(
    table: &'static [ParamInfo],
    id: u32,
    held: &std::collections::BTreeMap<u32, f64>,
) -> f32 {
    let Some(info) = find(table, id) else {
        return 0.0;
    };
    held.get(&id)
        .map_or(info.default, |&x| info.value(x as f32))
}

/// The normalised value `id` holds in a device's values ([`crate::effect::Values`]):
/// what the project says, or the table's default for a parameter it does not
/// name. What a device's latency or tail is worked out from.
pub fn normalised_of(table: &[ParamInfo], id: u32, values: &[(u32, f32)]) -> f32 {
    let Some(info) = table.iter().find(|p| p.id == id) else {
        return 0.0;
    };
    values
        .iter()
        .find(|(known, _)| *known == id)
        .map_or_else(|| info.normalised(info.default), |&(_, x)| x)
}

/// How long a value the interface moved takes to glide: 20 ms, as the mixer's
/// own levels do (`GLIDE_SECONDS` there).
pub const GLIDE_SECONDS: f32 = 0.02;

/// How long a lane's step takes to reach its new value: 1 ms, as the mixer's
/// own lanes ramp a step.
pub const STEP_SECONDS: f32 = 0.001;

/// One parameter's value as the last block left it: where a value the
/// interface moved glides from, and where a lane's curve continues.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    now: f32,
    target: f32,
    /// How far the value moves per frame while it glides; NaN until it is
    /// worked out for the rate.
    step: f32,
}

impl Glide {
    /// Settled at `value` (normalised).
    pub const fn new(value: f32) -> Self {
        Self {
            now: value,
            target: value,
            step: 0.0,
        }
    }

    /// The value now (normalised).
    pub fn value(&self) -> f32 {
        self.now
    }

    /// Settles on `value`, with no glide: a lane gave it that frame by frame.
    pub fn settle(&mut self, value: f32) {
        self.now = value;
        self.target = value;
        self.step = 0.0;
    }

    /// Jumps to where the value was heading: playback jumped to another place
    /// in the song (which the engine crossfades), or the device is reset.
    pub fn arrive(&mut self) {
        self.now = self.target;
        self.step = 0.0;
    }

    /// Carries on towards where the value is heading, fills `out` with it, and
    /// moves on: a block that does not name the parameter (the engine tells a
    /// device a value only when it changed, review L6) still finishes a glide.
    pub fn run(&mut self, out: &mut [f32], rate: u32) {
        self.glide_to(self.target, out, rate);
    }

    /// Fills `out` with a glide from where the value is to `target`, in a
    /// straight line over [`GLIDE_SECONDS`].
    fn glide_to(&mut self, target: f32, out: &mut [f32], rate: u32) {
        if target != self.target {
            self.target = target;
            self.step = f32::NAN;
        }
        if self.now == self.target || out.is_empty() {
            self.now = self.target;
            out.fill(self.now);
            return;
        }
        if self.step.is_nan() {
            let frames = (rate as f32 * GLIDE_SECONDS).max(1.0);
            self.step = (self.target - self.now).abs() / frames;
        }
        let (target, step) = (self.target, self.step);
        let mut value = self.now;
        for sample in out.iter_mut() {
            value = if (target - value).abs() <= step {
                target
            } else {
                value + step.copysign(target - value)
            };
            *sample = value;
        }
        self.now = value;
    }
}

/// Fills `out` with parameter `id`'s normalised value for each of its frames:
/// the points `events` gives it, on their exact frames and straight between
/// them, or the value at rest, glided, when no lane moves it.
///
/// A step (two points a frame apart) is moved over [`STEP_SECONDS`] rather
/// than jumped; a straight segment is played exactly, so a value on a lane's
/// own frame is the lane's value.
pub fn values(out: &mut [f32], glide: &mut Glide, events: &ParamBlock, id: u32, rate: u32) {
    let frames = out.len();
    if frames == 0 {
        return;
    }
    match queues_for(events, id) {
        // Nothing says where it is: it carries on towards where it was going
        // (a value the interface moved finishes its glide), and holds once it
        // has arrived.
        None => glide.run(out, rate),
        // A lane's points, or the value it holds when it does not move in
        // this block.
        Some((at, true)) => match events.queue(at).1 {
            [] => out.fill(glide.value()),
            points => curve(points, glide, out, rate),
        },
        // Only the value the project holds: a knob the interface moved, which
        // glides.
        Some((at, false)) => match events.queue(at).1 {
            [(_, value)] => glide.glide_to(*value as f32, out, rate),
            [] => out.fill(glide.value()),
            points => curve(points, glide, out, rate),
        },
    }
}

/// Fills `out` with a lane's points: straight between them, and a step moved
/// over [`STEP_SECONDS`] rather than jumped.
fn curve(points: &[(u32, f64)], glide: &mut Glide, out: &mut [f32], rate: u32) {
    let mut now = glide.value();
    let step = if rate == 0 {
        f32::MAX
    } else {
        1.0 / (rate as f32 * STEP_SECONDS)
    };
    let mut last = 0;
    for (frame, sample) in out.iter_mut().enumerate() {
        while last + 1 < points.len() && points[last + 1].0 as usize <= frame {
            last += 1;
        }
        let want = want(points, last, frame);
        let change = want - now;
        now += change.clamp(-step, step);
        *sample = now;
    }
    glide.settle(now);
}

/// The point's value at `frame`, straight between the two points it falls
/// between.
fn want(points: &[(u32, f64)], last: usize, frame: usize) -> f32 {
    let (at, value) = points[last];
    let value = value as f32;
    if at as usize > frame {
        return value;
    }
    match points.get(last + 1) {
        Some(&(next, next_value)) if next > at => {
            let span = (next - at) as f32;
            let into = (frame - at as usize) as f32;
            value + (next_value as f32 - value) * (into / span)
        }
        _ => value,
    }
}

/// The last queue in `events` that names parameter `id`, and whether it is a
/// lane's (a value the project holds is not, M8 slice B1, ADR-0146): a device
/// reads the last one, which is the lane's when one plays.
fn queues_for(events: &ParamBlock, id: u32) -> Option<(usize, bool)> {
    let mut found = None;
    for i in (0..events.len()).rev() {
        if events.queue(i).0 == id {
            found = Some((i, events.is_lane(i)));
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_db(db: f32) -> String {
        if db.abs() < 0.05 {
            "0.0 dB".to_owned()
        } else {
            format!("{db:+.1} dB").replace('-', "\u{2212}")
        }
    }

    fn gain_of(text: &str) -> Option<f32> {
        let text = text.trim().replace('\u{2212}', "-");
        let text = text.trim_end_matches("db").trim_end_matches("dB").trim();
        text.parse::<f32>().ok().filter(|v| v.is_finite())
    }

    const TABLE: &[ParamInfo] = &[
        ParamInfo {
            id: 1,
            name: "Gain",
            unit: "dB",
            low: -60.0,
            high: 24.0,
            taper: Taper::Linear,
            default: 0.0,
            steps: 0,
            choices: &[],
            text: gain_db,
            parse: gain_of,
        },
        ParamInfo {
            id: 2,
            name: "Mono below",
            unit: "Hz",
            low: 0.0,
            high: 400.0,
            taper: Taper::Power(2.0),
            default: 0.0,
            steps: 0,
            choices: &[],
            text: |hz| hz.to_string(),
            parse: |t| t.parse().ok(),
        },
        ParamInfo {
            id: 3,
            name: "Swap",
            unit: "",
            low: 0.0,
            high: 1.0,
            taper: Taper::Linear,
            default: 0.0,
            steps: 1,
            choices: &["Off", "On"],
            text: |v| if v >= 0.5 { "On".into() } else { "Off".into() },
            parse: |t| match t.trim().to_lowercase().as_str() {
                "on" => Some(1.0),
                "off" => Some(0.0),
                _ => None,
            },
        },
    ];

    /// A block for parameter 1: the value the project holds, then the points a
    /// lane gives it, when one does.
    fn block(held: f64, lane: Option<&[(u32, f64)]>) -> ParamBlock {
        let mut events = ParamBlock::new();
        events.start(256);
        events.open(1);
        events.point(0, held);
        if let Some(points) = lane {
            events.open_lane(1);
            for &(at, value) in points {
                events.point(at, value);
            }
        }
        events
    }

    #[test]
    fn a_taper_maps_both_ways() {
        let range = (0.0, 400.0);
        assert_eq!(Taper::Linear.value(range, 0.25), 100.0);
        assert_eq!(Taper::Linear.normalised(range, 100.0), 0.25);
        // Halfway up a squared frequency range is a quarter of the way in.
        assert_eq!(Taper::Power(2.0).value(range, 0.5), 100.0);
        assert!((Taper::Power(2.0).normalised(range, 100.0) - 0.5).abs() < 1e-6);
        // What is outside the range is brought in.
        assert_eq!(Taper::Linear.value(range, 2.0), 400.0);
        assert_eq!(Taper::Linear.normalised(range, -3.0), 0.0);
    }

    /// A log taper: half a turn is the geometric mean of the range, and every
    /// turn is the same ratio (M8 slice B1).
    #[test]
    fn a_log_taper_is_equal_ratios_for_equal_turns() {
        let range = (20.0, 20_000.0);
        let half = Taper::Log.value(range, 0.5);
        assert!((half - 632.45).abs() < 0.1, "{half}");
        for (x, value) in [(0.0, 20.0), (0.25, 112.5), (0.5, 632.45), (1.0, 20_000.0)] {
            let got = Taper::Log.value(range, x);
            assert!((got - value).abs() < 0.2, "{x}: {got} against {value}");
            let back = Taper::Log.normalised(range, got);
            assert!((back - x).abs() < 1e-4, "{x}: {back}");
        }
        // A range it cannot take is read straight.
        assert_eq!(Taper::Log.value((0.0, 10.0), 0.5), 5.0);
        assert_eq!(Taper::Log.normalised((0.0, 10.0), 5.0), 0.5);
    }

    #[test]
    fn a_value_reads_and_is_read_back() {
        let gain = &TABLE[0];
        assert_eq!(gain.text_of(0.25), "\u{2212}39.0 dB");
        let x = gain.value_of("\u{2212}39 dB").unwrap();
        assert!((x - 0.25).abs() < 2e-3, "{x}");
        assert_eq!(
            gain.value_of("\u{2212}39.0 dB").unwrap(),
            gain.value_of("-39.0 dB").unwrap()
        );
        assert!(gain.value_of("loud").is_none());
        let swap = &TABLE[2];
        assert_eq!(swap.text_of(1.0), "On");
        assert_eq!(swap.value_of("on"), Some(1.0));
        assert_eq!(swap.value_of("off"), Some(0.0));
    }

    #[test]
    fn a_choice_snaps_to_its_steps() {
        let swap = &TABLE[2];
        assert_eq!(swap.normalised(0.4), 0.0);
        assert_eq!(swap.normalised(0.6), 1.0);
        assert_eq!(swap.value(0.49), 0.0);
        assert_eq!(swap.value(0.51), 1.0);
        assert_eq!(swap.choice(1), Some("On"));
        assert_eq!(swap.choice(2), None);
    }

    /// A value with `frames` frames and no lane glides to a value the
    /// interface set, and holds it once it has arrived.
    #[test]
    fn a_value_the_interface_moved_glides_over_twenty_milliseconds() {
        let mut glide = Glide::new(0.0);
        let mut out = [0.0; 480];
        let events = block(1.0, None);
        values(&mut out, &mut glide, &events, 1, 48_000);
        // 20 ms at 48 kHz is 960 frames: 480 take it halfway.
        assert!((out[0] - 1.0 / 960.0).abs() < 1e-6, "{}", out[0]);
        assert!((out[479] - 0.5).abs() < 1e-3, "{}", out[479]);
        let mut rest = [0.0; 960];
        values(&mut rest, &mut glide, &events, 1, 48_000);
        assert_eq!(*rest.last().unwrap(), 1.0);
        assert_eq!(glide.value(), 1.0);
    }

    #[test]
    fn a_straight_lane_is_played_on_its_own_frames() {
        let mut glide = Glide::new(0.0);
        let events = block(0.0, Some(&[(0, 0.0), (200, 1.0)]));
        let mut out = [0.0; 256];
        values(&mut out, &mut glide, &events, 1, 48_000);
        assert_eq!(out[0], 0.0);
        assert!((out[100] - 0.5).abs() < 1e-6, "{}", out[100]);
        assert!((out[200] - 1.0).abs() < 1e-6);
        assert_eq!(out[255], 1.0);
    }

    /// A step in a lane arrives over a millisecond rather than in one frame.
    #[test]
    fn a_step_moves_over_a_millisecond() {
        let mut glide = Glide::new(0.0);
        let events = block(0.0, Some(&[(0, 0.0), (99, 0.0), (100, 1.0)]));
        let mut out = [0.0; 256];
        values(&mut out, &mut glide, &events, 1, 48_000);
        assert_eq!(out[99], 0.0);
        let ramp = 48;
        assert!(
            out[100] > 0.0 && out[100] < 0.1,
            "the ramp starts at the point: {}",
            out[100]
        );
        assert!(
            (out[100 + ramp / 2] - 0.5).abs() < 0.05,
            "{}",
            out[100 + ramp / 2]
        );
        assert_eq!(out[100 + ramp], 1.0);
    }

    /// The same lane plays the same frames at every block size.
    #[test]
    fn the_same_curve_at_any_block_size() {
        let points: Vec<(u32, f64)> = vec![
            (0, 0.1),
            (40, 0.2),
            (41, 0.9),
            (150, 0.95),
            (151, 0.2),
            (400, 0.2),
            (401, 0.8),
        ];
        // The lane's value at an absolute frame: straight between its points.
        let curve = |frame: u32| -> f64 {
            let after = points.partition_point(|p| p.0 <= frame);
            let Some(before) = after.checked_sub(1) else {
                return points[0].1;
            };
            match points.get(after) {
                Some(&(next, value)) if next > points[before].0 => {
                    let (at, from) = points[before];
                    from + (value - from) * f64::from(frame - at) / f64::from(next - at)
                }
                _ => points[before].1,
            }
        };
        // A block as the engine fills one: where the lane is at its first
        // frame, each of its points inside it, and where it has got to at its
        // last frame.
        let block_of = |at: usize, n: usize| {
            let mut entries: Vec<(usize, f64)> = vec![(0, curve(at as u32))];
            for &(offset, value) in &points {
                let offset = offset as usize;
                if offset > at && offset < at + n {
                    entries.push((offset - at, value));
                }
            }
            if n > 1 && !entries.iter().any(|&(offset, _)| offset == n - 1) {
                entries.push((n - 1, curve((at + n - 1) as u32)));
            }
            let mut events = ParamBlock::new();
            events.start(n);
            events.open(7);
            events.point(0, 0.1);
            events.open_lane(7);
            for (offset, value) in entries {
                events.point(offset as u32, value);
            }
            events
        };
        let mut whole = Vec::new();
        let mut glide = Glide::new(0.1);
        let mut out = vec![0.0; 512];
        values(&mut out, &mut glide, &block_of(0, 512), 7, 48_000);
        whole.extend(out);
        for size in [1usize, 7, 33, 64, 97, 128, 256] {
            let mut parts = Vec::new();
            let mut glide = Glide::new(0.1);
            let mut at = 0;
            while at < 512 {
                let n = size.min(512 - at);
                let mut out = vec![0.0; n];
                values(&mut out, &mut glide, &block_of(at, n), 7, 48_000);
                parts.extend(out);
                at += n;
            }
            for (k, (a, b)) in whole.iter().zip(&parts).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "size {size} frame {k}: {a} against {b}"
                );
            }
        }
    }

    #[test]
    fn a_parameter_a_block_does_not_name_holds_where_it_was() {
        let mut glide = Glide::new(0.7);
        let mut out = [0.0; 32];
        values(&mut out, &mut glide, &ParamBlock::none(), 4, 48_000);
        assert!(out.iter().all(|&x| x == 0.7));
    }

    /// A lane's queue says so, even with one point in the block: a lane moves
    /// over its millisecond, while the value the project holds glides over
    /// twenty (M8 slice B1, ADR-0146 — the two used to be told apart by
    /// counting queues, which a full table now cannot afford).
    #[test]
    fn a_lane_s_single_point_is_a_lane_not_a_knob() {
        let mut glide = Glide::new(0.0);
        let events = block(0.0, Some(&[(0, 1.0)]));
        assert!(events.is_lane(1), "the second queue is the lane's");
        let mut out = [0.0; 8];
        values(&mut out, &mut glide, &events, 1, 48_000);
        // A step of a lane: 1/48 a frame, not 1/960.
        assert!((out[0] - 1.0 / 48.0).abs() < 1e-6, "{}", out[0]);
        assert!(out[7] > 0.14, "{}", out[7]);
        // The value the project holds glides, as a knob does.
        let mut glide = Glide::new(0.0);
        let events = block(1.0, None);
        let mut out = [0.0; 8];
        values(&mut out, &mut glide, &events, 1, 48_000);
        assert!((out[0] - 1.0 / 960.0).abs() < 1e-6, "{}", out[0]);
    }

    #[test]
    fn a_project_with_no_value_plays_the_tables_default() {
        let held = std::collections::BTreeMap::new();
        assert_eq!(value_of_param(TABLE, 1, &held), 0.0);
        assert_eq!(value_of_param(TABLE, 2, &held), 0.0);
        let held: std::collections::BTreeMap<u32, f64> = [(1, 0.5)].into_iter().collect();
        assert!((value_of_param(TABLE, 1, &held) - -18.0).abs() < 1e-4);
    }
}
