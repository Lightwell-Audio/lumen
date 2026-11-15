//! Parallax, the stereo delay (M8 slice B3): one delay line per side, a
//! feedback loop with a low cut and a high cut in it, ping-pong across the
//! sides, freeze, and a time in notes that follows the song's tempo or in
//! milliseconds.
//!
//! Ping is a real ping-pong (OR-8): with it up, the sound enters the lines
//! summed to mono on the left one, and each repeat crosses to the other side,
//! so a centred sound bounces left, right, left. It blends from plain stereo
//! feedback at 0 to that at 1, so it moves under a hand or a lane without a
//! click, and at 0 it does exactly what it always did.
//!
//! The device also keeps the clock the card's picture draws its echoes by: how
//! long it is since sound last entered the lines, published lock-free through
//! an [`Onset`] handle the interface holds (OR-8: the picture is in step with
//! the repeats, not with the app's clock).
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

mod onset;
mod table;

pub use onset::Onset;
pub use table::{FEEDBACK, FREEZE, HIGH_CUT, LOW_CUT, MIX, NOTE, PING, TABLE, TIME};

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide};
use onset::NOTHING_YET;
use table::{NOTES, curve_note, curve_value, note_index, value};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "delay";

/// The longest a setting can ask for: 4 s. A whole note at 20 BPM would be
/// 12 s, so a note longer than this at a slow tempo is held here; the card's
/// tooltip says the time a synced note comes to.
pub const MOST_SECONDS: f32 = 4.0;

/// The level a sound has to reach to count as entering the lines, and the
/// share of it a whole block must stay under before another entrance counts:
/// −60 dBFS, then a quarter of that, so a block of near-silence between two
/// sounds is a new entrance and a sound crossing zero is not.
const ONSET_GATE: f32 = 0.001;
const ONSET_HYSTERESIS: f32 = 0.25;

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
    /// Frames since sound last entered the lines (OR-8): the clock the card's
    /// picture draws its echoes by.
    since: f32,
    /// Whether the lines have been fed near-silence since the last entrance
    /// (one block of it is enough to make the next sound a new one).
    quiet: bool,
    /// Whether any sound has entered since the device was made or reset: a
    /// device that has heard nothing has no clock to give.
    entered: bool,
    /// The interface's end of that clock.
    onset: Onset,
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
            since: 0.0,
            quiet: true,
            entered: false,
            onset: Onset::new(),
        }
    }

    /// The clock the card's picture reads the repeat's phase through (OR-8): a
    /// copy of the one the audio thread writes.
    pub fn onset(&self) -> Onset {
        self.onset.clone()
    }

    /// Parallax with its repeat clock, so the interface builds one and keeps
    /// the clock in one step.
    pub fn build() -> (Self, Onset) {
        let device = Self::new();
        let onset = device.onset();
        (device, onset)
    }

    /// The frame `behind` frames before the write point, read between the two
    /// frames it falls between.
    fn read(lines: &[Box<[f32]>; 2], write: usize, side: usize, behind: f32) -> f32 {
        crate::line::read_behind(&lines[side], write, behind)
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
        // The loudest frame this block fed the lines: at its end, a block of
        // near-silence opens the gate for the next sound to be a new entrance.
        let mut block_peak = 0.0f32;

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
            let dry = 1.0 - freeze;
            // What enters the lines: both sides as they come, blended toward
            // one side carrying their sum as Ping rises. A centred sound then
            // starts on the left and bounces, where crossing the feedback
            // alone would cross it back onto itself (OR-8).
            let in_left = left[frame] * dry;
            let in_right = right[frame] * dry;
            let (enter_left, enter_right) = if ping > 0.0 {
                let mono = 0.5 * (in_left + in_right);
                (
                    in_left * (1.0 - ping) + mono * ping,
                    in_right * (1.0 - ping),
                )
            } else {
                (in_left, in_right)
            };
            // The feedback crosses by the same amount, so at Ping every repeat
            // is the other side's and at none each stays home.
            let into_left = looped_left * (1.0 - ping) + looped_right * ping;
            let into_right = looped_right * (1.0 - ping) + looped_left * ping;
            // The repeat clock: an entrance is sound arriving after the lines
            // were fed near-silence, which is where the card's trail starts.
            let here = enter_left.abs().max(enter_right.abs());
            block_peak = block_peak.max(here);
            if self.quiet && here > ONSET_GATE {
                self.since = 0.0;
                self.entered = true;
                self.quiet = false;
            }
            if self.entered {
                self.since += 1.0;
            }
            let at = self.write;
            self.lines[0][at] = flush(enter_left + into_left * gain);
            self.lines[1][at] = flush(enter_right + into_right * gain);
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
        // The repeat clock: the time from the last entrance to this block's
        // last frame, or nothing before any sound has entered.
        let clock = if self.entered {
            self.since / rate as f32
        } else {
            NOTHING_YET
        };
        self.onset.record(clock);
        if block_peak < ONSET_GATE * ONSET_HYSTERESIS {
            self.quiet = true;
        }
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
        // Playback jumped: whatever was ringing is gone, and so is the clock
        // the picture started its trail from.
        self.since = 0.0;
        self.quiet = true;
        self.entered = false;
        self.onset.record(NOTHING_YET);
    }
}
