//! Iris, the parametric equaliser (M8 slice B1, ADR-0146): eight bands — a low
//! cut and a high cut at 12 to 48 dB an octave, a low and a high shelf, bells
//! and notches — an output gain, and an analyser.
//!
//! The signal runs through the bands in series. Each band's coefficients glide
//! from where they were to where its settings say, along a straight line
//! spread over the frames until the next knot of the song's own clock, so a
//! frequency that a lane sweeps never steps the sound; a band that is turned on
//! or off is mixed in and out over the `on` parameter's own glide, so nothing
//! clicks. Both are functions of the song's sample alone, so any block size
//! plays the same sound.
//!
//! The analyser (either side of the bands, by its own parameter) writes into a
//! lock-free window ([`crate::analyzer`]) that the face reads: nothing here
//! allocates after its constructor, and nothing blocks.

pub mod bands;

use crate::analyzer::{self, Tap};
use crate::effect::{Block, Description, Effect, Key, Stereo, Values};
use crate::params::{self, Glide, ParamInfo, Taper};

pub use bands::{Kind, Section, band_db};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
pub const NAME: &str = "eq";

/// The output gain, in decibels.
pub const OUTPUT: u32 = 1;
/// Which side of the bands the analyser shows: the input or the output.
pub const ANALYZER: u32 = 2;

/// How many bands there are.
pub const BANDS: usize = 8;
/// Every band's sections, for the array the filter runs: a 48 dB/octave cut is
/// four, each middle band one, and the other cut four.
pub const ALL_SECTIONS: usize = 14;

/// The frames between two settlings of the coefficients: a knot of the song's
/// own clock, so a block boundary never moves one (as the mixer's automation
/// knots).
const KNOT: i64 = 32;

/// Band `band`'s parameter `field`: 1 frequency, 2 gain (or slope, for the
/// cuts), 3 Q, 4 type, 5 on. Ids are stable and never reused; a cut has no
/// gain, Q or type, so those numbers in its band are never named.
pub const fn field_of(band: usize, field: u32) -> u32 {
    10 * band as u32 + field
}

/// Band `band`'s frequency.
pub const fn freq_of(band: usize) -> u32 {
    field_of(band, 1)
}

/// Band `band`'s gain, or its slope when it is a cut.
pub const fn gain_of(band: usize) -> u32 {
    field_of(band, 2)
}

/// Band `band`'s Q.
pub const fn q_of(band: usize) -> u32 {
    field_of(band, 3)
}

/// Band `band`'s type, one of [`Kind::CHOICES`].
pub const fn type_of(band: usize) -> u32 {
    field_of(band, 4)
}

/// Band `band`'s on/off switch.
pub const fn on_of(band: usize) -> u32 {
    field_of(band, 5)
}

/// Whether `band` is one of the two cuts, which have a slope and no gain.
pub fn is_cut(band: usize) -> bool {
    band == 1 || band == BANDS
}

/// The lowest and highest frequency a band takes.
pub const LOW_HZ: f32 = 20.0;
pub const HIGH_HZ: f32 = 20_000.0;
/// The lowest and highest Q.
pub const LOW_Q: f32 = 0.1;
pub const HIGH_Q: f32 = 20.0;
/// The gain range of a band and of the output.
pub const MOST_DB: f32 = 24.0;

/// The slope choices, 12 to 48 dB an octave.
pub const SLOPES: [&str; 4] = ["12 dB/oct", "24 dB/oct", "36 dB/oct", "48 dB/oct"];
/// The on/off words.
pub const ON_WORDS: [&str; 2] = ["Off", "On"];
/// The analyser's two sides.
pub const SIDES: [&str; 2] = ["Input", "Output"];

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

