//! Iris's face (M8 slice B1, the owner's BF C): the curve is the control.
//!
//! The screen holds the analyser's mist, the summed response of every band, and
//! one point a band on the curve. A point is dragged (Shift is fine), double-
//! clicked to go back to its band's defaults, has the wheel for its Q, and
//! takes the keyboard when it has focus. Every point is a Tab stop with its
//! band's own words ("Band 3, bell, 1.20 kHz, +3.0 dB"), and every control here
//! is also in All parameters, so nothing needs a pointer.

use eframe::egui::{
    self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use eframe::epaint::Mesh;

use lumen_devices::eq::{self, Kind};

use super::screen;
use crate::chain::strip::Output;
use crate::fonts;
use crate::theme::{self, personality};
use crate::widgets::{self, raised};

/// The band chips under the screen.
const ROW: f32 = theme::size::CONTROL;
const CHIP_GAP: f32 = 3.0;
/// How far over and under the middle the screen reaches, in decibels.
pub(crate) const MOST_DB: f32 = 24.0;
/// A band's point: its radius, the picked one's, and how near a press counts.
pub(crate) const LOW: f32 = lumen_devices::eq::LOW_HZ;
pub(crate) const HIGH: f32 = lumen_devices::eq::HIGH_HZ;
pub(crate) const POINTS: usize = 96;
const POINT: f32 = 4.5;
const POINT_PICKED: f32 = 6.0;
const POINT_REACH: f32 = 11.0;
/// The curve's line, the glow under it, and how many pieces it is drawn in so
/// the sweep runs along it.
const CURVE: f32 = 1.8;
const GLOW: f32 = 4.5;
const CURVE_PIECES: usize = 48;
/// The graticule's three frequencies.
const GRID_HZ: [f32; 3] = [100.0, 1_000.0, 10_000.0];

/// A band as the face draws and edits it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Band {
    pub number: usize,
    pub kind: Kind,
    pub on: bool,
    /// In its own units.
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub slope: f32,
}

