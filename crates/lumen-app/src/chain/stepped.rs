//! Stepped parameters on the card's strip (M6 slice 5g, the owner's A): a
//! parameter with two states is a switch, a parameter with a few named
//! choices is a chooser, a small button with the plugin's own word for the
//! value (click it for the next choice) and an arrow beside it that opens the
//! whole list. Every click is one change that lands at once, so nothing is left
//! to revert. A knob stays for what is continuous.

use eframe::egui::{
    self, Align2, FontId, Id as EguiId, Key, Rect, Response, Sense, Shape, Stroke, Ui, UiBuilder,
    pos2, vec2,
};
use eframe::emath::GuiRounding as _;

use super::strip::Slot;
use crate::widgets::param::step_by;
use crate::widgets::{self, raised};
use crate::{motion, theme};

use crate::hosting::MOST_CHOICES;
/// Room each side of a chooser, so neighbours do not touch.
const SIDE: f32 = 3.0;
/// The chooser's height, and its arrow's width.
const HEIGHT: f32 = theme::size::CONTROL_SMALL + 2.0;
const ARROW: f32 = 14.0;
/// The most rows the list shows before it scrolls.
const LIST_HEIGHT: f32 = 240.0;

/// What a parameter looks like on the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Knob,
    Switch,
    Chooser,
}

/// A knob for what is continuous or read only, a switch for two states, a
/// chooser for a short list.
pub(super) fn kind_of(slot: &Slot) -> Kind {
    if slot.read_only {
        return Kind::Knob;
    }
    match slot.steps {
        1 => Kind::Switch,
        2..=MOST_CHOICES => Kind::Chooser,
        _ => Kind::Knob,
    }
}

/// Whether the plugin has told its steps apart: some plugins answer a request
/// for the text of a value with the text of the value they hold now, so every
/// step comes back with the same word and none of them says anything.
fn words_differ(slot: &Slot) -> bool {
    let mut said = slot.choices.iter().filter(|t| !t.is_empty());
    said.next()
        .is_some_and(|first| said.any(|other| other != first))
}

/// The word for choice `index` of the slot: the plugin's own once it has said
/// it, else the current value's text for the current one, else its number.
pub(super) fn choice_text(slot: &Slot, index: usize) -> String {
    if words_differ(slot)
        && let Some(text) = slot.choices.get(index).filter(|t| !t.is_empty())
    {
        return text.clone();
    }
    if index == position(slot) {
        slot.text.clone()
    } else {
        format!("{}", index + 1)
    }
}

/// Which step the slot's value is at.
pub(super) fn position(slot: &Slot) -> usize {
    (slot.value.clamp(0.0, 1.0) * f64::from(slot.steps.max(1))).round() as usize
}

fn value_at(slot: &Slot, index: usize) -> f64 {
    index as f64 / f64::from(slot.steps.max(1))
}

/// A switch in `area`, centred. Returns its response and the value it asks
/// for when clicked.
pub(super) fn switch(ui: &mut Ui, area: Rect, id: EguiId, slot: &Slot) -> (Response, Option<f64>) {
    let mut on = slot.value >= 0.5;
    let response = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::LeftToRight,
                )),
            |ui| {
                ui.push_id(id, |ui| {
                    // A switch with no label of its own; named below.
                    widgets::switch(ui, &mut on, "", true)
                })
                .inner
            },
        )
        .inner;
    let name = format!("{}: {}", slot.name, slot.text);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            slot.value >= 0.5,
            &slot.name,
        )
    });
    let response = response.on_hover_text(name);
    let asks = response
        .clicked()
        .then_some(if slot.value >= 0.5 { 0.0 } else { 1.0 });
    (response, asks)
}