/// Iris's parameters, in the order the card shows them. Ids are stable and
/// never reused.
///
/// The strip's first eight are the output and the first six band controls a
/// person meets: the low cut's three, then a bell's beginning.
pub static TABLE: &[ParamInfo] = &[
    param(
        OUTPUT,
        "Output",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        freq_of(1),
        "1 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        30.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(1),
        "1 Slope",
        "dB/oct",
        12.0,
        48.0,
        Taper::Linear,
        24.0,
        3,
        &SLOPES,
        slope_text,
        slope_parse,
    ),
    param(
        on_of(1),
        "1 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(2),
        "2 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        100.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(2),
        "2 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(2),
        "2 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(2),
        "2 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(2),
        "2 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(3),
        "3 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        250.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(3),
        "3 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(3),
        "3 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(3),
        "3 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(3),
        "3 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(4),
        "4 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        630.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(4),
        "4 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(4),
        "4 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(4),
        "4 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(4),
        "4 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(5),
        "5 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        1_600.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(5),
        "5 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(5),
        "5 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(5),
        "5 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(5),
        "5 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(6),
        "6 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        4_000.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(6),
        "6 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(6),
        "6 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(6),
        "6 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(6),
        "6 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(7),
        "7 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        10_000.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(7),
        "7 Gain",
        "dB",
        -MOST_DB,
        MOST_DB,
        Taper::Linear,
        0.0,
        0,
        &[],
        db_text,
        db_parse,
    ),
    param(
        q_of(7),
        "7 Q",
        "",
        LOW_Q,
        HIGH_Q,
        Taper::Log,
        1.0,
        0,
        &[],
        q_text,
        q_parse,
    ),
    param(
        type_of(7),
        "7 Type",
        "",
        0.0,
        3.0,
        Taper::Linear,
        0.0,
        3,
        &Kind::CHOICES,
        kind_text,
        kind_parse,
    ),
    param(
        on_of(7),
        "7 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        freq_of(8),
        "8 Freq",
        "Hz",
        LOW_HZ,
        HIGH_HZ,
        Taper::Log,
        18_000.0,
        0,
        &[],
        hz_text,
        hz_parse,
    ),
    param(
        gain_of(8),
        "8 Slope",
        "dB/oct",
        12.0,
        48.0,
        Taper::Linear,
        24.0,
        3,
        &SLOPES,
        slope_text,
        slope_parse,
    ),
    param(
        on_of(8),
        "8 On",
        "",
        0.0,
        1.0,
        Taper::Linear,
        0.0,
        1,
        &ON_WORDS,
        on_text,
        on_parse,
    ),
    param(
        ANALYZER,
        "Analyzer",
        "",
        0.0,
        1.0,
        Taper::Linear,
        1.0,
        1,
        &SIDES,
        side_text,
        side_parse,
    ),
];

/// Iris's description: its table, and a timing that no value of its moves. An
/// equaliser holds nothing back and rings on for nothing a stop should wait
/// for.
pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

/// Frames Iris's output trails its input: none, whatever its values say.
pub fn latency(_values: Values<'_>) -> u32 {
    0
}

/// Frames Iris rings on after its input stops: none.
pub fn tail(_values: Values<'_>) -> u32 {
    0
}

/// "40 Hz", "1.20 kHz", "20.0 kHz".
pub fn hz_text(hz: f32) -> String {
    if hz < 999.5 {
        if hz < 99.95 {
            format!("{hz:.1} Hz")
        } else {
            format!("{hz:.0} Hz")
        }
    } else if hz < 9_999.5 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

/// "40", "40 Hz", "1.2 kHz", "1200": a frequency in hertz.
pub fn hz_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    let (body, scale) = if let Some(rest) = t.strip_suffix("khz") {
        (rest.trim(), 1000.0)
    } else if let Some(rest) = t.strip_suffix('k') {
        (rest.trim(), 1000.0)
    } else if let Some(rest) = t.strip_suffix("hz") {
        (rest.trim(), 1.0)
    } else {
        (t.as_str(), 1.0)
    };
    body.trim()
        .parse::<f32>()
        .ok()
        .filter(|hz| hz.is_finite())
        .map(|hz| hz * scale)
        .filter(|hz| *hz > 0.0)
}

/// "0.0 dB", "−6.0 dB".
pub fn db_text(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// "−6", "−6.0 dB": decibels, either minus, with or without the unit.
fn db_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-");
    let t = t
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim_end_matches("DB")
        .trim();
    t.parse::<f32>()
        .ok()
        .filter(|db| db.is_finite() && db.abs() <= MOST_DB * 2.0)
}

/// "1.00", "0.71", "12.0".
pub fn q_text(q: f32) -> String {
    if q < 9.995 {
        format!("{q:.2}")
    } else {
        format!("{q:.1}")
    }
}

/// "1.4", "4": a Q.
fn q_parse(text: &str) -> Option<f32> {
    let t = text.trim();
    let t = t
        .strip_prefix('Q')
        .or_else(|| t.strip_prefix('q'))
        .unwrap_or(t)
        .trim();
    t.parse::<f32>().ok().filter(|q| q.is_finite() && *q > 0.0)
}

