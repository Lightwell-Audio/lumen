//! Horizon, the limiter (M8 slice B2): a look-ahead limiter with a ceiling, a
//! release, a reported lookahead (delay compensation lines the track up with
//! the rest of the song) and a true-peak mode that measures the peaks between
//! the samples.
//!
//! The signal is delayed by the lookahead and played through the average of
//! the gain each frame of that window needs, so the gain has reached its
//! target by the time a peak arrives, and the average of values no larger than
//! a peak's own is never larger than it: **nothing leaves above the ceiling**.
//! The average's rises are then smoothed by the release time, which never
//! raises it above that average, so the guarantee holds with a release set.
//!
//! It allocates nothing after its constructor and flushes its denormals.

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

use super::{
    FLOOR_DB, Reading, Reduction, db_of, frames_of_ms, gain_of, held, level_text, number_in, value,
    word_of,
};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "limiter";

/// The level nothing leaves above, in dBFS.
pub const CEILING: u32 = 1;
/// How fast it lets go of a peak, in milliseconds.
pub const RELEASE: u32 = 2;
/// How far ahead it looks, in milliseconds; its reported latency.
pub const LOOK: u32 = 3;
/// Peak detection on the samples, or on the peaks between them.
pub const DETECT: u32 = 4;

/// The most lookahead the table offers, and so the longest delay line it can
/// need: 10 ms at the reference rate (48 kHz).
const MOST_LOOK_FRAMES: usize = 480;

/// The rough length of the delay line, in frames: the most lookahead, with the
/// room the sliding window needs.
const LINE: usize = MOST_LOOK_FRAMES + 2;

/// Frames the line holds back at `values`: the lookahead, the one number the
/// routing's compensation and the running device agree on.
pub fn latency(values: Values<'_>) -> u32 {
    frames_of_ms(value(
        TABLE,
        LOOK,
        params::normalised_of(TABLE, LOOK, values),
    ))
}

/// Frames Horizon rings on after its input stops: its delay line, which is as
/// long as its lookahead.
pub fn tail(values: Values<'_>) -> u32 {
    latency(values)
}

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

/// Horizon's parameters, in the order the card shows them: exactly the four
/// its face draws.
pub static TABLE: &[ParamInfo] = &[
    param(
        CEILING,
        "Ceiling",
        "dB",
        -24.0,
        0.0,
        Taper::Linear,
        -0.3,
        0,
        &[],
        level_text,
        ceiling_parse,
    ),
    param(
        RELEASE,
        "Release",
        "ms",
        10.0,
        1000.0,
        Taper::Power(2.0),
        60.0,
        0,
        &[],
        ms_text,
        ms_parse,
    ),
    param(
        LOOK,
        "Look",
        "ms",
        0.0,
        10.0,
        Taper::Linear,
        1.5,
        0,
        &[],
        look_text,
        look_parse,
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
        &["Sample", "True"],
        sample_true_text,
        sample_true_parse,
    ),
];

/// Horizon's description: its table, and a lookahead that is its latency and
/// its tail.
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// "−0.3 dB".
fn ceiling_parse(text: &str) -> Option<f32> {
    number_in(text, &["db"]).filter(|db| (-24.0..=0.0).contains(db))
}

