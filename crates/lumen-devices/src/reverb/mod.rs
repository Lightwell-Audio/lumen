//! Afterglow, the stereo reverb (M8 slice B3): an algorithmic room — a
//! pre-delay, early reflections from a short line, and a late tail from eight
//! feedback delay lines mixed through a Hadamard matrix, each line damped by a
//! one-pole lowpass and weighted so the whole decays in the time Decay names.
//!
//! The decay is the room's RT60, so the tail is predictable and measured (a
//! test holds it within 10 % of the setting): each line's feedback is
//! `10^(−3·length/decay)`, the standard relation between a loop's gain and the
//! time it takes to fall 60 dB. Nothing is copied from any named algorithm or
//! product; it is eight lines, a mixing matrix and a filter.
//!
//! Every buffer is allocated in the constructor for the longest a setting can
//! ask for at the highest rate Lumen opens (192 kHz): a 200 ms pre-delay, a
//! quarter-second early line and eight late lines of at most 54 ms. Nothing
//! allocates while it runs, and its filters flush denormals.

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "reverb";

/// The gap before the room answers, in milliseconds.
pub const PREDELAY: u32 = 1;
/// How large the room is, 0 to 100 %.
pub const SIZE: u32 = 2;
/// The time the tail takes to fall 60 dB, in seconds.
pub const DECAY: u32 = 3;
/// How quickly the high frequencies die away, 0 to 100 %.
pub const DAMPING: u32 = 4;
/// The stereo width of the tail, 0 to 200 %.
pub const WIDTH: u32 = 5;
/// Early reflections against the late tail, 0 to 100 %.
pub const EARLY: u32 = 6;
/// Dry to wet, 0 to 100 %.
pub const MIX: u32 = 7;

/// The longest decay a setting can ask for: 8 s. Its tail is a little longer
/// still, under the ten seconds a Stop wait and a render's tail are capped at.
pub const MOST_DECAY: f32 = 8.0;

/// The least decay.
const LEAST_DECAY: f32 = 0.2;

/// The longest pre-delay, and the early line: a quarter second holds every
/// tap at the largest room.
const MOST_PREDELAY_SECONDS: f32 = 0.2;
const EARLY_SECONDS: f32 = 0.25;

/// The highest sample rate Lumen opens (192 kHz): every buffer fits it.
pub const HIGHEST_RATE: u32 = 192_000;

/// Frames between two readings of the room's length, the damping and the
/// decay weights: the mixer's automation knots, so the same readings fall on
/// the same frames at any block size.
const KNOT: i64 = 32;

/// How long a change of pre-delay or size takes to move: as long as a level
/// the interface moves, so it slides rather than clicks.
const GLIDE_SECONDS: f32 = 0.02;

/// The longest tail any setting reports: ten seconds, the cap the engine's
/// Stop wait and a render's automatic tail already use.
pub const MOST_TAIL_SECONDS: f32 = 10.0;

/// The eight late lines' lengths at 48 kHz, at the smallest room: prime-ish
/// numbers of frames so their periods do not line up.
const BASE: [f32; 8] = [
    1116.0, 1188.0, 1277.0, 1356.0, 1422.0, 1491.0, 1557.0, 1617.0,
];

/// How far Size moves the lines, either side of their base length.
const SMALLEST: f32 = 0.5;
const LARGEST: f32 = 1.6;

/// The late lines' longest length in frames at [`HIGHEST_RATE`]: the buffers'
/// capacity.
const LINE_CAPACITY: usize =
    (BASE[7] as f64 / 48_000.0 * LARGEST as f64 * HIGHEST_RATE as f64) as usize + 4;

/// The early reflections' taps as fractions of the early line, and their
/// signs: two sets, one per side, so the two sides differ from the first
/// reflection on.
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

/// Afterglow's parameters, in the order the card shows them: the seven on the
/// strip. Ids are stable and never reused.
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

/// Afterglow's description: its table, and a tail that follows its pre-delay,
/// its decay and its mix (a pure function of its values, M8 slice B0, review
/// H1).
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// Frames Afterglow's output trails its input: none. The tail is the sound of
/// the room, not a late copy of what reaches it.
pub fn latency(_values: Values<'_>) -> u32 {
    0
}

/// Frames Afterglow rings on after its input stops at `values`, worked out at
/// the reference rate ([`tail_frames`]): what the engine keeps after a Stop
/// and what a render adds as its tail.
pub fn tail(values: Values<'_>) -> u32 {
    tail_frames(values, REFERENCE_RATE)
}

