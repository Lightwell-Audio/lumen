//! Turns a block's events and parameter changes into sound. Every change
//! and note lands on its exact sample: the block is rendered in segments
//! that begin where something happens. Nothing here allocates.

use crate::dsp::{Delay, Voices};
use crate::params::{Kind, LATENCY, LEVEL, WAVE, db_to_gain};

/// A note event inside a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Note {
    On {
        channel: i16,
        pitch: i16,
        velocity: f32,
        id: i32,
    },
    Off {
        channel: i16,
        pitch: i16,
        id: i32,
    },
}

/// Something that happens on sample `at` of the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timed<T> {
    pub at: i32,
    pub what: T,
}

/// A parameter takes a new normalized value from sample `at` on (a step,
/// not a ramp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    pub id: u32,
    pub value: f64,
}

/// The sound of one plugin instance.
pub struct Dsp {
    pub kind: Kind,
    pub rate: f64,
    /// Every parameter's normalized value.
    pub values: [f64; 2],
    voices: Voices,
    delay: Delay,
    /// Samples the instrument plays its notes late and reports as latency.
    instrument_latency: u32,
}

impl Dsp {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            rate: 48_000.0,
            values: kind.defaults(),
            voices: Voices::new(),
            delay: Delay::new(),
            instrument_latency: 0,
        }
    }

    /// Stops all sound and empties the delay, keeping the parameters.
    pub fn reset(&mut self) {
        self.voices.silence();
        self.delay.clear();
    }

    /// The delay the effect reports as latency, in samples.
    pub fn latency(&self) -> u32 {
        match self.kind {
            Kind::Effect => self.kind.plain(LATENCY, self.values[1]) as u32,
            Kind::Instrument => self.instrument_latency,
        }
    }

    /// The instrument plays its notes `samples` late and reports that.
    pub fn set_instrument_latency(&mut self, samples: u32) {
        self.instrument_latency = samples.min(crate::params::MAX_LATENCY);
    }

    pub fn sounding(&self) -> usize {
        self.voices.sounding()
    }

    /// Released notes fade out over `seconds` instead of stopping at once.
    pub fn set_release(&mut self, seconds: f64) {
        self.voices.set_release(seconds);
    }

    fn set(&mut self, id: u32, value: f64) {
        if let Some(slot) = self.values.get_mut(id as usize) {
            *slot = value.clamp(0.0, 1.0);
        }
    }

    fn note(&mut self, note: Note) {
        match note {
            Note::On {
                channel,
                pitch,
                velocity,
                id,
            } => self.voices.note_on(channel, pitch, velocity, id, self.rate),
            Note::Off { channel, pitch, id } => self.voices.note_off(channel, pitch, id),
        }
    }

    /// Renders `frames` frames. `changes` and `notes` are in time order. A
    /// null input reads as silence; a null output is skipped. Input and
    /// output may be the same memory.
    ///
    /// # Safety
    /// Each non-null pointer must be valid for `frames` samples.
    pub unsafe fn render(
        &mut self,
        frames: usize,
        input: [*const f32; 2],
        output: [*mut f32; 2],
        changes: &[Timed<Change>],
        notes: &[Timed<Note>],
    ) {
        let (mut ci, mut ni, mut pos) = (0, 0, 0_usize);
        while pos < frames {
            while ci < changes.len() && changes[ci].at as i64 <= pos as i64 {
                self.set(changes[ci].what.id, changes[ci].what.value);
                ci += 1;
            }
            while ni < notes.len() && notes[ni].at as i64 <= pos as i64 {
                self.note(notes[ni].what);
                ni += 1;
            }
            let mut end = frames;
            if let Some(c) = changes.get(ci) {
                end = end.min(c.at.max(0) as usize);
            }
            if let Some(n) = notes.get(ni) {
                end = end.min(n.at.max(0) as usize);
            }
            // SAFETY: the caller vouches for the pointers; `pos < end <= frames`.
            unsafe { self.segment(pos..end, input, output) };
            pos = end;
        }
    }

    /// # Safety
    /// As for `render`, with `range` inside the block.
    unsafe fn segment(
        &mut self,
        range: std::ops::Range<usize>,
        input: [*const f32; 2],
        output: [*mut f32; 2],
    ) {
        match self.kind {
            Kind::Instrument => {
                let level = self.kind.plain(LEVEL, self.values[0]) as f32;
                let saw = self.kind.plain(WAVE, self.values[1]) >= 0.5;
                let late = self.instrument_latency as usize;
                for i in range {
                    let mut s = self.voices.next_sample(level, saw);
                    if late > 0 {
                        s = self.delay.step([s, s], late)[0];
                    }
                    for out in output.into_iter().filter(|p| !p.is_null()) {
                        // SAFETY: valid for the block (see `render`).
                        unsafe { out.add(i).write(s) };
                    }
                }
            }
            Kind::Effect => {
                let gain = db_to_gain(self.kind.plain(LEVEL, self.values[0]));
                let delay = self.latency() as usize;
                for i in range {
                    // SAFETY: valid for the block; read before the same
                    // sample is written, so shared memory is fine.
                    let frame = input.map(|p| {
                        if p.is_null() {
                            0.0
                        } else {
                            // SAFETY: as above.
                            unsafe { p.add(i).read() }
                        }
                    });
                    let out = self.delay.step(frame, delay);
                    for (c, o) in output.into_iter().enumerate() {
                        if !o.is_null() {
                            // SAFETY: as above.
                            unsafe { o.add(i).write(out[c] * gain) };
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        dsp: &mut Dsp,
        frames: usize,
        input: &[Vec<f32>; 2],
        changes: &[Timed<Change>],
        notes: &[Timed<Note>],
    ) -> [Vec<f32>; 2] {
        let mut out = [vec![9.0; frames], vec![9.0; frames]];
        let ptrs = [out[0].as_mut_ptr(), out[1].as_mut_ptr()];
        let ins = [input[0].as_ptr(), input[1].as_ptr()];
        // SAFETY: every buffer holds `frames` samples.
        unsafe { dsp.render(frames, ins, ptrs, changes, notes) };
        out
    }

    fn on(at: i32, pitch: i16, velocity: f32) -> Timed<Note> {
        Timed {
            at,
            what: Note::On {
                channel: 0,
                pitch,
                velocity,
                id: -1,
            },
        }
    }

    fn off(at: i32, pitch: i16) -> Timed<Note> {
        Timed {
            at,
            what: Note::Off {
                channel: 0,
                pitch,
                id: -1,
            },
        }
    }

    #[test]
    fn a_note_starts_and_stops_on_its_exact_samples() {
        let mut d = Dsp::new(Kind::Instrument);
        d.values[0] = 1.0;
        let silent = [vec![], vec![]];
        let out = run(&mut d, 32, &silent, &[], &[on(5, 69, 0.5), off(20, 69)]);
        assert!(out[0][..5].iter().all(|&s| s == 0.0));
        assert!(out[0][5..20].iter().all(|&s| s == 0.5));
        assert!(out[0][20..].iter().all(|&s| s == 0.0));
        assert_eq!(out[0], out[1]);
    }

    #[test]
    fn a_note_held_across_blocks_keeps_sounding() {
        let mut d = Dsp::new(Kind::Instrument);
        d.values[0] = 1.0;
        let silent = [vec![], vec![]];
        run(&mut d, 16, &silent, &[], &[on(0, 69, 1.0)]);
        assert_eq!(d.sounding(), 1);
        let out = run(&mut d, 16, &silent, &[], &[off(3, 69)]);
        assert!(out[0][..3].iter().all(|&s| s != 0.0));
        assert!(out[0][3..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn level_changes_land_on_their_sample() {
        let mut d = Dsp::new(Kind::Instrument);
        d.values[0] = 1.0;
        let silent = [vec![], vec![]];
        let change = Timed {
            at: 8,
            what: Change {
                id: LEVEL,
                value: 0.5,
            },
        };
        let out = run(&mut d, 16, &silent, &[change], &[on(0, 69, 1.0)]);
        assert_eq!(out[0][7], 1.0);
        assert_eq!(out[0][8], 0.5);
    }

    #[test]
    fn splitting_a_block_does_not_change_the_sound() {
        let silent = [vec![], vec![]];
        let mut whole = Dsp::new(Kind::Instrument);
        whole.values = [1.0, 1.0];
        let all = run(&mut whole, 200, &silent, &[], &[on(3, 60, 0.8)]);
        let mut split = Dsp::new(Kind::Instrument);
        split.values = [1.0, 1.0];
        let mut halves = run(&mut split, 77, &silent, &[], &[on(3, 60, 0.8)]);
        let rest = run(&mut split, 123, &silent, &[], &[]);
        halves[0].extend(&rest[0]);
        assert_eq!(all[0], halves[0]);
    }

    #[test]
    fn the_effect_applies_gain_and_a_whole_number_delay() {
        let mut d = Dsp::new(Kind::Effect);
        d.values = [
            Kind::Effect.normalized(LEVEL, -6.0),
            Kind::Effect.normalized(LATENCY, 4.0),
        ];
        assert_eq!(d.latency(), 4);
        let mut impulse = vec![0.0; 16];
        impulse[0] = 1.0;
        let input = [impulse.clone(), impulse];
        let out = run(&mut d, 16, &input, &[], &[]);
        let gain = db_to_gain(-6.0);
        let heard: Vec<usize> = (0..16).filter(|&i| out[0][i] != 0.0).collect();
        assert_eq!(heard, [4]);
        assert!((out[0][4] - gain).abs() < 1e-6);
    }

    #[test]
    fn the_effect_works_in_place() {
        let mut d = Dsp::new(Kind::Effect);
        d.values[1] = 0.0;
        let mut buf = [vec![0.5_f32; 8], vec![-0.5_f32; 8]];
        let p = [buf[0].as_mut_ptr(), buf[1].as_mut_ptr()];
        // SAFETY: both buffers hold 8 samples; input and output are shared.
        unsafe { d.render(8, [p[0], p[1]], p, &[], &[]) };
        assert!(buf[0].iter().all(|&s| s == 0.5));
        assert!(buf[1].iter().all(|&s| s == -0.5));
    }

    #[test]
    fn reset_silences_but_keeps_parameters() {
        let mut d = Dsp::new(Kind::Instrument);
        d.values[0] = 0.9;
        let silent = [vec![], vec![]];
        run(&mut d, 8, &silent, &[], &[on(0, 69, 1.0)]);
        d.reset();
        assert_eq!(d.sounding(), 0);
        assert_eq!(d.values[0], 0.9);
    }
}