impl Band {
    /// What a screen reader says: "Band 3, bell, 1.20 kHz, +3.0 dB, Q 1.00".
    fn words(&self) -> String {
        let mut words = format!("Band {}, {}", self.number, self.kind.word());
        if !self.on {
            words.push_str(", off");
        }
        words.push_str(", ");
        words.push_str(&eq::hz_text(self.freq));
        if self.has_gain() {
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

    /// The band's value as the pieces a chip shows, most important first: its
    /// frequency, then its gain or slope, then its Q. A face fits as many as
    /// its room takes.
    fn value_pieces(&self) -> Vec<String> {
        let mut pieces = vec![eq::hz_text(self.freq)];
        if self.has_gain() {
            pieces.push(eq::db_text(self.gain));
        }
        if self.kind.has_slope() {
            pieces.push(eq::slope_text(self.slope));
        }
        if self.kind.has_q() {
            pieces.push(format!("Q {}", eq::q_text(self.q)));
        }
        pieces
    }

    /// Whether it has a height on the screen: a shelf or a bell does, a cut
    /// sits on the middle line.
    fn has_gain(&self) -> bool {
        self.kind.has_gain()
    }

    /// The parameters this band has on the face: its frequency, the pair a
    /// drag moves, its Q (or its slope, for a cut), and its switch.
    fn ids(&self) -> (u32, Option<u32>, Option<u32>, u32) {
        let number = self.number;
        (
            eq::freq_of(number),
            self.has_gain().then(|| eq::gain_of(number)),
            if self.kind.has_q() {
                Some(eq::q_of(number))
            } else if self.kind.has_slope() {
                Some(eq::gain_of(number))
            } else {
                None
            },
            eq::on_of(number),
        )
    }

    /// What putting the band back to its defaults writes.
    fn reset(&self) -> Vec<(u32, f64)> {
        let (freq, gain, q, _) = self.ids();
        let mut values = vec![(freq, default_of(freq))];
        if let Some(id) = gain {
            values.push((id, default_of(id)));
        }
        if let Some(id) = q {
            values.push((id, default_of(id)));
        }
        values
    }
}

/// What the face reads of its device, worked out once a frame.
pub(crate) struct Look {
    pub bands: [Band; 8],
    /// The summed response, [`POINTS`](eq::POINTS) of them from 20 Hz to
    /// 20 kHz.
    pub response: Vec<f32>,
    /// The analyser's mist, the same points, when it is listening and has a
    /// window.
    pub spectrum: Option<Vec<f32>>,
    /// The analyser is listening: the face keeps asking for frames.
    pub listening: bool,
}

/// What the face asked for this frame.
#[derive(Default)]
pub(crate) struct FaceOut {
    pub out: Vec<Output>,
    /// The band picked by a click, if one was.
    pub picked: Option<usize>,
    /// The spectrum is being drawn: the caller asks for the next frame.
    pub watching: bool,
}

/// A band's place on the screen.
#[derive(Clone, Copy)]
struct Plot {
    screen: Rect,
    middle: f32,
}

impl Plot {
    fn x(&self, freq: f32) -> f32 {
        self.screen.left() + self.screen.width() * eq::axis(freq).clamp(0.0, 1.0)
    }

    fn y(&self, db: f32) -> f32 {
        let half = self.screen.height() * 0.5;
        (self.middle - db / MOST_DB * half)
            .clamp(self.screen.top() + 2.0, self.screen.bottom() - 2.0)
    }

    /// A point's frequency and gain from where the pointer is: the test's way
    /// of holding the axis the drag reads to the one the curve is drawn on.
    #[cfg(test)]
    fn at(&self, at: Pos2) -> (f32, f32) {
        let across = ((at.x - self.screen.left()) / self.screen.width().max(1.0)).clamp(0.0, 1.0);
        let db = (self.middle - at.y) / (self.screen.height() * 0.5).max(1.0) * MOST_DB;
        (eq::axis_hz(across), db.clamp(-MOST_DB, MOST_DB))
    }
}

/// Paints Iris's face in `area` and answers it. `picked` is the band the face
/// picked last, 0-based.
pub(crate) fn paint(ui: &mut Ui, area: Rect, id: egui::Id, look: &Look, picked: usize) -> FaceOut {
    let mut out = FaceOut::default();
    if look.bands.is_empty() {
        return out;
    }
    let picked = picked.min(look.bands.len() - 1);
    let rows = Rect::from_min_max(pos2(area.left(), area.bottom() - ROW), area.max);
    let screen_rect =
        Rect::from_min_max(area.min, pos2(area.right(), rows.top() - theme::space::XS));
    if screen_rect.width() < 40.0 || screen_rect.height() < 30.0 {
        return out;
    }
    let plot = Plot {
        screen: screen_rect,
        middle: screen_rect.center().y,
    };
    let hue = personality::EQ;
    let painter = ui.painter().with_clip_rect(screen_rect.expand(2.0));
    screen(&painter, screen_rect, hue);
    paint_grid(&painter, plot, hue);
    if let Some(spectrum) = &look.spectrum {
        paint_spectrum(&painter, plot, spectrum);
        out.watching = look.listening;
    }
    paint_curve(&painter, plot, &look.response);
    // Every point, the picked one last so it is over the others.
    let order = (0..look.bands.len())
        .filter(|index| *index != picked)
        .chain([picked]);
    for index in order {
        let band = look.bands[index];
        paint_point(ui, plot, id, band, index == picked, &mut out);
    }
    paint_value(&painter, plot, &look.bands[picked]);
    paint_chips(ui, rows, id, look, picked, &mut out);
    paint_typing(ui, plot, id, look, &mut out);
    out
}

/// The graticule: 100 Hz, 1 kHz and 10 kHz up the screen, and the middle line
/// (no gain change) across it, with their words quiet at the foot.
fn paint_grid(painter: &eframe::egui::Painter, plot: Plot, hue: Color32) {
    let line = Stroke::new(
        theme::stroke::HAIRLINE,
        Color32::WHITE.gamma_multiply(theme::alpha::WASH_FAINT * 2.0),
    );
    for hz in GRID_HZ {
        let x = plot.x(hz);
        painter.line_segment(
            [
                pos2(x, plot.screen.top() + 3.0),
                pos2(x, plot.screen.bottom() - 10.0),
            ],
            line,
        );
        let words = if hz >= 1_000.0 {
            format!("{:.0}k", hz / 1_000.0)
        } else {
            format!("{hz:.0}")
        };
        painter.text(
            pos2(x + 2.0, plot.screen.bottom() - 2.0),
            Align2::LEFT_BOTTOM,
            words,
            FontId::monospace(theme::size::SCALE),
            Color32::WHITE.gamma_multiply(theme::alpha::INK_TRACE),
        );
    }
    painter.line_segment(
        [
            pos2(plot.screen.left() + 4.0, plot.middle),
            pos2(plot.screen.right() - 4.0, plot.middle),
        ],
        Stroke::new(
            theme::stroke::HAIRLINE,
            Color32::WHITE.gamma_multiply(theme::alpha::WASH_FAINT * 1.6),
        ),
    );
    painter.line_segment(
        [
            pos2(plot.screen.left() + 2.0, plot.screen.bottom() - 1.5),
            pos2(plot.screen.right() - 2.0, plot.screen.bottom() - 1.5),
        ],
        Stroke::new(
            theme::stroke::HAIRLINE,
            hue.gamma_multiply(theme::alpha::HALO_FAINT),
        ),
    );
}

/// The colour of the sweep at `share` across the screen.
fn sweep_at(share: f32) -> Color32 {
    let sweep = personality::EQ_SWEEP;
    let at = (share.clamp(0.0, 1.0) * (sweep.len() - 1) as f32).min(sweep.len() as f32 - 1.001);
    let low = at.floor() as usize;
    let into = at - low as f32;
    sweep[low].lerp_to_gamma(sweep[(low + 1).min(sweep.len() - 1)], into)
}

/// The analyser's mist: the spectrum's height at each point, faded to the
/// screen's foot.
fn paint_spectrum(painter: &eframe::egui::Painter, plot: Plot, spectrum: &[f32]) {
    let points = spectrum.len();
    if points < 2 {
        return;
    }
    let floor = plot.screen.bottom() - 1.0;
    let mut mesh = Mesh::default();
    for (i, db) in spectrum.iter().enumerate() {
        let share = i as f32 / (points - 1) as f32;
        let x = plot.screen.left() + plot.screen.width() * share;
        let top = plot.y(*db).min(floor);
        let colour = sweep_at(share);
        let i0 = mesh.vertices.len() as u32;
        mesh.colored_vertex(
            pos2(x, top),
            colour.gamma_multiply(theme::alpha::WASH_OUTLINE),
        );
        mesh.colored_vertex(pos2(x, floor), colour.gamma_multiply(theme::alpha::NONE));
        if let Some(before) = i0.checked_sub(2) {
            mesh.add_triangle(before, before + 1, i0);
            mesh.add_triangle(before + 1, i0 + 1, i0);
        }
    }
    painter.add(eframe::egui::Shape::mesh(mesh));
}

/// The summed response: a wash under it to the middle line, a glow, and the
/// line itself in the sweep's colours.
fn paint_curve(painter: &eframe::egui::Painter, plot: Plot, response: &[f32]) {
    let points = response.len();
    if points < 2 {
        return;
    }
    let at = |i: usize| -> Pos2 {
        let share = i as f32 / (points - 1) as f32;
        pos2(
            plot.screen.left() + plot.screen.width() * share,
            plot.y(response[i]),
        )
    };
    // The wash: every point down to the middle line, the sweep across it.
    let mut mesh = Mesh::default();
    for i in 0..points {
        let share = i as f32 / (points - 1) as f32;
        let colour = sweep_at(share).gamma_multiply(theme::alpha::WASH_RANGE * 0.6);
        let p = at(i);
        let i0 = mesh.vertices.len() as u32;
        mesh.colored_vertex(p, colour);
        mesh.colored_vertex(pos2(p.x, plot.middle), colour);
        if let Some(before) = i0.checked_sub(2) {
            mesh.add_triangle(before, before + 1, i0);
            mesh.add_triangle(before + 1, i0 + 1, i0);
        }
    }
    painter.add(eframe::egui::Shape::mesh(mesh));
    // The glow, then the line, in pieces so the sweep runs along it.
    for piece in 0..CURVE_PIECES {
        let from = piece * (points - 1) / CURVE_PIECES;
        let to = ((piece + 1) * (points - 1) / CURVE_PIECES).max(from + 1);
        let share = (from + to) as f32 / 2.0 / (points - 1) as f32;
        let colour = sweep_at(share);
        let line: Vec<Pos2> = (from..=to.min(points - 1)).map(at).collect();
        if line.len() < 2 {
            continue;
        }
        painter.add(eframe::egui::Shape::line(
            line.clone(),
            Stroke::new(GLOW, colour.gamma_multiply(theme::alpha::HALO * 0.8)),
        ));
        painter.add(eframe::egui::Shape::line(line, Stroke::new(CURVE, colour)));
    }
}

/// One band's point: its place, its name, and what the pointer and keys do to
/// it.
fn paint_point(ui: &mut Ui, plot: Plot, id: egui::Id, band: Band, picked: bool, out: &mut FaceOut) {
    let db = if band.has_gain() { band.gain } else { 0.0 };
    let centre = pos2(plot.x(band.freq), plot.y(db));
    let reach = Rect::from_center_size(centre, Vec2::splat(POINT_REACH * 2.0));
    let response = ui.interact(
        reach,
        id.with(("point", band.number)),
        Sense::click_and_drag(),
    );
    let name = band.words();
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone()));
    let hot = response.hovered() || response.has_focus() || response.dragged();
    let ink = if band.on {
        personality::EQ
    } else {
        theme::grey_of(personality::EQ)
    };
    let painter = ui.painter().with_clip_rect(plot.screen.expand(2.0));
    if picked || hot {
        painter.circle_filled(
            centre,
            POINT_PICKED + 5.0,
            ink.gamma_multiply(if picked {
                theme::alpha::HALO_WIDE
            } else {
                theme::alpha::HALO_FAINT
            }),
        );
    }
    if picked {
        // Six spokes, so the picked point reads apart from its neighbours
        // without a second colour.
        for spoke in 0..6 {
            let angle = spoke as f32 * std::f32::consts::TAU / 6.0 + 0.3;
            let (sin, cos) = angle.sin_cos();
            painter.line_segment(
                [
                    centre + vec2(cos, sin) * (POINT_PICKED + 3.0),
                    centre + vec2(cos, sin) * (POINT_PICKED + 6.0),
                ],
                Stroke::new(
                    theme::stroke::HAIRLINE,
                    Color32::WHITE.gamma_multiply(theme::alpha::INK_QUIET),
                ),
            );
        }
    }
    painter.circle_filled(centre, if picked { POINT_PICKED } else { POINT }, ink);
    painter.circle_stroke(
        centre,
        if picked { POINT_PICKED } else { POINT },
        Stroke::new(
            if picked {
                theme::stroke::LIT
            } else {
                theme::stroke::RING
            },
            Color32::WHITE.gamma_multiply(if band.on {
                theme::alpha::INK_QUIET
            } else {
                theme::alpha::INK_TRACE
            }),
        ),
    );
    if response.has_focus() {
        raised::paint_focus(&painter, reach, theme::radius::FULL);
    }

