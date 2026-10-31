//! The MIDI page of Preferences (M5 slice 0; a page of Preferences since M9
//! R7): every input and output with its switches, and what to do when there
//! are none. Outputs have Sync (MIDI clock, M5 slice 7) and, while it is
//! on, its delay.
//!
//! The ports are a table: a column per switch with its caption over it, the
//! last column at the page's right edge where every other switch sits, and
//! a port's name cut short before it reaches them.

use eframe::egui::{
    self, Align, Align2, FontId, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_midi::{Direction, PortState, Use};

use super::Marks;
use crate::midi::{Action, Midi, SYNC_DELAY_MS};
use crate::widgets::{SliderKind, hint, warning};
use crate::{fonts, settings, theme, widgets};

/// What the page shows: the ports, and what is saved for them (the clock
/// delays).
#[derive(Clone, Copy)]
pub struct Shown<'a> {
    pub midi: &'a Midi,
    pub saved: &'a settings::Midi,
}

/// The delay field's width.
const DELAY_WIDTH: f32 = 64.0;

/// Each switch column's width. The switches sit at their column's right
/// edge, so the last one lines up with every other switch on the pages,
/// and the column's caption is centred over its switch.
const COLUMN: f32 = 64.0;

/// Room for a port's state dot at the start of its row.
const DOT: f32 = 12.0;

/// The ports. `marks` outlines the row a search led to.
pub fn page(ui: &mut Ui, shown: Shown<'_>, marks: &Marks, actions: &mut Vec<Action>) {
    let midi = shown.midi;
    if let Some(why) = midi.unavailable() {
        ui.add_space(theme::space::M);
        warning(ui, &why, 0.0);
        return;
    }
    let ports = midi.ports();
    let (inputs, outputs): (Vec<_>, Vec<_>) = ports
        .iter()
        .partition(|p| p.info.id.direction == Direction::Input);
    marks.row(ui, "midi.inputs", |ui| {
        header(ui, "Inputs", ["Track", "Remote"])
    });
    list(ui, &inputs, "No MIDI inputs found", shown.saved, actions);
    marks.row(ui, "midi.outputs", |ui| {
        header(ui, "Outputs", ["Track", "Sync"])
    });
    list(ui, &outputs, "No MIDI outputs found", shown.saved, actions);
    ui.add_space(theme::space::S);
    hint(
        ui,
        "Track: tracks may use the port. Remote: controls can be mapped to it. \
         Sync: it gets MIDI clock while the song plays, moved by its delay. \
         Devices appear when plugged in.",
    );
    ui.add_space(theme::space::XS);
    marks.row(ui, "midi.rescan", |ui| {
        if crate::widgets::raised::button(ui, "Look for devices")
            .on_hover_text("Check for new ports now rather than at the next poll")
            .clicked()
        {
            actions.push(Action::Rescan);
        }
    });
    // New devices appear while the page is open.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(300));
}

/// A section's title with the captions of its two switch columns.
fn header(ui: &mut Ui, title: &str, captions: [&str; 2]) {
    ui.add_space(theme::space::M);
    let row = take_row(ui, theme::size::CONTROL_SMALL);
    cell(ui, row, title, Layout::left_to_right(Align::Center), |ui| {
        ui.label(RichText::new(title).font(fonts::medium(theme::size::BODY)));
    });
    let font = FontId::proportional(theme::size::CAPTION);
    for (caption, column) in captions.into_iter().zip(columns(row)) {
        ui.painter().text(
            pos2(switch_middle(column), row.center().y),
            Align2::CENTER_CENTER,
            caption,
            font.clone(),
            theme::p().text_secondary,
        );
    }
    ui.add_space(theme::space::XXS);
}

fn list(
    ui: &mut Ui,
    ports: &[&PortState],
    empty: &str,
    saved: &settings::Midi,
    actions: &mut Vec<Action>,
) {
    if ports.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(DOT);
            hint(ui, empty);
        });
        return;
    }
    for port in ports {
        let mut using = port.using;
        port_row(ui, port, &mut using);
        if using.sync {
            delay_row(ui, &port.info.name, saved, actions);
        }
        if let Some(why) = &port.error {
            ui.horizontal(|ui| {
                ui.add_space(DOT);
                warning(ui, &format!("Could not open: {why}"), 0.0);
            });
        }
        if using != port.using {
            actions.push(Action::Use(port.info.id.clone(), Use { ..using }));
        }
    }
}

