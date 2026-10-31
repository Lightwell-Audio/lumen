//! The MIDI chip at the end of the transport bar (M5 slice 2d, the owner's
//! C B): a DIN glyph over two small lights, IN and OUT, that flash with
//! traffic, and the popover that lists every port with its own light and
//! its Track and Remote switches, in columns.

use eframe::egui::{self, Id, Pos2, Rect, Sense, Stroke, Ui, vec2};
use eframe::emath::GuiRounding as _;
use lumen_midi::{Direction, PortId, Use};

use super::Transport;
use crate::icons::{self, Icon};
use crate::midi::{Action, Midi};
use crate::widgets::warning;
use crate::{light, motion, settings, theme, widgets};

/// The chip's width; its height is a dense control's.
const WIDTH: f32 = 34.0;
/// A switch column's width in the popover (wide enough for "Remote").
const COLUMN: f32 = 52.0;
/// The popover's width.
const PANEL_WIDTH: f32 = 320.0;
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
    /// A key is held down on it: the light stays lit.
    pub held: bool,
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
    /// The computer keyboard's traffic, and whether a key of it is held.
    pub keyboard: u32,
    pub keyboard_held: bool,
    /// Why MIDI is not there, if it is not.
    pub unavailable: Option<String>,
}

impl View {
    /// The ports present now, then the remembered ones that are away.
    pub fn gather(midi: &Midi, saved: &settings::Midi, keyboard: u32, keyboard_held: bool) -> Self {
        let mut view = View {
            keyboard,
            keyboard_held,
            unavailable: midi.unavailable(),
            ..View::default()
        };
        for port in midi.ports() {
            let id = port.info.id.clone();
            let row = Row {
                count: midi.activity(&id),
                held: midi.held(&id),
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
                        sync: port.sync,
                    },
                    failed: false,
                    count: 0,
                    held: false,
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
        let keyboard = light::flash_held(&ctx, keyboard_id(), view.keyboard, view.keyboard_held);
        let mut lights = Lights {
            keyboard,
            input: keyboard,
            ..Lights::default()
        };
        for row in view.inputs.iter().filter(|r| r.present) {
            lights.input = lights.input.max(light::flash_held(
                &ctx,
                light_id(&row.id),
                row.count,
                row.held,
            ));
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
            let rect = rect.round_to_pixels(ui.pixels_per_point());
            // Quiet, like the panel toggles beside it (the interface
            // audit's TB1): bare at rest, the Ember wash under the pointer,
            // sunk while pressed; the lights carry it.
            let r = f32::from(theme::radius::SMALL);
            if response.is_pointer_button_down_on() {
                ui.painter().rect_filled(rect, r, controls.press);
            } else if hover > 0.0 {
                ui.painter()
                    .rect_filled(rect, r, palette.ember.gamma_multiply(hover));
            }
            widgets::raised::focus_ring(ui.painter(), &response, rect);
            let painter = ui.painter();
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
    /// Each port is one row: its light, its name, and switches in a Track
    /// and a Remote column, captioned once per section.
    fn midi_panel(&mut self, ui: &mut Ui, view: &View) {
        ui.set_min_width(PANEL_WIDTH);
        ui.set_max_width(PANEL_WIDTH);
        self.midi_lights.rows.clear();
        if let Some(why) = &view.unavailable {
            warning(ui, why, 0.0);
            ui.separator();
        } else {
            section_head(ui, "Inputs", &["Track", "Remote"]);
            self.keyboard_row(ui, view.keyboard, view.keyboard_held);
            for row in &view.inputs {
                self.port_row(ui, row);
            }
            if view.inputs.is_empty() {
                widgets::menu_caption(ui, "No MIDI inputs found");
            }
            ui.add_space(theme::space::S);
            section_head(ui, "Outputs", &["Track"]);
            for row in &view.outputs {
                self.port_row(ui, row);
            }
            if view.outputs.is_empty() {
                widgets::menu_caption(ui, "No MIDI outputs found");
            }
            ui.separator();
        }
        if widgets::raised::button(ui, "MIDI settings\u{2026}")
            .on_hover_text("Open Preferences on the MIDI page")
            .clicked()
        {
            self.open_preferences = Some(crate::preferences::Page::Midi);
            ui.close();
        }
    }

    /// The computer keyboard: an input that is always there for tracks (its
    /// switch is on and cannot be turned off).
    fn keyboard_row(&mut self, ui: &mut Ui, count: u32, held: bool) {
        let brightness = light::flash_held(ui.ctx(), keyboard_id(), count, held);
        let cells = port_cells(
            ui,
            (Resting::Open, brightness),
            "Computer keyboard",
            theme::p().text,
            None,
        );
        let mut on = true;
        switch_in(ui, cells.track, (&mut on, false), ("keyboard", false))
            .on_hover_text("The computer keyboard can always play the tracks that listen")
            .widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    false,
                    true,
                    "Track Computer keyboard input, always on",
                )
            });
    }

