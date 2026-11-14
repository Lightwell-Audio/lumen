//! Gravity, the compressor (M8 slice B2): a soft-knee gain computer on a peak
//! or mean-square detector, keyed from the track's sidechain when it has one,
//! with an automatic make-up gain, parallel mix and a listen button.
//!
//! Everything it does to the audio is smooth by construction: a value the
//! interface moved glides over 20 ms, a step in a lane is moved over a
//! millisecond, and the gain itself is smoothed with the attack and release
//! times, so nothing it does clicks. It allocates nothing after its
//! constructor and flushes the detector's denormals.
//!
//! The **static curve** ([`curve_db`]) is a pure function, pinned against the
//! textbook soft-knee formula by a test; it is what the card's face draws.

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

use super::{
    FLOOR_DB, Reading, Reduction, db_of, flush, level_text, ms_parse, ms_text, one_pole, reach,
    value, word_of,
};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "compressor";

/// The level above which it starts to pull the signal down, in dBFS.
pub const THRESHOLD: u32 = 1;
/// How far over the threshold it compresses, as a ratio to one.
pub const RATIO: u32 = 2;
/// How fast it takes hold, in milliseconds.
pub const ATTACK: u32 = 3;
/// How fast it lets go, in milliseconds.
pub const RELEASE: u32 = 4;
/// The width of the soft knee, in decibels (0 is a hard knee).
pub const KNEE: u32 = 5;
/// Whether the gain taken away is made up automatically.
pub const MAKEUP: u32 = 6;
/// How much of the compressed signal is heard against the plain one.
pub const MIX: u32 = 7;
/// Peak or RMS detection.
pub const DETECT: u32 = 8;
/// A high-pass on the key, in Hz (0 is off).
pub const KEY_HPF: u32 = 9;
/// Plays what keys it, over the output.
pub const LISTEN: u32 = 10;

/// The lowest level the detector works at: under it, silence.
const FLOOR: f32 = -120.0;

/// The window a mean-square detector averages over: 10 ms.
const RMS_SECONDS: f32 = 0.010;

/// The most make-up gain automatic make-up gives: +24 dB.
const MOST_MAKEUP_DB: f32 = 24.0;

/// Frames between two readings of the key filter's corner: the mixer's
/// automation knots, so the reading falls on the same frames at any block
/// size.
const KNOT: i64 = 32;

/// What the columns of the table are for, so a reader sees the shape of a row.
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

/// Gravity's parameters, in the order the card shows them: the four the face's
/// controls are (and so the four the compact card keeps), then the rest.
/// Ids are stable and never reused.
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
        1.0,
        1,
        &["Off", "Auto"],
        off_auto_text,
        off_auto_parse,
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

/// Gravity's description: its table, no latency (a compressor reports what it
/// does as it happens) and no tail.
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// Frames Gravity's output trails its input: none, whatever its values say.
pub fn latency(_values: Values<'_>) -> u32 {
    0
}

/// Frames Gravity rings on after its input stops: none.
pub fn tail(_values: Values<'_>) -> u32 {
    0
}

/// The gain computer: the level that comes out for an input at `level_db`, a
/// threshold, a ratio and a soft knee `knee` decibels wide.
///
/// Below the knee it does nothing; over it the level rises by one decibel of
/// output for every `ratio` of input; between the two the two meet with a
/// continuous slope (the quadratic of the standard soft-knee form, whose two
/// halves join the straight sections at both knee edges).
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

/// The make-up gain automatic make-up gives for a threshold and a ratio: the
/// level a full-scale signal would have lost, so the loud end stays put.
fn auto_makeup_db(threshold: f32, ratio: f32) -> f32 {
    (-threshold * (1.0 - 1.0 / ratio.max(1.0))).clamp(0.0, MOST_MAKEUP_DB)
}

/// "−18.0 dB", "0.0 dB".
fn db_parse(text: &str) -> Option<f32> {
    super::number_in(text, &["db"]).filter(|db| (FLOOR..=0.0).contains(db))
}

/// "4.0:1".
fn ratio_text(ratio: f32) -> String {
    format!("{ratio:.1}:1")
}

/// "4:1", "4.0 : 1", "4".
fn ratio_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-");
    let number = t.split(':').next().unwrap_or(&t).trim().to_owned();
    super::number_in(&number, &[]).filter(|ratio| (1.0..=20.0).contains(ratio))
}

/// "6.0 dB".
fn knee_text(db: f32) -> String {
    format!("{db:.1} dB")
}

/// "6", "6 dB", "off".
fn knee_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    super::number_in(&t, &["db"])
}

/// "Off", "Auto".
fn off_auto_text(value: f32) -> String {
    word_text(value, "Off", "Auto")
}

fn off_auto_parse(text: &str) -> Option<f32> {
    word_of(text, &["Off", "Auto"])
}

/// "Peak", "RMS".
fn peak_rms_text(value: f32) -> String {
    word_text(value, "Peak", "RMS")
}

fn peak_rms_parse(text: &str) -> Option<f32> {
    word_of(text, &["Peak", "RMS"])
}

/// "Off", "On".
fn on_off_text(value: f32) -> String {
    word_text(value, "Off", "On")
}

fn on_off_parse(text: &str) -> Option<f32> {
    word_of(text, &["Off", "On"])
}

fn word_text(value: f32, low: &str, high: &str) -> String {
    if value >= 0.5 { high } else { low }.to_owned()
}

/// "100 %".
fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    super::number_in(text, &["%"])
}

/// "Off", "120 Hz".
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

/// The parameters' values through one block, normalised.
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