/// The rate the description's tail is worked out at: 48 kHz. A running device
/// reports the tail at the rate it is running.
pub const REFERENCE_RATE: u32 = 48_000;

/// Frames Afterglow rings on after its input stops at `values` and `rate`:
/// the pre-delay plus the decay, capped at [`MOST_TAIL_SECONDS`].
pub fn tail_frames(values: Values<'_>, rate: u32) -> u32 {
    if value(values, MIX) / 100.0 <= 0.005 {
        return 0;
    }
    let seconds =
        (value(values, PREDELAY) / 1000.0 + value(values, DECAY) + 0.05).min(MOST_TAIL_SECONDS);
    (seconds * rate as f32).round() as u32
}

/// The value of parameter `id` in its own units.
fn value(values: Values<'_>, id: u32) -> f32 {
    let x = params::normalised_of(TABLE, id, values);
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// "20 ms".
fn ms_text(ms: f32) -> String {
    format!("{ms:.0} ms")
}

/// "20", "20 ms".
fn ms_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("ms").trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "55 %".
fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

/// "55", "55 %".
fn percent_parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "2.00 s".
fn seconds_text(seconds: f32) -> String {
    format!("{seconds:.2} s")
}

/// "2.4", "2.4 s".
fn seconds_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches('s').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// A one-pole's coefficient for a corner at `hz`, at `rate`.
fn one_pole(hz: f32, rate: u32) -> f32 {
    if rate == 0 || hz <= 0.0 {
        return 0.0;
    }
    let x = -std::f32::consts::TAU * hz / rate as f32;
    (1.0 - x.exp()).clamp(0.0, 1.0)
}

/// A tiny value becomes zero, so a decaying filter never reaches denormals.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

/// What one late line's loop does: how far behind the write point it is read,
/// where that distance is heading, how fast it gets there, how much it loses
/// each pass, and how quickly its top end dies.
#[derive(Debug, Clone, Copy)]
struct Line {
    length: f32,
    target: f32,
    step: f32,
    gain: f32,
    damp: f32,
    /// The one-pole's state.
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

/// The block's parameter curves, worked out once each block.
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

/// Frames of pre-delay the longest setting is at [`HIGHEST_RATE`].
const PREDELAY_CAPACITY: usize = (MOST_PREDELAY_SECONDS * HIGHEST_RATE as f32) as usize + 4;
/// Frames of early line at [`HIGHEST_RATE`].
const EARLY_CAPACITY: usize = (EARLY_SECONDS * HIGHEST_RATE as f32) as usize + 4;

/// Afterglow.
pub struct Reverb {
    /// The pre-delay line, per side.
    pre: [Box<[f32]>; 2],
    pre_write: usize,
    /// How far the pre-delay reads behind now, the distance it is heading for
    /// and the frames it moves each frame.
    pre_now: f32,
    pre_target: f32,
    pre_step: f32,
    /// The early reflection line: the pre-delayed sound, once.
    early_line: Box<[f32]>,
    early_write: usize,
    /// The eight late lines.
    lines: [Box<[f32]>; 8],
    at: [usize; 8],
    /// Each line's place in the room this knot.
    place: [Line; 8],
    /// The size the lines are heading for, so a change is seen once.\
    size_target: f32,
    /// The block's curves and where each parameter was at the last block.
    curves: Box<Curves>,
    predelay: Glide,
    size: Glide,
    decay: Glide,
    damping: Glide,
    width: Glide,
    early: Glide,
    mix: Glide,
    /// The values of the last block, normalised, by id: what its tail is
    /// reported from.
    held: [(u32, f32); 7],
    rate: u32,
}

impl Reverb {
    /// Afterglow at its defaults: 20 ms ahead, a middle room, a 2 s tail,
    /// gentle damping, full width, 30 % early, 25 % wet. Allocates its
    /// buffers (about 0.8 MB): call it on the interface side.
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

    /// The pre-delay line read `behind` frames before its write point, between
    /// the two frames it falls between.
    fn read(line: &[f32], write: usize, behind: f32) -> f32 {
        let len = line.len();
        let position = (write as f32 - behind).rem_euclid(len as f32);
        let first = (position as usize) % len;
        let frac = position - first as f32;
        let a = line[first];
        let b = line[(first + 1) % len];
        a + (b - a) * frac
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

/// The gain one pass round a line of `seconds` loses so the room falls 60 dB
/// in `decay` seconds: `10^(−3·seconds/decay)`.
fn decay_gain(seconds: f32, decay: f32) -> f32 {
    let decay = decay.clamp(LEAST_DECAY, MOST_DECAY);
    if seconds <= 0.0 {
        return 0.0;
    }
    10f32.powf(-3.0 * seconds / decay).clamp(0.0, 1.0)
}

/// How far Size stretches the lines: [`SMALLEST`] at 0 %, [`LARGEST`] at 100 %.
fn size_factor(x: f32) -> f32 {
    SMALLEST + (LARGEST - SMALLEST) * x.clamp(0.0, 1.0)
}

/// The eight-point Hadamard transform, normalized so it preserves energy:
/// `v` becomes `H·v/√8`.
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
        // The late lines' outputs this frame, and what the matrix makes of
        // them.
        let mut out = [0.0f32; 8];
        // The parameters that do not need a curve's own frame: read at the
        // knots, as the corners are, so the same readings fall at any block
        // size.
        let (mut pre_ms, mut early_x, mut width_x, mut mix_x) = (
            curve_value(PREDELAY, curves.predelay[0]),
            curves.early[0].clamp(0.0, 1.0),
            curve_value(WIDTH, curves.width[0]) / 100.0,
            curve_value(MIX, curves.mix[0]) / 100.0,
        );

        for frame in 0..frames {
            // Each line slides towards where Size puts it, a frame at a time,
            // so a room that grows does not step its read point.
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
                // A hundred percent damping takes the top down to 1 kHz; none
                // leaves it at 20 kHz.
                let cutoff = 20_000.0 * (1_000.0f32 / 20_000.0).powf(damp);
                let damp_now = one_pole(cutoff, rate);
                // A new size sets where each line is heading; each line then
                // slides there over the glide.
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
                early_x = curves.early[frame].clamp(0.0, 1.0);
                width_x = curve_value(WIDTH, curves.width[frame]) / 100.0;
                mix_x = curve_value(MIX, curves.mix[frame]) / 100.0;
            }
            // The pre-delay: a line read behind the write point, its distance
            // sliding when the setting moves.
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

            // The early reflections: taps of the pre-delayed sound, before it
            // reaches the late lines.
            let mono = 0.5 * (pre_left + pre_right);
            let early_reads: [[f32; 6]; 2] = EARLY_TAPS.map(|taps| {
                taps.map(|(fraction, _)| {
                    let behind =
                        (fraction * EARLY_SECONDS * rate as f32).min(early_len as f32 - 2.0);
                    let position = (self.early_write as f32 - behind).rem_euclid(early_len as f32);
                    let first = (position as usize) % early_len;
                    let frac = position - first as f32;
                    let a = self.early_line[first];
                    let b = self.early_line[(first + 1) % early_len];
                    a + (b - a) * frac
                })
            });
            let mut early_pair = [0.0f32; 2];
            for side in 0..2 {
                for (tap, (_, sign)) in early_reads[side].iter().zip(EARLY_TAPS[side]) {
                    early_pair[side] += tap * sign;
                }
                early_pair[side] *= 1.0 / 3.0;
            }

            // The late lines, read, damped, weighted and mixed.
            for (index, line) in self.place.iter_mut().enumerate() {
                let wet = Self::read(&self.lines[index], self.at[index], line.length);
                line.low = flush(line.low + line.damp * (wet - line.low));
                out[index] = flush(line.low * line.gain);
            }
            let mut next = out;
            hadamard(&mut next);
            // What enters the room: the pre-delayed sound, once.
            let into_room = 0.25 * (pre_left + pre_right);
            for ((line, at), fed) in self.lines.iter_mut().zip(&mut self.at).zip(next) {
                line[*at] = flush(into_room + fed);
                *at += 1;
                if *at == line.len() {
                    *at = 0;
                }
            }

            // The early line takes what the room was given.
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

            let early_mix = early_x;
            let late_mix = 1.0 - early_mix;
            let late_left = 0.5 * (out[0] - out[1] + out[2] - out[3]);
            let late_right = 0.5 * (out[4] - out[5] + out[6] - out[7]);
            let mut wet_left = early_pair[0] * early_mix + late_left * late_mix;
            let mut wet_right = early_pair[1] * early_mix + late_right * late_mix;
            // Width on the room's sides: 0 is mono, 200 % doubles them.
            let width = width_x;
            let mid = 0.5 * (wet_left + wet_right);
            let side = 0.5 * (wet_left - wet_right) * width;
            wet_left = mid + side;
            wet_right = mid - side;
            let mix = mix_x;
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
    }
}

/// The value (in its own units) of the normalised `x` of parameter `id`.
fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}
