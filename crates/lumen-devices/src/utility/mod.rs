use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

pub const NAME: &str = "utility";

pub const GAIN: u32 = 1;
pub const PAN: u32 = 2;
pub const WIDTH: u32 = 3;
pub const BASS_MONO: u32 = 4;
pub const POLARITY_L: u32 = 5;
pub const POLARITY_R: u32 = 6;
pub const SWAP: u32 = 7;
pub const DC_BLOCK: u32 = 8;

const FLOOR_DB: f32 = -60.0;

const DC_HZ: f32 = 10.0;

const BASS_PARKED_HZ: f32 = 120.0;

const KNOT: i64 = 32;

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
        GAIN,
        "Gain",
        "dB",
        FLOOR_DB,
        24.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        gain_text,
        gain_parse,
    ),
    param(
        PAN,
        "Pan",
        "",
        -100.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        pan_text,
        pan_parse,
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
        BASS_MONO,
        "Mono below",
        "Hz",
        0.0,
        400.0,
        Taper::Power(2.0),
        0.0,
        0,
        &[],
        bass_text,
        bass_parse,
    ),
    param(
        POLARITY_L,
        "Polarity L",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Normal", "Inverted"],
        normal_text,
        normal_parse,
    ),
    param(
        POLARITY_R,
        "Polarity R",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Normal", "Inverted"],
        normal_text,
        normal_parse,
    ),
    param(
        SWAP,
        "Swap L/R",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Off", "On"],
        on_off_text,
        on_off_parse,
    ),
    param(
        DC_BLOCK,
        "DC block",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Off", "On"],
        on_off_text,
        on_off_parse,
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

pub fn tail(_values: Values<'_>) -> u32 {
    0
}

fn gain_text(db: f32) -> String {
    if db <= FLOOR_DB {
        "\u{2212}\u{221E} dB".to_owned()
    } else if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

fn gain_parse(text: &str) -> Option<f32> {
    number_in(text, &["db"]).filter(|db| *db <= 24.0)
}

fn pan_text(pan: f32) -> String {
    let n = pan.round() as i32;
    match n {
        0 => "C".to_owned(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}

fn pan_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    if matches!(t.as_str(), "c" | "centre" | "center") {
        return Some(0.0);
    }
    let (sign, rest) = match t.split_at_checked(1) {
        Some(("l", rest)) => (-1.0, rest),
        Some(("r", rest)) => (1.0, rest),
        _ => (1.0, t.as_str()),
    };
    number_in(rest, &["%"]).map(|n| sign * n)
}

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    number_in(text, &["%"])
}

fn bass_text(hz: f32) -> String {
    if hz < 0.5 {
        "Off".to_owned()
    } else if hz < 9.95 {
        format!("{hz:.1} Hz")
    } else {
        format!("{hz:.0} Hz")
    }
}

fn bass_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    number_in(&t, &["hz"])
}

fn normal_text(value: f32) -> String {
    if value >= 0.5 {
        "Inverted".to_owned()
    } else {
        "Normal".to_owned()
    }
}

fn normal_parse(text: &str) -> Option<f32> {
    word_of(text, &["Normal", "Inverted"])
}

fn on_off_text(value: f32) -> String {
    if value >= 0.5 { "On" } else { "Off" }.to_owned()
}

fn on_off_parse(text: &str) -> Option<f32> {
    word_of(text, &["Off", "On"])
}

fn word_of(text: &str, choices: &[&str]) -> Option<f32> {
    let t = text.trim();
    if let Some(index) = choices.iter().position(|c| c.eq_ignore_ascii_case(t)) {
        return Some(if index > 0 { 1.0 } else { 0.0 });
    }
    match t {
        "0" => Some(0.0),
        "1" => Some(1.0),
        _ => None,
    }
}

fn number_in(text: &str, units: &[&str]) -> Option<f32> {
    let mut t = text.trim().replace('\u{2212}', "-").to_lowercase();
    for unit in units {
        if let Some(rest) = t.strip_suffix(unit) {
            t = rest.trim_end().to_owned();
        }
    }
    t.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
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

fn db_to_gain(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DcBlock {
    last_in: f32,
    last_out: f32,
}

impl DcBlock {
    fn run(&mut self, x: f32, keep: f32) -> f32 {
        let y = flush(x - self.last_in + keep * self.last_out);
        self.last_in = x;
        self.last_out = y;
        y
    }
}

struct Curves {
    gain: [f32; crate::MOST_FRAMES],
    pan: [f32; crate::MOST_FRAMES],
    width: [f32; crate::MOST_FRAMES],
    bass: [f32; crate::MOST_FRAMES],
    polarity_l: [f32; crate::MOST_FRAMES],
    polarity_r: [f32; crate::MOST_FRAMES],
    swap: [f32; crate::MOST_FRAMES],
    dc: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
        Box::new(Self {
            gain: [0.0; crate::MOST_FRAMES],
            pan: [0.0; crate::MOST_FRAMES],
            width: [0.0; crate::MOST_FRAMES],
            bass: [0.0; crate::MOST_FRAMES],
            polarity_l: [0.0; crate::MOST_FRAMES],
            polarity_r: [0.0; crate::MOST_FRAMES],
            swap: [0.0; crate::MOST_FRAMES],
            dc: [0.0; crate::MOST_FRAMES],
        })
    }
}

pub struct Utility {
    gain: Glide,
    pan: Glide,
    width: Glide,
    bass: Glide,
    polarity_l: Glide,
    polarity_r: Glide,
    swap: Glide,
    dc: Glide,
    curves: Box<Curves>,
    side: f32,
    side_tuning: (f32, f32),
    blockers: [DcBlock; 2],
}

impl Utility {
    pub fn new() -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        Self {
            gain: Glide::new(at(GAIN)),
            pan: Glide::new(at(PAN)),
            width: Glide::new(at(WIDTH)),
            bass: Glide::new(at(BASS_MONO)),
            polarity_l: Glide::new(at(POLARITY_L)),
            polarity_r: Glide::new(at(POLARITY_R)),
            swap: Glide::new(at(SWAP)),
            dc: Glide::new(at(DC_BLOCK)),
            curves: Curves::new(),
            side: 0.0,
            side_tuning: (0.0, one_pole(BASS_PARKED_HZ, 48_000)),
            blockers: [DcBlock::default(); 2],
        }
    }
}

impl Default for Utility {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Utility {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, _key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate;
        let events = block.events;
        let curves = &mut self.curves;
        params::values(
            &mut curves.gain[..frames],
            &mut self.gain,
            events,
            GAIN,
            rate,
        );
        params::values(&mut curves.pan[..frames], &mut self.pan, events, PAN, rate);
        params::values(
            &mut curves.width[..frames],
            &mut self.width,
            events,
            WIDTH,
            rate,
        );
        params::values(
            &mut curves.bass[..frames],
            &mut self.bass,
            events,
            BASS_MONO,
            rate,
        );
        params::values(
            &mut curves.polarity_l[..frames],
            &mut self.polarity_l,
            events,
            POLARITY_L,
            rate,
        );
        params::values(
            &mut curves.polarity_r[..frames],
            &mut self.polarity_r,
            events,
            POLARITY_R,
            rate,
        );
        params::values(
            &mut curves.swap[..frames],
            &mut self.swap,
            events,
            SWAP,
            rate,
        );
        params::values(
            &mut curves.dc[..frames],
            &mut self.dc,
            events,
            DC_BLOCK,
            rate,
        );

        let (mut bass, mut side_coef) = self.side_tuning;
        let dc_keep = 1.0 - one_pole(DC_HZ, rate);
        let law = block.law;

        let (mut last_db, mut gain) = (f32::NAN, 1.0f32);
        let (mut last_pan, mut sides) = (f32::NAN, [1.0f32; 2]);
        let mut side_state = self.side;
        for frame in 0..frames {
            if (block.start + frame as i64).rem_euclid(KNOT) == 0 {
                bass = curve_value(BASS_MONO, curves.bass[frame]);
                side_coef = one_pole(if bass > 0.0 { bass } else { BASS_PARKED_HZ }, rate);
            }
            let remove_low = bass > 0.0;
            let db = curve_value(GAIN, curves.gain[frame]);
            if db != last_db {
                gain = db_to_gain(db);
                last_db = db;
            }
            let pan = curve_value(PAN, curves.pan[frame]) / 100.0;
            if pan != last_pan {
                sides = law.gains(pan);
                last_pan = pan;
            }
            let width = curve_value(WIDTH, curves.width[frame]) / 100.0;
            let polarity_l = 1.0 - 2.0 * curves.polarity_l[frame];
            let polarity_r = 1.0 - 2.0 * curves.polarity_r[frame];
            let swap = curves.swap[frame];

            let mut l = left[frame] * gain * polarity_l;
            let mut r = right[frame] * gain * polarity_r;
            if swap > 0.0 {
                let (a, b) = (l, r);
                l = a * (1.0 - swap) + b * swap;
                r = b * (1.0 - swap) + a * swap;
            }
            let mid = 0.5 * (l + r);
            let mut side = 0.5 * (l - r) * width;
            side_state = flush(side_state + side_coef * (side - side_state));
            if remove_low {
                side -= side_state;
            }
            let mut l = (mid + side) * sides[0];
            let mut r = (mid - side) * sides[1];
            let dc = curves.dc[frame];
            let blocked = [
                self.blockers[0].run(l, dc_keep),
                self.blockers[1].run(r, dc_keep),
            ];
            if dc > 0.0 {
                l += (blocked[0] - l) * dc;
                r += (blocked[1] - r) * dc;
            }
            left[frame] = l;
            right[frame] = r;
        }
        self.side = side_state;
        self.side_tuning = (bass, side_coef);
    }

    fn reset(&mut self) {
        for glide in [
            &mut self.gain,
            &mut self.pan,
            &mut self.width,
            &mut self.bass,
            &mut self.polarity_l,
            &mut self.polarity_r,
            &mut self.swap,
            &mut self.dc,
        ] {
            glide.arrive();
        }
        self.side = 0.0;
        self.side_tuning = (0.0, 0.0);
        self.blockers = [DcBlock::default(); 2];
    }
}

fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}
