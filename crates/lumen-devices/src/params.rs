use crate::ParamBlock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Taper {
    Linear,
    Power(f32),
    Log,
}

impl Taper {
    pub fn value(self, (low, high): (f32, f32), x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => low + (high - low) * x,
            Self::Power(power) => low + (high - low) * f64::from(x).powf(f64::from(power)) as f32,
            Self::Log if low > 0.0 && high > 0.0 => {
                (f64::from(low) * (f64::from(high) / f64::from(low)).powf(f64::from(x))) as f32
            }
            Self::Log => low + (high - low) * x,
        }
    }

    pub fn normalised(self, (low, high): (f32, f32), value: f32) -> f32 {
        let span = high - low;
        if span <= 0.0 {
            return 0.0;
        }
        let x = ((value - low) / span).clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Power(power) => f64::from(x).powf(1.0 / f64::from(power)) as f32,
            Self::Log if low > 0.0 && high > 0.0 => {
                let ratio = (f64::from(high) / f64::from(low)).ln();
                if ratio <= 0.0 {
                    0.0
                } else {
                    (((f64::from(value.max(low)) / f64::from(low)).ln() / ratio) as f32)
                        .clamp(0.0, 1.0)
                }
            }
            Self::Log => x,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamInfo {
    pub id: u32,
    pub name: &'static str,
    pub unit: &'static str,
    pub low: f32,
    pub high: f32,
    pub taper: Taper,
    pub default: f32,
    pub steps: i32,
    pub choices: &'static [&'static str],
    pub text: fn(f32) -> String,
    pub parse: fn(&str) -> Option<f32>,
}

impl ParamInfo {
    pub fn range(&self) -> (f32, f32) {
        (self.low, self.high)
    }

    pub fn is_choice(&self) -> bool {
        self.steps > 0
    }

    pub fn value(&self, x: f32) -> f32 {
        let value = self.taper.value(self.range(), x);
        if self.is_choice() {
            let steps = f32::from(i16::try_from(self.steps).unwrap_or(i16::MAX));
            (value * steps).round() / steps
        } else {
            value
        }
    }

    pub fn normalised(&self, value: f32) -> f32 {
        let x = self.taper.normalised(self.range(), value);
        if self.is_choice() {
            let steps = f32::from(i16::try_from(self.steps).unwrap_or(i16::MAX));
            (x * steps).round() / steps
        } else {
            x
        }
    }

    pub fn text_of(&self, x: f32) -> String {
        (self.text)(self.value(x))
    }

    pub fn value_of(&self, text: &str) -> Option<f32> {
        (self.parse)(text).map(|value| self.normalised(value))
    }

    pub fn choice(&self, index: usize) -> Option<&'static str> {
        self.choices.get(index).copied()
    }
}

pub fn find(table: &'static [ParamInfo], id: u32) -> Option<&'static ParamInfo> {
    table.iter().find(|p| p.id == id)
}

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

pub fn normalised_of(table: &[ParamInfo], id: u32, values: &[(u32, f32)]) -> f32 {
    let Some(info) = table.iter().find(|p| p.id == id) else {
        return 0.0;
    };
    values
        .iter()
        .find(|(known, _)| *known == id)
        .map_or_else(|| info.normalised(info.default), |&(_, x)| x)
}

pub const GLIDE_SECONDS: f32 = 0.02;

pub const STEP_SECONDS: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    now: f32,
    target: f32,
    step: f32,
}

impl Glide {
    pub const fn new(value: f32) -> Self {
        Self {
            now: value,
            target: value,
            step: 0.0,
        }
    }

    pub fn value(&self) -> f32 {
        self.now
    }

    pub fn settle(&mut self, value: f32) {
        self.now = value;
        self.target = value;
        self.step = 0.0;
    }

    pub fn arrive(&mut self) {
        self.now = self.target;
        self.step = 0.0;
    }

    pub fn head_to(&mut self, value: f32) {
        if value != self.target {
            self.target = value;
            self.step = f32::NAN;
        }
    }

    pub fn run(&mut self, out: &mut [f32], rate: u32) {
        self.glide_to(self.target, out, rate);
    }

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

pub fn at(points: &[(u32, f64)], frame: u32) -> f32 {
    if points.is_empty() {
        return 0.0;
    }
    let after = points.partition_point(|p| p.0 <= frame);
    let Some(before) = after.checked_sub(1) else {
        return points[0].1 as f32;
    };
    match points.get(after) {
        Some(&(next, value)) if next > points[before].0 => {
            let (at, from) = points[before];
            let span = (next - at) as f32;
            let into = (frame - at) as f32;
            (from as f32) + ((value as f32) - (from as f32)) * (into / span)
        }
        _ => points[before].1 as f32,
    }
}

pub fn values(out: &mut [f32], glide: &mut Glide, events: &ParamBlock, id: u32, rate: u32) {
    let frames = out.len();
    if frames == 0 {
        return;
    }
    match queues_for(events, id) {
        None => glide.run(out, rate),
        Some((at, true)) => match events.queue(at).1 {
            [] => out.fill(glide.value()),
            points => curve(points, glide, out, rate),
        },
        Some((at, false)) => match events.queue(at).1 {
            [(_, value)] => glide.glide_to(*value as f32, out, rate),
            [] => out.fill(glide.value()),
            points => curve(points, glide, out, rate),
        },
    }
}

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
        assert_eq!(Taper::Power(2.0).value(range, 0.5), 100.0);
        assert!((Taper::Power(2.0).normalised(range, 100.0) - 0.5).abs() < 1e-6);
        assert_eq!(Taper::Linear.value(range, 2.0), 400.0);
        assert_eq!(Taper::Linear.normalised(range, -3.0), 0.0);
    }

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

    #[test]
    fn a_value_the_interface_moved_glides_over_twenty_milliseconds() {
        let mut glide = Glide::new(0.0);
        let mut out = [0.0; 480];
        let events = block(1.0, None);
        values(&mut out, &mut glide, &events, 1, 48_000);
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

    #[test]
    fn a_lane_s_single_point_is_a_lane_not_a_knob() {
        let mut glide = Glide::new(0.0);
        let events = block(0.0, Some(&[(0, 1.0)]));
        assert!(events.is_lane(1), "the second queue is the lane's");
        let mut out = [0.0; 8];
        values(&mut out, &mut glide, &events, 1, 48_000);
        assert!((out[0] - 1.0 / 48.0).abs() < 1e-6, "{}", out[0]);
        assert!(out[7] > 0.14, "{}", out[7]);
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