/// Milliseconds, as a limiter reads them: "60 ms", "12.5 ms".
fn ms_text(ms: f32) -> String {
    if ms < 10.0 {
        format!("{ms:.2} ms")
    } else if ms < 100.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

fn ms_parse(text: &str) -> Option<f32> {
    number_in(text, &["ms", "milliseconds"])
}

/// "1.50 ms".
fn look_text(ms: f32) -> String {
    format!("{ms:.2} ms")
}

fn look_parse(text: &str) -> Option<f32> {
    number_in(text, &["ms", "milliseconds"])
}

/// "Sample", "True".
fn sample_true_text(value: f32) -> String {
    if value >= 0.5 {
        "True".to_owned()
    } else {
        "Sample".to_owned()
    }
}

fn sample_true_parse(text: &str) -> Option<f32> {
    word_of(text, &["Sample", "True"])
}

/// Oversampling for the true-peak detector: eight phases of a windowed sinc,
/// so a peak between the samples is found even for a tone near the top of the
/// band (four phases are a fifth of a decibel short at 19 kHz).
const PHASES: usize = 8;
/// Taps per phase: eleven, enough for the windowed sinc to sit on the waveform
/// within a twentieth of a decibel across the band with eight phases.
const TAPS: usize = 11;

/// The interpolation filter: a Kaiser-windowed sinc, `PHASES` phases of `TAPS`
/// taps each, every phase summing to one (the same design the engine's
/// loudness meter uses, with more phases so a tone near the top of the band is
/// not a fifth of a decibel short).
fn interpolator() -> [[f32; TAPS]; PHASES] {
    let length = PHASES * (TAPS - 1) + 1;
    let centre = (length - 1) as f64 / 2.0;
    let beta = 8.0;
    let i0 = |x: f64| {
        // Modified Bessel function of the first kind, order zero.
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)).powi(2);
            sum += term;
            k += 1.0;
        }
        sum
    };
    let mut phases = [[0.0f32; TAPS]; PHASES];
    for (p, phase) in phases.iter_mut().enumerate() {
        let mut taps = [0.0f64; TAPS];
        for (j, tap) in taps.iter_mut().enumerate() {
            let n = j * PHASES + p;
            if n >= length {
                continue;
            }
            let x = (n as f64 - centre) / PHASES as f64;
            let sinc = if x == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let r = (n as f64 - centre) / centre;
            *tap = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
        }
        let sum: f64 = taps.iter().sum();
        for (out, tap) in phase.iter_mut().zip(taps) {
            *out = (tap / sum) as f32;
        }
    }
    phases
}

/// Finds the peaks between the samples, four times over.
struct TruePeak {
    /// Each phase's taps, oldest sample first.
    phases: [[f32; TAPS]; PHASES],
    /// The last `TAPS` samples, written twice so the window ending at any
    /// position is one contiguous slice.
    history: [f32; 2 * TAPS],
    at: usize,
}

impl TruePeak {
    fn new() -> Self {
        // The taps run oldest sample first, as the window does.
        let mut phases = interpolator();
        for phase in &mut phases {
            phase.reverse();
        }
        Self {
            phases,
            history: [0.0; 2 * TAPS],
            at: 0,
        }
    }

    /// The highest of the sample and the points between it and the samples
    /// before it, linear.
    fn peak(&mut self, x: f32) -> f32 {
        self.at = if self.at + 1 == TAPS { 0 } else { self.at + 1 };
        self.history[self.at] = x;
        self.history[self.at + TAPS] = x;
        let start = self.at + 1;
        let window: &[f32; TAPS] = self
            .history
            .get(start..start + TAPS)
            .and_then(|w| w.try_into().ok())
            .unwrap_or(&[0.0; TAPS]);
        let mut peak = x.abs();
        for phase in &self.phases {
            let y: f32 = phase.iter().zip(window).map(|(h, s)| h * s).sum();
            peak = peak.max(y.abs());
        }
        peak
    }

    fn reset(&mut self) {
        self.history = [0.0; 2 * TAPS];
        self.at = 0;
    }
}

/// The parameters' values through one block, normalised.
struct Curves {
    ceiling: [f32; crate::MOST_FRAMES],
    release: [f32; crate::MOST_FRAMES],
    detect: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
        Box::new(Self {
            ceiling: [0.0; crate::MOST_FRAMES],
            release: [0.0; crate::MOST_FRAMES],
            detect: [0.0; crate::MOST_FRAMES],
        })
    }
}

/// Horizon.
pub struct Limiter {
    /// Where each held parameter was at the end of the last block.
    ceiling: Glide,
    release: Glide,
    detect: Glide,
    /// The block's curves, worked out once each block.
    curves: Box<Curves>,
    /// The lookahead the delay line is running at, in frames: the value the
    /// project holds, read without a glide so it is the number the routing was
    /// told about.
    look_frames: usize,
    /// The signal, delayed by the lookahead, per side.
    line: [Vec<f32>; 2],
    /// The sliding minimum over the lookahead's window: the frames whose
    /// requirement is the smallest of those still in it, oldest first (a
    /// monotonic deque, so a frame costs nothing on average).
    least: Vec<(u64, f32)>,
    front: usize,
    back: usize,
    /// Frames written since the device was made, which names the window.
    clock: u64,
    /// Where the newest frame is written in the delay line.
    at: usize,
    /// The gain currently applied: the window's smallest need, or a release on
    /// its way back up to it.
    gain: f32,
    /// The true-peak detectors, per side.
    peaks: [TruePeak; 2],
    /// The gain-reduction meter the card's face reads.
    meter: Reduction,
}

