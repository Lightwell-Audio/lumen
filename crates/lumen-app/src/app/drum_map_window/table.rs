//! The map editor's table (M8 slice D1): one row per sound, every field of
//! the model's `Sound` editable where it is shown, in the design system's
//! controls.
//!
//! The table draws and says what the person did: it never edits the project
//! itself (the window applies each [`Action`] as one named `Document` edit).
//! Its columns are the model's fields in the order a kit reads them: the
//! drawing and name, the input and output notes, the channel, mute and solo,
//! the sound's own quantize and length, its choke group, its colour, the
//! learn button and Remove. Every number is type-in, wheel and double-click
//! (`widgets::number`); the menus are the design system's.

use eframe::egui::{
    self, Align2, FontId, Id, Key, Modifiers, Rect, Sense, StrokeKind, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_core::grid::{Feel, Size};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{Color, DrumIcon, Quantize, Sound};

use super::Action;
use crate::design::icons::{DrumGlyph, paint_drum_glyph};
use crate::theme;
use crate::widgets::number::{self, Number};
use crate::widgets::{Dropdown, Menu, raised};

/// One row's height: the lanes' row (`DV`, 2 x space::M + space::XS).
pub(crate) const ROW: f32 = 28.0;
/// The gap between columns.
const GAP: f32 = 6.0;
/// The icon column: the drawing, which is also the row's drag handle.
const ICON: f32 = 26.0;
/// The fixed columns right of the name.
const IN: f32 = 48.0;
const OUT: f32 = 48.0;
const CH: f32 = 40.0;
const LETTER: f32 = 22.0;
const Q: f32 = 68.0;
const LEN: f32 = 88.0;
const CHOKE: f32 = 44.0;
const COLOUR: f32 = 22.0;
const LEARN: f32 = 44.0;
const REMOVE: f32 = 22.0;
/// The height of a row's fields, and the least a name column is.
const SMALL: f32 = 20.0;
const NAME_LEAST: f32 = 80.0;

/// Where each field is, for a table `width` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Columns {
    pub name: (f32, f32),
    pub input: f32,
    pub output: f32,
    pub channel: f32,
    pub mute: f32,
    pub solo: f32,
    pub quantize: f32,
    pub length: f32,
    pub choke: f32,
    pub colour: f32,
    pub learn: f32,
    pub remove: f32,
}

impl Columns {
    /// The columns of a table `width` points wide; the name takes what the
    /// fixed ones leave.
    pub fn of(width: f32) -> Self {
        let fixed = ICON
            + IN
            + OUT
            + CH
            + 2.0 * LETTER
            + Q
            + LEN
            + CHOKE
            + COLOUR
            + LEARN
            + REMOVE
            + GAP * 12.0;
        let name_width = (width - fixed).max(NAME_LEAST);
        let name_at = ICON + GAP;
        let input = name_at + name_width + GAP;
        let output = input + IN + GAP;
        let channel = output + OUT + GAP;
        let mute = channel + CH + GAP;
        let solo = mute + LETTER + GAP;
        let quantize = solo + LETTER + GAP;
        let length = quantize + Q + GAP;
        let choke = length + LEN + GAP;
        let colour = choke + CHOKE + GAP;
        let learn = colour + COLOUR + GAP;
        let remove = learn + LEARN + GAP;
        Self {
            name: (name_at, name_width),
            input,
            output,
            channel,
            mute,
            solo,
            quantize,
            length,
            choke,
            colour,
            learn,
            remove,
        }
    }

    /// The least width a table takes: the fixed columns and a small name.
    #[cfg(test)]
    pub fn least_width() -> f32 {
        ICON + GAP
            + NAME_LEAST
            + GAP
            + IN
            + GAP
            + OUT
            + GAP
            + CH
            + GAP
            + LETTER
            + GAP
            + LETTER
            + GAP
            + Q
            + GAP
            + LEN
            + GAP
            + CHOKE
            + GAP
            + COLOUR
            + GAP
            + LEARN
            + GAP
            + REMOVE
    }
}

/// What the table is given.
pub(crate) struct Input<'a> {
    /// Every sound, in the map's order (the lanes').
    pub sounds: &'a [Sound],
    /// The sound waiting for a note (by its input note), when one is.
    pub learning: Option<u8>,
    /// The keyboard starts in the first row's name (the window just opened).
    pub focus_first: bool,
}

/// What the table remembers between frames: which row the keyboard is on and
/// a drag in progress. Nothing of the project.
#[derive(Debug, Clone, Copy, Default)]
struct Scratch {
    focused: Option<usize>,
    drag: Option<usize>,
}

fn scratch_id() -> Id {
    Id::new("lumen drum map table")
}