    let (freq_id, gain_id, q_id, on_id) = band.ids();
    let mut moved: Option<(f32, f32)> = None;
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        out.out.push(Output::Reset(band.reset()));
        out.picked = Some(band.number - 1);
    }
    if response.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        ui.data_mut(|d| {
            d.insert_temp(
                id.with(("point from", band.number)),
                (band.freq, band.gain, origin),
            )
        });
    }
    if response.dragged()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let from = ui
            .data(|d| d.get_temp::<(f32, f32, Option<Pos2>)>(id.with(("point from", band.number))))
            .unwrap_or((band.freq, band.gain, None));
        let (start_freq, start_gain, origin) = match from {
            (freq, gain, Some(origin)) => (freq, gain, origin),
            (freq, gain, None) => (freq, gain, pointer),
        };
        let fine = ui.input(|i| i.modifiers.shift);
        let scale = if fine { 0.1 } else { 1.0 };
        let delta = (pointer - origin) * scale;
        // Across is the screen's axis (three decades over its width); up is
        // the gain.
        let decades = delta.x / plot.screen.width().max(1.0) * 3.0;
        let freq = (start_freq * 10f32.powf(decades)).clamp(eq::LOW_HZ, eq::HIGH_HZ);
        let gain = (start_gain - delta.y / (plot.screen.height() * 0.5) * MOST_DB)
            .clamp(-eq::MOST_DB, eq::MOST_DB);
        moved = Some((freq, gain));
        out.picked = Some(band.number - 1);
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<(f32, f32, Option<Pos2>)>(id.with(("point from", band.number))));
    }
    // The wheel is the Q (or the slope, for a cut).
    let notches = widgets::wheel::steps(ui, id.with(("point wheel", band.number)), reach);
    if notches != 0.0
        && let Some(control) = q_id
    {
        let fine = ui.input(|i| i.modifiers.shift);
        let step = if fine { 0.08 } else { 0.8 } * notches;
        let value = if band.kind.has_q() {
            (band.q * 2f32.powf(step)).clamp(eq::LOW_Q, eq::HIGH_Q)
        } else if notches > 0.0 {
            (band.slope + 12.0).min(48.0)
        } else {
            (band.slope - 12.0).max(12.0)
        };
        out.out
            .push(Output::Set(control, normalised(control, value)));
        out.picked = Some(band.number - 1);
    }
    // The keyboard: arrows move it, Page Up and Down its Q, Enter types a
    // frequency, and it is a Tab stop like every control.
    if response.has_focus() && !typing_for(ui, id, band.number) {
        let (left, right, up, down, q_up, q_down, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
                i.key_pressed(Key::Enter) || i.key_pressed(Key::F2),
            )
        });
        let fine = ui.input(|i| i.modifiers.shift);
        let mut freq = moved.map_or(band.freq, |m| m.0);
        let mut gain = moved.map_or(band.gain, |m| m.1);
        let octave = if fine { 1.0 / 96.0 } else { 1.0 / 24.0 };
        if left {
            freq = (freq * 2f32.powf(-octave)).max(eq::LOW_HZ);
        }
        if right {
            freq = (freq * 2f32.powf(octave)).min(eq::HIGH_HZ);
        }
        let db = if fine { 0.1 } else { 0.5 };
        if up {
            gain = (gain + db).min(eq::MOST_DB);
        }
        if down {
            gain = (gain - db).max(-eq::MOST_DB);
        }
        if (left || right || up || down) && (freq != band.freq || gain != band.gain) {
            moved = Some((freq, gain));
        }
        if (q_up || q_down)
            && let Some(control) = q_id
        {
            let value = if band.kind.has_q() {
                let factor = if q_up { 2f32 } else { 0.5 };
                (band.q * factor).clamp(eq::LOW_Q, eq::HIGH_Q)
            } else if q_up {
                (band.slope + 12.0).min(48.0)
            } else {
                (band.slope - 12.0).max(12.0)
            };
            out.out
                .push(Output::Set(control, normalised(control, value)));
        }

        if enter {
            crate::widgets::param::start_typing(
                ui,
                id.with(("point type", band.number)),
                &eq::hz_text(band.freq),
            );
        }
        out.picked = Some(band.number - 1);
    }
    if let Some((freq, gain)) = moved {
        let mut values = vec![(freq_id, normalised(freq_id, freq))];
        if let Some(id) = gain_id {
            values.push((id, normalised(id, gain)));
        }
        out.out.push(Output::Moved(values));
    }
    // Right-click: the band's own actions, as a knob's menu has.
    widgets::Menu::context(&response).show(|ui| {
        if widgets::menu_item(ui, "Reset this band", None, true).clicked() {
            out.out.push(Output::Reset(band.reset()));
            ui.close();
        }
        if widgets::menu_item(ui, "Type a frequency\u{2026}", None, true).clicked() {
            crate::widgets::param::start_typing(
                ui,
                id.with(("point type", band.number)),
                &eq::hz_text(band.freq),
            );
            ui.close();
        }
        let words = if band.on {
            "Turn this band off"
        } else {
            "Turn this band on"
        };
        if widgets::menu_item(ui, words, None, true).clicked() {
            out.out
                .push(Output::Choose(on_id, if band.on { 0.0 } else { 1.0 }));
            ui.close();
        }
        ui.separator();
        if let Some(ask) = widgets::control_items(ui, None, false) {
            out.out.push(Output::Ask(freq_id, ask));
        }
    });
    response.on_hover_text(format!(
        "{}. Drag to move it (Shift is fine), double-click to reset it, the wheel sets its Q",
        band.words()
    ));
}

