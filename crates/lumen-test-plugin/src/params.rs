//! The parameters of the two test plugins, and how their normalized values
//! (0 to 1, what a host stores) map to plain values (what the DSP uses).

/// Which plugin a component or controller belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Instrument,
    Effect,
}

/// The parameter every plugin has: instrument level or effect gain.
pub const LEVEL: u32 = 0;
/// The instrument's waveform (0 square, 1 saw).
pub const WAVE: u32 = 1;
/// The effect's delay in samples, reported to the host as latency.
pub const LATENCY: u32 = 1;
/// The longest delay the effect offers.
pub const MAX_LATENCY: u32 = 4096;

const GAIN_FLOOR_DB: f64 = -60.0;
const GAIN_CEIL_DB: f64 = 12.0;

pub struct Param {
    pub id: u32,
    pub title: &'static str,
    pub units: &'static str,
    /// 0 for continuous, else the number of steps.
    pub steps: i32,
    pub default: f64,
}

const INSTRUMENT: [Param; 2] = [
    Param {
        id: LEVEL,
        title: "Level",
        units: "",
        steps: 0,
        default: 0.5,
    },
    Param {
        id: WAVE,
        title: "Wave",
        units: "",
        steps: 1,
        default: 0.0,
    },
];

const EFFECT: [Param; 2] = [
    Param {
        id: LEVEL,
        title: "Gain",
        units: "dB",
        steps: 0,
        default: -GAIN_FLOOR_DB / (GAIN_CEIL_DB - GAIN_FLOOR_DB),
    },
    Param {
        id: LATENCY,
        title: "Latency",
        units: "samples",
        steps: MAX_LATENCY as i32,
        default: 0.0,
    },
];

impl Kind {
    pub fn params(self) -> &'static [Param] {
        match self {
            Kind::Instrument => &INSTRUMENT,
            Kind::Effect => &EFFECT,
        }
    }

    pub fn defaults(self) -> [f64; 2] {
        let p = self.params();
        [p[0].default, p[1].default]
    }

    /// The plain value a normalized one stands for: a linear level, a gain
    /// in dB, a waveform index or a delay in samples.
    pub fn plain(self, id: u32, normalized: f64) -> f64 {
        let n = normalized.clamp(0.0, 1.0);
        match (self, id) {
            (Kind::Instrument, LEVEL) => n,
            (Kind::Instrument, WAVE) => n.round(),
            (Kind::Effect, LEVEL) => GAIN_FLOOR_DB + n * (GAIN_CEIL_DB - GAIN_FLOOR_DB),
            (Kind::Effect, LATENCY) => (n * f64::from(MAX_LATENCY)).round(),
            _ => 0.0,
        }
    }

    pub fn normalized(self, id: u32, plain: f64) -> f64 {
        match (self, id) {
            (Kind::Instrument, LEVEL) => plain.clamp(0.0, 1.0),
            (Kind::Instrument, WAVE) => plain.clamp(0.0, 1.0).round(),
            (Kind::Effect, LEVEL) => {
                ((plain - GAIN_FLOOR_DB) / (GAIN_CEIL_DB - GAIN_FLOOR_DB)).clamp(0.0, 1.0)
            }
            (Kind::Effect, LATENCY) => (plain / f64::from(MAX_LATENCY)).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// The text a host shows for a normalized value.
    pub fn display(self, id: u32, normalized: f64) -> String {
        let plain = self.plain(id, normalized);
        match (self, id) {
            (Kind::Instrument, WAVE) => if plain < 0.5 { "Square" } else { "Saw" }.to_string(),
            (Kind::Effect, LEVEL) => format!("{plain:.1}"),
            (Kind::Effect, LATENCY) => format!("{plain:.0}"),
            _ => format!("{plain:.2}"),
        }
    }

    /// The normalized value a typed string means, if it is a number (or a
    /// wave name).
    pub fn parse(self, id: u32, text: &str) -> Option<f64> {
        let text = text.trim();
        if self == Kind::Instrument && id == WAVE {
            return match text.to_ascii_lowercase().as_str() {
                "square" => Some(0.0),
                "saw" => Some(1.0),
                _ => None,
            };
        }
        text.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| self.normalized(id, v))
    }
}

/// A linear gain from decibels.
pub fn db_to_gain(db: f64) -> f32 {
    10_f64.powf(db / 20.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mean_unity_gain_and_no_delay() {
        let [gain, latency] = Kind::Effect.defaults();
        assert!(Kind::Effect.plain(LEVEL, gain).abs() < 1e-12);
        assert_eq!(db_to_gain(0.0), 1.0);
        assert_eq!(Kind::Effect.plain(LATENCY, latency), 0.0);
    }

    #[test]
    fn plain_and_normalized_are_inverses() {
        for kind in [Kind::Instrument, Kind::Effect] {
            for p in kind.params() {
                for n in [0.0, 0.25, 0.5, 1.0] {
                    let plain = kind.plain(p.id, n);
                    let back = kind.normalized(p.id, plain);
                    let step = if p.steps > 0 {
                        1.0 / p.steps as f64
                    } else {
                        1e-9
                    };
                    assert!((back - n).abs() <= step, "{kind:?} {} {n}", p.title);
                }
            }
        }
    }

    #[test]
    fn display_and_parse_agree() {
        let k = Kind::Effect;
        assert_eq!(k.display(LATENCY, k.normalized(LATENCY, 128.0)), "128");
        assert_eq!(k.parse(LATENCY, "128"), Some(k.normalized(LATENCY, 128.0)));
        assert_eq!(Kind::Instrument.parse(WAVE, "Saw"), Some(1.0));
        assert_eq!(k.parse(LEVEL, "loud"), None);
        assert_eq!(k.parse(LEVEL, "NaN"), None);
    }

    #[test]
    fn out_of_range_values_clamp() {
        assert_eq!(Kind::Effect.plain(LATENCY, 7.0), f64::from(MAX_LATENCY));
        assert_eq!(Kind::Instrument.plain(LEVEL, -1.0), 0.0);
    }
}