fn scratch(ui: &Ui) -> Scratch {
    ui.data(|d| d.get_temp::<Scratch>(scratch_id()).unwrap_or_default())
}

fn set_scratch(ui: &Ui, change: impl FnOnce(&mut Scratch)) {
    let mut held = scratch(ui);
    change(&mut held);
    ui.data_mut(|d| d.insert_temp(scratch_id(), held));
}

/// Draws the table; the first thing the person did this frame.
pub(crate) fn table(ui: &mut Ui, input: Input<'_>) -> Option<Action> {
    let width = ui.available_width();
    let cols = Columns::of(width);
    let mut action = None;
    caption_row(ui, &cols);
    let mut focused = None;
    let mut rows = Rect::NOTHING;
    for (index, sound) in input.sounds.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        rows = if index == 0 { rect } else { rows.union(rect) };
        if scratch(ui).focused == Some(index) {
            ui.painter().rect_filled(
                rect,
                0.0,
                theme::p().accent.gamma_multiply(theme::alpha::WASH_FAINT),
            );
        }
        let row = RowIds::of(ui.id().with("drum map table"), index);
        if row.any_focused(ui) {
            focused = Some(index);
        }
        if let Some(done) = draw_row(ui, &input, &cols, sound, index, rect, &row) {
            action.get_or_insert(done);
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(theme::stroke::HAIRLINE, theme::p().border),
        );
    }
    // A drag of a row's icon that ended over another row reorders it.
    let dragging = scratch(ui).drag;
    if let Some(from) = dragging {
        if ui.input(|i| i.pointer.any_released()) {
            set_scratch(ui, |s| s.drag = None);
            if let Some(at) = ui.ctx().pointer_latest_pos()
                && rows.height() > 0.0
            {
                let to = (((at.y - rows.top()) / ROW).floor() as i64)
                    .clamp(0, input.sounds.len() as i64 - 1) as usize;
                if from != to && from < input.sounds.len() {
                    action.get_or_insert(Action::Move { from, to });
                }
            }
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            // Where it would land: the row under the pointer, lit.
            if let Some(at) = ui.ctx().pointer_latest_pos()
                && rows.contains(at)
            {
                let to = (((at.y - rows.top()) / ROW).floor() as i64)
                    .clamp(0, input.sounds.len() as i64 - 1) as usize;
                let top = rows.top() + to as f32 * ROW;
                ui.painter().hline(
                    rows.x_range(),
                    top,
                    egui::Stroke::new(theme::stroke::LIT, theme::p().accent),
                );
            }
        }
    }
    // The keyboard's row: Alt+Up and Alt+Down move the sound there.
    let focused = focused.or(scratch(ui).focused);
    if let Some(index) = focused
        && input.sounds.len() > 1
        && ui.input(|i| i.modifiers.alt)
    {
        let (up, down) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::ALT, Key::ArrowUp),
                i.consume_key(Modifiers::ALT, Key::ArrowDown),
            )
        });
        let to = if up {
            index.checked_sub(1)
        } else if down {
            Some(index + 1).filter(|to| *to < input.sounds.len())
        } else {
            None
        };
        if let Some(to) = to {
            action.get_or_insert(Action::Move { from: index, to });
        }
    }
    set_scratch(ui, |s| s.focused = focused);
    action
}

/// The captions over the fields (DESIGN section 7: a table names its columns in its
/// heading row).
fn caption_row(ui: &mut Ui, cols: &Columns) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, SMALL), Sense::hover());
    let painter = ui.painter();
    let colour = theme::p().text_hint;
    let font = FontId::proportional(theme::size::CAPTION);
    let base = rect.left();
    let bottom = rect.bottom();
    let at = |x: f32, w: f32, text: &str| {
        painter.text(
            pos2(base + x + w / 2.0, bottom),
            Align2::CENTER_BOTTOM,
            text,
            font.clone(),
            colour,
        );
    };
    painter.text(
        pos2(base + cols.name.0, bottom),
        Align2::LEFT_BOTTOM,
        "Sound",
        font.clone(),
        colour,
    );
    at(cols.input, IN, "In");
    at(cols.output, OUT, "Out");
    at(cols.channel, CH, "Ch");
    at(cols.mute, LETTER, "M");
    at(cols.solo, LETTER, "S");
    at(cols.quantize, Q, "Quantize");
    at(cols.length, LEN, "Length");
    at(cols.choke, CHOKE, "Choke");
    at(cols.colour, COLOUR, "Col");
    at(cols.learn, LEARN, "");
    at(cols.remove, REMOVE, "");
}