/// The picked band's value over the curve, as near its point as it fits: the
/// whole of it where there is room for it, and less where there is not (the
/// point's tooltip and All parameters always have it all).
fn paint_value(painter: &eframe::egui::Painter, plot: Plot, band: &Band) {
    let font = FontId::monospace(theme::size::MONO_SMALL);
    let padding = vec2(theme::space::XS + 2.0, theme::space::XXS + 1.0);
    let room = plot.screen.width() - 6.0;
    let pieces = band.value_pieces();
    // Whole, then the frequency with its gain or slope, then the frequency.
    let mut words = String::new();
    if !band.on {
        words.push_str("off \u{00B7} ");
    }
    for take in (1..=pieces.len()).rev() {
        let mut candidate = words.clone();
        candidate.push_str(&pieces[..take].join(" \u{00B7} "));
        let width = painter
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE)
            .size()
            .x;
        if width + padding.x * 2.0 <= room {
            words = candidate;
            break;
        }
    }
    if words.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(words, font, Color32::WHITE);
    let size = galley.size() + padding * 2.0;
    let point = pos2(
        plot.x(band.freq),
        plot.y(if band.has_gain() { band.gain } else { 0.0 }),
    );
    let left_edge = plot.screen.left() + 2.0;
    let right_edge = (plot.screen.right() - size.x - 2.0).max(left_edge);
    let left = (point.x - size.x / 2.0).clamp(left_edge, right_edge);
    let top = (point.y - size.y - POINT_PICKED - 8.0).max(plot.screen.top() + 2.0);
    let chip = Rect::from_min_size(pos2(left, top), size);
    painter.rect_filled(
        chip,
        f32::from(theme::radius::SMALL),
        personality::SCREEN.gamma_multiply(0.9),
    );
    painter.rect_stroke(
        chip,
        f32::from(theme::radius::SMALL),
        Stroke::new(theme::stroke::HAIRLINE, personality::EQ),
        StrokeKind::Inside,
    );
    painter.galley(chip.min + padding, galley, Color32::WHITE);
}

