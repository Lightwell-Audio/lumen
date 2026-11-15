use eframe::egui::{self, Align, Id, Key, Rect, Sense, Ui, vec2};
use eframe::emath::GuiRounding as _;

use crate::theme;
use crate::widgets::{raised, wheel};

#[derive(Clone, Copy)]
pub struct Number<'a> {
    pub name: &'a str,
    pub value: f64,
    pub range: (f64, f64),
    pub step: f64,
    pub reset: f64,
    pub shown: Option<&'a str>,
    pub parse: Option<fn(&str) -> Option<f64>>,
}

impl<'a> Number<'a> {
    pub fn new(name: &'a str, value: f64, range: (f64, f64), reset: f64) -> Self {
        Self {
            name,
            value,
            range,
            step: 1.0,
            reset,
            shown: None,
            parse: None,
        }
    }

    pub fn shown(mut self, shown: &'a str) -> Self {
        self.shown = Some(shown);
        self
    }

    pub fn parse(mut self, parse: fn(&str) -> Option<f64>) -> Self {
        self.parse = Some(parse);
        self
    }

    fn text(&self, value: f64) -> String {
        match self.shown {
            Some(shown) => shown.to_owned(),
            None => format!("{}", value.round() as i64),
        }
    }
}

pub fn number(ui: &mut Ui, rect: Rect, id: Id, spec: Number<'_>) -> Option<f64> {
    let typing_id = id.with("typing");
    let typing: Option<String> = ui.data(|d| d.get_temp(typing_id));
    if let Some(mut text) = typing {
        let (commit, cancel) =
            ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        raised::paint_sunken(ui.painter(), rect, theme::radius::for_height(rect.height()));
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(egui::FontId::monospace(theme::size::MONO_SMALL))
                .text_color(theme::p().text)
                .horizontal_align(Align::Center)
                .frame(egui::Frame::NONE)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        if cancel || commit || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel {
                let read = match spec.parse {
                    Some(parse) => parse(&text),
                    None => text.trim().parse::<f64>().ok(),
                };
                if let Some(value) = read {
                    return Some(value.clamp(spec.range.0, spec.range.1));
                }
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return None;
    }

    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text(format!(
            "{}: wheel, arrows, click and type, or double-click for {}",
            spec.name,
            spec.text(spec.reset)
        ));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("{}: {}", spec.name, spec.text(spec.value)),
        )
    });
    let mut out = spec.value;
    let mut changed = false;
    let step = if ui.input(|i| i.modifiers.shift) {
        spec.step / 10.0
    } else {
        spec.step
    };
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        out = spec.reset;
        changed = true;
    }
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
        let (up, down, typed) = ui.input(|i| {
            let typed = i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) if t.chars().all(|c| c.is_ascii_graphic() || c == ' ') => {
                    Some(t.clone())
                }
                _ => None,
            });
            (
                i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowLeft),
                typed,
            )
        });
        if up {
            out += step;
            changed = true;
        }
        if down {
            out -= step;
            changed = true;
        }
        if let Some(text) = typed {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            ui.data_mut(|d| d.insert_temp(typing_id, spec.text(spec.value)));
        }
    }
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        out += f64::from(notches) * step;
        changed = true;
    }

    let palette = theme::p();
    let hover = crate::motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.has_focus(),
    );
    let radius = theme::radius::for_height(rect.height());
    if ui.is_rect_visible(rect) {
        raised::paint_sunken(ui.painter(), rect, radius);
        if hover > 0.0 {
            ui.painter()
                .rect_filled(rect, f32::from(radius), palette.ember.gamma_multiply(hover));
        }
        let galley = ui.painter().layout_no_wrap(
            spec.text(out.clamp(spec.range.0, spec.range.1)),
            egui::FontId::monospace(theme::size::MONO_SMALL),
            palette.text,
        );
        let at = egui::Align2::CENTER_CENTER
            .anchor_size(rect.center(), galley.size())
            .round_to_pixels(ui.pixels_per_point())
            .min;
        ui.painter().galley(at, galley, palette.text);
        raised::focus_ring(ui.painter(), &response, rect);
    }
    changed.then(|| out.clamp(spec.range.0, spec.range.1))
}

pub fn parse_note(text: &str) -> Option<f64> {
    let text = text.trim();
    if let Ok(number) = text.parse::<f64>() {
        return Some(number);
    }
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let text = text.to_uppercase();
    let split = text
        .find(|c: char| c.is_ascii_digit() || c == '-')
        .unwrap_or(text.len());
    let (name, octave) = text.split_at(split);
    let octave: i32 = octave.parse().ok()?;
    let semitone = NAMES.iter().position(|n| *n == name.trim())? as i32;
    let note = (octave + 1) * 12 + semitone;
    (0..=127).contains(&note).then_some(f64::from(note))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_read_as_numbers_or_names() {
        assert_eq!(parse_note("36"), Some(36.0));
        assert_eq!(
            parse_note("C1"),
            Some(24.0),
            "middle C is C4, as the roll says"
        );
        assert_eq!(parse_note("c#2"), Some(37.0));
        assert_eq!(parse_note("F#2"), Some(42.0));
        assert_eq!(parse_note("60"), Some(60.0));
        assert_eq!(parse_note("C-1"), Some(0.0));
        assert_eq!(parse_note("G9"), Some(127.0));
        assert_eq!(parse_note("C10"), None, "past the notes there are");
        assert_eq!(parse_note("hat"), None);
        assert_eq!(parse_note(""), None);
    }

    #[test]
    fn a_field_shows_the_text_it_is_given() {
        let spec = Number::new("Input note", 36.0, (0.0, 127.0), 36.0).shown("36");
        assert_eq!(spec.text(36.0), "36");
        let plain = Number::new("Choke group", 2.4, (0.0, 8.0), 0.0);
        assert_eq!(plain.text(2.4), "2", "numbers are rounded to whole ones");
    }
}
