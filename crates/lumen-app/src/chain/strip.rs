//! The card's parameter strip (M6 slice 4, the owner's S2 with the send
//! knobs' comet style): eight chosen parameters as knobs in two rows, each
//! with its name and the plugin's own text for its value, and under them one
//! row of buttons: the plugin's own window, Learn, and every parameter. A
//! slot with nothing in it offers to be filled.

use eframe::egui::{self, Align2, FontId, Rect, Shape, Stroke, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_hosting::ParamInfo;

use crate::theme::Tone;
use crate::widgets::param::{ParamLook, param_knob, start_typing};
use crate::widgets::{self, raised};
use crate::{motion, theme, widgets::fit};

/// The knobs on a card.
pub(super) const SLOTS: usize = 8;
const COLUMNS: usize = 4;
/// A knob's size.
const KNOB: f32 = 26.0;
/// The row of buttons under the knobs.
pub(super) const BUTTONS: f32 = 22.0;
const GAP: f32 = 6.0;

/// One chosen parameter, as the card needs it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Slot {
    pub id: u32,
    pub name: String,
    /// The plugin's own words for the value now.
    pub text: String,
    pub value: f64,
    pub default: f64,
    pub steps: i32,
    pub read_only: bool,
}

/// What the strip asked for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Output {
    /// A parameter was moved.
    Set(u32, f64),
    /// The user typed text for a parameter: the plugin reads it.
    Type(u32, String),
    /// A parameter leaves the strip.
    Remove(u32),
    /// An empty slot was clicked: choose a parameter for it.
    Fill,
    /// The button for every parameter.
    All,
    /// The editor button: open the window, or close it.
    Editor,
    /// Learn: touch a control in the plugin's window and it takes the next
    /// free slot.
    Learn,
    /// End a plugin whose window stopped answering.
    End,
}

/// What the editor button says and does (M6 slice 4f).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Editor {
    /// Closed: the button opens it.
    #[default]
    Closed,
    /// Asked for, not shown yet.
    Opening,
    Open,
    /// The plugin's window stopped answering.
    NotResponding,
    /// No window to open, and why: the plugin has none, or this platform
    /// cannot show one.
    Unavailable(String),
}

/// The row of buttons under the knobs.
#[derive(Debug, Clone, Copy)]
pub(super) struct Buttons<'a> {
    pub editor: &'a Editor,
    pub learning: bool,
    /// How many parameters the plugin lists (0 hides Learn and All).
    pub parameters: usize,
}

/// A parameter a strip may show: not hidden by the plugin.
pub(super) fn offered(p: &ParamInfo) -> bool {
    !p.hidden()
}

/// What a plugin's first card shows when nothing was chosen: the first
/// parameters it lists that a person would move (not hidden, not read only,
/// not a program or a bypass), at most [`SLOTS`].
pub(super) fn default_shown(list: &[ParamInfo]) -> Vec<u32> {
    list.iter()
        .filter(|p| offered(p) && !p.read_only() && !p.program_change() && !p.bypass())
        .take(SLOTS)
        .map(|p| p.id)
        .collect()
}

/// The ids on the strip: the chosen ones the plugin still lists and offers,
/// in order, at most [`SLOTS`]; or the default when nothing was chosen.
pub(super) fn resolve(list: &[ParamInfo], chosen: Option<&[u32]>) -> Vec<u32> {
    match chosen {
        None => default_shown(list),
        Some(ids) => {
            let mut out: Vec<u32> = Vec::new();
            for id in ids {
                if !out.contains(id) && list.iter().any(|p| p.id == *id && offered(p)) {
                    out.push(*id);
                }
            }
            out.truncate(SLOTS);
            out
        }
    }
}