/// The band chips: one per band, lit while it is on, the picked one ringed.
/// A click picks a band; a double-click turns it on or off.
fn paint_chips(
    ui: &mut Ui,
    rows: Rect,
    id: egui::Id,
    look: &Look,
    picked: usize,
    out: &mut FaceOut,
) {
    let count = look.bands.len();
    let width = (rows.width() - CHIP_GAP * (count - 1) as f32) / count as f32;
    for (index, band) in look.bands.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(rows.left() + index as f32 * (width + CHIP_GAP), rows.top()),
            vec2(width.max(8.0), rows.height()),
        );
        let response = ui.interact(rect, id.with(("band", band.number)), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                band.on,
                format!("Band {} ({})", band.number, band.kind.word()),
            )
        });
        // The chips are Lumen's own controls, not the device's picture: a lit
        // one is the design system's On light (DESIGN.md 7k).
        let face =
            raised::face_of(ui, &response).lit(if band.on { 1.0 } else { 0.0 }, theme::Tone::On);
        let body = raised::paint(ui.painter(), rect, theme::radius::SMALL, &face);
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            band.number.to_string(),
            fonts::medium(theme::size::CAPTION),
            face.text(),
        );
        if index == picked {
            raised::paint_focus(ui.painter(), body, theme::radius::SMALL);
        }
        if response.has_focus() {
            raised::paint_focus(ui.painter(), rect, theme::radius::SMALL);
        }
        if response.double_clicked() {
            out.out.push(Output::Choose(
                eq::on_of(band.number),
                if band.on { 0.0 } else { 1.0 },
            ));
        } else if response.clicked() {
            out.picked = Some(index);
        }
        response.on_hover_text(format!(
            "{}. Click to pick it; double-click to turn it {}",
            band.words(),
            if band.on { "off" } else { "on" }
        ));
    }
}

