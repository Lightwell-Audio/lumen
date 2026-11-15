use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

pub const NAME: &str = "reverb";

pub const PREDELAY: u32 = 1;
pub const SIZE: u32 = 2;
pub const DECAY: u32 = 3;
pub const DAMPING: u32 = 4;
pub const WIDTH: u32 = 5;
pub const EARLY: u32 = 6;
pub const MIX: u32 = 7;

pub const MOST_DECAY: f32 = 8.0;

const LEAST_DECAY: f32 = 0.2;

const MOST_PREDELAY_SECONDS: f32 = 0.2;
const EARLY_SECONDS: f32 = 0.25;

pub const HIGHEST_RATE: u32 = 192_000;

const KNOT: i64 = 32;

const GLIDE_SECONDS: f32 = 0.02;

pub const MOST_TAIL_SECONDS: f32 = 10.0;

const BASE: [f32; 8] = [
    1116.0, 1188.0, 1277.0, 1356.0, 1422.0, 1491.0, 1557.0, 1617.0,
];

const SMALLEST: f32 = 0.5;
const LARGEST: f32 = 1.6;

const LINE_CAPACITY: usize =
    (BASE[7] as f64 / 48_000.0 * LARGEST as f64 * HIGHEST_RATE as f64) as usize + 4;

const EARLY_TAPS: [[(f32, f32); 6]; 2] = [
    [
        (0.071, 1.0),
        (0.113, -0.8),
        (0.163, 0.7),
        (0.227, -0.6),
        (0.311, 0.5),
        (0.433, -0.4),
    ],
    [
        (0.089, 1.0),
        (0.131, -0.8),
        (0.181, 0.6),
        (0.251, -0.7),
        (0.337, 0.5),
        (0.461, -0.35),
    ],
];

#[allow(clippy::too_many_arguments)]
const fn param(
    id: u32,
    name: &'static str,
    unit: &'static str,
    low: f32,
    high: f32,
    taper: Taper,
    default: f32,
    steps: i32,
    choices: &'static [&'static str],
    text: fn(f32) -> String,
    parse: fn(&str) -> Option<f32>,
) -> ParamInfo {
    ParamInfo {
        id,
        name,
        unit,
        low,
        high,
        taper,
        default,
        steps,
        choices,
        text,
        parse,
    }
}

