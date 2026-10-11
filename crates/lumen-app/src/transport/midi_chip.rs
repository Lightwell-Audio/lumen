//! The MIDI chip at the end of the transport bar (M5 slice 2d, the owner's
//! C B): a DIN glyph over two small lights, IN and OUT, that flash with
//! traffic, and the popover that lists every port with its own light and
//! its Track and Remote switches.

use eframe::egui::{self, Id, Pos2, Sense, Stroke, Ui, vec2};
use lumen_midi::{Direction, PortId, Use};

use super::Transport;
use crate::audio_settings::warning;
use crate::icons::{self, Icon};
use crate::midi::{Action, Midi};
use crate::{light, motion, settings, theme, widgets};

/// The chip's width; its height is a dense control's.
const WIDTH: f32 = 34.0;
/// Room for a port's name in the popover, so the switches line up.
const NAME_WIDTH: f32 = 150.0;
/// The popover's width.
const PANEL_WIDTH: f32 = 300.0;
/// A light's radius at rest.
const LIGHT: f32 = 2.5;

/// One port as the chip shows it.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: PortId,
    /// Plugged in now (a remembered port that is away is not).
    pub present: bool,
    pub using: Use,
    pub failed: bool,
    /// Traffic so far: a change is a flash.
    pub count: u32,
}

impl Row {
    fn on(&self) -> bool {
        match self.id.direction {
            Direction::Input => self.using.any(),
            Direction::Output => self.using.track,
        }
    }
}

/// What the chip needs from the app each frame.
#[derive(Debug, Clone, Default)]
pub struct View {
    pub inputs: Vec<Row>,
    pub outputs: Vec<Row>,
    /// The computer keyboard's traffic.
    pub keyboard: u32,
    /// Why MIDI is not there, if it is not.
    pub unavailable: Option<String>,
}

impl View {
    /// The ports present now, then the remembered ones that are away.
    pub fn gather(midi: &Midi, saved: &settings::Midi, keyboard: u32) -> Self {
        let mut view = View {
            keyboard,
            unavailable: midi.unavailable(),
            ..View::default()
        };
        for port in midi.ports() {
            let id = port.info.id.clone();
            let row = Row {
                count: midi.activity(&id),
                present: true,
                using: port.using,
                failed: port.error.is_some(),
                id,
            };
            match row.id.direction {
                Direction::Input => view.inputs.push(row),
                Direction::Output => view.outputs.push(row),
            }
        }
        for port in &saved.ports {
            let direction = match port.direction.as_str() {
                "input" => Direction::Input,
                "output" => Direction::Output,
                _ => continue,
            };
            let id = PortId {
                direction,
                name: port.name.clone(),
            };
            let list = match direction {
                Direction::Input => &mut view.inputs,
                Direction::Output => &mut view.outputs,
            };
            if !list.iter().any(|r| r.id == id) {
                list.push(Row {
                    id,
                    present: false,
                    using: Use {
                        track: port.track,
                        remote: port.remote,
                    },
                    failed: false,
                    count: 0,
                });
            }
        }
        view
    }

    fn inputs_on(&self) -> usize {
        self.inputs.iter().filter(|r| r.present && r.on()).count()
    }

    fn outputs_on(&self) -> usize {
        self.outputs.iter().filter(|r| r.present && r.on()).count()
    }

    /// The chip's accessible name.
    pub fn name(&self) -> String {
        if let Some(why) = &self.unavailable {
            return format!("MIDI: unavailable. {why}");
        }
        let (i, o) = (self.inputs_on(), self.outputs_on());
        format!(
            "MIDI: {i} input{}, {o} output{}",
            if i == 1 { "" } else { "s" },
            if o == 1 { "" } else { "s" },
        )
    }
}

/// What the lights showed when last drawn (for tests, which cannot see
/// brightness in the accessibility tree).
#[derive(Debug, Clone, Default)]
pub struct Lights {
    pub input: f32,
    pub output: f32,
    /// Each row of the popover, while it is drawn.
    pub rows: Vec<(PortId, f32)>,
    pub keyboard: f32,
}

/// A port's light memory: the chip and its row share it.
fn light_id(id: &PortId) -> Id {
    Id::new(("midi light", id.direction, &id.name))
}

fn keyboard_id() -> Id {
    Id::new("midi light keyboard")
}

