//! Parallax, the stereo delay (M8 slice B3): one delay line per side, a
//! feedback loop with a low cut and a high cut in it, ping-pong across the
//! sides, freeze, and a time in notes that follows the song's tempo or in
//! milliseconds.
//!
//! Everything it does to the audio is smooth by construction: a time the
//! interface moved glides over 20 ms (a tempo change is one), so the read
//! point slides rather than jumps, and no change of a setting steps the
//! output. The lines are allocated in the constructor for the longest time a
//! setting can ask for (4 s) at the highest rate Lumen opens (192 kHz), so
//! nothing allocates while it runs; its filters flush denormals.
//!
//! A synced time is a pure function of the block's tempo ([`Block::bpm`]) and
//! the note chosen, worked out at the block's first frame; the delay in
//! samples is that number, so at 120 BPM an eighth note is exactly 9000 frames
//! and the echo lands on the beat.

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "delay";

/// The time in milliseconds, when the note is Free.
pub const TIME: u32 = 1;
/// The time in notes: Free, or a note the song's tempo places.
pub const NOTE: u32 = 2;
/// How much of each echo returns to the loop, 0 to 99 %.
pub const FEEDBACK: u32 = 3;
/// Cross the feedback to the other side: the echoes alternate.
pub const PING: u32 = 4;
/// A highpass in the loop, in Hz; 0 is off.
pub const LOW_CUT: u32 = 5;
/// A lowpass in the loop, in Hz; 20 kHz is off.
pub const HIGH_CUT: u32 = 6;
/// Dry to wet, 0 to 100 %.
pub const MIX: u32 = 7;
/// Hold what the loop carries and stop taking new input.
pub const FREEZE: u32 = 8;

/// The longest a setting can ask for: 4 s. A whole note at 20 BPM would be
/// 12 s, so a note longer than this at a slow tempo is held here; the card's
/// tooltip says the time a synced note comes to.
pub const MOST_SECONDS: f32 = 4.0;

/// The least: under a millisecond the loop is a click, not an echo.
const LEAST_SECONDS: f32 = 0.001;

/// The highest sample rate Lumen opens (192 kHz): the lines are allocated for
/// it, so any rate a project runs at fits.
pub const HIGHEST_RATE: u32 = 192_000;

/// Frames between two readings of the loop filters' corners: the mixer's own
/// automation knots, so the same reading falls on the same frames at any block
/// size.
const KNOT: i64 = 32;

/// How long a change of time takes to glide: as long as a level the interface
/// moves, so a tempo change slides rather than clicks.
const GLIDE_SECONDS: f32 = 0.02;