/// The ids of one row's widgets.
struct RowIds {
    icon: Id,
    name: Id,
    input: Id,
    output: Id,
    channel: Id,
    mute: Id,
    solo: Id,
    quantize: Id,
    length: Id,
    choke: Id,
    colour: Id,
    learn: Id,
    remove: Id,
}

impl RowIds {
    fn of(table: Id, index: usize) -> Self {
        let row = table.with(index);
        Self {
            icon: row.with("icon"),
            name: row.with("name"),
            input: row.with("in"),
            output: row.with("out"),
            channel: row.with("ch"),
            mute: row.with("m"),
            solo: row.with("s"),
            quantize: row.with("q"),
            length: row.with("len"),
            choke: row.with("choke"),
            colour: row.with("colour"),
            learn: row.with("learn"),
            remove: row.with("remove"),
        }
    }

    fn any_focused(&self, ui: &Ui) -> bool {
        let ids = [
            self.icon,
            self.name,
            self.input,
            self.output,
            self.channel,
            self.mute,
            self.solo,
            self.quantize,
            self.length,
            self.choke,
            self.colour,
            self.learn,
            self.remove,
        ];
        ui.ctx().memory(|m| ids.iter().any(|id| m.has_focus(*id)))
    }
}

/// The rectangle of a field `x` points into the row, `width` wide and
/// [`SMALL`] tall, centred in the row.
fn cell(row: Rect, x: f32, width: f32) -> Rect {
    Rect::from_center_size(
        pos2(row.left() + x + width / 2.0, row.center().y),
        vec2(width, SMALL),
    )
}

/// The rectangle a dropdown takes in a row: the design system's control
/// height, as tall as the row allows.
fn dropdown_cell(row: Rect, x: f32, width: f32) -> Rect {
    let height = theme::size::CONTROL.min(row.height());
    Rect::from_center_size(
        pos2(row.left() + x + width / 2.0, row.center().y),
        vec2(width, height),
    )
}

/// Draws a widget at `rect` rather than where the row's layout would put it
/// (a dropdown is a control, not a painted field).
fn at_rect<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add)
        .inner
}