/// The field a point's keyboard opens: `start_typing` keeps the text under this
/// id and one step in, so both ends read the same place.
fn typing_id(id: egui::Id, number: usize) -> egui::Id {
    id.with(("point type", number)).with("typing")
}

/// Whether the typing field of band `number` is up.
fn typing_for(ui: &Ui, id: egui::Id, number: usize) -> bool {
    ui.data(|d| d.get_temp::<String>(typing_id(id, number)).is_some())
}

/// The text in band `number`'s field, and where it is kept.
fn typing_text(ui: &Ui, id: egui::Id, number: usize) -> Option<String> {
    ui.data(|d| d.get_temp::<String>(typing_id(id, number)))
}

/// The typing field a point opened: a frequency typed for its band, read on
/// Enter or when it is left, Esc cancels.
fn paint_typing(ui: &mut Ui, plot: Plot, id: egui::Id, look: &Look, out: &mut FaceOut) {
    let Some((band, mut text)) = look
        .bands
        .iter()
        .find_map(|band| typing_text(ui, id, band.number).map(|text| (*band, text)))
    else {
        return;
    };
    let field = Rect::from_center_size(
        pos2(plot.screen.center().x, plot.screen.top() + 16.0),
        vec2(110.0, theme::size::CONTROL),
    );
    let edit_id = id.with(("point type edit", band.number));
    let response = ui.put(
        field,
        egui::TextEdit::singleline(&mut text)
            .id(edit_id)
            .font(FontId::monospace(theme::size::MONO_SMALL))
            .horizontal_align(egui::Align::Center)
            .margin(vec2(2.0, 2.0)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            format!("Band {}: type a frequency", band.number),
        )
    });
    if !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    // A field just opened starts with what it holds selected, so typing
    // replaces it (as a knob's field does).
    if response.has_focus()
        && ui
            .data_mut(|d| {
                d.remove_temp::<bool>(id.with(("point type", band.number)).with("select"))
            })
            .unwrap_or(false)
    {
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let end = egui::text::CCursor::new(text.chars().count());
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                end,
            )));
        state.store(ui.ctx(), edit_id);
        ui.ctx().request_repaint();
    }
    let cancel = ui.input(|i| i.key_pressed(Key::Escape));
    if cancel || response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(typing_id(id, band.number)));
        if !cancel && let Some(hz) = eq::hz_parse(&text) {
            let control = eq::freq_of(band.number);
            out.out.push(Output::Set(control, normalised(control, hz)));
        }
    } else {
        ui.data_mut(|d| d.insert_temp(typing_id(id, band.number), text));
    }
}

