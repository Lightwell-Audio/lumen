//! Iris's running filter (M8 slice B1, ADR-0146): the bands' coefficients and
//! the audio they shape.
//!
//! The signal runs through the bands in series. Each band's coefficients glide
//! from where they were to where its settings say, along a straight line
//! spread over the frames until the next knot of the song's own clock, so a
//! frequency that a lane sweeps never steps the sound; a band that is turned on
//! or off is mixed in and out over the `on` parameter's own glide, so nothing
//! clicks. Both are functions of the song's sample alone, so any block size
//! plays the same sound.
//!
//! Nothing here allocates after the constructor, and nothing blocks.

use crate::analyzer::Tap;
use crate::effect::{Block, Effect, Key, Stereo};
use crate::params::{self, Glide};

use super::bands::{self, Kind, Section};
use super::{
    ALL_SECTIONS, ANALYZER, BANDS, OUTPUT, TABLE, freq_of, gain_of, is_cut, on_of, q_of, type_of,
};

/// The frames between two settlings of the coefficients: a knot of the song's
/// own clock, so a block boundary never moves one (as the mixer's automation
/// knots).
const KNOT: i64 = 32;

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Where each band's parameters are in [`TABLE`], worked out once: the frame
/// loop reads thirty-eight curves a block and cannot search for them.
#[derive(Debug, Clone, Copy, Default)]
struct Layout {
    freq: usize,
    gain: usize,
    q: usize,
    kind: usize,
    on: usize,
    /// The first section this band runs, and how many.
    first: usize,
    count: usize,
}

/// The block's parameter curves, by table slot: `TABLE.len()` rows of
/// [`crate::MOST_FRAMES`]. Allocated once, in the constructor.
struct Curves {
    values: Vec<f32>,
}

impl Curves {
    fn new() -> Self {
        Self {
            values: vec![0.0; TABLE.len() * crate::MOST_FRAMES],
        }
    }

    fn at(&self, slot: usize, frame: usize) -> f32 {
        self.values[slot * crate::MOST_FRAMES + frame]
    }
}

/// Each band's sections, per channel, in transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
struct State {
    /// `state[section][channel]` is `(s1, s2)`.
    state: [[[f32; 2]; 2]; ALL_SECTIONS],
}

/// Iris.
pub struct Eq {
    /// Where each parameter is, by table slot.
    glides: Vec<Glide>,
    /// The block's curves, by table slot.
    curves: Box<Curves>,
    /// Where each band's parameters are in the table, and its sections in the
    /// filter.
    bands: [Layout; BANDS],
    output: usize,
    side: usize,
    /// The filter's coefficients: where they are now, where the settings say
    /// they should be, and how far each moves a frame.
    now: [Section; ALL_SECTIONS],
    target: [Section; ALL_SECTIONS],
    step: [Section; ALL_SECTIONS],
    /// The filter's state.
    state: Box<State>,
    /// The output gain's last reading, and the factor it made.
    gain: (f32, f32),
    /// The analyser, when one is listening.
    tap: Option<Tap>,
    /// The analyser reads the bands' input rather than their output.
    tap_input: bool,
}

impl Eq {
    /// Iris at its defaults: flat, with the analyser off. Allocates (its
    /// curves): call it on the interface side.
    pub fn new() -> Self {
        Self::with_tap(None)
    }

    /// Iris writing its analyser into `tap`.
    pub fn with_tap(tap: Option<Tap>) -> Self {
        let mut glides = Vec::with_capacity(TABLE.len());
        for info in TABLE {
            glides.push(Glide::new(info.normalised(info.default)));
        }
        let mut bands = [Layout::default(); BANDS];
        let mut first = 0;
        for (index, layout) in bands.iter_mut().enumerate() {
            let band = index + 1;
            let slot = |id: u32| TABLE.iter().position(|info| info.id == id);
            *layout = Layout {
                freq: slot(freq_of(band)).expect("every band has a frequency"),
                gain: slot(gain_of(band)).expect("every band has a second control"),
                q: slot(q_of(band)).unwrap_or(0),
                kind: slot(type_of(band)).unwrap_or(0),
                on: slot(on_of(band)).expect("every band has a switch"),
                first,
                count: 0,
            };
            first += if is_cut(band) { 4 } else { 1 };
        }
        Self {
            glides,
            curves: Box::new(Curves::new()),
            bands,
            output: TABLE.iter().position(|info| info.id == OUTPUT).unwrap_or(0),
            side: TABLE
                .iter()
                .position(|info| info.id == ANALYZER)
                .unwrap_or(0),
            now: [Section::FLAT; ALL_SECTIONS],
            target: [Section::FLAT; ALL_SECTIONS],
            step: [Section::FLAT; ALL_SECTIONS],
            state: Box::new(State::default()),
            gain: (0.0, 1.0),
            tap,
            tap_input: true,
        }
    }

    /// A band's settings at `frame` of the block, in its own units.
    fn setting(&self, band: usize, frame: usize) -> bands::Setting {
        let layout = self.bands[band];
        let value = |slot: usize| TABLE[slot].value(self.curves.at(slot, frame));
        let number = band + 1;
        let kind = if number == 1 {
            Kind::LowCut
        } else if number == BANDS {
            Kind::HighCut
        } else {
            Kind::of_choice(value(layout.kind).max(0.0) as usize)
        };
        bands::Setting {
            kind,
            freq: value(layout.freq),
            gain: if is_cut(number) {
                0.0
            } else {
                value(layout.gain)
            },
            q: if is_cut(number) { 0.7 } else { value(layout.q) },
            slope: if is_cut(number) {
                value(layout.gain)
            } else {
                12.0
            },
        }
    }