fn draw_row(
    ui: &mut Ui,
    input: &Input<'_>,
    cols: &Columns,
    sound: &Sound,
    index: usize,
    rect: Rect,
    row: &RowIds,
) -> Option<Action> {
    let mut action = None;
    // The drawing: a click opens the grid of icons, a drag reorders the row.
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + ICON / 2.0, rect.center().y),
        vec2(SMALL, SMALL),
    );
    let response = ui
        .interact(icon_rect, row.icon, Sense::click_and_drag())
        .on_hover_text(format!(
            "{}: click for its icon, drag to move it in the map",
            sound.name
        ));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "{} icon: {}",
                sound.name,
                DrumGlyph::from(sound.icon).name()
            ),
        )
    });
    if response.drag_started() {
        set_scratch(ui, |s| s.drag = Some(index));
    }
    if ui.is_rect_visible(icon_rect) {
        paint_drum_glyph(
            ui.painter(),
            icon_rect.shrink(2.0).round_to_pixels(ui.pixels_per_point()),
            DrumGlyph::from(sound.icon),
            theme::wave(crate::arrangement::color_to(sound.color)),
        );
        raised::focus_ring(ui.painter(), &response, icon_rect);
    }
    if let Some(picked) = icon_popover(ui, &response, row.icon, sound)
        && picked != sound.icon
    {
        action.get_or_insert(Action::Icon {
            input: sound.input,
            icon: picked,
        });
    }

    // The name, edited in place and applied when the field is left.
    let name_rect = Rect::from_min_size(
        pos2(rect.left() + cols.name.0, rect.center().y - SMALL / 2.0),
        vec2(cols.name.1, SMALL),
    );
    if let Some(name) = name_field(
        ui,
        row.name,
        name_rect,
        &sound.name,
        input.focus_first && index == 0,
        row.input,
    ) && name != sound.name
    {
        action.get_or_insert(Action::Rename {
            input: sound.input,
            name,
        });
    }

    // The notes.
    if let Some(note) = number::number(
        ui,
        cell(rect, cols.input, IN),
        row.input,
        Number::new(
            &format!("{} input note", sound.name),
            f64::from(sound.input),
            (0.0, 127.0),
            f64::from(sound.output),
        )
        .parse(number::parse_note)
        .shown(&sound.input.to_string()),
    ) {
        let to = note.round().clamp(0.0, 127.0) as u8;
        if to != sound.input {
            action.get_or_insert(Action::Input {
                input: sound.input,
                to,
            });
        }
    }
    if let Some(note) = number::number(
        ui,
        cell(rect, cols.output, OUT),
        row.output,
        Number::new(
            &format!("{} output note", sound.name),
            f64::from(sound.output),
            (0.0, 127.0),
            f64::from(sound.input),
        )
        .parse(number::parse_note)
        .shown(&sound.output.to_string()),
    ) {
        let to = note.round().clamp(0.0, 127.0) as u8;
        if to != sound.output {
            action.get_or_insert(Action::Output {
                input: sound.input,
                to,
            });
        }
    }
    // The channel: 1 to 16 as a person counts them, or none for the track's.
    let channel_shown = match sound.channel {
        Some(c) => (c + 1).to_string(),
        None => "\u{2013}".to_owned(),
    };
    if let Some(ch) = number::number(
        ui,
        cell(rect, cols.channel, CH),
        row.channel,
        Number::new(
            &format!("{} channel", sound.name),
            f64::from(sound.channel.map_or(0, |c| c + 1)),
            (0.0, 16.0),
            0.0,
        )
        .shown(&channel_shown),
    ) {
        let channel = match ch.round().clamp(0.0, 16.0) as u8 {
            0 => None,
            c => Some(c - 1),
        };
        if channel != sound.channel {
            action.get_or_insert(Action::Channel {
                input: sound.input,
                channel,
            });
        }
    }
    // Mute and solo.
    if letter_toggle(
        ui,
        cell(rect, cols.mute, LETTER),
        row.mute,
        "M",
        sound.mute,
        &format!("Mute {}", sound.name),
        theme::Tone::Mute,
    ) {
        action.get_or_insert(Action::Mute {
            input: sound.input,
            on: !sound.mute,
        });
    }
    if letter_toggle(
        ui,
        cell(rect, cols.solo, LETTER),
        row.solo,
        "S",
        sound.solo,
        &format!("Solo {}", sound.name),
        theme::Tone::Accent,
    ) {
        action.get_or_insert(Action::Solo {
            input: sound.input,
            on: !sound.solo,
        });
    }
    // The sound's own quantize.
    let quantize = quantize_menu(
        ui,
        dropdown_cell(rect, cols.quantize, Q),
        row.quantize,
        &sound.name,
        sound.quantize,
    );
    if let Some(quantize) = quantize
        && quantize != sound.quantize
    {
        action.get_or_insert(Action::Quantize {
            input: sound.input,
            quantize,
        });
    }
    // The length override.
    if let Some(length) = length_menu(
        ui,
        dropdown_cell(rect, cols.length, LEN),
        row.length,
        &sound.name,
        sound.length,
    ) && length != sound.length
    {
        action.get_or_insert(Action::Length {
            input: sound.input,
            length,
        });
    }
    // The choke group.
    let choke_shown = sound
        .choke
        .map_or_else(|| "\u{2013}".to_owned(), |g| g.to_string());
    if let Some(group) = number::number(
        ui,
        cell(rect, cols.choke, CHOKE),
        row.choke,
        Number::new(
            &format!("{} choke group", sound.name),
            f64::from(sound.choke.unwrap_or(0)),
            (0.0, 8.0),
            0.0,
        )
        .shown(&choke_shown),
    ) {
        let group = match group.round().clamp(0.0, 8.0) as u8 {
            0 => None,
            g => Some(g),
        };
        if group != sound.choke {
            action.get_or_insert(Action::Choke {
                input: sound.input,
                group,
            });
        }
    }
    // The colour.
    let swatch = Rect::from_center_size(
        pos2(rect.left() + cols.colour + COLOUR / 2.0, rect.center().y),
        vec2(COLOUR - 4.0, SMALL - 4.0),
    );
    let response = ui
        .interact(swatch, row.colour, Sense::click())
        .on_hover_text(format!("{} colour", sound.name));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{} colour", sound.name),
        )
    });
    ui.painter().rect_filled(
        swatch,
        theme::radius::SMALL,
        theme::wave(crate::arrangement::color_to(sound.color)),
    );
    raised::focus_ring(ui.painter(), &response, swatch);
    if let Some(picked) = Menu::below(&response).show(|ui| colour_grid(ui, sound.color)) {
        if let Some(colour) = picked.flatten()
            && colour != sound.color
        {
            action.get_or_insert(Action::Colour {
                input: sound.input,
                color: colour,
            });
        }
        ui.close();
    }
    // Learn: the next note played sets this sound's input note.
    let learning = input.learning == Some(sound.input);
    if word_button(
        ui,
        cell(rect, cols.learn, LEARN),
        row.learn,
        if learning { "Listen" } else { "Learn" },
        learning,
        &format!(
            "{} input note: learn it from the next note played",
            sound.name
        ),
    ) {
        action.get_or_insert(Action::Learn { input: sound.input });
    }
    // Remove.
    let remove_rect = cell(rect, cols.remove, REMOVE);
    let response = ui
        .interact(remove_rect, row.remove, Sense::click())
        .on_hover_text(format!("Remove {} from the map", sound.name));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Remove {}", sound.name),
        )
    });
    if ui.is_rect_visible(remove_rect) {
        let colour = if response.hovered() {
            theme::p().text
        } else {
            theme::p().text_hint
        };
        ui.painter().text(
            remove_rect.center(),
            Align2::CENTER_CENTER,
            "\u{00D7}",
            FontId::proportional(theme::size::BODY),
            colour,
        );
        raised::focus_ring(ui.painter(), &response, remove_rect);
    }
    if response.clicked() {
        action.get_or_insert(Action::Remove { input: sound.input });
    }
    action
}