/// A parameter's default, normalised.
fn default_of(id: u32) -> f64 {
    eq::TABLE
        .iter()
        .find(|info| info.id == id)
        .map_or(0.0, |info| f64::from(info.normalised(info.default)))
}

/// A value in a parameter's own units as the normalised value a project holds.
pub(crate) fn normalised(id: u32, value: f32) -> f64 {
    eq::TABLE
        .iter()
        .find(|info| info.id == id)
        .map_or(0.0, |info| f64::from(info.normalised(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(number: usize, kind: Kind) -> Band {
        Band {
            number,
            kind,
            on: true,
            freq: 1_200.0,
            gain: 3.0,
            q: 1.0,
            slope: 24.0,
        }
    }

    /// The brief's own words: "Band 3, bell, 1.2 kHz, +3 dB".
    #[test]
    fn a_point_says_its_band_in_words() {
        let words = band(3, Kind::Bell).words();
        assert!(
            words.starts_with("Band 3, bell, 1.20 kHz, +3.0 dB"),
            "{words}"
        );
        assert!(words.contains("Q 1.00"), "{words}");
        let mut off = band(1, Kind::LowCut);
        off.on = false;
        let words = off.words();
        assert!(words.contains("low cut"), "{words}");
        assert!(words.contains(", off,"), "{words}");
        assert!(words.contains("24/oct"), "{words}");
    }

    #[test]
    fn the_value_chip_names_what_the_band_is() {
        assert_eq!(
            band(3, Kind::Bell).value_pieces(),
            ["1.20 kHz", "+3.0 dB", "Q 1.00"]
        );
        let mut notch = band(4, Kind::Notch);
        notch.q = 4.0;
        assert_eq!(notch.value_pieces(), ["1.20 kHz", "Q 4.00"]);
        let cut = band(1, Kind::LowCut);
        assert_eq!(cut.value_pieces(), ["1.20 kHz", "24/oct"]);
    }

    /// The axis the face draws and the one a drag reads agree: a point dragged
    /// across the screen lands where the pointer is.
    #[test]
    fn a_drag_reads_the_axis_the_curve_is_drawn_on() {
        let screen = Rect::from_min_size(pos2(10.0, 20.0), vec2(208.0, 120.0));
        let plot = Plot {
            screen,
            middle: screen.center().y,
        };
        for hz in [30.0, 120.0, 1_000.0, 12_345.0] {
            let x = plot.x(hz);
            let (freq, _) = plot.at(pos2(x, plot.middle));
            assert!((freq - hz).abs() < hz * 0.01, "{hz} reads {freq}");
        }
        let (_, db) = plot.at(pos2(screen.center().x, plot.middle));
        assert!(db.abs() < 0.01, "{db}");
        let (_, top) = plot.at(pos2(screen.center().x, screen.top()));
        assert!(top > 20.0, "{top}");
    }

    /// A band's own units cross to the project and back, and a band's reset is
    /// only the parameters it has.
    #[test]
    fn a_value_crosses_between_its_units_and_the_project() {
        let x = normalised(eq::freq_of(3), 1_000.0);
        let info = eq::TABLE.iter().find(|i| i.id == eq::freq_of(3)).unwrap();
        assert!((info.value(x as f32) - 1_000.0).abs() < 0.5);
        // A cut has a frequency, a slope and a switch — no gain, no Q.
        let cut = band(1, Kind::LowCut).reset();
        let ids: Vec<u32> = cut.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [eq::freq_of(1), eq::gain_of(1)]);
        // A bell has a frequency, a gain and a Q.
        let bell = band(3, Kind::Bell).reset();
        let ids: Vec<u32> = bell.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [eq::freq_of(3), eq::gain_of(3), eq::q_of(3)]);
    }
}