/// A light at `centre`: grey when the port is open and quiet, a hollow
/// ring when nothing is switched on (or the port is away), the BEAM level
/// at `brightness` while it flashes.
fn paint_light(ui: &Ui, centre: Pos2, resting: Resting, brightness: f32) {
    let palette = theme::p();
    let painter = ui.painter();
    match resting {
        Resting::Open => {
            painter.circle_filled(centre, LIGHT, palette.text_hint);
        }
        Resting::Hollow => {
            painter.circle_stroke(centre, LIGHT - 0.6, Stroke::new(1.2, palette.text_disabled));
        }
    }
    light::dot(
        painter,
        light::BEAM,
        centre,
        LIGHT,
        palette.accent,
        brightness,
    );
}

#[derive(Clone, Copy, PartialEq)]
enum Resting {
    Open,
    Hollow,
}

impl Transport {
    /// The chip, with its popover. Returns its response.
    pub fn midi_chip(&mut self, ui: &mut Ui, view: &View) -> egui::Response {
        let ctx = ui.ctx().clone();
        // The lights: each port's own memory, the chip taking the brightest.
        let keyboard = light::flash(&ctx, keyboard_id(), view.keyboard);
        let mut lights = Lights {
            keyboard,
            input: keyboard,
            ..Lights::default()
        };
        for row in view.inputs.iter().filter(|r| r.present) {
            lights.input = lights
                .input
                .max(light::flash(&ctx, light_id(&row.id), row.count));
        }
        for row in view.outputs.iter().filter(|r| r.present) {
            lights.output = lights
                .output
                .max(light::flash(&ctx, light_id(&row.id), row.count));
        }
        // Nothing to poll: the drivers wake the interface themselves
        // (`MidiSystem` waker), so an idle input costs no frames.

        let (rect, response) =
            ui.allocate_exact_size(vec2(WIDTH, theme::size::CONTROL_SMALL), Sense::click());
        let name = view.name();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if ui.is_rect_visible(rect) {
            let palette = theme::p();
            let controls = theme::controls(false);
            let open = widgets::Menu::is_open_for(&ctx, response.id);
            let hover = motion::hover(
                &ctx,
                response.id.with("hover"),
                response.hovered() || response.has_focus() || open,
            );
            let painter = ui.painter();
            painter.rect_filled(
                rect,
                theme::radius::SMALL,
                controls.fill.lerp_to_gamma(controls.hover, hover),
            );
            if response.has_focus() {
                painter.rect_stroke(
                    rect,
                    theme::radius::SMALL,
                    Stroke::new(1.5, palette.accent),
                    egui::StrokeKind::Inside,
                );
            }
            let glyph = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                vec2(12.0, 12.0),
            );
            let tint = if view.unavailable.is_some() {
                palette.text_disabled
            } else {
                palette.text_secondary
            };
            for shape in icons::shapes(Icon::Din, glyph, tint) {
                painter.add(shape);
            }
            let x = rect.right() - 9.0;
            let resting = |on: bool| {
                if on && view.unavailable.is_none() {
                    Resting::Open
                } else {
                    Resting::Hollow
                }
            };
            paint_light(
                ui,
                egui::pos2(x, rect.center().y - 4.5),
                resting(view.inputs_on() > 0),
                lights.input,
            );
            paint_light(
                ui,
                egui::pos2(x, rect.center().y + 4.5),
                resting(view.outputs_on() > 0),
                lights.output,
            );
        }
        let response = widgets::tip(
            response.on_hover_cursor(egui::CursorIcon::PointingHand),
            "MIDI activity \u{b7} click for ports",
            None,
            None,
        );
        self.midi_lights.input = lights.input;
        self.midi_lights.output = lights.output;
        self.midi_lights.keyboard = lights.keyboard;
        widgets::Menu::below(&response).show(|ui| self.midi_panel(ui, view));
        response
    }

    /// The popover: inputs, then outputs, then the way to the settings.
    fn midi_panel(&mut self, ui: &mut Ui, view: &View) {
        ui.set_min_width(PANEL_WIDTH);
        ui.set_max_width(PANEL_WIDTH);
        self.midi_lights.rows.clear();
        if let Some(why) = &view.unavailable {
            warning(ui, why, 0.0);
            ui.separator();
        } else {
            widgets::menu_caption(ui, "Inputs");
            self.keyboard_row(ui, view.keyboard);
            for row in &view.inputs {
                self.port_row(ui, row);
            }
            if view.inputs.is_empty() {
                widgets::menu_caption(ui, "No MIDI inputs found");
            }
            ui.add_space(theme::space::XS);
            widgets::menu_caption(ui, "Outputs");
            for row in &view.outputs {
                self.port_row(ui, row);
            }
            if view.outputs.is_empty() {
                widgets::menu_caption(ui, "No MIDI outputs found");
            }
            ui.separator();
        }
        if ui
            .button("MIDI settings\u{2026}")
            .on_hover_text("Open the audio and MIDI settings on the MIDI page")
            .clicked()
        {
            self.settings_window.open = true;
            self.settings_window.page = 1;
            ui.close();
        }
    }

    /// The computer keyboard: an input that is always there for tracks
    /// (its switch is lit and cannot be turned off).
    fn keyboard_row(&mut self, ui: &mut Ui, count: u32) {
        let brightness = light::flash(ui.ctx(), keyboard_id(), count);
        ui.horizontal(|ui| {
            light_cell(ui, Resting::Open, brightness);
            name_cell(ui, "Computer keyboard", theme::p().text, &[]);
            let (rect, response) = ui.allocate_exact_size(
                vec2(pill_width(ui, "Track"), theme::size::CONTROL),
                Sense::hover(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    "Track Computer keyboard input, always on",
                )
            });
            let palette = theme::p();
            ui.painter()
                .rect_filled(rect, theme::radius::SMALL, palette.accent_muted);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Track",
                egui::FontId::proportional(theme::size::BODY),
                palette.accent,
            );
            response.on_hover_text("The computer keyboard can always play the tracks that listen");
        });
    }

    fn port_row(&mut self, ui: &mut Ui, row: &Row) {
        let brightness = if row.present {
            light::flash(ui.ctx(), light_id(&row.id), row.count)
        } else {
            0.0
        };
        self.midi_lights.rows.push((row.id.clone(), brightness));
        let is_input = row.id.direction == Direction::Input;
        let what = if is_input { "input" } else { "output" };
        let name = &row.id.name;
        let mut using = row.using;
        ui.horizontal(|ui| {
            if !row.present {
                ui.multiply_opacity(0.6);
            }
            let resting = if row.present && row.on() {
                Resting::Open
            } else {
                Resting::Hollow
            };
            light_cell(ui, resting, brightness);
            let mut captions = Vec::new();
            if row.failed {
                captions.push(("could not open", theme::p().warning));
            }
            if !row.present {
                captions.push(("not connected", theme::p().text_hint));
            }
            let colour = if row.present {
                theme::p().text
            } else {
                theme::p().text_disabled
            };
            name_cell(ui, name, colour, &captions);
            let mut track = using.track;
            let response = widgets::toggle_button(ui, &mut track, "Track");
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    track,
                    format!("Track {name} {what} in the MIDI panel"),
                )
            });
            response.on_hover_text(if is_input {
                "Tracks can record from and play through this input"
            } else {
                "Tracks can send to this output"
            });
            using.track = track;
            if is_input {
                let mut remote = using.remote;
                let response = widgets::toggle_button(ui, &mut remote, "Remote");
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        true,
                        remote,
                        format!("Remote {name} {what} in the MIDI panel"),
                    )
                });
                response.on_hover_text("Controls can be mapped to this input (MIDI learn)");
                using.remote = remote;
            }
        });
        if using != row.using {
            // The same path as the settings page's switches.
            self.midi_asked.push(Action::Use(row.id.clone(), using));
        }
    }
}

