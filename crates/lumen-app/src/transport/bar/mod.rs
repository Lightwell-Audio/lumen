//! The transport bar's controls: buttons, tempo, time signature, Follow
//! and the position field. `layout` is the bar and what fits in it, `controls`
//! its buttons, `numbers` the numeric well, `position` the position piece and
//! `menus` the metronome's and the loop's menus.

use eframe::egui::{self, Key, RichText, Ui};
use eframe::emath::GuiRounding as _;

use crate::commands::{Command as Run, Keymap};
use crate::widgets::Tip;
use lumen_core::time::Ticks;
use lumen_engine::{ClickSound, Command, TransportSnapshot};

use crate::audio::Status;
use crate::icons::{self, Icon, Look};
use crate::recording::record_settings::{CountIn, RecordSettings};
use crate::{motion, position, theme, widgets};

use super::record::{self, Phase};
use super::{Transport, fit};

/// Overdub's tooltip: what the switch decides about a take.
const OVERDUB_HINT: &str = "On: a take adds to what is under it. Off: a take replaces it.";

/// The keyboard readout's type.
fn keyboard_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::CAPTION)
}

/// The type of a readout's values: Martian Mono 13, so numbers never
/// jitter (the Transport and Field cards).
fn value_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::BODY)
}

/// The type of a readout's units (" BPM"), in `text_hint`.
fn unit_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::MONO_SMALL)
}

/// The Tap piece's width: room for "Tap" or a count of up to three digits.
fn tap_width(ui: &Ui) -> f32 {
    let text =
        fit::text_width(ui, "Tap", unit_font()).max(fit::text_width(ui, "999", value_font()));
    text + 2.0 * theme::space::S
}

/// The room beside a readout's values inside its well (`.lm-display`:
/// `space_m`; the compact bar tightens it to `space_s`, so Record and
/// Overdub fit beside the position at 960 and the values keep clear of the
/// well's rounded ends; `space_xs` read as about to overflow, owner).
fn well_pad(compact: bool) -> f32 {
    if compact {
        theme::space::S
    } else {
        theme::space::M
    }
}

pub use menus::punch_menu;

mod controls;
mod layout;
mod menus;
mod numbers;
mod readout;