    /// Works out where each band's coefficients are heading from `frame`, and
    /// how far each moves a frame until the next knot.
    fn settle(&mut self, frame: usize, rate: u32) {
        for band in 0..BANDS {
            let layout = self.bands[band];
            let wet = self.curves.at(layout.on, frame);
            let setting = self.setting(band, frame);
            let mut sections = [Section::FLAT; bands::MOST_SECTIONS];
            let count = if wet > 0.0 {
                setting.sections(rate, &mut sections)
            } else {
                // A band that is off passes everything, so turning it on
                // glides from nothing rather than stepping.
                0
            };
            for (i, section) in sections.iter().enumerate() {
                let at = layout.first + i;
                if at >= ALL_SECTIONS {
                    break;
                }
                let target = if i < count { *section } else { Section::FLAT };
                self.target[at] = target;
                self.step[at] = delta(self.now[at], target, 1.0 / KNOT as f32);
            }
            self.bands[band].count = count;
        }
    }
}

impl Default for Eq {
    fn default() -> Self {
        Self::new()
    }
}

/// A `share` of the way from `from` to `to`, coefficient by coefficient: how
/// far one frame of a glide moves.
fn delta(from: Section, to: Section, share: f32) -> Section {
    Section {
        b0: (to.b0 - from.b0) * share,
        b1: (to.b1 - from.b1) * share,
        b2: (to.b2 - from.b2) * share,
        a1: (to.a1 - from.a1) * share,
        a2: (to.a2 - from.a2) * share,
    }
}

/// This frame's coefficients, one step closer to where the settings point.
fn advance(section: &mut Section, step: Section) {
    section.b0 += step.b0;
    section.b1 += step.b1;
    section.b2 += step.b2;
    section.a1 += step.a1;
    section.a2 += step.a2;
}

impl Effect for Eq {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, _key: Option<Key<'_>>) {
        let Stereo { left, right } = io;
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if frames == 0 {
            return;
        }
        let rate = block.rate.max(1);
        // The parameter curves of the block, and which side the analyser looks
        // at.
        let (curves, side) = (&mut self.curves, self.side);
        for (slot, info) in TABLE.iter().enumerate() {
            params::values(
                &mut curves.values[slot * crate::MOST_FRAMES..][..frames],
                &mut self.glides[slot],
                block.events,
                info.id,
                rate,
            );
        }
        // `SIDES` is Input then Output: the tap writes the input first.
        let input = TABLE[side].value(curves.at(side, 0)) < 0.5;
        self.tap_input = input;
        if let Some(tap) = self.tap.as_mut()
            && tap.is_on()
            && input
        {
            tap.write(left, right);
        }

        // The first frame of the block that falls on a knot of the song's
        // clock.
        let mut knot = (KNOT - block.start.rem_euclid(KNOT)) as usize % KNOT as usize;
        let slots: [usize; BANDS] = std::array::from_fn(|band| self.bands[band].on);
        let mut gain = self.gain;
        for frame in 0..frames {
            // The coefficients are settled on knots of the song's clock, so a
            // block boundary never makes one.
            if frame == knot {
                self.settle(frame, rate);
                knot += KNOT as usize;
            }
            for section in 0..ALL_SECTIONS {
                let step = self.step[section];
                advance(&mut self.now[section], step);
            }
            let db = TABLE[self.output].value(self.curves.at(self.output, frame));
            if db != gain.0 {
                gain = (db, db_to_gain(db));
            }
            let mut l = left[frame];
            let mut r = right[frame];
            for (band, &slot) in slots.iter().enumerate() {
                let wet = self.curves.at(slot, frame);
                if wet <= 0.0 {
                    continue;
                }
                let layout = self.bands[band];
                let (dry_l, dry_r) = (l, r);
                let state = &mut self.state.state;
                for (section, place) in (layout.first..layout.first + layout.count)
                    .zip(&mut state[layout.first..layout.first + layout.count])
                {
                    let c = self.now[section];
                    let (l1, l2) = (place[0][0], place[0][1]);
                    let l_out = c.b0 * l + l1;
                    place[0][0] = flush(c.b1 * l - c.a1 * l_out + l2);
                    place[0][1] = flush(c.b2 * l - c.a2 * l_out);
                    let (r1, r2) = (place[1][0], place[1][1]);
                    let r_out = c.b0 * r + r1;
                    place[1][0] = flush(c.b1 * r - c.a1 * r_out + r2);
                    place[1][1] = flush(c.b2 * r - c.a2 * r_out);
                    l = flush(l_out);
                    r = flush(r_out);
                }
                if wet < 1.0 {
                    l = dry_l + (l - dry_l) * wet;
                    r = dry_r + (r - dry_r) * wet;
                }
            }
            left[frame] = l * gain.1;
            right[frame] = r * gain.1;
        }
        self.gain = gain;
        if let Some(tap) = self.tap.as_mut()
            && tap.is_on()
            && !input
        {
            tap.write(left, right);
        }
    }

    fn reset(&mut self) {
        for glide in &mut self.glides {
            glide.arrive();
        }
        self.now.fill(Section::FLAT);
        self.step.fill(Section::FLAT);
        self.target.fill(Section::FLAT);
        *self.state = State::default();
        // The next frame reads the gain again rather than trusting the cache.
        self.gain = (f32::NAN, 1.0);
    }
}

/// A tiny value becomes zero, so a decaying filter never reaches denormals.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}