/// "24/oct": short enough for a strip's value line; All parameters shows the
/// choice's own words.
pub fn slope_text(slope: f32) -> String {
    format!("{slope:.0}/oct")
}

/// "24", "24 dB/oct", "24/oct".
fn slope_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t
        .strip_suffix("db/oct")
        .or_else(|| t.strip_suffix("/oct"))
        .or_else(|| t.strip_suffix("db"))
        .unwrap_or(&t)
        .trim();
    t.parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0)
}

/// The band type's word.
fn kind_text(index: f32) -> String {
    let index = index.round().max(0.0) as usize;
    Kind::CHOICES
        .get(index)
        .copied()
        .unwrap_or(Kind::CHOICES[0])
        .to_owned()
}

/// "Bell", "notch", "shelf high".
fn kind_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    Kind::CHOICES
        .iter()
        .position(|word| word.to_lowercase() == t)
        .map(|index| index as f32)
}

fn on_text(value: f32) -> String {
    if value >= 0.5 { "On" } else { "Off" }.to_owned()
}

fn on_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "on" | "1" => Some(1.0),
        "off" | "0" => Some(0.0),
        _ => None,
    }
}

fn side_text(value: f32) -> String {
    let index = if value >= 0.5 { 1 } else { 0 };
    SIDES[index].to_owned()
}

fn side_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "input" | "pre" | "0" => Some(0.0),
        "output" | "post" | "1" => Some(1.0),
        _ => None,
    }
}

/// A gain in decibels as a factor.
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

/// Where an analyser's window is spread over the frequency axis a face draws:
/// 20 Hz to 20 kHz, and the share of the way across a frequency is.
pub fn axis(hz: f32) -> f32 {
    (hz / LOW_HZ).log10() / (HIGH_HZ / LOW_HZ).log10()
}

/// The frequency at `share` of the way across the face's frequency axis.
pub fn axis_hz(share: f32) -> f32 {
    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(share.clamp(0.0, 1.0))
}

/// The whole EQ's response at `hz`, in dB, from the values a project holds:
/// what the face draws. Worked out from each band's own settings, so it is the
/// same answer the running filter gives.
pub fn response(values: Values<'_>, rate: u32, out: &mut [f32], (low, high): (f32, f32)) {
    let mut settings: [(bands::Setting, bool); BANDS] = [(flat_setting(), false); BANDS];
    for (index, band) in settings.iter_mut().enumerate() {
        let number = index + 1;
        let read = |id: u32| params::normalised_of(TABLE, id, values);
        let value = |id: u32| params::find(TABLE, id).map_or(0.0, |info| info.value(read(id)));
        let mut setting = bands::Setting {
            kind: Kind::Bell,
            freq: value(freq_of(number)),
            gain: 0.0,
            q: 1.0,
            slope: 12.0,
        };
        if number == 1 {
            setting.kind = Kind::LowCut;
            setting.slope = value(gain_of(number));
        } else if number == BANDS {
            setting.kind = Kind::HighCut;
            setting.slope = value(gain_of(number));
        } else {
            setting.kind = Kind::of_choice(value(type_of(number)).max(0.0) as usize);
            setting.gain = value(gain_of(number));
            setting.q = value(q_of(number));
        }
        *band = (setting, read(on_of(number)) > 0.0);
    }
    let output = params::normalised_of(TABLE, OUTPUT, values);
    let output_db = params::find(TABLE, OUTPUT).map_or(0.0, |info| info.value(output));
    let points = out.len();
    for (i, point) in out.iter_mut().enumerate() {
        let share = if points > 1 {
            i as f32 / (points - 1) as f32
        } else {
            0.0
        };
        let hz = low * (high / low).powf(share);
        let mut db = output_db;
        for (setting, on) in settings {
            if on {
                db += band_db(&setting, rate, hz);
            }
        }
        *point = db;
    }
}

/// A settings row nothing has set: a bell that does nothing.
fn flat_setting() -> bands::Setting {
    bands::Setting {
        kind: Kind::Bell,
        freq: 1_000.0,
        gain: 0.0,
        q: 1.0,
        slope: 12.0,
    }
}

/// A tap for Iris's analyser, and the reader its face draws from.
pub fn pair() -> (Tap, analyzer::Reader) {
    analyzer::pair()
}

#[cfg(test)]
mod tests;
