use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

use super::{
    FLOOR_DB, Reading, Reduction, db_of, flush, level_text, ms_parse, ms_text, one_pole, reach,
    value, word_of,
};

pub const NAME: &str = "compressor";

pub const THRESHOLD: u32 = 1;
pub const RATIO: u32 = 2;
pub const ATTACK: u32 = 3;
pub const RELEASE: u32 = 4;
pub const KNEE: u32 = 5;
pub const MAKEUP: u32 = 6;
pub const MIX: u32 = 7;
pub const DETECT: u32 = 8;
pub const KEY_HPF: u32 = 9;
pub const LISTEN: u32 = 10;

const FLOOR: f32 = -120.0;

const RMS_SECONDS: f32 = 0.010;

const MOST_MAKEUP_DB: f32 = 24.0;

const FIXED_MAKEUP_DB: f32 = 6.0;

const PEAK_FALL: f32 = 0.05;

const PEAK_MOST_SECONDS: f64 = 0.030;

const PEAK_HOLD_MARGIN: f64 = 0.05;

const PEAK_HOLD_LEAST: f64 = 2.0;

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
        THRESHOLD,
        "Pull",
        "dB",
        -60.0,
        0.0,
        Taper::Linear,
        -18.0,
        0,
        &[],
        level_text,
        db_parse,
    ),
    param(
        RATIO,
        "Ratio",
        ":1",
        1.0,
        20.0,
        Taper::Linear,
        4.0,
        0,
        &[],
        ratio_text,
        ratio_parse,
    ),
    param(
        ATTACK,
        "Attack",
        "ms",
        0.05,
        200.0,
        Taper::Power(2.0),
        5.0,
        0,
        &[],
        ms_text,
        ms_parse,
    ),
    param(
        RELEASE,
        "Release",
        "ms",
        5.0,
        2000.0,
        Taper::Power(2.0),
        120.0,
        0,
        &[],
        ms_text,
        ms_parse,
    ),
    param(
        KNEE,
        "Knee",
        "dB",
        0.0,
        24.0,
        Taper::Linear,
        6.0,
        0,
        &[],
        knee_text,
        knee_parse,
    ),
    param(
        MAKEUP,
        "Makeup",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.5,
        2,
        &["Off", "+6 dB", "Auto"],
        makeup_text,
        makeup_parse,
    ),
    param(
        MIX,
        "Mix",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        100.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        DETECT,
        "Detect",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &["Peak", "RMS"],
        peak_rms_text,
        peak_rms_parse,
    ),
    param(
        KEY_HPF,
        "Key HPF",
        "Hz",
        0.0,
        1000.0,
        Taper::Power(2.0),
        0.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        LISTEN,
        "Listen",
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

pub fn curve_db(level_db: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let ratio = ratio.max(1.0);
    let over = level_db - threshold;
    let half = (knee.max(0.0)) / 2.0;
    if half > 0.0 && over.abs() < half {
        level_db + (1.0 / ratio - 1.0) * (over + half).powi(2) / (2.0 * knee)
    } else if over > 0.0 {
        threshold + over / ratio
    } else {
        level_db
    }
}

fn auto_makeup_db(threshold: f32, ratio: f32) -> f32 {
    (-threshold * (1.0 - 1.0 / ratio.max(1.0))).clamp(0.0, MOST_MAKEUP_DB)
}

fn makeup_gain(choice: f32, auto_db: f32) -> f32 {
    let fixed = (1.0 - (choice - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let automatic = ((choice - 0.5) * 2.0).clamp(0.0, 1.0);
    fixed * FIXED_MAKEUP_DB + automatic * auto_db
}

fn db_parse(text: &str) -> Option<f32> {
    super::number_in(text, &["db"]).filter(|db| (FLOOR..=0.0).contains(db))
}

fn ratio_text(ratio: f32) -> String {
    format!("{ratio:.1}:1")
}

fn ratio_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-");
    let number = t.split(':').next().unwrap_or(&t).trim().to_owned();
    super::number_in(&number, &[]).filter(|ratio| (1.0..=20.0).contains(ratio))
}

fn knee_text(db: f32) -> String {
    format!("{db:.1} dB")
}

fn knee_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    super::number_in(&t, &["db"])
}

fn makeup_text(value: f32) -> String {
    if value >= 0.75 {
        "Auto".to_owned()
    } else if value >= 0.25 {
        "+6 dB".to_owned()
    } else {
        "Off".to_owned()
    }
}

fn makeup_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    if t.starts_with("auto") {
        return Some(1.0);
    }
    t.contains('6').then_some(0.5)
}