/// The name's field: a draft while it is edited, applied when it is left (or
/// Enter, which leaves it), so a rename is one edit and Esc keeps the name.
/// Tab and Enter then take the keyboard to `next` (the row's input note), so
/// a row is edited from the keyboard without hunting for the field.
fn name_field(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    current: &str,
    focus: bool,
    next: Id,
) -> Option<String> {
    let draft = id.with("draft");
    let editing: Option<String> = ui.data(|d| d.get_temp(draft));
    let edit = id.with("edit");
    let had_focus = ui.ctx().memory(|m| m.has_focus(edit));
    let mut text = editing.clone().unwrap_or_else(|| current.to_owned());
    // Read the keys before the field draws: a text field takes Enter and
    // Escape for itself.
    let (advance, escape) = ui.input(|i| {
        (
            i.key_pressed(Key::Enter) || i.key_pressed(Key::Tab),
            i.key_pressed(Key::Escape),
        )
    });
    // A field of the design system: a sunken well with the text in it.
    raised::paint_sunken(ui.painter(), rect, theme::radius::for_height(rect.height()));
    let response = ui.put(
        rect.shrink2(vec2(theme::space::XS, 2.0)),
        egui::TextEdit::singleline(&mut text)
            .id(edit)
            .font(FontId::proportional(theme::size::BODY))
            .text_color(theme::p().text)
            .frame(egui::Frame::NONE)
            .margin(vec2(theme::space::XXS, 0.0))
            .hint_text("Sound"),
    );
    if ui.is_rect_visible(rect) {
        raised::focus_ring(ui.painter(), &response, rect);
    }
    if focus && !had_focus && editing.is_none() {
        response.request_focus();
    }
    let mut applied = None;
    let living = response.has_focus() || response.lost_focus();
    if escape && living {
        ui.data_mut(|d| d.remove::<String>(draft));
        ui.ctx().memory_mut(|m| m.surrender_focus(response.id));
        return None;
    }
    if advance && living {
        // Enter or Tab commits the name and takes the keyboard to the row's
        // next field, so a row is edited without hunting for it.
        let text = text.trim().to_owned();
        ui.data_mut(|d| d.remove::<String>(draft));
        ui.ctx().memory_mut(|m| {
            m.surrender_focus(response.id);
            m.request_focus(next);
        });
        return (!text.is_empty() && text != current).then_some(text);
    }
    if response.has_focus() {
        if text != current {
            ui.data_mut(|d| d.insert_temp(draft, text.clone()));
        }
    } else if response.lost_focus() {
        let text = text.trim().to_owned();
        ui.data_mut(|d| d.remove::<String>(draft));
        if !text.is_empty() && text != current {
            applied = Some(text);
        }
    }
    applied
}