/// A port's row: its state, its name, then its two switches (Track, and
/// Remote for an input or Sync for an output).
fn port_row(ui: &mut Ui, port: &PortState, using: &mut Use) {
    let name = port.info.name.as_str();
    let is_input = port.info.id.direction == Direction::Input;
    let what = if is_input { "input" } else { "output" };
    let salt = (name, what);
    let row = take_row(ui, theme::size::CONTROL);
    let [track, second] = columns(row);
    dot(
        ui,
        Rect::from_min_size(row.min, vec2(DOT, row.height())),
        port,
        salt,
    );
    let label = Rect::from_min_max(
        pos2(row.left() + DOT, row.top()),
        pos2(track.left() - theme::space::S, row.bottom()),
    );
    cell(
        ui,
        label,
        (salt, "name"),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(egui::Label::new(name).truncate())
                .on_hover_text(name);
        },
    );
    using.track = column_switch(
        ui,
        track,
        (salt, "track"),
        using.track,
        &format!("Track {name} {what}"),
        if is_input {
            "Tracks can record from and play through this input"
        } else {
            "Tracks can send to this output"
        },
    );
    if is_input {
        using.remote = column_switch(
            ui,
            second,
            (salt, "remote"),
            using.remote,
            &format!("Remote {name} {what}"),
            "Controls can be mapped to this input (MIDI learn)",
        );
    } else {
        using.sync = column_switch(
            ui,
            second,
            (salt, "sync"),
            using.sync,
            &format!("Sync {name} output"),
            "Sends MIDI clock, Start, Stop and the song position while the song plays",
        );
    }
}

/// A switch at the right of `column`, named `name`; returns its state.
fn column_switch(
    ui: &mut Ui,
    column: Rect,
    salt: impl egui::AsIdSalt,
    on: bool,
    name: &str,
    tip: &str,
) -> bool {
    let mut on = on;
    cell(
        ui,
        column,
        salt,
        Layout::right_to_left(Align::Center),
        |ui| {
            let response = widgets::switch(ui, &mut on, "", true).on_hover_text(tip);
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, name)
            });
        },
    );
    on
}

/// Under an output with Sync on: its clock delay (the owner's F3 and F4),
/// the field under the Sync switch.
fn delay_row(ui: &mut Ui, name: &str, saved: &settings::Midi, actions: &mut Vec<Action>) {
    let row = take_row(ui, theme::size::CONTROL);
    let label = Rect::from_min_max(
        pos2(row.left() + DOT + theme::space::M, row.top()),
        pos2(row.right() - DELAY_WIDTH - theme::space::S, row.bottom()),
    );
    cell(
        ui,
        label,
        (name, "delay label"),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(RichText::new("Clock delay").weak()).on_hover_text(
                "Moves the clock earlier or later, to line the device up with the song",
            );
        },
    );
    let field = Align2::RIGHT_CENTER
        .anchor_size(
            row.right_center(),
            vec2(DELAY_WIDTH, theme::size::CONTROL_SMALL),
        )
        .round_to_pixels(ui.pixels_per_point());
    let (min, max) = SYNC_DELAY_MS;
    let delay = Midi::sync_delay(saved, name);
    let kind = SliderKind::Offset {
        min_ms: min as f32,
        max_ms: max as f32,
    };
    let chosen = cell(
        ui,
        field,
        (name, "delay"),
        Layout::left_to_right(Align::Center),
        |ui| {
            widgets::value_field(
                ui,
                field,
                egui::Id::new(("sync delay", name)),
                delay as f32,
                kind,
                &format!("Sync delay of {name}"),
                theme::controls(false),
            )
        },
    )
    .round() as i32;
    if chosen != delay {
        actions.push(Action::SyncDelay(name.to_owned(), chosen));
    }
}

/// Takes a row of the page's width and `height`, snapped to pixels.
fn take_row(ui: &mut Ui, height: f32) -> Rect {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    row.round_to_pixels(ui.pixels_per_point())
}

/// The two switch columns at the right of `row`.
fn columns(row: Rect) -> [Rect; 2] {
    let second = Rect::from_min_max(pos2(row.right() - COLUMN, row.top()), row.max);
    [second.translate(vec2(-COLUMN, 0.0)), second]
}

/// Where the middle of a switch at the right of `column` is.
fn switch_middle(column: Rect) -> f32 {
    column.right() - widgets::SWITCH_WIDTH / 2.0
}

/// Draws `add` inside `rect`, laid out by `layout`, without moving the
/// page's cursor (the row was taken already).
fn cell<R>(
    ui: &mut Ui,
    rect: Rect,
    salt: impl egui::AsIdSalt,
    layout: Layout,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut child = ui.new_child(UiBuilder::new().id_salt(salt).max_rect(rect).layout(layout));
    add(&mut child)
}

/// Open ports are lit, failed ones warn, the rest are quiet.
fn dot(ui: &mut Ui, rect: Rect, port: &PortState, salt: impl egui::AsIdSalt) {
    let palette = theme::p();
    let colour = if port.error.is_some() {
        palette.warning
    } else if port.open {
        palette.accent
    } else {
        palette.text_disabled
    };
    let response = ui.interact(rect, ui.id().with(salt).with("dot"), Sense::hover());
    let centre = pos2(rect.left() + 3.5, rect.center().y);
    ui.painter().circle_filled(centre, 3.5, colour);
    response.on_hover_text(if port.error.is_some() {
        "Could not open"
    } else if port.open {
        "Open"
    } else {
        "Not in use"
    });
}