    fn port_row(&mut self, ui: &mut Ui, row: &Row) {
        let brightness = if row.present {
            light::flash_held(ui.ctx(), light_id(&row.id), row.count, row.held)
        } else {
            0.0
        };
        self.midi_lights.rows.push((row.id.clone(), brightness));
        let is_input = row.id.direction == Direction::Input;
        let what = if is_input { "input" } else { "output" };
        let name = &row.id.name;
        let mut using = row.using;
        let palette = theme::p();
        let resting = if row.present && row.on() {
            Resting::Open
        } else {
            Resting::Hollow
        };
        // An away port keeps its switches (they apply when it is plugged
        // back in), quieter, and says so after its name.
        let (colour, note) = if !row.present {
            (
                palette.text_secondary,
                Some(("not connected", palette.text_hint)),
            )
        } else if row.failed {
            (palette.text, Some(("could not open", palette.warning)))
        } else {
            (palette.text, None)
        };
        let cells = port_cells(ui, (resting, brightness), name, colour, note);
        let switch = |ui: &mut Ui, rect: Rect, on: &mut bool, column: &str, tip: &str| {
            let response = switch_in(ui, rect, (on, true), ((name, column), !row.present));
            let state = *on;
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    state,
                    format!("{column} {name} {what} in the MIDI panel"),
                )
            });
            response.on_hover_text(tip);
        };
        switch(
            ui,
            cells.track,
            &mut using.track,
            "Track",
            if is_input {
                "Tracks can record from and play through this input"
            } else {
                "Tracks can send to this output"
            },
        );
        if is_input {
            switch(
                ui,
                cells.remote,
                &mut using.remote,
                "Remote",
                "Controls can be mapped to this input (MIDI learn)",
            );
        }
        if using != row.using {
            // The same path as the settings page's switches.
            self.midi_asked.push(Action::Use(row.id.clone(), using));
        }
    }
}

/// How much an away port's switches keep of their strength.
const AWAY: f32 = 0.6;

/// Where a port row's switches go.
struct Cells {
    track: Rect,
    remote: Rect,
}

/// A switch with no label centred in `rect`, `on` and `enabled`, without
/// taking room in the row (which is laid out already); `id` keeps each
/// switch's own, and `away` dims it.
fn switch_in(
    ui: &mut Ui,
    rect: Rect,
    (on, enabled): (&mut bool, bool),
    (id, away): (impl egui::AsIdSalt, bool),
) -> egui::Response {
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("midi switch", id))
            .max_rect(rect)
            .layout(egui::Layout::centered_and_justified(
                egui::Direction::LeftToRight,
            )),
    );
    if away {
        cell.multiply_opacity(AWAY);
    }
    widgets::switch(&mut cell, on, "", enabled)
}

/// The columns of a row in `row`, from the right: Remote, then Track.
fn columns(row: Rect, side: f32) -> Cells {
    let right = row.right() - side;
    let remote = Rect::from_x_y_ranges(right - COLUMN..=right, row.y_range());
    let track = remote.translate(vec2(-COLUMN, 0.0));
    Cells { track, remote }
}

/// A section's caption ("Inputs") and, over the switch columns, theirs.
fn section_head(ui: &mut Ui, caption: &str, columns_shown: &[&str]) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let colour = ui.visuals().weak_text_color();
    let side = ui.spacing().button_padding.x;
    let height = ui.text_style_height(&egui::TextStyle::Small) + theme::space::XS;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.text(
        egui::pos2(row.left() + side, row.center().y),
        egui::Align2::LEFT_CENTER,
        caption,
        font.clone(),
        colour,
    );
    let cells = columns(row, side);
    for (cell, label) in [cells.track, cells.remote].into_iter().zip(columns_shown) {
        painter.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            colour,
        );
    }
}

/// Lays out a port row: its light and its name (shortened to the room left
/// of the switches, the whole name on hover), with an optional `note` after
/// it. Returns where the switches go.
fn port_cells(
    ui: &mut Ui,
    (resting, brightness): (Resting, f32),
    name: &str,
    colour: egui::Color32,
    note: Option<(&str, egui::Color32)>,
) -> Cells {
    let side = ui.spacing().button_padding.x;
    let (row, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::size::CONTROL),
        Sense::hover(),
    );
    let cells = columns(row, side);
    paint_light(
        ui,
        egui::pos2(row.left() + side + LIGHT, row.center().y),
        resting,
        brightness,
    );
    let painter = ui.painter();
    let left = row.left() + side + 2.0 * LIGHT + theme::space::S;
    let room = cells.track.left() - theme::space::S - left;
    let font = egui::FontId::proportional(theme::size::BODY);
    let note_font = egui::FontId::proportional(theme::size::CAPTION);
    let note_galley =
        note.map(|(text, c)| painter.layout_no_wrap(text.to_owned(), note_font.clone(), c));
    let note_room = note_galley
        .as_ref()
        .map_or(0.0, |g| g.size().x + theme::space::S);
    let name_room = (room - note_room).max(0.0);
    let whole = painter.layout_no_wrap(name.to_owned(), font.clone(), colour);
    let shortened = whole.size().x > name_room;
    let galley = if shortened {
        painter.layout_no_wrap(
            widgets::shorten(painter, name, font, name_room),
            egui::FontId::proportional(theme::size::BODY),
            colour,
        )
    } else {
        whole
    };
    let name_rect =
        egui::Align2::LEFT_CENTER.anchor_size(egui::pos2(left, row.center().y), galley.size());
    let end = name_rect.right();
    painter.galley(name_rect.min, galley, colour);
    // The name and the note are painted, so each is named for a screen
    // reader here.
    let label = |rect, salt: &str, text: &str| {
        let text = text.to_owned();
        let read = ui.interact(rect, response.id.with(salt), Sense::hover());
        read.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
        read
    };
    if let (Some(note_galley), Some((note_text, _))) = (note_galley, note) {
        let at = egui::Align2::LEFT_CENTER.anchor_size(
            egui::pos2(end + theme::space::S, row.center().y),
            note_galley.size(),
        );
        painter.galley(at.min, note_galley, colour);
        label(at, "note", note_text);
    }
    let named = label(name_rect, "name", name);
    if shortened {
        named.on_hover_text(name);
    }
    cells
}
