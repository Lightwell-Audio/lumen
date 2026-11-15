mod onset;
mod table;

pub use onset::Onset;
pub use table::{FEEDBACK, FREEZE, HIGH_CUT, LOW_CUT, MIX, NOTE, PING, TABLE, TIME};

use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide};
use onset::NOTHING_YET;
use table::{NOTES, curve_note, curve_value, note_index, value};

pub const NAME: &str = "delay";

pub const MOST_SECONDS: f32 = 4.0;

const ONSET_GATE: f32 = 0.001;
const ONSET_HYSTERESIS: f32 = 0.25;

const LEAST_SECONDS: f32 = 0.001;

pub const HIGHEST_RATE: u32 = 192_000;

const KNOT: i64 = 32;

const GLIDE_SECONDS: f32 = 0.02;

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
    tail_frames(values, REFERENCE_BPM, REFERENCE_RATE)
}

pub const REFERENCE_BPM: f32 = 120.0;

pub const REFERENCE_RATE: u32 = 48_000;

pub const MOST_TAIL_SECONDS: f32 = 10.0;

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
        (0.001f64.ln() / f64::from(feedback).ln()).ceil() as f32
    };
    let seconds = (seconds * repeats).min(MOST_TAIL_SECONDS);
    (seconds * rate as f32).round() as u32
}

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

#[derive(Debug, Clone, Copy, Default)]
struct Loop {
    low_state: f32,
    high_state: f32,
}

impl Loop {
    fn run(&mut self, x: f32, (low, high): (f32, f32)) -> f32 {
        self.low_state = flush(self.low_state + low * (x - self.low_state));
        let without_low = if low > 0.0 { x - self.low_state } else { x };
        self.high_state = flush(self.high_state + high * (without_low - self.high_state));
        self.high_state
    }
}

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

const CAPACITY: usize = (MOST_SECONDS * HIGHEST_RATE as f32) as usize + 4;

pub struct Delay {
    lines: [Box<[f32]>; 2],
    write: usize,
    now: f32,
    target: f32,
    step: f32,
    loops: [Loop; 2],
    time: Glide,
    note: Glide,
    feedback: Glide,
    ping: Glide,
    low_cut: Glide,
    high_cut: Glide,
    mix: Glide,
    freeze: Glide,
    curves: Box<Curves>,
    held: [(u32, f32); 8],
    bpm: f32,
    rate: u32,
    since: f32,
    quiet: bool,
    entered: bool,
    onset: Onset,
}

impl Delay {
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

    pub fn onset(&self) -> Onset {
        self.onset.clone()
    }

    pub fn build() -> (Self, Onset) {
        let device = Self::new();
        let onset = device.onset();
        (device, onset)
    }

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

        let glide_frames = (GLIDE_SECONDS * rate as f32).max(1.0);
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
        let mut block_peak = 0.0f32;

        for frame in 0..frames {
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
            let looped_left = self.loops[0].run(wet_left, (low, high));
            let looped_right = self.loops[1].run(wet_right, (low, high));
            let gain = (feedback * (1.0 - freeze) + freeze).clamp(0.0, 1.0);
            let dry = 1.0 - freeze;
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
            let into_left = looped_left * (1.0 - ping) + looped_right * ping;
            let into_right = looped_right * (1.0 - ping) + looped_left * ping;
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

            let dry_gain = 1.0 - mix;
            left[frame] = flush(left[frame] * dry_gain + wet_left * mix);
            right[frame] = flush(right[frame] * dry_gain + wet_right * mix);

            self.write += 1;
            if self.write == len0 {
                self.write = 0;
            }
        }

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
        self.since = 0.0;
        self.quiet = true;
        self.entered = false;
        self.onset.record(NOTHING_YET);
    }
}
