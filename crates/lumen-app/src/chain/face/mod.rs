//! What a built-in device draws on its card (M8, the owner's chosen card in
//! `docs/specimens/m8/chosen.html`, "Device faces"; DESIGN.md §7k).
//!
//! Every built-in has the same chrome, drawn by `chain/card/`: a header tinted
//! with the device's hue, the name in the display face with what the device is
//! under it, a kind badge ("FX"), the preset row, and a footer with the signal
//! and "All n ▸". What differs from device to device is one **drawing** on a
//! deep tinted screen (the same in both themes, as a clip is), which a device
//! declares in a [`Drawing`] and places in one of two ways:
//!
//! * [`Place::Band`], for a device on the generic strip (Prism, Parallax,
//!   Afterglow, Flare): the strip's eight knobs stay, and the drawing is the
//!   72 pt picture above the header on a whole card. A card without room for
//!   the picture (272 pt tall) or a compact one does not draw it.
//! * [`Place::Face`], for a device whose picture is its control (Iris,
//!   Gravity, Horizon, Shutter): the drawing replaces the strip, and on a
//!   whole card the picture too, so it has everything between the preset row
//!   and the footer (158 pt, or 230 pt on a whole card). Its
//!   [`Control`]s sit on the drawing: knobs in the device's hue and pills,
//!   under idea names ("Pull") with the plain name ("threshold") kept for the
//!   tooltip, All parameters and automation. A compact card folds a face back
//!   to the strip's first four knobs.
//!
//! Every knob and pill is the strip's own control (`strip::paint_cell`, which
//! uses `param_knob`'s interaction): drag, Shift fine, the wheel,
//! double-click reset, type-in, the right-click menu (automation, MIDI learn),
//! keyboard focus, accessible names and the override state are Lumen's, never
//! a second path.
//!
//! # Plugging in a device
//!
//! 1. Write `chain/face/<device>.rs` with a `pub(crate) static DRAWING:
//!    Drawing` and the `fn draw(&Scene)` it names. Sizes and colours of the
//!    card are in `theme::personality`; a drawing's own geometry stays in its
//!    file as named constants.
//! 2. Add one line to [`DRAWINGS`], keyed by the device's stable saved name.
//!
//! ```ignore
//! pub(crate) static DRAWING: Drawing = Drawing {
//!     place: Place::Face,
//!     deep: personality::SCREEN,
//!     reads: &[dynamics::GAIN_REDUCTION],
//!     draw,
//!     controls: &[
//!         Control::knob(dynamics::THRESHOLD, "Pull", |s| cell_at(s, 0.20, 1.0)),
//!         Control::pill(dynamics::TRUE_PEAK, "True", |s| cell_at(s, 0.88, 1.0)),
//!     ],
//! };
//! ```
//!
//! A `param` is the device table's id, so a value read, a drag, an automation
//! lane and a preset are the parameter's own. A readout that needs the audio's
//! live state (a gain reduction, a level) needs a channel from the engine and
//! is that device's own slice to add: a [`Scene`] carries what the interface
//! knows, which is every value the drawing `reads` or has a control for.

use eframe::egui::{Color32, Painter, Pos2, Rect, vec2};
use lumen_devices::params::ParamInfo;

use super::strip::Slot;

pub(crate) mod prism;

/// Every device with a drawing, by its stable saved name: add a line here.
const DRAWINGS: &[(&str, &Drawing)] = &[(lumen_devices::utility::NAME, &prism::DRAWING)];

/// The drawing of the device saved as `name`, if it has one.
pub(crate) fn drawing_of(name: &str) -> Option<&'static Drawing> {
    DRAWINGS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, drawing)| *drawing)
}

/// Where a drawing sits on its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// The picture above the header of a whole card, over the strip.
    Band,
    /// The whole body: no strip, and on a whole card no picture.
    Face,
}

