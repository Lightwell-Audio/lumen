use crate::analyzer::Tap;
use crate::effect::{Block, Effect, Key, Stereo};
use crate::params::{self, Glide};

use super::bands::{self, Kind, Section};
use super::{
    ALL_SECTIONS, ANALYZER, BANDS, OUTPUT, TABLE, freq_of, gain_of, is_cut, on_of, q_of, type_of,
};

const KNOT: i64 = 32;

fn db_to_gain(db: f32) -> f32 {
    10f64.powf(f64::from(db) / 20.0) as f32
}

#[derive(Debug, Clone, Copy, Default)]
struct Layout {
    freq: usize,
    gain: usize,
    q: usize,
    kind: usize,
    on: usize,
    first: usize,
    count: usize,
}

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

#[derive(Debug, Clone, Copy, Default)]
struct State {
    state: [[[f32; 2]; 2]; ALL_SECTIONS],
}

pub struct Eq {
    glides: Vec<Glide>,
    curves: Box<Curves>,
    bands: [Layout; BANDS],
    output: usize,
    side: usize,
    now: [Section; ALL_SECTIONS],
    target: [Section; ALL_SECTIONS],
    step: [Section; ALL_SECTIONS],
    state: Box<State>,
    gain: (f32, f32),
    tap: Option<Tap>,
    tap_input: bool,
}

impl Eq {
    pub fn new() -> Self {
        Self::with_tap(None)
    }

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

    fn settle(&mut self, frame: usize, rate: u32) {
        for band in 0..BANDS {
            let layout = self.bands[band];
            let wet = self.curves.at(layout.on, frame);
            let setting = self.setting(band, frame);
            let mut sections = [Section::FLAT; bands::MOST_SECTIONS];
            let count = if wet > 0.0 {
                setting.sections(rate, &mut sections)
            } else {
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

fn delta(from: Section, to: Section, share: f32) -> Section {
    Section {
        b0: (to.b0 - from.b0) * share,
        b1: (to.b1 - from.b1) * share,
        b2: (to.b2 - from.b2) * share,
        a1: (to.a1 - from.a1) * share,
        a2: (to.a2 - from.a2) * share,
    }
}

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
        let input = TABLE[side].value(curves.at(side, 0)) < 0.5;
        self.tap_input = input;
        if let Some(tap) = self.tap.as_mut()
            && tap.is_on()
            && input
        {
            tap.write(left, right);
        }

        let mut knot = (KNOT - block.start.rem_euclid(KNOT)) as usize % KNOT as usize;
        let slots: [usize; BANDS] = std::array::from_fn(|band| self.bands[band].on);
        let mut gain = self.gain;
        for frame in 0..frames {
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
        self.gain = (f32::NAN, 1.0);
    }
}

fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}