/// Paints the strip in `area` and answers it.
pub(super) fn paint(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    slots: &[Option<Slot>],
    selected: bool,
    row: Buttons<'_>,
) -> Vec<Output> {
    let palette = *theme::p();
    let mut out = Vec::new();
    let grid = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - BUTTONS - GAP));
    let cell_w = grid.width() / COLUMNS as f32;
    let cell_h = (grid.height() - GAP) / 2.0;
    let caption = FontId::proportional(theme::size::CAPTION);
    let mono = FontId::monospace(theme::size::MONO_SMALL);
    for i in 0..SLOTS {
        let (col, row) = (i % COLUMNS, i / COLUMNS);
        let cell = Rect::from_min_size(
            pos2(
                grid.left() + col as f32 * cell_w,
                grid.top() + row as f32 * (cell_h + GAP),
            ),
            vec2(cell_w, cell_h),
        );
        let knob = Rect::from_center_size(
            pos2(cell.center().x, cell.top() + KNOB / 2.0),
            vec2(KNOB, KNOB),
        )
        .round_to_pixels(ui.pixels_per_point());
        let slot_id = id.with(("slot", i));
        match slots.get(i).and_then(Option::as_ref) {
            Some(slot) => {
                let (response, moved) = param_knob(
                    ui,
                    knob,
                    slot_id,
                    ParamLook {
                        name: &slot.name,
                        text: &slot.text,
                        value: slot.value,
                        default: slot.default,
                        steps: slot.steps,
                        read_only: slot.read_only,
                        selected,
                    },
                );
                if let Some(value) = moved.value {
                    out.push(Output::Set(slot.id, value));
                }
                if let Some(text) = moved.typed {
                    out.push(Output::Type(slot.id, text));
                }
                let painter = ui.painter();
                let name = fit(painter, &slot.name, caption.clone(), cell_w - 2.0);
                painter.text(
                    pos2(cell.center().x, knob.bottom() + 3.0),
                    Align2::CENTER_TOP,
                    name,
                    caption.clone(),
                    palette.text_secondary,
                );
                let text = fit(painter, &slot.text, mono.clone(), cell_w - 2.0);
                painter.text(
                    pos2(cell.center().x, knob.bottom() + 3.0 + 13.0),
                    Align2::CENTER_TOP,
                    text,
                    mono.clone(),
                    palette.text,
                );
                let (slot_value, slot_default, slot_id_param, read_only) =
                    (slot.value, slot.default, slot.id, slot.read_only);
                widgets::Menu::context(&response).show(|ui| {
                    if widgets::menu_item(ui, "Type a value…", None, !read_only).clicked() {
                        start_typing(ui, slot_id, &slot.text);
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Reset to default", None, !read_only).clicked() {
                        if (slot_value - slot_default).abs() > 1e-9 {
                            out.push(Output::Set(slot_id_param, slot_default));
                        }
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Remove from strip", None, true).clicked() {
                        out.push(Output::Remove(slot_id_param));
                        ui.close();
                    }
                });
            }
            None => {
                let response = ui.interact(knob, slot_id, egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Choose a parameter for this slot",
                    )
                });
                let hot = response.hovered() || response.has_focus();
                let painter = ui.painter();
                let ring: Vec<_> = (0..=24)
                    .map(|k| {
                        let a = k as f32 / 24.0 * std::f32::consts::TAU;
                        knob.center() + vec2(a.cos(), a.sin()) * (KNOB / 2.0 - 2.0)
                    })
                    .collect();
                let ink = if hot {
                    palette.text_secondary
                } else {
                    palette.border_strong
                };
                painter.extend(Shape::dashed_line(&ring, Stroke::new(1.0, ink), 2.0, 3.0));
                painter.text(
                    knob.center(),
                    Align2::CENTER_CENTER,
                    "+",
                    FontId::proportional(theme::size::BODY),
                    if hot {
                        palette.text_secondary
                    } else {
                        palette.text_hint
                    },
                );
                if response.has_focus() {
                    raised::paint_focus(painter, knob, theme::radius::FULL);
                }
                if response
                    .on_hover_text("Choose a parameter for the strip")
                    .clicked()
                {
                    out.push(Output::Fill);
                }
            }
        }
    }
    let strip = Rect::from_min_max(pos2(area.left(), area.bottom() - BUTTONS), area.max);
    out.extend(buttons(ui, strip, id, row));
    out
}

