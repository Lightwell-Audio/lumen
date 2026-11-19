use eframe::egui::Color32;
use lumen_devices::eq::{self, Kind};

use crate::chain::face::Values;
use crate::theme::personality as p;

pub(super) const SLOPE_LEAST: f32 = 12.0;
pub(super) const SLOPE_MOST: f32 = 48.0;
pub(super) const SLOPE_STEP: f32 = 12.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct Band {
    pub number: usize,
    pub kind: Kind,
    pub on: bool,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub slope: f32,
}

pub(super) fn band(values: &Values<'_>, index: usize) -> Band {
    let number = index + 1;
    let kind = if number == 1 {
        Kind::LowCut
    } else if number == eq::BANDS {
        Kind::HighCut
    } else {
        Kind::of_choice(values.plain(eq::type_of(number)).max(0.0).round() as usize)
    };
    Band {
        number,
        kind,
        on: values.norm(eq::on_of(number)) > 0.5,
        freq: values.plain(eq::freq_of(number)),
        gain: if kind.has_gain() {
            values.plain(eq::gain_of(number))
        } else {
            0.0
        },
        q: if kind.has_q() {
            values.plain(eq::q_of(number))
        } else {
            1.0
        },
        slope: if kind.has_slope() {
            values.plain(eq::gain_of(number))
        } else {
            SLOPE_LEAST
        },
    }
}

impl Band {
    pub(super) fn setting(&self) -> eq::bands::Setting {
        eq::bands::Setting {
            kind: self.kind,
            freq: self.freq,
            gain: self.gain,
            q: self.q,
            slope: self.slope,
        }
    }

    pub(super) fn words(&self) -> String {
        let mut words = format!("Band {}, {}", self.number, self.kind.word());
        if !self.on {
            words.push_str(", off");
        }
        words.push_str(", ");
        words.push_str(&eq::hz_text(self.freq));
        if self.kind.has_gain() {
            words.push_str(", ");
            words.push_str(&eq::db_text(self.gain));
        }
        if self.kind.has_slope() {
            words.push_str(", ");
            words.push_str(&eq::slope_text(self.slope));
        }
        if self.kind.has_q() {
            words.push_str(&format!(", Q {}", eq::q_text(self.q)));
        }
        words
    }

    pub(super) fn chip(&self) -> String {
        let mut words = eq::hz_text(self.freq);
        if self.kind.has_gain() {
            words.push(' ');
            words.push_str(&format!("{:+.1}", self.gain));
        } else if self.kind.has_slope() {
            words.push(' ');
            words.push_str(&eq::slope_text(self.slope));
        }
        if self.kind.has_q() && !self.kind.has_gain() {
            words.push_str(&format!(" Q {}", eq::q_text(self.q)));
        }
        words
    }

    pub(super) fn has_stem(&self) -> bool {
        self.kind.has_gain() || self.kind == Kind::Notch
    }

    pub(super) fn half_width_hz(&self) -> Option<(f32, f32)> {
        if !self.kind.has_q() {
            return None;
        }
        let q = self.q.max(1e-3);
        let low = self.freq * ((1.0 + 1.0 / (4.0 * q * q)).sqrt() - 1.0 / (2.0 * q));
        Some((low, low + self.freq / q))
    }
}

pub(super) fn colour(index: usize) -> Color32 {
    p::EQ_BANDS[index.min(p::EQ_BANDS.len() - 1)]
}

pub(super) fn sweep_at(share: f32) -> Color32 {
    let sweep = p::EQ_SWEEP;
    let at = (share.clamp(0.0, 1.0) * (sweep.len() - 1) as f32).min(sweep.len() as f32 - 1.001);
    let low = at.floor() as usize;
    sweep[low].lerp_to_gamma(sweep[low + 1], at - low as f32)
}
