//! Drawn faces for built-in devices whose picture is the device's idea (BF C,
//! M8 slice B2): Gravity's transfer curve over the well its signal falls into,
//! and Horizon's ceiling drawn as a horizon line over the land it flattens.
//! The two of them are in `dynamics`.
//!
//! A face takes the card's parameter strip: above it the device's own screen,
//! under it the four parameters it draws as Lumen's controls, so every gesture
//! (drag, Shift for fine, double-click for the default, the wheel, typing, and
//! a right-click's Automate and Learn) is the strip's own. Every other
//! parameter is still in All parameters, and the buttons row under the face is
//! the strip's too. The card's frame, power, preset field and footer stay
//! Lumen's: only what the device *is* changes (DESIGN.md §7k).

use eframe::egui::{self, Color32, FontId, Rect, Ui};

use super::strip;
use crate::builtin;
use crate::theme;
use crate::widgets::{self, raised};

mod gravity;
mod horizon;

/// What the curve and the meter are measured against: the loud end, in dBFS.
pub(super) const TOP_DB: f32 = 0.0;
/// The quiet end of the curve's screen, in dBFS.
pub(super) const FLOOR_DB: f32 = -36.0;
/// The most reduction the meters show, in decibels.
pub(super) const MOST_REDUCTION: f32 = 24.0;
/// Gravity's meter width.
pub(super) const METER_W: f32 = 22.0;
/// Air round a screen's drawing.
pub(super) const PAD: f32 = 8.0;

/// A device's hue at `alpha`, for a drawing inside its screen.
pub(super) fn tint(hue: Color32, alpha: f32) -> Color32 {
    hue.gamma_multiply(alpha)
}

/// The mono font the screens write in.
pub(super) fn mono() -> FontId {
    FontId::monospace(theme::size::MONO_SMALL)
}

/// A parameter's value in its own units, from the face's slots.
pub(super) fn value_of(face: &Face, id: u32) -> f32 {
    let Some(slot) = face.slots.iter().find(|s| s.id == id) else {
        return 0.0;
    };
    lumen_devices::params::find(face.identity().timing.table, id)
        .map_or(0.0, |info| info.value(slot.value as f32))
}

/// The gain reduction to draw, in decibels, and the level that drove it.
pub(super) fn reading_of(face: &Face) -> (f32, f32) {
    face.reading.map_or((0.0, -120.0), |reading| {
        (
            reading.reduction_db.clamp(0.0, MOST_REDUCTION),
            reading.input_db,
        )
    })
}

/// Which device's face a card draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Gravity,
    Horizon,
}

/// The face of the device saved under `name` (its stable name, never its
/// personality's), or `None` for a device that keeps the eight-knob strip.
pub(super) fn of(name: &str) -> Option<Kind> {
    match name {
        lumen_devices::dynamics::compressor::NAME => Some(Kind::Gravity),
        lumen_devices::dynamics::limiter::NAME => Some(Kind::Horizon),
        _ => None,
    }
}

/// A face's state: the four parameters it draws, and what its meter read.
pub(super) struct Face {
    pub kind: Kind,
    /// Its controls, in the face's order (the table's first four).
    pub slots: Vec<strip::Slot>,
    /// Its gain-reduction meter, for a device that has one.
    pub reading: Option<lumen_devices::MeterReading>,
}

impl Face {
    /// Its identity (name, subtitle, hue) as the card shows it.
    pub(super) fn identity(&self) -> builtin::Identity {
        let name = match self.kind {
            Kind::Gravity => lumen_devices::dynamics::compressor::NAME,
            Kind::Horizon => lumen_devices::dynamics::limiter::NAME,
        };
        builtin::identity(name).expect("a face belongs to a built-in device")
    }
}

/// Paints `face` in `area` and answers what its controls asked for, in the same
/// words the strip uses for its knobs.
pub(super) fn paint(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    face: &Face,
    selected: bool,
) -> Vec<strip::Output> {
    let mut out = Vec::new();
    // The screen takes the room over the controls: the picture is what the
    // device is, and the four controls are Lumen's, under it. The row is the
    // strip's own cell height (18 pt of knob, the name's line and the value's),
    // and the screen keeps at least a picture's worth above it.
    let row = CONTROLS_ROOM.min(area.height() - 40.0).max(24.0);
    let screen = Rect::from_min_max(area.min, egui::pos2(area.right(), area.bottom() - row));
    let controls = Rect::from_min_max(egui::pos2(area.left(), screen.bottom()), area.max);
    let hue = face.identity().colour;
    paint_screen(ui, screen, hue);
    match face.kind {
        Kind::Gravity => gravity::paint(ui, screen, id.with("gravity"), face, hue),
        Kind::Horizon => horizon::paint(ui, screen, id.with("horizon"), face, hue),
    }
    // The four controls, the strip's own row of cells, reaching a little past
    // the card's margins as the strip's do so a value has the room it needs.
    let cells = Rect::from_min_max(
        egui::pos2(
            controls.left() - GRID_OUT,
            controls.top() + theme::space::XS,
        ),
        egui::pos2(controls.right() + GRID_OUT, controls.bottom()),
    );
    let options: Vec<Option<strip::Slot>> = face
        .slots
        .iter()
        .cloned()
        .map(|mut slot| {
            slot.text = fit_value(ui, &slot.text, cells.width() / 4.0 - 8.0);
            Some(slot)
        })
        .collect();
    out.extend(strip::paint_row(
        ui,
        cells,
        id.with("face"),
        &options,
        selected,
    ));
    out
}

/// The room the controls row takes under a face: the strip's cell (an 18 pt
/// knob, the name's line and the value's) and the air above it.
const CONTROLS_ROOM: f32 = 52.0;

/// How far the controls reach past the card's margins, as the strip's grid
/// does.
const GRID_OUT: f32 = 8.0;

/// A value's text as a face's cell can show it: the number alone when the unit
/// after it does not fit (the unit is in the screen, in All parameters and in
/// the tooltip), and the cell's own cutting only as a last resort.
fn fit_value(ui: &Ui, text: &str, room: f32) -> String {
    let font = eframe::egui::FontId::monospace(theme::size::MONO_SMALL);
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), theme::p().text)
            .size()
            .x
    };
    if width(text) <= room {
        return text.to_owned();
    }
    if let Some((number, unit)) = text.rsplit_once(' ')
        && matches!(unit, "dB" | "Hz" | "%" | "ms" | "s" | ":1")
        && width(number) <= room
    {
        return number.to_owned();
    }
    widgets::fit(ui.painter(), text, font, room)
}

/// The screen a face draws on: deep and tinted with the device's hue, the same
/// in both themes (as a clip's colour is), with a hairline of its signature
/// colour (DESIGN.md §7k).
pub(super) fn paint_screen(ui: &Ui, rect: Rect, hue: eframe::egui::Color32) {
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return;
    }
    let radius = f32::from(theme::radius::MEDIUM);
    let painter = ui.painter();
    raised::gradient(
        painter,
        rect,
        radius,
        theme::personality::SCREEN.lerp_to_gamma(hue, theme::personality::SCREEN_TINT),
        theme::personality::SCREEN,
    );
    painter.rect_stroke(
        rect,
        radius,
        eframe::egui::Stroke::new(
            theme::stroke::HAIRLINE,
            hue.gamma_multiply(theme::alpha::INK_QUIET),
        ),
        eframe::egui::StrokeKind::Inside,
    );
}