/// One side of a two-pole key high-pass: two one-poles in series, each with
/// its own last input and output.
#[derive(Debug, Clone, Copy, Default)]
struct HighPass {
    first_out: f32,
    second_out: f32,
}

impl HighPass {
    /// Runs one sample through at a one-pole coefficient `coef` (0 is off).
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

/// Gravity.
pub struct Compressor {
    /// Where each parameter was at the end of the last block.
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
    /// The block's curves, worked out once each block.
    curves: Box<Curves>,
    /// The gain currently applied, in decibels (0 or less): what the attack
    /// and release times smooth towards the gain computer's answer.
    gain_db: f32,
    /// The mean-square detector's state.
    mean_square: f32,
    /// The key's high-pass, per side.
    key_filter: [HighPass; 2],
    /// The key filter's corner (in Hz) and one-pole coefficient, set at the
    /// last knot.
    key_tuning: (f32, f32),
    /// The gain-reduction meter the card's face reads.
    meter: Reduction,
}

impl Compressor {
    /// Gravity at its defaults: −18 dB, 4:1, a 6 dB knee, 5 ms of attack,
    /// 120 ms of release, automatic make-up, fully compressed, peak detection,
    /// no key filter and not listening. Allocates (its curves): call it on the
    /// interface side.
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
            key_filter: [HighPass::default(); 2],
            key_tuning: (0.0, 0.0),
            meter: Reduction::new(),
        }
    }

    /// The meter the card's face reads: a copy of the one the audio thread
    /// writes.
    pub fn meter(&self) -> Reduction {
        self.meter.clone()
    }

    /// A device by name, with its meter, so the interface can build one in one
    /// step.
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
        // The key filter's corner is read at knots of the song's own clock,
        // every [`KNOT`] frames, as Prism reads its mono fold: a corner that a
        // lane moves is a coefficient, and the same knots fall at any block
        // size.
        let (mut hpf, mut key_coef) = self.key_tuning;
        // What does not move is worked out once: a knob still costs nothing.
        let (mut last_threshold, mut threshold, mut makeup_db, mut last_makeup) =
            (f32::NAN, 0.0f32, 0.0f32, f32::NAN);
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
            let makeup = curves.makeup[frame];
            if makeup != last_makeup {
                last_makeup = makeup;
            }

            // The detector: the key when the track has one, its own input
            // otherwise, high-passed when a corner is set. A key shorter than
            // the block keys only the frames it has (the engine gives a device
            // the key's own bus, but a device never trusts a length it is
            // handed).
            let (sample_l, sample_r) = match key {
                Some(key) if frame < key.left.len() && frame < key.right.len() => (
                    self.key_filter[0].run(key.left[frame], key_coef),
                    self.key_filter[1].run(key.right[frame], key_coef),
                ),
                _ => (left[frame], right[frame]),
            };
            let peak = sample_l.abs().max(sample_r.abs());
            // The mean square of the summed sides, as a stereo meter reads it.
            let mean = 0.5 * (sample_l * sample_l + sample_r * sample_r);
            mean_square = flush(mean_square * mean_keep + mean * (1.0 - mean_keep));
            let rms = mean_square.sqrt();

            let detect = curves.detect[frame];
            let level = peak + (rms - peak) * detect;
            let level_db = db_of(level);
            input_db = input_db.max(level_db);

            // The gain computer, and the ballistics that reach it.
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
            let gain = super::gain_of(gain_db + makeup_db * makeup);
            let mut l = left[frame];
            let mut r = right[frame];
            l += (left[frame] * gain - l) * mix;
            r += (right[frame] * gain - r) * mix;
            // Listen plays what keys it, after the key's own filter.
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

/// What the detector last read, for a test of the meter: the reduction the
/// face shows, as the audio thread published it.
impl Compressor {
    /// The reading the meter holds, without reading it.
    pub fn reading(&self) -> Reading {
        self.meter.peek()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_is_straight_below_the_knee_and_a_ratio_above_it() {
        // No knee: unity under the threshold, one to `ratio` over it.
        for ratio in [1.0f32, 2.0, 4.0, 20.0] {
            assert_eq!(curve_db(-40.0, -18.0, ratio, 0.0), -40.0);
            let over = curve_db(-8.0, -18.0, ratio, 0.0);
            assert!((over - (-18.0 + 10.0 / ratio)).abs() < 1e-6, "{over}");
        }
        // A ratio of one is a pass-through everywhere.
        for level in [-60.0f32, -20.0, 0.0] {
            assert!((curve_db(level, -18.0, 1.0, 6.0) - level).abs() < 1e-6);
        }
    }

    /// The soft knee joins the two straight sections at both of its edges, with
    /// the same value and the same slope: no step, no crease.
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
            // The slope from inside the knee is the straight section's.
            let inside = (curve_db(level + step, threshold, ratio, knee)
                - curve_db(level - step, threshold, ratio, knee))
                / (2.0 * step);
            let want_slope = if level == upper { 1.0 / ratio } else { 1.0 };
            assert!(
                (inside - want_slope).abs() < 1e-2,
                "slope at {level}: {inside} against {want_slope}"
            );
        }
        // And it is monotone and never above the input.
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
        // −18 dB at 4:1 loses 13.5 dB at full scale.
        assert!((auto_makeup_db(-18.0, 4.0) - 13.5).abs() < 1e-4);
        assert_eq!(auto_makeup_db(0.0, 4.0), 0.0);
        assert_eq!(auto_makeup_db(-60.0, 20.0), MOST_MAKEUP_DB);
        assert_eq!(auto_makeup_db(-18.0, 1.0), 0.0);
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