/// The 5 x 3 grid of the fifteen drawings, in a menu under the icon. Returns
/// the icon chosen this frame. Keyboard: the grid opens with its ring on the
/// current icon, the arrows move between the cells (Tab too), Enter or Space
/// chooses, Esc closes it; the pointer works as ever. Each cell is named.
fn icon_popover(ui: &mut Ui, response: &egui::Response, id: Id, sound: &Sound) -> Option<DrumIcon> {
    const COLUMNS: usize = 5;
    let at_key = id.with("at");
    let opened_key = id.with("opened");
    let current = DrumGlyph::ALL
        .iter()
        .position(|g| DrumGlyph::from(sound.icon) == *g)
        .unwrap_or(0);
    let was_open = ui.data(|d| d.get_temp::<bool>(opened_key)).unwrap_or(false);
    let shown = Menu::below(response).show(|ui| {
        let mut picked = None;
        let cell = 30.0;
        let rows = DrumGlyph::ALL.len().div_ceil(COLUMNS);
        let (area, _) = ui.allocate_exact_size(
            vec2(cell * COLUMNS as f32, cell * rows as f32),
            Sense::hover(),
        );
        let cells: Vec<Id> = (0..DrumGlyph::ALL.len())
            .map(|i| id.with(("cell", i)))
            .collect();
        let mut at: usize = ui.data(|d| d.get_temp::<usize>(at_key)).unwrap_or(current);
        // The grid keeps its ring (and the keyboard, for a screen reader) on
        // the cell it is at; the arrows and Tab are read below.
        ui.ctx()
            .memory_mut(|m| m.request_focus(cells[at.min(cells.len() - 1)]));
        // The keys are read only once the grid is open: the Space or Enter
        // that opened it must not choose a glyph in the same breath.
        if was_open {
            let (left, right, up, down, choose) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                    i.consume_key(Modifiers::NONE, Key::ArrowRight),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::Enter)
                        || i.consume_key(Modifiers::NONE, Key::Space),
                )
            });
            if std::env::var("DRUM_DEBUG").is_ok() {
                eprintln!("at={at} left={left} right={right} choose={choose}");
            }
            let step = if left {
                Some(-1)
            } else if right {
                Some(1)
            } else if up {
                Some(-(COLUMNS as i64))
            } else if down {
                Some(COLUMNS as i64)
            } else {
                None
            };
            if let Some(step) = step {
                let next = at as i64 + step;
                if (0..DrumGlyph::ALL.len() as i64).contains(&next) {
                    at = next as usize;
                    ui.ctx().memory_mut(|m| m.request_focus(cells[at]));
                }
            }
            if choose && at < DrumGlyph::ALL.len() {
                picked = Some(DrumGlyph::ALL[at]);
                ui.close();
            }
        }
        ui.data_mut(|d| d.insert_temp(at_key, at));
        for (index, glyph) in DrumGlyph::ALL.into_iter().enumerate() {
            let cell_rect = Rect::from_min_size(
                pos2(
                    area.left() + (index % COLUMNS) as f32 * cell,
                    area.top() + (index / COLUMNS) as f32 * cell,
                ),
                vec2(cell, cell),
            )
            .shrink(2.0);
            let response = ui
                .interact(cell_rect, cells[index], Sense::click())
                .on_hover_text(glyph.name());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, glyph.name())
            });
            let on = DrumGlyph::from(sound.icon) == glyph;
            if on {
                ui.painter()
                    .rect_filled(cell_rect, theme::radius::MEDIUM, theme::p().accent_muted);
            }
            if index == at {
                ui.painter().rect_stroke(
                    cell_rect,
                    theme::radius::MEDIUM,
                    egui::Stroke::new(theme::stroke::FOCUS, theme::p().accent),
                    StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    cell_rect,
                    theme::radius::MEDIUM,
                    theme::p().ember.gamma_multiply(0.35),
                );
            }
            paint_drum_glyph(
                ui.painter(),
                cell_rect.shrink(6.0).round_to_pixels(ui.pixels_per_point()),
                glyph,
                if on {
                    theme::p().accent
                } else {
                    theme::p().text_secondary
                },
            );
            if response.clicked() {
                picked = Some(glyph);
                ui.close();
            }
        }
        picked
    });
    if !was_open && Menu::is_open_for(ui.ctx(), response.id) {
        // It opened this frame: the ring starts on the current icon.
        ui.data_mut(|d| d.insert_temp(opened_key, true));
        ui.data_mut(|d| d.insert_temp(at_key, current));
    } else if !Menu::is_open_for(ui.ctx(), response.id) {
        ui.data_mut(|d| {
            d.remove::<bool>(opened_key);
            d.remove::<usize>(at_key);
        });
    }
    shown.flatten().map(DrumIcon::from)
}

/// The palette's colours, six to a row, as the track header's colour menu
/// draws them. Returns `Some(Some(colour))` when a colour was picked.
fn colour_grid(ui: &mut Ui, current: Color) -> Option<Option<Color>> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(6.0 * 26.0);
        for (index, c) in theme::CLIP_COLORS.into_iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
            let name = format!("Colour {}", index + 1);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            let response = response.on_hover_text(name);
            ui.painter().rect_filled(rect, theme::radius::SMALL, c);
            raised::focus_ring(ui.painter(), &response, rect);
            if Color::in_turn(index) == current {
                ui.painter().rect_stroke(
                    rect,
                    theme::radius::SMALL,
                    egui::Stroke::new(theme::stroke::LIT, theme::p().text),
                    StrokeKind::Inside,
                );
            }
            if response.clicked() {
                picked = Some(Color::in_turn(index));
            }
        }
    });
    picked.map(Some)
}

