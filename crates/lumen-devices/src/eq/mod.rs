pub mod bands;
pub mod curve;
mod device;
mod table;
mod words;

use crate::analyzer::{self, Tap};
use crate::effect::{Description, Values};
use crate::params;

pub use bands::{Kind, Section, band_db};
pub use curve::{Curve, Sample};
pub use device::Eq;
pub use table::TABLE;
pub use words::{db_text, hz_parse, hz_text, q_text, slope_text};

pub const NAME: &str = "eq";

pub const OUTPUT: u32 = 1;
pub const ANALYZER: u32 = 2;

pub const BANDS: usize = 8;
pub const ALL_SECTIONS: usize = 14;

pub const fn field_of(band: usize, field: u32) -> u32 {
    10 * band as u32 + field
}

pub const fn freq_of(band: usize) -> u32 {
    field_of(band, 1)
}

pub const fn gain_of(band: usize) -> u32 {
    field_of(band, 2)
}

pub const fn q_of(band: usize) -> u32 {
    field_of(band, 3)
}

pub const fn type_of(band: usize) -> u32 {
    field_of(band, 4)
}

pub const fn on_of(band: usize) -> u32 {
    field_of(band, 5)
}

pub fn is_cut(band: usize) -> bool {
    band == 1 || band == BANDS
}

pub const LOW_HZ: f32 = 20.0;
pub const HIGH_HZ: f32 = 20_000.0;
pub const LOW_Q: f32 = 0.1;
pub const HIGH_Q: f32 = 20.0;
pub const MOST_DB: f32 = 24.0;

pub const SLOPES: [&str; 4] = ["12 dB/oct", "24 dB/oct", "36 dB/oct", "48 dB/oct"];
pub const ON_WORDS: [&str; 2] = ["Off", "On"];
pub const SIDES: [&str; 2] = ["Input", "Output"];

pub static DESCRIPTION: Description = Description {
    name: NAME,
    table: TABLE,
    latency,
    tail,
};

pub fn latency(_values: Values<'_>) -> u32 {
    0
}

pub fn tail(_values: Values<'_>) -> u32 {
    0
}

pub fn axis(hz: f32) -> f32 {
    (hz / LOW_HZ).log10() / (HIGH_HZ / LOW_HZ).log10()
}

pub fn axis_hz(share: f32) -> f32 {
    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(share.clamp(0.0, 1.0))
}

pub fn response(values: Values<'_>, rate: u32, out: &mut [f32], (low, high): (f32, f32)) {
    let (settings, output_db) = settings_of(values);
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

pub fn settings_of(values: Values<'_>) -> ([(bands::Setting, bool); BANDS], f32) {
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
        *band = (setting, read(on_of(number)) >= 0.5);
    }
    let output = params::normalised_of(TABLE, OUTPUT, values);
    let output_db = params::find(TABLE, OUTPUT).map_or(0.0, |info| info.value(output));
    (settings, output_db)
}

fn flat_setting() -> bands::Setting {
    bands::Setting {
        kind: Kind::Bell,
        freq: 1_000.0,
        gain: 0.0,
        q: 1.0,
        slope: 12.0,
    }
}

pub fn pair() -> (Tap, analyzer::Reader) {
    analyzer::pair()
}

#[cfg(test)]
mod tests;