/// A row's light, in a cell as wide as the dot.
fn light_cell(ui: &mut Ui, resting: Resting, brightness: f32) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(2.0 * LIGHT + 4.0, theme::size::CONTROL),
        Sense::hover(),
    );
    paint_light(ui, rect.center(), resting, brightness);
}

fn pill_width(ui: &Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        theme::p().text,
    );
    galley.size().x + 2.0 * theme::space::S
}

/// A port's name in a column of its own, so the switches line up, with
/// small captions under it ("not connected").
fn name_cell(ui: &mut Ui, name: &str, colour: egui::Color32, captions: &[(&str, egui::Color32)]) {
    let layout = if captions.is_empty() {
        egui::Layout::top_down(egui::Align::Min).with_main_align(egui::Align::Center)
    } else {
        egui::Layout::top_down(egui::Align::Min)
    };
    ui.allocate_ui_with_layout(vec2(NAME_WIDTH, theme::size::CONTROL), layout, |ui| {
        ui.set_width(NAME_WIDTH);
        ui.set_min_height(theme::size::CONTROL);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(egui::Label::new(egui::RichText::new(name).color(colour)).truncate())
            .on_hover_text(name);
        for (text, colour) in captions {
            ui.label(
                egui::RichText::new(*text)
                    .size(theme::size::CAPTION)
                    .color(*colour),
            );
        }
    });
}