/// The note choices, in order: Free first, then the note the tempo places.
const NOTES: [(&str, f32); 7] = [
    ("Free", 0.0),
    ("1/32", 0.125),
    ("1/16", 0.25),
    ("1/8", 0.5),
    ("1/4", 1.0),
    ("1/2", 2.0),
    ("1/1", 4.0),
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

/// Parallax's parameters, in the order the card shows them: the eight on the
/// strip. Ids are stable and never reused.
pub static TABLE: &[ParamInfo] = &[
    param(
        TIME,
        "Time",
        "ms",
        1.0,
        4000.0,
        Taper::Power(2.0),
        375.0,
        0,
        &[],
        time_text,
        time_parse,
    ),
    param(
        NOTE,
        "Note",
        "",
        0.0,
        1.0,
        Taper::Linear,
        4.0 / 6.0,
        6,
        &["Free", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1"],
        note_text,
        note_parse,
    ),
    param(
        FEEDBACK,
        "Repeats",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        35.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        PING,
        "Ping",
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
        LOW_CUT,
        "Low cut",
        "Hz",
        0.0,
        2000.0,
        Taper::Power(2.0),
        0.0,
        0,
        &[],
        low_cut_text,
        low_cut_parse,
    ),
    param(
        HIGH_CUT,
        "High cut",
        "Hz",
        200.0,
        20_000.0,
        Taper::Power(2.0),
        20_000.0,
        0,
        &[],
        high_cut_text,
        high_cut_parse,
    ),
    param(
        MIX,
        "Mix",
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
        FREEZE,
        "Freeze",
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

/// Parallax's description: its table, and a tail that follows its time, its
/// feedback, its freeze and its mix (a pure function of its values, M8 slice
/// B0, review H1).
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// Frames Parallax's output trails its input: none. An echo is the sound of
/// the device, not a late copy of what reaches it, so nothing is compensated.
pub fn latency(_values: Values<'_>) -> u32 {
    0
}

/// Frames Parallax rings on after its input stops at `values`, worked out at
/// the reference tempo and rate ([`tail_frames`]): what the engine keeps after
/// a Stop and what a render adds as its tail. The running instance reports the
/// same number at the same tempo and rate, and the true one at any tempo.
pub fn tail(values: Values<'_>) -> u32 {
    tail_frames(values, REFERENCE_BPM, REFERENCE_RATE)
}

/// The tempo the description's tail is worked out at: 120 BPM, the tempo a
/// note-synced time has no other way to know (M8 slice B3's ADR).
pub const REFERENCE_BPM: f32 = 120.0;

/// The rate the description's tail is worked out at: 48 kHz, the rate this
/// crate's other timing numbers are in. A running device reports the tail at
/// the rate it is running, which is the one the mixer compares against.
pub const REFERENCE_RATE: u32 = 48_000;

/// The longest tail any setting reports: ten seconds, the cap the engine's
/// Stop wait and a render's automatic tail already use. A frozen loop rings
/// for ever, so it reports this and stops there rather than holding a render
/// open (the ADR says how).
pub const MOST_TAIL_SECONDS: f32 = 10.0;

/// Frames Parallax rings on after its input stops, at `values`, a tempo of
/// `bpm` and a rate of `rate`: the time of one echo times the echoes that are
/// still above −60 dB, or [`MOST_TAIL_SECONDS`] for a frozen loop.
pub fn tail_frames(values: Values<'_>, bpm: f32, rate: u32) -> u32 {
    let seconds = seconds_of(values, bpm);
    let mix = value(values, MIX) / 100.0;
    if mix <= 0.005 {
        return 0;
    }
    let freeze = value(values, FREEZE) >= 0.5;
    let feedback = (value(values, FEEDBACK) / 100.0).clamp(0.0, 1.0);
    let repeats = if freeze {
        MOST_TAIL_SECONDS / seconds
    } else if feedback <= 0.001 {
        1.0
    } else {
        // The repeats until the echo is a thousandth of the first (−60 dB).
        (0.001f64.ln() / f64::from(feedback).ln()).ceil() as f32
    };
    let seconds = (seconds * repeats).min(MOST_TAIL_SECONDS);
    (seconds * rate as f32).round() as u32
}

/// Seconds the delay's time comes to at `bpm`: the note the tempo places, or
/// the milliseconds when the note is Free, held between the least and the most
/// the line holds.
pub fn seconds_of(values: Values<'_>, bpm: f32) -> f32 {
    let note = note_index(values);
    let seconds = if note == 0 {
        value(values, TIME) / 1000.0
    } else {
        let quarters = NOTES[note].1;
        let bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm
        } else {
            120.0
        };
        quarters * 60.0 / bpm
    };
    seconds.clamp(LEAST_SECONDS, MOST_SECONDS)
}

/// The note index (0 is Free) the values hold.
fn note_index(values: Values<'_>) -> usize {
    let x = params::normalised_of(TABLE, NOTE, values);
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}

/// The value of parameter `id` in its own units.
fn value(values: Values<'_>, id: u32) -> f32 {
    let x = params::normalised_of(TABLE, id, values);
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// "375 ms", "1.20 s".
fn time_text(ms: f32) -> String {
    if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// "375", "375 ms", "1.2 s", "1200".
fn time_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    if let Some(rest) = t.strip_suffix(" ms").or_else(|| t.strip_suffix("ms")) {
        return rest.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    }
    if let Some(rest) = t.strip_suffix(" s").or_else(|| t.strip_suffix('s')) {
        return rest
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|s| s * 1000.0);
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "1/4", "Free".
fn note_text(x: f32) -> String {
    let index = (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize;
    NOTES[index].0.to_owned()
}

/// "1/4", "free", "quarter": the note's word, or the note it spells.
fn note_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let index = match t.as_str() {
        "free" | "off" | "ms" => 0,
        "1/32" | "thirty-second" | "32nd" => 1,
        "1/16" | "sixteenth" | "16th" => 2,
        "1/8" | "eighth" | "8th" => 3,
        "1/4" | "quarter" | "4th" => 4,
        "1/2" | "half" => 5,
        "1/1" | "whole" => 6,
        _ => return None,
    };
    Some(index as f32 / (NOTES.len() - 1) as f32)
}

/// "35 %".
fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

/// "35", "35 %".
fn percent_parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// "Off", "120 Hz".
fn low_cut_text(hz: f32) -> String {
    if hz < 0.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.2} kHz", hz / 1000.0)
    }
}

/// "off", "120", "120 Hz".
fn low_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(0.0);
    }
    hertz(&t)
}

/// "Off" at the top of the range, else "8.0 kHz".
fn high_cut_text(hz: f32) -> String {
    if hz >= 19_999.5 {
        "Off".to_owned()
    } else if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

/// "off", "8000", "8 kHz".
fn high_cut_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if t.starts_with("off") {
        return Some(20_000.0);
    }
    hertz(&t)
}

/// A frequency with or without its unit.
fn hertz(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(rest) = t.strip_suffix("khz") {
        return rest.trim().parse::<f32>().ok().map(|k| k * 1000.0);
    }
    if let Some(rest) = t.strip_suffix("hz") {
        return rest.trim().parse::<f32>().ok();
    }
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn on_off_text(value: f32) -> String {
    if value >= 0.5 { "On" } else { "Off" }.to_owned()
}

fn on_off_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "on" | "1" => Some(1.0),
        "off" | "0" => Some(0.0),
        _ => None,
    }
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

/// The loop's filters on one side: a highpass (the low cut) then a lowpass
/// (the high cut), each bypassed at the end of its range.
#[derive(Debug, Clone, Copy, Default)]
struct Loop {
    low_state: f32,
    high_state: f32,
}

impl Loop {
    fn run(&mut self, x: f32, (low, high): (f32, f32)) -> f32 {
        // A low cut at 0 Hz has a coefficient of nothing: the input is let by.
        self.low_state = flush(self.low_state + low * (x - self.low_state));
        let without_low = if low > 0.0 { x - self.low_state } else { x };
        self.high_state = flush(self.high_state + high * (without_low - self.high_state));
        self.high_state
    }
}

/// The block's parameter curves, worked out once each block.
struct Curves {
    time: [f32; crate::MOST_FRAMES],
    note: [f32; crate::MOST_FRAMES],
    feedback: [f32; crate::MOST_FRAMES],
    ping: [f32; crate::MOST_FRAMES],
    low_cut: [f32; crate::MOST_FRAMES],
    high_cut: [f32; crate::MOST_FRAMES],
    mix: [f32; crate::MOST_FRAMES],
    freeze: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
        Box::new(Self {
            time: [0.0; crate::MOST_FRAMES],
            note: [0.0; crate::MOST_FRAMES],
            feedback: [0.0; crate::MOST_FRAMES],
            ping: [0.0; crate::MOST_FRAMES],
            low_cut: [0.0; crate::MOST_FRAMES],
            high_cut: [0.0; crate::MOST_FRAMES],
            mix: [0.0; crate::MOST_FRAMES],
            freeze: [0.0; crate::MOST_FRAMES],
        })
    }
}