/// A lettered toggle the height of a row (Mute, Solo), the specimen's small
/// square: lit in its tone when on, a raised control when off. True when it
/// was clicked (or Space/Enter was pressed on it).
fn letter_toggle(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    letter: &str,
    on: bool,
    name: &str,
    tone: theme::Tone,
) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let response = response.on_hover_text(name);
    if ui.is_rect_visible(rect) {
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let radius = theme::radius::for_height(rect.height());
        let face = raised::Face::new(theme::controls(on))
            .hover(crate::motion::hover(
                ui.ctx(),
                id.with("hover"),
                response.hovered(),
            ))
            .lit(if on { 1.0 } else { 0.0 }, tone);
        let body = raised::paint(ui.painter(), rect, radius, &face);
        if !on {
            // A hairline edge, as the lanes' M and S draw it.
            ui.painter().rect_stroke(
                rect,
                f32::from(radius),
                egui::Stroke::new(theme::stroke::HAIRLINE, theme::p().border_strong),
                StrokeKind::Inside,
            );
        }
        let galley = ui.painter().layout_no_wrap(
            letter.to_owned(),
            FontId::proportional(theme::size::SCALE),
            face.text(),
        );
        let at = Align2::CENTER_CENTER
            .anchor_size(body.center(), galley.size())
            .round_to_pixels(ui.pixels_per_point())
            .min;
        ui.painter().galley(at, galley, face.text());
        raised::focus_ring(ui.painter(), &response, rect);
    }
    response.clicked()
}

/// A small raised button with a word in it (Learn).
fn word_button(ui: &mut Ui, rect: Rect, id: Id, label: &str, on: bool, name: &str) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let response = response.on_hover_text(name);
    if ui.is_rect_visible(rect) {
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let radius = theme::radius::for_height(rect.height());
        let face = raised::Face::new(theme::controls(on))
            .hover(crate::motion::hover(
                ui.ctx(),
                id.with("hover"),
                response.hovered(),
            ))
            .lit(if on { 1.0 } else { 0.0 }, theme::Tone::On);
        let body = raised::paint(ui.painter(), rect, radius, &face);
        if !on {
            ui.painter().rect_stroke(
                rect,
                f32::from(radius),
                egui::Stroke::new(theme::stroke::HAIRLINE, theme::p().border_strong),
                StrokeKind::Inside,
            );
        }
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            FontId::proportional(theme::size::SCALE),
            face.text(),
        );
        let at = Align2::CENTER_CENTER
            .anchor_size(body.center(), galley.size())
            .round_to_pixels(ui.pixels_per_point())
            .min;
        ui.painter().galley(at, galley, face.text());
        raised::focus_ring(ui.painter(), &response, rect);
    }
    response.clicked()
}

/// The sound's quantize as a dropdown of the ladder at each feel, at `rect`.
/// `None` when nothing was picked this frame; `Some(None)` picks "no grid".
fn quantize_menu(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    sound: &str,
    current: Option<Quantize>,
) -> Option<Option<Quantize>> {
    static CHOICES: std::sync::OnceLock<(Vec<String>, Vec<Option<Quantize>>)> =
        std::sync::OnceLock::new();
    let (labels, values) = CHOICES.get_or_init(|| {
        let mut labels = vec!["\u{2013}".to_owned()];
        let mut values = vec![None];
        for feel in [Feel::Straight, Feel::Triplet, Feel::Dotted] {
            for size in Size::all() {
                labels.push(quantize_short(size, feel));
                values.push(Some(Quantize { size, feel }));
            }
        }
        (labels, values)
    });
    let chosen =
        current.and_then(|current| values.iter().position(|value| *value == Some(current)));
    let shown = current.map_or_else(|| "\u{2013}".to_owned(), |q| quantize_short(q.size, q.feel));
    at_rect(ui, rect, |ui| {
        Dropdown::new(&format!("{sound} quantize"), &shown, rect.width())
            .show(ui, id, labels, chosen)
    })
    .map(|index| values.get(index).copied().flatten())
}

/// The sound's length override as a dropdown at `rect`. `None` when nothing
/// was picked this frame; `Some(None)` picks "as played".
fn length_menu(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    sound: &str,
    current: Option<Ticks>,
) -> Option<Option<Ticks>> {
    static CHOICES: std::sync::OnceLock<(Vec<String>, Vec<Option<Ticks>>)> =
        std::sync::OnceLock::new();
    let (labels, values) = CHOICES.get_or_init(|| {
        let values: Vec<Option<Ticks>> = length_values();
        let labels = values
            .iter()
            .map(|value| {
                value.map_or_else(
                    || "As played".to_owned(),
                    |ticks| length_text(ticks).to_owned(),
                )
            })
            .collect();
        (labels, values)
    });
    let shown = current.map_or_else(|| "Played".to_owned(), |l| length_text(l).to_owned());
    let chosen =
        current.and_then(|current| values.iter().position(|value| *value == Some(current)));
    at_rect(ui, rect, |ui| {
        Dropdown::new(&format!("{sound} length"), &shown, rect.width()).show(ui, id, labels, chosen)
    })
    .map(|index| values.get(index).copied().flatten())
}

