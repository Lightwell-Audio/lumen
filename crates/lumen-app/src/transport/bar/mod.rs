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

const OVERDUB_HINT: &str = "On: a take adds to what is under it. Off: a take replaces it.";

fn keyboard_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::CAPTION)
}

fn value_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::BODY)
}

fn unit_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::MONO_SMALL)
}

fn tap_width(ui: &Ui) -> f32 {
    let text =
        fit::text_width(ui, "Tap", unit_font()).max(fit::text_width(ui, "999", value_font()));
    text + 2.0 * theme::space::S
}

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