/// What a device declares about its drawing.
pub(crate) struct Drawing {
    pub place: Place,
    /// The deep colour the screen fades to from the device's hue.
    pub deep: Color32,
    /// The parameters the drawing reads besides its controls' own.
    pub reads: &'static [u32],
    /// Draws the picture and its live readouts, clipped to the screen.
    pub draw: fn(&Scene<'_>),
    /// A face's knobs and pills (empty for a band).
    pub controls: &'static [Control],
}

impl Drawing {
    /// Every parameter the drawing reads or has a control for, controls first:
    /// the slots a row builds, in this order.
    pub(crate) fn params(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.controls.iter().map(|c| c.param).collect();
        for id in self.reads {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }
}

/// A knob or pill on a face.
pub(crate) struct Control {
    /// The device table's id.
    pub param: u32,
    /// The idea name drawn under it ("Pull"); the parameter's own name is in
    /// its tooltip and accessible name.
    pub name: &'static str,
    pub kind: Kind,
    /// Where it sits, from the screen's rectangle.
    pub place: fn(Rect) -> Rect,
}

#[allow(dead_code)] // the face half of the hook: the first Face device (Iris) is its user, and the tests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A comet knob in the device's hue.
    Knob,
    /// A pill: a switch, or a chooser's current word.
    Pill,
}

#[allow(dead_code)] // the face half of the hook: the first Face device (Iris) is its user, and the tests
impl Control {
    pub(crate) const fn knob(param: u32, name: &'static str, place: fn(Rect) -> Rect) -> Self {
        Self {
            param,
            name,
            kind: Kind::Knob,
            place,
        }
    }

    pub(crate) const fn pill(param: u32, name: &'static str, place: fn(Rect) -> Rect) -> Self {
        Self {
            param,
            name,
            kind: Kind::Pill,
            place,
        }
    }
}

#[allow(dead_code)] // the face half of the hook: the first Face device (Iris) is its user, and the tests
/// A knob's cell (the knob, its name and value) centred `across` of the way
/// along `screen` and standing on its bottom edge `up` points above it.
pub(crate) fn cell_at(screen: Rect, across: f32, up: f32) -> Rect {
    let size = vec2(CELL_W, CELL_H);
    let centre = Pos2::new(
        screen.left() + screen.width() * across,
        screen.bottom() - up - size.y / 2.0,
    );
    Rect::from_center_size(centre, size)
}

/// A face control's cell.
#[allow(dead_code)] // the face half of the hook: the first Face device (Iris) is its user, and the tests
pub(crate) const CELL_W: f32 = 46.0;
#[allow(dead_code)] // as CELL_W
pub(crate) const CELL_H: f32 = 52.0;

/// The device's current values, for the drawing.
pub(crate) struct Values<'a> {
    pub slots: &'a [Slot],
    pub table: &'static [ParamInfo],
}

impl Values<'_> {
    fn slot(&self, id: u32) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// The value, 0 to 1 (0 for a parameter the drawing did not declare).
    pub(crate) fn norm(&self, id: u32) -> f32 {
        self.slot(id).map_or(0.0, |s| s.value as f32)
    }

    /// The value in the parameter's own units (130.0 for a width of 130 %).
    pub(crate) fn plain(&self, id: u32) -> f32 {
        lumen_devices::params::find(self.table, id).map_or(0.0, |info| info.value(self.norm(id)))
    }
}

/// What a drawing is given.
pub(crate) struct Scene<'a> {
    /// Clipped to the screen.
    pub painter: &'a Painter,
    /// The screen's rectangle.
    pub rect: Rect,
    /// The device's hue (greyed while it is switched off).
    #[allow(dead_code)] // a face draws in it; Prism draws its own beams
    pub hue: Color32,
    pub values: Values<'a>,
    /// The device is bypassed or not running: draw it drained of colour.
    pub muted: bool,
}

impl Scene<'_> {
    /// `colour` as the screen shows it: drained while muted.
    pub(crate) fn ink(&self, colour: Color32) -> Color32 {
        if self.muted {
            crate::theme::grey_of(colour).gamma_multiply(MUTED)
        } else {
            colour
        }
    }
}

/// How strong a drained colour stays.
const MUTED: f32 = 0.5;

#[cfg(test)]
mod tests;