/// How a button looks in the row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Plain,
    /// Lit: the thing it names is on.
    On,
    /// The one thing to do about a stuck plugin.
    Fill,
    /// Says how things are and cannot be pressed.
    Warn,
    Disabled,
}

/// One button of the row.
struct Part<'a> {
    /// What is painted.
    shown: &'a str,
    /// What a screen reader says (and the tests look for).
    name: String,
    tip: &'a str,
    style: Style,
    /// A lit dot before the letters.
    dot: bool,
    weight: f32,
    output: Option<Output>,
}

/// The editor, Learn and All buttons in `area`, as the editor's state allows:
/// a plugin with no window hides Learn; a stuck one offers to be ended; a
/// plugin with no parameters has only its window.
pub(super) fn buttons(ui: &mut Ui, area: Rect, id: egui::Id, row: Buttons<'_>) -> Vec<Output> {
    let mut parts: Vec<Part<'_>> = Vec::new();
    let learn_ok = row.parameters > 0;
    let all = Part {
        shown: "All\u{2026}",
        name: format!("All parameters ({})", row.parameters),
        tip: "Every parameter of the plugin, with search",
        style: Style::Plain,
        dot: false,
        weight: 0.7,
        output: Some(Output::All),
    };
    let learn = |on: bool| Part {
        shown: if on { "Learning\u{2026}" } else { "Learn" },
        name: if on { "Stop learning" } else { "Learn" }.to_owned(),
        tip: "Touch a control in the plugin's window and it takes the next free slot. \
              Esc or a second click ends it",
        style: if on { Style::On } else { Style::Plain },
        dot: false,
        weight: 1.15,
        output: Some(Output::Learn),
    };
    match row.editor {
        Editor::Closed | Editor::Opening | Editor::Open => {
            let (shown, name, tip, style) = match row.editor {
                Editor::Closed => (
                    "Open editor",
                    "Open editor",
                    "Open the plugin's own window",
                    Style::Plain,
                ),
                Editor::Opening => (
                    "Opening\u{2026}",
                    "Opening editor",
                    "The plugin's window is opening",
                    Style::On,
                ),
                _ => (
                    "Editor open",
                    "Close editor",
                    "Close the plugin's window",
                    Style::On,
                ),
            };
            parts.push(Part {
                shown,
                name: name.to_owned(),
                tip,
                style,
                dot: *row.editor == Editor::Open,
                weight: 1.35,
                output: (*row.editor != Editor::Opening).then_some(Output::Editor),
            });
            if learn_ok {
                parts.push(learn(row.learning));
                parts.push(all);
            }
        }
        Editor::NotResponding => {
            parts.push(Part {
                shown: "Not responding",
                name: "Editor not responding".to_owned(),
                tip: "The plugin's window has stopped answering. Lumen never ends a plugin on its own",
                style: Style::Warn,
                dot: false,
                weight: 1.4,
                output: None,
            });
            parts.push(Part {
                shown: "End plugin",
                name: "End plugin".to_owned(),
                tip: "Ends this plugin. The card then offers Reload",
                style: Style::Fill,
                dot: false,
                weight: 1.0,
                output: Some(Output::End),
            });
        }
        Editor::Unavailable(why) => {
            parts.push(Part {
                shown: "No editor",
                name: "No editor".to_owned(),
                tip: why,
                style: Style::Disabled,
                dot: false,
                weight: 1.2,
                output: None,
            });
            if learn_ok {
                parts.push(all);
            }
        }
    }
    let gap = 4.0;
    let total: f32 = parts.iter().map(|p| p.weight).sum();
    let room = area.width() - gap * (parts.len().saturating_sub(1)) as f32;
    let mut out = Vec::new();
    let mut left = area.left();
    for (i, part) in parts.iter().enumerate() {
        let width = room * part.weight / total;
        let rect = Rect::from_min_size(pos2(left, area.top()), vec2(width, area.height()))
            .round_to_pixels(ui.pixels_per_point());
        left += width + gap;
        if pill(ui, rect, id.with(("button", i)), part) {
            out.extend(part.output.clone());
        }
    }
    out
}