/// The lengths a sound can play for, as played first.
fn length_values() -> Vec<Option<Ticks>> {
    vec![
        None,
        Some(Ticks(PPQ / 8)),
        Some(Ticks(PPQ / 4)),
        Some(Ticks(PPQ / 2)),
        Some(Ticks(PPQ)),
        Some(Ticks(PPQ * 2)),
        Some(Ticks(PPQ * 4)),
    ]
}

/// A length as its note value, or as milliseconds when it is not a note's.
fn length_text(length: Ticks) -> &'static str {
    let values = length_values();
    let labels = ["As played", "1/32", "1/16", "1/8", "1/4", "1/2", "1 bar"];
    values
        .iter()
        .position(|value| *value == Some(length))
        .map_or("a length", |index| labels[index])
}

/// A sound's quantize as the table shows it: "16", "8T", "4D".
fn quantize_short(size: Size, feel: Feel) -> String {
    let name = size.name();
    let name = name.strip_prefix("1/").unwrap_or(&name);
    match feel {
        Feel::Straight => name.to_owned(),
        Feel::Triplet => format!("{name}T"),
        Feel::Dotted => format!("{name}D"),
    }
}

/// The widgets' keycaps for a table hint (the window's footer).
pub(crate) fn hint() -> &'static str {
    "Tab moves between fields; Alt+Up and Alt+Down move a sound; drag a sound's icon to move it"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_fit_together_and_the_name_takes_the_rest() {
        let width = Columns::least_width();
        let cols = Columns::of(width);
        assert_eq!(cols.name.0, ICON + GAP);
        assert!(cols.name.1 >= NAME_LEAST);
        // Each field starts where the one before it ends, plus a gap.
        assert_eq!(cols.output, cols.input + IN + GAP);
        assert_eq!(cols.channel, cols.output + OUT + GAP);
        assert_eq!(cols.mute, cols.channel + CH + GAP);
        assert_eq!(cols.solo, cols.mute + LETTER + GAP);
        assert_eq!(cols.quantize, cols.solo + LETTER + GAP);
        assert_eq!(cols.length, cols.quantize + Q + GAP);
        assert_eq!(cols.choke, cols.length + LEN + GAP);
        assert_eq!(cols.colour, cols.choke + CHOKE + GAP);
        assert_eq!(cols.learn, cols.colour + COLOUR + GAP);
        assert_eq!(cols.remove, cols.learn + LEARN + GAP);
        // Everything ends at the table's right edge.
        let right = cols.remove + REMOVE;
        assert!((right - width).abs() < 0.01, "{right} against {width}");
        // A wider table gives the room to the name, and nothing else moves.
        let wider = Columns::of(width + 100.0);
        assert_eq!(wider.name.1, cols.name.1 + 100.0);
        assert_eq!(wider.remove + REMOVE, right + 100.0);
    }

    #[test]
    fn quantizes_and_lengths_read_as_a_drummer_says_them() {
        assert_eq!(quantize_short(Size::Note(16), Feel::Straight), "16");
        assert_eq!(quantize_short(Size::Note(8), Feel::Triplet), "8T");
        assert_eq!(quantize_short(Size::Note(4), Feel::Dotted), "4D");
        assert_eq!(quantize_short(Size::Bars(1), Feel::Straight), "1 bar");
        assert_eq!(length_text(Ticks(PPQ / 4)), "1/16");
        assert_eq!(length_text(Ticks(PPQ * 4)), "1 bar");
        assert_eq!(length_text(Ticks(PPQ / 3)), "a length");
        assert_eq!(length_values().first(), Some(&None));
        assert_eq!(
            length_values().iter().position(|v| *v == Some(Ticks(PPQ))),
            Some(4)
        );
    }

    #[test]
    fn the_quantize_menu_offers_no_grid_then_each_feel() {
        let mut labels = vec!["\u{2013}".to_owned()];
        let mut values = vec![None];
        for feel in [Feel::Straight, Feel::Triplet, Feel::Dotted] {
            for size in Size::all() {
                labels.push(quantize_short(size, feel));
                values.push(Some(Quantize { size, feel }));
            }
        }
        assert_eq!(labels.len(), values.len());
        assert_eq!(labels[1], "8 bars");
        assert_eq!(labels[8], "16");
        assert_eq!(labels[18], "16T");
        assert_eq!(labels[28], "16D");
        assert_eq!(values[0], None);
    }
}