impl Limiter {
    /// Horizon at its defaults: a ceiling of −0.3 dB, a 60 ms release, 1.5 ms
    /// of lookahead and sample-peak detection. Allocates its delay line: call
    /// it on the interface side.
    pub fn new() -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        let look_frames = frames_of_ms(value(TABLE, LOOK, at(LOOK))) as usize;
        Self {
            ceiling: Glide::new(at(CEILING)),
            release: Glide::new(at(RELEASE)),
            detect: Glide::new(at(DETECT)),
            curves: Curves::new(),
            look_frames,
            line: [vec![0.0; LINE], vec![0.0; LINE]],
            least: vec![(0, 1.0); LINE],
            front: 0,
            back: 0,
            clock: 0,
            at: 0,
            gain: 1.0,
            peaks: [TruePeak::new(), TruePeak::new()],
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

    /// The frames of lookahead it is running at.
    pub fn lookahead(&self) -> usize {
        self.look_frames
    }

    /// The reading the meter holds, without reading it.
    pub fn reading(&self) -> Reading {
        self.meter.peek()
    }

    /// Sets the lookahead. The window the minimum is taken over moves with it;
    /// frames that fall out of it are dropped as the next frames are written.
    fn set_lookahead(&mut self, frames: usize) {
        self.look_frames = frames.min(MOST_LOOK_FRAMES);
    }

    /// How many frames the gain's window covers: the lookahead's, and the one
    /// being written.
    fn window(&self) -> u64 {
        self.look_frames as u64 + 1
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Limiter {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, _key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate;
        let events = block.events;
        // The lookahead is the device's timing, not a shape: read straight
        // from the values the block carries, so it is exactly the number the
        // routing compensates. It is read before the curves are borrowed: a
        // changed lookahead starts the window's sum again.
        if let Some(x) = held(events, LOOK) {
            self.set_lookahead(frames_of_ms(value(TABLE, LOOK, x)) as usize);
        }
        let window = self.window();
        let curves = &mut self.curves;
        params::values(
            &mut curves.ceiling[..frames],
            &mut self.ceiling,
            events,
            CEILING,
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
            &mut curves.detect[..frames],
            &mut self.detect,
            events,
            DETECT,
            rate,
        );
        let mut gain = self.gain;
        let mut at = self.at;
        let mut clock = self.clock;
        let mut front = self.front;
        let mut back = self.back;
        let mut most_reduction = 0.0f32;
        let mut input_db = FLOOR_DB;

        for frame in 0..frames {
            let ceiling = gain_of(value(TABLE, CEILING, curves.ceiling[frame]));
            let release = super::reach(value(TABLE, RELEASE, curves.release[frame]) / 1000.0, rate);
            let detect = curves.detect[frame];
            let l = left[frame];
            let r = right[frame];
            // The level to limit: the loudest sample, or the peaks between them.
            let sample = l.abs().max(r.abs());
            let between = self.peaks[0].peak(l).max(self.peaks[1].peak(r));
            let level = sample + (between - sample) * detect;
            input_db = input_db.max(db_of(level));
            // What the gain has to be for this frame to leave under the
            // ceiling.
            let wanted = if level > ceiling && level > 0.0 {
                ceiling / level
            } else {
                1.0
            };
            // Into the delay line: the frame written now is played `look`
            // frames later, so it leaves at `read`.
            at = if at + 1 == LINE { 0 } else { at + 1 };
            let read = (at + LINE - self.look_frames) % LINE;
            let delayed_l = self.line[0][read];
            let delayed_r = self.line[1][read];
            self.line[0][at] = l;
            self.line[1][at] = r;
            // The sliding minimum over the window: this frame's need is kept
            // while it is the smallest of those still in it, and every need
            // larger than it goes, so the front is always the window's least.
            clock += 1;
            while back != front && self.least[(back + LINE - 1) % LINE].1 >= wanted {
                back -= 1;
            }
            self.least[back % LINE] = (clock, wanted);
            back += 1;
            // A frame that has left the window is forgotten.
            while front != back && self.least[front % LINE].0 + window <= clock {
                front += 1;
            }
            let least = self.least[front % LINE].1;
            // Down at once — the lookahead has already held this back — and up
            // over the release. The gain never rises above the window's least,
            // so the sample played now leaves under the ceiling.
            gain = if least <= gain {
                least
            } else {
                gain + (least - gain) * release
            };
            most_reduction = most_reduction.max(-db_of(gain.max(1e-6)));
            left[frame] = delayed_l * gain;
            right[frame] = delayed_r * gain;
        }
        self.at = at;
        self.clock = clock;
        self.front = front;
        self.back = back;
        self.gain = gain;
        self.meter.record(most_reduction, input_db);
    }

    fn latency(&self) -> u32 {
        self.look_frames as u32
    }

    fn tail(&self) -> u32 {
        self.look_frames as u32
    }

    fn reset(&mut self) {
        for glide in [&mut self.ceiling, &mut self.release, &mut self.detect] {
            glide.arrive();
        }
        for side in &mut self.line {
            side.fill(0.0);
        }
        self.front = 0;
        self.back = 0;
        self.clock = 0;
        self.at = 0;
        self.gain = 1.0;
        for peak in &mut self.peaks {
            peak.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamics::REFERENCE_RATE;

    /// A steady tone's true peak is its amplitude, and the interpolator must
    /// find it wherever the samples fall.
    #[test]
    fn the_true_peak_finds_the_peak_between_the_samples() {
        for frequency in [400.0f32, 5_000.0, 12_000.0, 19_000.0] {
            for phase in [0.0f32, 0.3, 1.1] {
                let mut peak = TruePeak::new();
                let mut most = 0.0f32;
                // Settle the filter, then measure a few cycles.
                for k in 0..4_800 {
                    let t = k as f32 / REFERENCE_RATE as f32;
                    let x = (std::f32::consts::TAU * frequency * t + phase).sin();
                    let found = peak.peak(x);
                    if k > 1_000 {
                        most = most.max(found);
                    }
                }
                let error_db = 20.0 * most.log10();
                assert!(
                    error_db.abs() < 0.06,
                    "{frequency} Hz, phase {phase}: {error_db} dB"
                );
            }
        }
        // And it never misses a sample's own peak.
        let mut peak = TruePeak::new();
        assert!(peak.peak(0.9) >= 0.9);
    }

    #[test]
    fn the_lookahead_is_the_latency_and_the_tail() {
        let at = |ms: f32| {
            let x = params::find(TABLE, LOOK).unwrap().normalised(ms);
            latency(&[(LOOK, x)])
        };
        assert_eq!(at(1.5), 72);
        assert_eq!(at(0.0), 0);
        assert_eq!(at(10.0), 480);
        // The running device's report is the description's, from the same
        // values (the mixer's tail after a Stop is read from it).
        let mut device = Limiter::new();
        assert_eq!(device.latency(), at(1.5));
        assert_eq!(device.tail(), at(1.5));
        device.set_lookahead(480);
        assert_eq!(device.latency(), 480);
        assert_eq!(device.tail(), 480);
    }

    #[test]
    fn the_lookahead_reads_in_milliseconds() {
        assert_eq!(look_text(1.5), "1.50 ms");
        assert_eq!(look_parse("2 ms"), Some(2.0));
        assert_eq!(sample_true_text(1.0), "True");
        assert_eq!(sample_true_parse("sample"), Some(0.0));
        assert_eq!(ceiling_parse("-0.3 dB"), Some(-0.3));
        assert_eq!(ms_text(60.0), "60.0 ms");
        assert_eq!(ms_parse("120"), Some(120.0));
    }
}