/// A chooser in `area`'s middle: the word, and the arrow that opens the list.
/// Returns the word's response (for the context menu) and the value asked
/// for, by a click, an arrow key, the wheel or a row of the list.
pub(super) fn chooser(
    ui: &mut Ui,
    area: Rect,
    id: EguiId,
    slot: &Slot,
    selected: bool,
) -> (Response, Option<f64>) {
    let palette = *theme::p();
    let rect = Rect::from_center_size(area.center(), vec2(area.width() - 2.0 * SIDE, HEIGHT))
        .round_to_pixels(ui.pixels_per_point());
    let arrow_rect = Rect::from_min_max(pos2(rect.right() - ARROW, rect.top()), rect.max);
    let word_rect = Rect::from_min_max(rect.min, pos2(arrow_rect.left(), rect.bottom()));
    let count = slot.steps.max(1) as usize + 1;
    let here = position(slot);
    let word = choice_text(slot, here);
    let mut asks = None;

    let word_response = ui.interact(word_rect, id.with("word"), Sense::click());
    word_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}, choice: {word}", slot.name),
        )
    });
    let arrow_response = ui.interact(arrow_rect, id.with("arrow"), Sense::click());
    arrow_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Choose {}", slot.name),
        )
    });

    // Next choice, wrapping round; the keys and the wheel step either way.
    if word_response.clicked() {
        asks = Some(value_at(slot, (here + 1) % count));
        word_response.request_focus();
    }
    if word_response.has_focus() {
        let (down, up) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
            )
        });
        if down {
            asks = Some(step_by(slot.value, -1.0, false, slot.steps));
        }
        if up {
            asks = Some(step_by(slot.value, 1.0, false, slot.steps));
        }
    }
    let notches = widgets::wheel::steps(ui, id, rect);
    if notches != 0.0 {
        asks = Some(step_by(slot.value, f64::from(notches), false, slot.steps));
    }

    // The body, raised, with the word and the arrow.
    let face = raised::face_of(ui, &word_response)
        .hover(motion::hover(
            ui.ctx(),
            id.with("hover any"),
            word_response.hovered() || arrow_response.hovered(),
        ))
        .pressed(
            word_response.is_pointer_button_down_on() || arrow_response.is_pointer_button_down_on(),
        );
    let radius = theme::radius::for_height(rect.height());
    let body = raised::paint(ui.painter(), rect, radius, &face);
    let painter = ui.painter();
    let font = FontId::proportional(theme::size::CAPTION);
    let room = word_rect.width() - 6.0;
    let shown = widgets::fit(painter, &word, font.clone(), room);
    painter.text(
        pos2(word_rect.center().x, body.center().y),
        Align2::CENTER_CENTER,
        shown,
        font,
        if selected { palette.text } else { face.text() },
    );
    painter.vline(
        arrow_rect.left(),
        rect.top() + 4.0..=rect.bottom() - 4.0,
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
    let c = pos2(arrow_rect.center().x, body.center().y);
    painter.add(Shape::line(
        vec![
            pos2(c.x - 3.5, c.y - 1.5),
            pos2(c.x, c.y + 2.0),
            pos2(c.x + 3.5, c.y - 1.5),
        ],
        Stroke::new(theme::stroke::LIT, palette.text_secondary),
    ));
    raised::focus_ring(ui.painter(), &word_response, word_rect);
    raised::focus_ring(ui.painter(), &arrow_response, arrow_rect);

    let word_response = word_response.on_hover_text(format!(
        "{}: {word}. Click for the next choice, or use the arrow to see them all",
        slot.name
    ));
    let arrow_response = arrow_response.on_hover_text(format!("All choices of {}", slot.name));

    // The whole list, under the arrow.
    let rows: Vec<String> = (0..count).map(|i| choice_text(slot, i)).collect();
    let picked = widgets::Menu::below(&arrow_response)
        .keeping_open()
        .show(|ui| list(ui, &rows, here))
        .flatten();
    if let Some(index) = picked {
        asks = Some(value_at(slot, index));
    }
    (
        word_response,
        asks.filter(|v| (*v - slot.value).abs() > 1e-9),
    )
}

/// The choices as rows, the current one marked; the one clicked.
fn list(ui: &mut Ui, rows: &[String], here: usize) -> Option<usize> {
    let mut picked = None;
    let row_height = ui.spacing().interact_size.y;
    ui.set_min_width(120.0);
    let show = |ui: &mut Ui, range: std::ops::Range<usize>| {
        for index in range {
            if widgets::choice_row(ui, &rows[index], index == here).clicked() {
                picked = Some(index);
                ui.close();
            }
        }
    };
    egui::ScrollArea::vertical()
        .max_height(LIST_HEIGHT)
        .auto_shrink([true, true])
        .show_rows(ui, row_height, rows.len(), show);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(steps: i32, value: f64) -> Slot {
        Slot {
            id: 1,
            name: "Wave".into(),
            text: "Saw".into(),
            value,
            default: 0.0,
            steps,
            read_only: false,
            choices: vec![],
        }
    }

    #[test]
    fn two_states_are_a_switch_a_short_list_a_chooser_and_the_rest_a_knob() {
        assert_eq!(kind_of(&slot(0, 0.3)), Kind::Knob);
        assert_eq!(kind_of(&slot(1, 0.0)), Kind::Switch);
        assert_eq!(kind_of(&slot(3, 0.0)), Kind::Chooser);
        assert_eq!(kind_of(&slot(MOST_CHOICES, 0.0)), Kind::Chooser);
        assert_eq!(kind_of(&slot(MOST_CHOICES + 1, 0.0)), Kind::Knob);
        let mut read_only = slot(3, 0.0);
        read_only.read_only = true;
        assert_eq!(
            kind_of(&read_only),
            Kind::Knob,
            "a value that cannot be set"
        );
    }

    #[test]
    fn a_choice_is_the_plugins_own_word_or_the_current_text_or_its_number() {
        let mut s = slot(3, 2.0 / 3.0);
        s.choices = vec!["Sine".into(), String::new(), String::new(), "Square".into()];
        assert_eq!(position(&s), 2);
        assert_eq!(choice_text(&s, 0), "Sine");
        // Not said yet: the current one is the value's own text, the rest are numbers.
        assert_eq!(choice_text(&s, 2), "Saw");
        assert_eq!(choice_text(&s, 1), "2");
        assert_eq!(choice_text(&s, 3), "Square");
    }

    #[test]
    fn a_plugin_that_answers_every_step_with_its_current_word_says_nothing_of_the_others() {
        let mut s = slot(3, 1.0 / 3.0);
        s.text = "Mono".into();
        s.choices = vec!["Mono".into(); 4];
        assert_eq!(
            choice_text(&s, 1),
            "Mono",
            "the current one is the plugin's word"
        );
        assert_eq!(choice_text(&s, 0), "1");
        assert_eq!(choice_text(&s, 3), "4");
    }

    #[test]
    fn a_step_is_a_value_the_plugin_names_exactly() {
        let s = slot(3, 0.0);
        assert_eq!(value_at(&s, 0), 0.0);
        assert!((value_at(&s, 2) - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(value_at(&s, 3), 1.0);
    }
}