/// A raised button with a label; true when it was pressed.
fn pill(ui: &mut Ui, rect: Rect, id: egui::Id, part: &Part<'_>) -> bool {
    let enabled = part.output.is_some() && part.style != Style::Disabled;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            enabled,
            part.style == Style::On,
            part.name.clone(),
        )
    });
    let palette = *theme::p();
    let lit = motion::hover(ui.ctx(), id.with("lit"), part.style == Style::On);
    let mut face = raised::face_of(ui, &response).enabled(part.style != Style::Disabled);
    face = match part.style {
        Style::On => face.lit(lit, Tone::On),
        Style::Fill => face.lit(1.0, Tone::Accent),
        _ => face,
    };
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    let ink = if part.style == Style::Warn {
        palette.warning
    } else {
        face.text()
    };
    let font = FontId::proportional(theme::size::CAPTION);
    let text = fit(ui.painter(), part.shown, font.clone(), body.width() - 10.0);
    let galley = ui.painter().layout_no_wrap(text, font, ink);
    let dot = if part.dot { 10.0 } else { 0.0 };
    let width = galley.size().x + dot;
    if part.dot {
        ui.painter().circle_filled(
            pos2(body.center().x - width / 2.0 + 3.0, body.center().y),
            2.5,
            ink,
        );
    }
    let mid = pos2(body.center().x + dot / 2.0, body.center().y);
    raised::centre_galley(
        ui.painter(),
        Rect::from_center_size(mid, galley.size()),
        galley,
        ink,
    );
    raised::focus_ring(ui.painter(), &response, rect);
    response.on_hover_text(part.tip).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: u32, flags: u32) -> ParamInfo {
        ParamInfo {
            id,
            title: format!("P{id}"),
            units: String::new(),
            steps: 0,
            default: 0.5,
            flags,
            unit: 0,
            value: 0.5,
        }
    }

    const AUTOMATE: u32 = 1;
    const READ_ONLY: u32 = 1 << 1;
    const HIDDEN: u32 = 1 << 4;
    const PROGRAM: u32 = 1 << 15;
    const BYPASS: u32 = 1 << 16;

    #[test]
    fn the_default_strip_is_the_first_parameters_a_person_would_move() {
        let list = vec![
            info(1, AUTOMATE | BYPASS),
            info(2, HIDDEN),
            info(3, AUTOMATE | READ_ONLY),
            info(4, AUTOMATE | PROGRAM),
            info(10, AUTOMATE),
            info(11, AUTOMATE),
        ];
        assert_eq!(default_shown(&list), [10, 11]);
        // At most eight.
        let many: Vec<_> = (100..130).map(|id| info(id, AUTOMATE)).collect();
        assert_eq!(default_shown(&many), (100..108).collect::<Vec<_>>());
        assert!(default_shown(&[]).is_empty());
    }

    #[test]
    fn a_chosen_strip_keeps_its_order_and_drops_what_the_plugin_no_longer_lists() {
        let list = vec![
            info(1, AUTOMATE),
            info(2, HIDDEN),
            info(3, AUTOMATE),
            info(4, 0),
        ];
        // 9 is gone, 2 is hidden, 3 is twice.
        assert_eq!(resolve(&list, Some(&[4, 9, 3, 2, 3, 1])), [4, 3, 1]);
        // A choice of nothing is an empty strip, not the default.
        assert!(resolve(&list, Some(&[])).is_empty());
        // No choice made: the default.
        assert_eq!(resolve(&list, None), [1, 3, 4]);
        // Read-only parameters may be chosen, though never by default.
        let list = vec![info(1, READ_ONLY)];
        assert_eq!(resolve(&list, Some(&[1])), [1]);
        assert!(resolve(&list, None).is_empty());
    }

    #[test]
    fn a_long_choice_is_cut_at_eight() {
        let list: Vec<_> = (0..20).map(|id| info(id, AUTOMATE)).collect();
        let chosen: Vec<u32> = (0..20).rev().collect();
        assert_eq!(resolve(&list, Some(&chosen)).len(), SLOTS);
    }
}