fn peak_rms_text(value: f32) -> String {
    word_text(value, "Peak", "RMS")
}

fn peak_rms_parse(text: &str) -> Option<f32> {
    word_of(text, &["Peak", "RMS"])
}

fn on_off_text(value: f32) -> String {
    word_text(value, "Off", "On")
}

fn on_off_parse(text: &str) -> Option<f32> {
    word_of(text, &["Off", "On"])
}

fn word_text(value: f32, low: &str, high: &str) -> String {
    if value >= 0.5 { high } else { low }.to_owned()
}

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    super::number_in(text, &["%"])
}

fn hz_text(hz: f32) -> String {
    if hz < 0.5 {
        "Off".to_owned()
    } else {
        format!("{hz:.0} Hz")
    }
}

fn hz_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    super::number_in(&t, &["hz"])
}

struct Curves {
    threshold: [f32; crate::MOST_FRAMES],
    ratio: [f32; crate::MOST_FRAMES],
    attack: [f32; crate::MOST_FRAMES],
    release: [f32; crate::MOST_FRAMES],
    knee: [f32; crate::MOST_FRAMES],
    makeup: [f32; crate::MOST_FRAMES],
    mix: [f32; crate::MOST_FRAMES],
    detect: [f32; crate::MOST_FRAMES],
    key_hpf: [f32; crate::MOST_FRAMES],
    listen: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
        Box::new(Self {
            threshold: [0.0; crate::MOST_FRAMES],
            ratio: [0.0; crate::MOST_FRAMES],
            attack: [0.0; crate::MOST_FRAMES],
            release: [0.0; crate::MOST_FRAMES],
            knee: [0.0; crate::MOST_FRAMES],
            makeup: [0.0; crate::MOST_FRAMES],
            mix: [0.0; crate::MOST_FRAMES],
            detect: [0.0; crate::MOST_FRAMES],
            key_hpf: [0.0; crate::MOST_FRAMES],
            listen: [0.0; crate::MOST_FRAMES],
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct HighPass {
    first_out: f32,
    second_out: f32,
}

impl HighPass {
    fn run(&mut self, x: f32, coef: f32) -> f32 {
        if coef <= 0.0 {
            return x;
        }
        self.first_out = flush(self.first_out + coef * (x - self.first_out));
        let high = x - self.first_out;
        self.second_out = flush(self.second_out + coef * (high - self.second_out));
        high - self.second_out
    }
}

#[derive(Debug, Clone, Copy)]
struct Peak {
    level: f32,
    candidate: f32,
    candidate_at: f64,
    last_peak_at: f64,
    hold_until: f64,
    interval: f64,
    now: f64,
    armed: bool,
}

impl Default for Peak {
    fn default() -> Self {
        Self {
            level: 0.0,
            candidate: 0.0,
            candidate_at: 0.0,
            last_peak_at: f64::NEG_INFINITY,
            hold_until: -1.0,
            interval: PEAK_HOLD_LEAST,
            now: 0.0,
            armed: false,
        }
    }
}

impl Peak {
    fn run(&mut self, sample: f32, most: f64) -> f32 {
        let now = self.now;
        if sample > self.candidate {
            self.candidate = sample;
            self.candidate_at = now;
            self.armed = true;
        } else if self.armed
            && sample < self.candidate * (1.0 - PEAK_FALL)
            && now - self.candidate_at <= most
        {
            self.interval = if now - self.last_peak_at <= most {
                (self.candidate_at - self.last_peak_at).clamp(PEAK_HOLD_LEAST, most)
            } else {
                PEAK_HOLD_LEAST
            };
            self.last_peak_at = self.candidate_at;
            self.hold_until = now + self.interval * (1.0 + PEAK_HOLD_MARGIN) + PEAK_HOLD_LEAST;
            self.level = flush(self.level.max(self.candidate));
            self.candidate = sample;
            self.candidate_at = now;
            self.armed = false;
        }
        if sample >= self.level || now >= self.hold_until {
            self.level = flush(sample);
        }
        self.now += 1.0;
        self.level
    }
}

pub struct Compressor {
    threshold: Glide,
    ratio: Glide,
    attack: Glide,
    release: Glide,
    knee: Glide,
    makeup: Glide,
    mix: Glide,
    detect: Glide,
    key_hpf: Glide,
    listen: Glide,
    curves: Box<Curves>,
    gain_db: f32,
    mean_square: f32,
    peak: Peak,
    key_filter: [HighPass; 2],
    key_tuning: (f32, f32),
    meter: Reduction,
}

impl Compressor {
    pub fn new() -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        Self {
            threshold: Glide::new(at(THRESHOLD)),
            ratio: Glide::new(at(RATIO)),
            attack: Glide::new(at(ATTACK)),
            release: Glide::new(at(RELEASE)),
            knee: Glide::new(at(KNEE)),
            makeup: Glide::new(at(MAKEUP)),
            mix: Glide::new(at(MIX)),
            detect: Glide::new(at(DETECT)),
            key_hpf: Glide::new(at(KEY_HPF)),
            listen: Glide::new(at(LISTEN)),
            curves: Curves::new(),
            gain_db: 0.0,
            mean_square: 0.0,
            peak: Peak::default(),
            key_filter: [HighPass::default(); 2],
            key_tuning: (0.0, 0.0),
            meter: Reduction::new(),
        }
    }

    pub fn meter(&self) -> Reduction {
        self.meter.clone()
    }

    pub fn build() -> (Self, Reduction) {
        let device = Self::new();
        let meter = device.meter();
        (device, meter)
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Compressor {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate;
        let events = block.events;
        let curves = &mut self.curves;
        params::values(
            &mut curves.threshold[..frames],
            &mut self.threshold,
            events,
            THRESHOLD,
            rate,
        );
        params::values(
            &mut curves.ratio[..frames],
            &mut self.ratio,
            events,
            RATIO,
            rate,
        );
        params::values(
            &mut curves.attack[..frames],
            &mut self.attack,
            events,
            ATTACK,
            rate,
        );
        params::values(
            &mut curves.release[..frames],
            &mut self.release,
            events,
            RELEASE,
            rate,
        );
        params::values(
            &mut curves.knee[..frames],
            &mut self.knee,
            events,
            KNEE,
            rate,
        );
        params::values(
            &mut curves.makeup[..frames],
            &mut self.makeup,
            events,
            MAKEUP,
            rate,
        );
        params::values(&mut curves.mix[..frames], &mut self.mix, events, MIX, rate);
        params::values(
            &mut curves.detect[..frames],
            &mut self.detect,
            events,
            DETECT,
            rate,
        );
        params::values(
            &mut curves.key_hpf[..frames],
            &mut self.key_hpf,
            events,
            KEY_HPF,
            rate,
        );
        params::values(
            &mut curves.listen[..frames],
            &mut self.listen,
            events,
            LISTEN,
            rate,
        );

        let mean_keep = 1.0 - reach(RMS_SECONDS, rate);
        let most = (f64::from(rate) * PEAK_MOST_SECONDS).max(PEAK_HOLD_LEAST);
        let (mut hpf, mut key_coef) = self.key_tuning;
        let (mut last_threshold, mut threshold, mut makeup_db) = (f32::NAN, 0.0f32, 0.0f32);
        let mut last_ratio = f32::NAN;
        let (mut last_attack, mut attack) = (f32::NAN, 1.0f32);
        let (mut last_release, mut release) = (f32::NAN, 1.0f32);
        let (mut last_knee, mut knee) = (f32::NAN, 0.0f32);
        let (mut last_mix, mut mix) = (f32::NAN, 1.0f32);
        let mut mean_square = self.mean_square;
        let mut gain_db = self.gain_db;
        let mut most_reduction = 0.0f32;
        let mut input_db = FLOOR_DB;

        for frame in 0..frames {
            if (block.start + frame as i64).rem_euclid(KNOT) == 0 {
                hpf = value(TABLE, KEY_HPF, curves.key_hpf[frame]);
                key_coef = one_pole(hpf, rate);
            }
            let threshold_now = value(TABLE, THRESHOLD, curves.threshold[frame]);
            let ratio_now = value(TABLE, RATIO, curves.ratio[frame]);
            if threshold_now != last_threshold || ratio_now != last_ratio {
                threshold = threshold_now;
                makeup_db = auto_makeup_db(threshold_now, ratio_now);
                last_threshold = threshold_now;
                last_ratio = ratio_now;
            }
            let attack_now = value(TABLE, ATTACK, curves.attack[frame]);
            if attack_now != last_attack {
                attack = reach(attack_now / 1000.0, rate);
                last_attack = attack_now;
            }
            let release_now = value(TABLE, RELEASE, curves.release[frame]);
            if release_now != last_release {
                release = reach(release_now / 1000.0, rate);
                last_release = release_now;
            }
            let knee_now = value(TABLE, KNEE, curves.knee[frame]);
            if knee_now != last_knee {
                knee = knee_now;
                last_knee = knee_now;
            }
            let makeup = makeup_gain(curves.makeup[frame], makeup_db);

            let (sample_l, sample_r) = match key {
                Some(key) if frame < key.left.len() && frame < key.right.len() => (
                    self.key_filter[0].run(key.left[frame], key_coef),
                    self.key_filter[1].run(key.right[frame], key_coef),
                ),
                _ => (left[frame], right[frame]),
            };
            let peak = self.peak.run(sample_l.abs().max(sample_r.abs()), most);
            let mean = 0.5 * (sample_l * sample_l + sample_r * sample_r);
            mean_square = flush(mean_square * mean_keep + mean * (1.0 - mean_keep));
            let rms = mean_square.sqrt();

            let detect = curves.detect[frame];
            let level = peak + (rms - peak) * detect;
            let level_db = db_of(level);
            input_db = input_db.max(level_db);

            let want = curve_db(level_db, threshold, ratio_now, knee) - level_db;
            let coef = if want < gain_db { attack } else { release };
            gain_db += (want - gain_db) * coef;
            gain_db = gain_db.min(0.0);
            most_reduction = most_reduction.max(-gain_db);

            let mix_now = curves.mix[frame];
            if mix_now != last_mix {
                mix = value(TABLE, MIX, mix_now) / 100.0;
                last_mix = mix_now;
            }
            let gain = super::gain_of(gain_db + makeup);
            let mut l = left[frame];
            let mut r = right[frame];
            l += (left[frame] * gain - l) * mix;
            r += (right[frame] * gain - r) * mix;
            let listen = curves.listen[frame];
            if listen > 0.0 {
                l += sample_l * listen;
                r += sample_r * listen;
            }
            left[frame] = l;
            right[frame] = r;
        }
        self.mean_square = mean_square;
        self.gain_db = gain_db;
        self.key_tuning = (hpf, key_coef);
        self.meter.record(most_reduction, input_db);
    }

    fn reset(&mut self) {
        for glide in [
            &mut self.threshold,
            &mut self.ratio,
            &mut self.attack,
            &mut self.release,
            &mut self.knee,
            &mut self.makeup,
            &mut self.mix,
            &mut self.detect,
            &mut self.key_hpf,
            &mut self.listen,
        ] {
            glide.arrive();
        }
        self.gain_db = 0.0;
        self.mean_square = 0.0;
        self.peak = Peak::default();
        self.key_filter = [HighPass::default(); 2];
        self.key_tuning = (0.0, 0.0);
    }

    fn tail(&self) -> u32 {
        tail(&[])
    }

    fn latency(&self) -> u32 {
        latency(&[])
    }
}

impl Compressor {
    pub fn reading(&self) -> Reading {
        self.meter.peek()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_is_straight_below_the_knee_and_a_ratio_above_it() {
        for ratio in [1.0f32, 2.0, 4.0, 20.0] {
            assert_eq!(curve_db(-40.0, -18.0, ratio, 0.0), -40.0);
            let over = curve_db(-8.0, -18.0, ratio, 0.0);
            assert!((over - (-18.0 + 10.0 / ratio)).abs() < 1e-6, "{over}");
        }
        for level in [-60.0f32, -20.0, 0.0] {
            assert!((curve_db(level, -18.0, 1.0, 6.0) - level).abs() < 1e-6);
        }
    }

    #[test]
    fn a_soft_knee_joins_both_sections() {
        let (threshold, ratio, knee) = (-18.0f32, 4.0f32, 6.0f32);
        let lower = threshold - knee / 2.0;
        let upper = threshold + knee / 2.0;
        let step = 1e-3;
        for (level, want) in [
            (lower, lower),
            (upper, threshold + (upper - threshold) / ratio),
        ] {
            let got = curve_db(level, threshold, ratio, knee);
            assert!(
                (got - want).abs() < 1e-4,
                "at {level}: {got} against {want}"
            );
            let inside = (curve_db(level + step, threshold, ratio, knee)
                - curve_db(level - step, threshold, ratio, knee))
                / (2.0 * step);
            let want_slope = if level == upper { 1.0 / ratio } else { 1.0 };
            assert!(
                (inside - want_slope).abs() < 1e-2,
                "slope at {level}: {inside} against {want_slope}"
            );
        }
        let mut last = f32::NEG_INFINITY;
        for k in 0..=1200 {
            let level = -60.0 + k as f32 * 0.05;
            let out = curve_db(level, threshold, ratio, knee);
            assert!(
                out >= last - 1e-5 && out <= level + 1e-5,
                "{level} -> {out}"
            );
            last = out;
        }
    }

    #[test]
    fn automatic_make_up_is_what_the_loud_end_lost() {
        assert!((auto_makeup_db(-18.0, 4.0) - 13.5).abs() < 1e-4);
        assert_eq!(auto_makeup_db(0.0, 4.0), 0.0);
        assert_eq!(auto_makeup_db(-60.0, 20.0), MOST_MAKEUP_DB);
        assert_eq!(auto_makeup_db(-18.0, 1.0), 0.0);
    }

    #[test]
    fn the_makeup_choice_is_off_six_decibels_or_automatic() {
        assert_eq!(makeup_gain(0.0, 13.5), 0.0);
        assert!((makeup_gain(0.5, 13.5) - FIXED_MAKEUP_DB).abs() < 1e-6);
        assert!((makeup_gain(1.0, 13.5) - 13.5).abs() < 1e-6);
        assert!((makeup_gain(0.25, 13.5) - 3.0).abs() < 1e-6);
        assert_eq!(makeup_text(0.0), "Off");
        assert_eq!(makeup_text(0.5), "+6 dB");
        assert_eq!(makeup_text(1.0), "Auto");
        assert_eq!(makeup_parse("off"), Some(0.0));
        assert_eq!(makeup_parse("+6 dB"), Some(0.5));
        assert_eq!(makeup_parse("auto"), Some(1.0));
    }

    #[test]
    fn the_ratio_reads_and_is_read_back_with_its_colon() {
        assert_eq!(ratio_text(4.0), "4.0:1");
        assert_eq!(ratio_parse("4:1"), Some(4.0));
        assert_eq!(ratio_parse("4.0 : 1"), Some(4.0));
        assert_eq!(ratio_parse("2"), Some(2.0));
        assert_eq!(ratio_parse("0.5"), None);
        assert_eq!(db_parse("\u{2212}18 dB"), Some(-18.0));
        assert_eq!(db_parse("loud"), None);
        assert_eq!(knee_parse("off"), Some(0.0));
        assert_eq!(hz_parse("120 Hz"), Some(120.0));
    }
}