pub static TABLE: &[ParamInfo] = &[
    param(
        PREDELAY,
        "Pre",
        "ms",
        0.0,
        200.0,
        Taper::Power(2.0),
        20.0,
        0,
        &[],
        ms_text,
        ms_parse,
    ),
    param(
        SIZE,
        "Size",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        55.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        DECAY,
        "Decay",
        "s",
        LEAST_DECAY,
        MOST_DECAY,
        Taper::Power(2.0),
        2.0,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        DAMPING,
        "Damp",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        40.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        WIDTH,
        "Width",
        "%",
        0.0,
        200.0,
        Taper::Linear,
        100.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        EARLY,
        "Early",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        30.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        MIX,
        "Mix",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        25.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
];

pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

pub fn latency(_values: Values<'_>) -> u32 {
    0
}

pub fn tail(values: Values<'_>) -> u32 {
    tail_frames(values, REFERENCE_RATE)
}

pub const REFERENCE_RATE: u32 = 48_000;

pub fn tail_frames(values: Values<'_>, rate: u32) -> u32 {
    if value(values, MIX) / 100.0 <= 0.005 {
        return 0;
    }
    let seconds =
        (value(values, PREDELAY) / 1000.0 + value(values, DECAY) + 0.05).min(MOST_TAIL_SECONDS);
    (seconds * rate as f32).round() as u32
}

fn value(values: Values<'_>, id: u32) -> f32 {
    let x = params::normalised_of(TABLE, id, values);
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

fn ms_text(ms: f32) -> String {
    format!("{ms:.0} ms")
}

fn ms_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("ms").trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn seconds_text(seconds: f32) -> String {
    format!("{seconds:.2} s")
}

fn seconds_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches('s').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn one_pole(hz: f32, rate: u32) -> f32 {
    if rate == 0 || hz <= 0.0 {
        return 0.0;
    }
    let x = -std::f32::consts::TAU * hz / rate as f32;
    (1.0 - x.exp()).clamp(0.0, 1.0)
}

fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

#[derive(Debug, Clone, Copy)]
struct Line {
    length: f32,
    target: f32,
    step: f32,
    gain: f32,
    damp: f32,
    low: f32,
}

impl Line {
    fn new() -> Self {
        Self {
            length: BASE[0],
            target: BASE[0],
            step: 0.0,
            gain: 0.9,
            damp: 0.9,
            low: 0.0,
        }
    }
}

struct Curves {
    predelay: [f32; crate::MOST_FRAMES],
    size: [f32; crate::MOST_FRAMES],
    decay: [f32; crate::MOST_FRAMES],
    damping: [f32; crate::MOST_FRAMES],
    width: [f32; crate::MOST_FRAMES],
    early: [f32; crate::MOST_FRAMES],
    mix: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
        Box::new(Self {
            predelay: [0.0; crate::MOST_FRAMES],
            size: [0.0; crate::MOST_FRAMES],
            decay: [0.0; crate::MOST_FRAMES],
            damping: [0.0; crate::MOST_FRAMES],
            width: [0.0; crate::MOST_FRAMES],
            early: [0.0; crate::MOST_FRAMES],
            mix: [0.0; crate::MOST_FRAMES],
        })
    }
}

const PREDELAY_CAPACITY: usize = (MOST_PREDELAY_SECONDS * HIGHEST_RATE as f32) as usize + 4;
const EARLY_CAPACITY: usize = (EARLY_SECONDS * HIGHEST_RATE as f32) as usize + 4;

pub struct Reverb {
    pre: [Box<[f32]>; 2],
    pre_write: usize,
    pre_now: f32,
    pre_target: f32,
    pre_step: f32,
    early_line: Box<[f32]>,
    early_write: usize,
    lines: [Box<[f32]>; 8],
    at: [usize; 8],
    place: [Line; 8],
    size_target: f32,
    curves: Box<Curves>,
    predelay: Glide,
    size: Glide,
    decay: Glide,
    damping: Glide,
    width: Glide,
    early: Glide,
    mix: Glide,
    held: [(u32, f32); 7],
    rate: u32,
}

impl Reverb {
    pub fn new() -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        let held = [
            (PREDELAY, at(PREDELAY)),
            (SIZE, at(SIZE)),
            (DECAY, at(DECAY)),
            (DAMPING, at(DAMPING)),
            (WIDTH, at(WIDTH)),
            (EARLY, at(EARLY)),
            (MIX, at(MIX)),
        ];
        let mut place = [Line::new(); 8];
        for (line, base) in place.iter_mut().zip(BASE) {
            line.length = base;
            line.target = base;
            line.gain = decay_gain(base / REFERENCE_RATE as f32, 2.0);
            line.damp = 0.9;
        }
        Self {
            pre: [
                vec![0.0; PREDELAY_CAPACITY].into_boxed_slice(),
                vec![0.0; PREDELAY_CAPACITY].into_boxed_slice(),
            ],
            pre_write: 0,
            pre_now: at(PREDELAY) * (REFERENCE_RATE as f32 / 1000.0),
            pre_target: f32::NAN,
            pre_step: 0.0,
            early_line: vec![0.0; EARLY_CAPACITY].into_boxed_slice(),
            early_write: 0,
            lines: [
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
                vec![0.0; LINE_CAPACITY].into_boxed_slice(),
            ],
            at: [0; 8],
            place,
            size_target: f32::NAN,
            curves: Curves::new(),
            predelay: Glide::new(at(PREDELAY)),
            size: Glide::new(at(SIZE)),
            decay: Glide::new(at(DECAY)),
            damping: Glide::new(at(DAMPING)),
            width: Glide::new(at(WIDTH)),
            early: Glide::new(at(EARLY)),
            mix: Glide::new(at(MIX)),
            held,
            rate: REFERENCE_RATE,
        }
    }

    fn read(line: &[f32], write: usize, behind: f32) -> f32 {
        crate::line::read_behind(line, write, behind)
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

fn decay_gain(seconds: f32, decay: f32) -> f32 {
    let decay = decay.clamp(LEAST_DECAY, MOST_DECAY);
    if seconds <= 0.0 {
        return 0.0;
    }
    10f32.powf(-3.0 * seconds / decay).clamp(0.0, 1.0)
}

fn size_factor(x: f32) -> f32 {
    SMALLEST + (LARGEST - SMALLEST) * x.clamp(0.0, 1.0)
}

#[inline]
fn hadamard(v: &mut [f32; 8]) {
    let mut step = 1;
    while step < 8 {
        for block in (0..8).step_by(step * 2) {
            for k in block..block + step {
                let (a, b) = (v[k], v[k + step]);
                v[k] = a + b;
                v[k + step] = a - b;
            }
        }
        step *= 2;
    }
    for value in v.iter_mut() {
        *value *= 0.353_553_38;
    }
}

impl Effect for Reverb {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, _key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate.max(1);
        let events = block.events;
        let curves = &mut self.curves;
        params::values(
            &mut curves.predelay[..frames],
            &mut self.predelay,
            events,
            PREDELAY,
            rate,
        );
        params::values(
            &mut curves.size[..frames],
            &mut self.size,
            events,
            SIZE,
            rate,
        );
        params::values(
            &mut curves.decay[..frames],
            &mut self.decay,
            events,
            DECAY,
            rate,
        );
        params::values(
            &mut curves.damping[..frames],
            &mut self.damping,
            events,
            DAMPING,
            rate,
        );
        params::values(
            &mut curves.width[..frames],
            &mut self.width,
            events,
            WIDTH,
            rate,
        );
        params::values(
            &mut curves.early[..frames],
            &mut self.early,
            events,
            EARLY,
            rate,
        );
        params::values(&mut curves.mix[..frames], &mut self.mix, events, MIX, rate);
        self.rate = rate;

        let glide_frames = (GLIDE_SECONDS * rate as f32).max(1.0);
        let pre_capacity = PREDELAY_CAPACITY as f32 - 2.0;
        let early_len = self.early_line.len();
        let mut out = [0.0f32; 8];
        let mut pre_ms = curve_value(PREDELAY, curves.predelay[0]);

        for frame in 0..frames {
            for line in &mut self.place {
                if (line.target - line.length).abs() <= line.step.abs() {
                    line.length = line.target;
                } else {
                    line.length += line.step;
                }
            }
            if (block.start + frame as i64).rem_euclid(KNOT) == 0 {
                pre_ms = curve_value(PREDELAY, curves.predelay[frame]);
                let decay = curve_value(DECAY, curves.decay[frame]).clamp(LEAST_DECAY, MOST_DECAY);
                let damp = curves.damping[frame].clamp(0.0, 1.0);
                let cutoff = 20_000.0 * (1_000.0f32 / 20_000.0).powf(damp);
                let damp_now = one_pole(cutoff, rate);
                let size = size_factor(curves.size[frame]);
                if size != self.size_target {
                    self.size_target = size;
                    for (index, line) in self.place.iter_mut().enumerate() {
                        line.target = BASE[index] / 48_000.0 * size * rate as f32;
                        line.step = (line.target - line.length) / glide_frames;
                    }
                }
                for line in &mut self.place {
                    line.gain = decay_gain(line.length / rate as f32, decay);
                    line.damp = damp_now;
                }
            }
            let pre_target = (pre_ms / 1000.0 * rate as f32).clamp(0.0, pre_capacity);
            if pre_target != self.pre_target {
                self.pre_target = pre_target;
                self.pre_step = (pre_target - self.pre_now) / glide_frames;
            }
            if (pre_target - self.pre_now).abs() <= self.pre_step.abs() {
                self.pre_now = pre_target;
            } else {
                self.pre_now += self.pre_step;
            }
            let pre_left = if self.pre_now > 0.0 {
                Self::read(&self.pre[0], self.pre_write, self.pre_now)
            } else {
                0.0
            };
            let pre_right = if self.pre_now > 0.0 {
                Self::read(&self.pre[1], self.pre_write, self.pre_now)
            } else {
                0.0
            };

            let mono = 0.5 * (pre_left + pre_right);
            let early_reads: [[f32; 6]; 2] = EARLY_TAPS.map(|taps| {
                taps.map(|(fraction, _)| {
                    let behind =
                        (fraction * EARLY_SECONDS * rate as f32).min(early_len as f32 - 2.0);
                    crate::line::read_behind(&self.early_line, self.early_write, behind)
                })
            });
            let mut early_pair = [0.0f32; 2];
            for side in 0..2 {
                for (tap, (_, sign)) in early_reads[side].iter().zip(EARLY_TAPS[side]) {
                    early_pair[side] += tap * sign;
                }
                early_pair[side] *= 1.0 / 3.0;
            }

            for (index, line) in self.place.iter_mut().enumerate() {
                let wet = Self::read(&self.lines[index], self.at[index], line.length);
                line.low = flush(line.low + line.damp * (wet - line.low));
                out[index] = flush(line.low * line.gain);
            }
            let mut next = out;
            hadamard(&mut next);
            let into_room = 0.25 * (pre_left + pre_right);
            for ((line, at), fed) in self.lines.iter_mut().zip(&mut self.at).zip(next) {
                line[*at] = flush(into_room + fed);
                *at += 1;
                if *at == line.len() {
                    *at = 0;
                }
            }

            self.early_line[self.early_write] = flush(mono);
            self.early_write += 1;
            if self.early_write == early_len {
                self.early_write = 0;
            }
            self.pre[0][self.pre_write] = flush(left[frame]);
            self.pre[1][self.pre_write] = flush(right[frame]);
            self.pre_write += 1;
            if self.pre_write == self.pre[0].len() {
                self.pre_write = 0;
            }

            let early_mix = curves.early[frame].clamp(0.0, 1.0);
            let late_mix = 1.0 - early_mix;
            let late_left = 0.5 * (out[0] - out[1] + out[2] - out[3]);
            let late_right = 0.5 * (out[4] - out[5] + out[6] - out[7]);
            let mut wet_left = early_pair[0] * early_mix + late_left * late_mix;
            let mut wet_right = early_pair[1] * early_mix + late_right * late_mix;
            let width = curve_value(WIDTH, curves.width[frame]) / 100.0;
            let mid = 0.5 * (wet_left + wet_right);
            let side = 0.5 * (wet_left - wet_right) * width;
            wet_left = mid + side;
            wet_right = mid - side;
            let mix = curve_value(MIX, curves.mix[frame]) / 100.0;
            let dry = 1.0 - mix;
            left[frame] = flush(left[frame] * dry + wet_left * mix);
            right[frame] = flush(right[frame] * dry + wet_right * mix);
        }

        let last = frames - 1;
        self.held = [
            (PREDELAY, curves.predelay[last]),
            (SIZE, curves.size[last]),
            (DECAY, curves.decay[last]),
            (DAMPING, curves.damping[last]),
            (WIDTH, curves.width[last]),
            (EARLY, curves.early[last]),
            (MIX, curves.mix[last]),
        ];
    }

    fn tail(&self) -> u32 {
        tail_frames(&self.held, self.rate)
    }

    fn reset(&mut self) {
        for line in &mut self.pre {
            line.fill(0.0);
        }
        self.early_line.fill(0.0);
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.at = [0; 8];
        self.pre_write = 0;
        self.early_write = 0;
        for line in &mut self.place {
            line.low = 0.0;
        }
        for glide in [
            &mut self.predelay,
            &mut self.size,
            &mut self.decay,
            &mut self.damping,
            &mut self.width,
            &mut self.early,
            &mut self.mix,
        ] {
            glide.arrive();
        }
        self.pre_target = f32::NAN;
        self.pre_step = 0.0;
        self.pre_now = value(&self.held, PREDELAY) / 1000.0 * self.rate as f32;
        self.size_target = f32::NAN;
    }
}

fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}