/// Frames the longest time a setting can ask for is at the highest rate: the
/// line's capacity, plus room for the read's interpolation.
const CAPACITY: usize = (MOST_SECONDS * HIGHEST_RATE as f32) as usize + 4;

/// Parallax.
pub struct Delay {
    /// One line per side, allocated to [`CAPACITY`].
    lines: [Box<[f32]>; 2],
    /// Where the loop writes next.
    write: usize,
    /// The delay in frames now, the one it is heading for, and the frames it
    /// moves each frame on the way.
    now: f32,
    target: f32,
    step: f32,
    /// The loop's filters, per side.
    loops: [Loop; 2],
    /// Where each parameter was at the end of the last block.
    time: Glide,
    note: Glide,
    feedback: Glide,
    ping: Glide,
    low_cut: Glide,
    high_cut: Glide,
    mix: Glide,
    freeze: Glide,
    /// The block's curves.
    curves: Box<Curves>,
    /// The parameter values of the last block, normalised, by id: what its
    /// tail is reported from.
    held: [(u32, f32); 8],
    /// The tempo and rate the last block played at: a tail reported after a
    /// Stop is the one the song's tempo gives.
    bpm: f32,
    rate: u32,
}

impl Delay {
    /// Parallax at its defaults: 375 ms, a quarter note, 35 % feedback, off
    /// centre, 30 % wet. Allocates its lines (6.2 MB): call it on the
    /// interface side.
    pub fn new() -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        let held = [
            (TIME, at(TIME)),
            (NOTE, at(NOTE)),
            (FEEDBACK, at(FEEDBACK)),
            (PING, at(PING)),
            (LOW_CUT, at(LOW_CUT)),
            (HIGH_CUT, at(HIGH_CUT)),
            (MIX, at(MIX)),
            (FREEZE, at(FREEZE)),
        ];
        Self {
            lines: [
                vec![0.0; CAPACITY].into_boxed_slice(),
                vec![0.0; CAPACITY].into_boxed_slice(),
            ],
            write: 0,
            now: seconds_of(&held, REFERENCE_BPM) * REFERENCE_RATE as f32,
            target: f32::NAN,
            step: 0.0,
            loops: [Loop::default(); 2],
            time: Glide::new(at(TIME)),
            note: Glide::new(at(NOTE)),
            feedback: Glide::new(at(FEEDBACK)),
            ping: Glide::new(at(PING)),
            low_cut: Glide::new(at(LOW_CUT)),
            high_cut: Glide::new(at(HIGH_CUT)),
            mix: Glide::new(at(MIX)),
            freeze: Glide::new(at(FREEZE)),
            curves: Curves::new(),
            held,
            bpm: REFERENCE_BPM,
            rate: REFERENCE_RATE,
        }
    }

    /// The frame `behind` frames before the write point, read between the two
    /// frames it falls between.
    fn read(lines: &[Box<[f32]>; 2], write: usize, side: usize, behind: f32) -> f32 {
        let line = &lines[side];
        let len = line.len();
        let position = (write as f32 - behind).rem_euclid(len as f32);
        let first = (position as usize) % len;
        let frac = position - first as f32;
        let a = line[first];
        let b = line[(first + 1) % len];
        a + (b - a) * frac
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Delay {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, _key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate.max(1);
        let bpm = if block.bpm.is_finite() && block.bpm > 0.0 {
            block.bpm
        } else {
            120.0
        };
        let events = block.events;
        let curves = &mut self.curves;
        {
            // Keep a private glide per parameter across blocks, as every
            // built-in does: a value the interface moved glides over 20 ms.
            params::values(
                &mut curves.time[..frames],
                &mut self.time,
                events,
                TIME,
                rate,
            );
            params::values(
                &mut curves.note[..frames],
                &mut self.note,
                events,
                NOTE,
                rate,
            );
            params::values(
                &mut curves.feedback[..frames],
                &mut self.feedback,
                events,
                FEEDBACK,
                rate,
            );
            params::values(
                &mut curves.ping[..frames],
                &mut self.ping,
                events,
                PING,
                rate,
            );
            params::values(
                &mut curves.low_cut[..frames],
                &mut self.low_cut,
                events,
                LOW_CUT,
                rate,
            );
            params::values(
                &mut curves.high_cut[..frames],
                &mut self.high_cut,
                events,
                HIGH_CUT,
                rate,
            );
            params::values(&mut curves.mix[..frames], &mut self.mix, events, MIX, rate);
            params::values(
                &mut curves.freeze[..frames],
                &mut self.freeze,
                events,
                FREEZE,
                rate,
            );
        }
        self.bpm = bpm;
        self.rate = rate;

        // A time the interface (or a tempo change) moved glides over 20 ms, so
        // the read point slides and nothing clicks.
        let glide_frames = (GLIDE_SECONDS * rate as f32).max(1.0);
        // The corners for the frames before this block's first knot: read from
        // the same curve at the same frame, so a block that starts anywhere
        // gives the same audio.
        let mut low = one_pole(curve_value(LOW_CUT, curves.low_cut[0]), rate);
        let mut high = {
            let hz = curve_value(HIGH_CUT, curves.high_cut[0]);
            if hz >= 19_999.5 {
                1.0
            } else {
                one_pole(hz, rate)
            }
        };
        let one_pole_rate = rate;
        let capacity = CAPACITY as f32 - 2.0;
        let len0 = self.lines[0].len();

        for frame in 0..frames {
            // The corners are read at knots of the song's clock, so the same
            // readings fall at any block size.
            if (block.start + frame as i64).rem_euclid(KNOT) == 0 {
                let low_hz = curve_value(LOW_CUT, curves.low_cut[frame]);
                let high_hz = curve_value(HIGH_CUT, curves.high_cut[frame]);
                low = one_pole(low_hz, one_pole_rate);
                high = if high_hz >= 19_999.5 {
                    1.0
                } else {
                    one_pole(high_hz, one_pole_rate)
                };
            }
            let seconds = if curve_note(curves.note[frame]) == 0 {
                curve_value(TIME, curves.time[frame]) / 1000.0
            } else {
                let quarters = NOTES[curve_note(curves.note[frame])].1;
                quarters * 60.0 / bpm
            };
            let target =
                (seconds.clamp(LEAST_SECONDS, MOST_SECONDS) * rate as f32).clamp(1.0, capacity);
            if target != self.target {
                self.target = target;
                self.step = (target - self.now) / glide_frames;
            }
            if (target - self.now).abs() <= self.step.abs() {
                self.now = target;
            } else {
                self.now += self.step;
            }
            let feedback = curve_value(FEEDBACK, curves.feedback[frame]) / 100.0;
            let freeze = curves.freeze[frame].clamp(0.0, 1.0);
            let ping = curves.ping[frame].clamp(0.0, 1.0);
            let mix = curve_value(MIX, curves.mix[frame]) / 100.0;

            let wet_left = Self::read(&self.lines, self.write, 0, self.now);
            let wet_right = Self::read(&self.lines, self.write, 1, self.now);
            // The loop's filters, then what returns to the other side.
            let looped_left = self.loops[0].run(wet_left, (low, high));
            let looped_right = self.loops[1].run(wet_right, (low, high));
            // Freeze holds the loop and takes no new sound; its feedback is
            // one, so the repeats neither grow nor die.
            let gain = (feedback * (1.0 - freeze) + freeze).clamp(0.0, 1.0);
            let into_left = looped_left * (1.0 - ping) + looped_right * ping;
            let into_right = looped_right * (1.0 - ping) + looped_left * ping;
            let dry = 1.0 - freeze;
            let at = self.write;
            self.lines[0][at] = flush(left[frame] * dry + into_left * gain);
            self.lines[1][at] = flush(right[frame] * dry + into_right * gain);
            // The output: dry to wet, both glided.

            let dry_gain = 1.0 - mix;
            left[frame] = flush(left[frame] * dry_gain + wet_left * mix);
            right[frame] = flush(right[frame] * dry_gain + wet_right * mix);

            self.write += 1;
            if self.write == len0 {
                self.write = 0;
            }
        }

        // What the tail is reported from: the values the block ended at.
        let last = frames - 1;
        self.held = [
            (TIME, curves.time[last]),
            (NOTE, curves.note[last]),
            (FEEDBACK, curves.feedback[last]),
            (PING, curves.ping[last]),
            (LOW_CUT, curves.low_cut[last]),
            (HIGH_CUT, curves.high_cut[last]),
            (MIX, curves.mix[last]),
            (FREEZE, curves.freeze[last]),
        ];
    }

    fn tail(&self) -> u32 {
        tail_frames(&self.held, self.bpm, self.rate)
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write = 0;
        self.loops = [Loop::default(); 2];
        // Playback jumped: the values are the project's, not a glide from the
        // defaults, so a locate and a render agree on the first block.
        for glide in [
            &mut self.time,
            &mut self.note,
            &mut self.feedback,
            &mut self.ping,
            &mut self.low_cut,
            &mut self.high_cut,
            &mut self.mix,
            &mut self.freeze,
        ] {
            glide.arrive();
        }
        self.now = seconds_of(&self.held, self.bpm) * self.rate as f32;
        self.target = f32::NAN;
        self.step = 0.0;
    }
}

/// The value (in its own units) of the normalised `x` of parameter `id`.
fn curve_value(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// The note index a normalised value picks.
fn curve_note(x: f32) -> usize {
    (x * (NOTES.len() - 1) as f32).round().clamp(0.0, 6.0) as usize
}
