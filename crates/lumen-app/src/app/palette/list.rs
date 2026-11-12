//! The palette's list: the rows with their matched letters and keycaps, the
//! sliding highlight, and the strip under them.

use eframe::egui::{self, Color32, FontId, RichText, Sense, text::LayoutJob, vec2};

use super::{CHROME, Entry, ROW, ROWS, Row, StripRoom, TOP};
use crate::{App, motion, theme, widgets};

impl App {
    /// The rows; the index of one clicked.
    pub(super) fn palette_list(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[Row],
        moved: bool,
    ) -> Option<usize> {
        let palette = *theme::p();
        let recent = self.recent_shown().min(rows.len());
        let mut picked = None;
        // Short windows show fewer rows, so the palette never runs over
        // the transport bar or off the bottom.
        let room = ui.ctx().content_rect().height() - TOP - CHROME;
        egui::ScrollArea::vertical()
            .max_height(room.clamp(ROW * 3.0, ROW * ROWS))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.add_space(theme::space::S);
                    ui.label(
                        RichText::new(format!(
                            "No command matches \u{201C}{}\u{201D}. Try fewer letters.",
                            self.palette.query.trim()
                        ))
                        .color(palette.text_hint),
                    );
                    ui.add_space(theme::space::S);
                }
                // The chosen row's fill, drawn under the rows once they are
                // laid out, slides from row to row (M4.5, M5).
                let highlight = ui.painter().add(egui::Shape::Noop);
                let list_top = ui.min_rect().top();
                let mut chosen_rect = None;
                for (i, row) in rows.iter().enumerate() {
                    if recent > 0 && (i == 0 || i == recent) {
                        let heading = if i == 0 { "Recent" } else { "All commands" };
                        ui.label(
                            RichText::new(heading)
                                .size(theme::size::CAPTION)
                                .color(palette.text_hint),
                        );
                    }
                    let (response, rekey) = self.palette_row(ui, row, i == self.palette.chosen);
                    if i == self.palette.chosen {
                        chosen_rect = Some(response.rect);
                    }
                    if let (Some(slot), Entry::Command(command)) = (rekey, &row.entry) {
                        self.palette.chosen = i;
                        self.start_rebind(*command, Some(slot));
                    }
                    if i == self.palette.chosen && moved {
                        response.scroll_to_me(None);
                    }
                    // Hover picks a row only when the pointer moves, so a
                    // resting pointer does not fight the arrow keys.
                    if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.palette.chosen = i;
                    }
                    if response.clicked() {
                        picked = Some(i);
                    }
                }
                if let Some(rect) = chosen_rect {
                    let y = motion::glide_over(
                        ui.ctx(),
                        egui::Id::new("palette highlight"),
                        rect.top() - list_top,
                        true,
                        motion::FAST,
                    );
                    let shown = rect.translate(vec2(0.0, list_top + y - rect.top()));
                    ui.painter().set(
                        highlight,
                        egui::Shape::rect_filled(
                            shown,
                            theme::radius::MEDIUM,
                            theme::p().accent_muted,
                        ),
                    );
                }
            });
        picked
    }

    /// A row; with the chord slot whose keycaps were clicked, to record
    /// new keys for it. The `chosen` row is under the list's sliding
    /// highlight, so its keycaps step up; a hovered row that is not chosen
    /// (the pointer rests while the arrow keys move) is Ember.
    fn palette_row(
        &self,
        ui: &mut egui::Ui,
        row: &Row,
        chosen: bool,
    ) -> (egui::Response, Option<usize>) {
        let mut rekey = None;
        let palette = *theme::p();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        let keys = match row.entry {
            Entry::Command(command) => self.keymap.text(command),
            _ => None,
        };
        let name = match (&row.ready, keys) {
            (Err(why), _) => format!("{}, unavailable: {why}", row.label),
            (Ok(()), Some(keys)) => format!("{}, {keys}", row.label),
            (Ok(()), None) => row.label.clone(),
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, row.ready.is_ok(), &name)
        });
        if !ui.is_rect_visible(rect) {
            return (response, None);
        }
        // The chosen row's fill is drawn by the list, sliding (M4.5).
        if response.hovered() && !chosen {
            ui.painter()
                .rect_filled(rect, theme::radius::MEDIUM, palette.ember);
        }
        let text = if row.ready.is_ok() {
            palette.text
        } else {
            palette.text_disabled
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(theme::space::S, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.label(highlighted(&row.label, &row.matched, text, palette.accent));
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Entry::Command(command) = row.entry {
                let area = command.info().area;
                if let Some(chord) = self.keymap.chords(command).first() {
                    let caps = widgets::keycaps(
                        ui,
                        &chord.modifier_names(),
                        chord.key_name(),
                        area.light(),
                        chosen,
                    );
                    caps.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Change {} shortcut {}", command.name(), chord.text()),
                        )
                    });
                    if caps.clicked() {
                        rekey = Some(0);
                    }
                }
                ui.label(
                    RichText::new(area.name())
                        .size(theme::size::CAPTION)
                        .color(palette.text_hint),
                );
            }
        });
        (response, rekey)
    }

    /// What the chosen row does, or why it cannot run, then the list's keys
    /// with every chord that runs it. The text keeps the room of the
    /// longest a row can say, so the palette never changes height as the
    /// chosen row changes.
    pub(super) fn palette_strip(&mut self, ui: &mut egui::Ui, row: Option<&Row>) {
        let palette = *theme::p();
        let rebind = match row.map(|r| &r.entry) {
            Some(Entry::Command(command)) => self.rebind.filter(|r| r.command == *command),
            _ => None,
        };
        let said;
        let (text, colour) = match (row, rebind) {
            (None, _) => ("", palette.text_secondary),
            (Some(_), Some(r)) => match r.clash {
                Some((chord, other)) => {
                    said = format!(
                        "{} is used by {}. Enter takes it \u{B7} Esc cancels",
                        chord.text(),
                        other.name()
                    );
                    (said.as_str(), palette.warning)
                }
                None => {
                    said = format!(
                        "Press the new keys for {} \u{B7} Esc cancels",
                        r.command.name()
                    );
                    (said.as_str(), palette.accent)
                }
            },
            (Some(row), None) => match (self.palette.notice, row.ready) {
                (Some(why), _) | (None, Err(why)) => (why, palette.warning),
                (None, Ok(())) => (row.entry.about(), palette.text_secondary),
            },
        };
        let width = ui.available_width();
        let font = FontId::proportional(theme::size::BODY);
        let ppp = ui.ctx().pixels_per_point();
        let room = match self.palette.strip_room {
            Some(room) if room.width == width && room.pixels_per_point == ppp => room.height,
            _ => {
                let room = Entry::every_about()
                    .map(|about| {
                        ui.fonts_mut(|f| {
                            f.layout(about.to_owned(), font.clone(), colour, width)
                                .size()
                                .y
                        })
                    })
                    .fold(0.0, f32::max);
                self.palette.strip_room = Some(StripRoom {
                    width,
                    pixels_per_point: ppp,
                    height: room,
                });
                room
            }
        };
        let galley = ui.fonts_mut(|f| f.layout(text.to_owned(), font, colour, width));
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, room.max(galley.size().y)), Sense::hover());
        if !text.is_empty() {
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
        }
        ui.painter().galley(rect.left_top(), galley, colour);

        // As tall as keycaps whether the row has a shortcut or not, so the
        // palette's height does not change with the chosen row.
        let footer = widgets::caps_height(ui);
        // The palette's own keys, which are not remappable; Ctrl is Cmd on
        // macOS, as the keys it reads.
        let command_key = crate::commands::keys::modifier_names(true, false, false)[0];
        ui.horizontal(|ui| {
            ui.set_min_height(footer);
            ui.label(
                RichText::new(format!(
                    "\u{2191}\u{2193} move \u{B7} Enter run \u{B7} {command_key}+Enter change keys \u{B7} Esc close"
                ))
                .size(theme::size::CAPTION)
                .color(palette.text_hint),
            );
            let Some(Entry::Command(command)) = row.map(|r| &r.entry) else {
                return;
            };
            let area = command.info().area;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let chords = self.keymap.chords(*command);
                for (i, chord) in chords.iter().enumerate().rev() {
                    let caps = widgets::keycaps(
                        ui,
                        &chord.modifier_names(),
                        chord.key_name(),
                        area.light(),
                        false,
                    );
                    caps.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            format!("Shortcut {}", chord.text()),
                        )
                    });
                    if i > 0 {
                        ui.label(RichText::new("or").color(palette.text_hint));
                    }
                }
            });
        });
    }
}

/// `label` with the characters at `matched` in `accent`.
fn highlighted(label: &str, matched: &[usize], text: Color32, accent: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::proportional(theme::size::BODY);
    for (i, c) in label.chars().enumerate() {
        let colour = if matched.contains(&i) { accent } else { text };
        job.append(
            &c.to_string(),
            0.0,
            egui::TextFormat::simple(font.clone(), colour),
        );
    }
    job
}
