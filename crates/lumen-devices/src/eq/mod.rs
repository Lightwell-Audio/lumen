//! Iris, the parametric equaliser (M8 slice B1, ADR-0146): eight bands — a low
//! cut and a high cut at 12 to 48 dB an octave, a low and a high shelf, bells
//! and notches — an output gain, and an analyser.
//!
//! The bands are here as a table ([`table`]), as filter maths ([`bands`]) and
//! as a running device ([`device`]); what this file keeps is what the rest of
//! the crate and the interface name: the ids, the words, the description the
//! engine reads for a latency and a tail, the face's axis, and the whole EQ's
//! response.
//!
//! The analyser (either side of the bands, by its own parameter) writes into a
//! lock-free window ([`crate::analyzer`]) that the face reads.

pub mod bands;
mod device;
mod table;
mod words;

use crate::analyzer::{self, Tap};
use crate::effect::{Description, Values};
use crate::params;

pub use bands::{Kind, Section, band_db};
pub use device::Eq;
pub use table::TABLE;
pub use words::{db_text, hz_parse, hz_text, q_text, slope_text};

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
