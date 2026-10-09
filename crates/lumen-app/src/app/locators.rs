//! Named locators on the strip (M3 slice 5d, as in Ableton): flags above
//! the loop brace. Set one at the playhead with the Set button or M (again
//! on the same place removes it); click a flag to jump there, drag it to
//! move it (snapped; Alt places freely), double-click or F2 to rename,
//! Delete to remove; jump between them with ◂ ▸ or Ctrl+Left and
//! Ctrl+Right. Every change is one undo step.

use eframe::egui::{self, Align2, CursorIcon, FontId, Key, Rect, Sense, Stroke, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Marker};

use super::grid::draw_snap_guide;
use super::rulers::Layout;
use crate::commands::Command;
use crate::{App, motion, ruler, theme, widgets};

/// The flags' height, from the strip's top; the brace takes the rest.
const FLAG_HEIGHT: f32 = 14.0;
/// The flag's triangle, and the gap before the name.
const FLAG_POINT: f32 = 6.0;
const NAME_GAP: f32 = 3.0;

/// The selected locator and the one being renamed (not saved).
#[derive(Debug, Default)]
pub(crate) struct Locators {
    pub selected: Option<Id>,
    renaming: Option<(Id, String)>,
}

/// The latest locator strictly before `at`.
pub(crate) fn previous(markers: &[Marker], at: Ticks) -> Option<Ticks> {
    markers.iter().rev().map(|m| m.at).find(|t| *t < at)
}

/// The earliest locator strictly after `at`.
pub(crate) fn next(markers: &[Marker], at: Ticks) -> Option<Ticks> {
    markers.iter().map(|m| m.at).find(|t| *t > at)
}

/// A locator exactly at `at`, if there is one.
pub(crate) fn at(markers: &[Marker], at: Ticks) -> Option<Id> {
    markers.iter().find(|m| m.at == at).map(|m| m.id)
}

impl App {
    /// Starts renaming locator `id` on its flag.
    pub(crate) fn start_locator_rename(&mut self, id: Id) {
        if let Some(m) = self.document.project().markers.iter().find(|m| m.id == id) {
            self.locators.renaming = Some((id, m.name.clone()));
        }
    }

    /// Adds a locator at the playhead, or removes the one already there.
    pub(crate) fn set_locator(&mut self) {
        let here = self.transport.state().ticks;
        match at(&self.document.project().markers, here) {
            Some(id) => {
                self.document.delete_marker(id);
                if self.locators.selected == Some(id) {
                    self.locators.selected = None;
                }
            }
            None => {
                let id = self.document.add_marker(here, None);
                self.locators.selected = Some(id);
            }
        }
    }

    /// Moves the playhead to the previous (`forward` false) or next locator.
    pub(crate) fn jump_to_locator(&mut self, forward: bool) {
        let here = self.transport.state().ticks;
        let markers = &self.document.project().markers;
        let target = if forward {
            next(markers, here)
        } else {
            previous(markers, here)
        };
        if let Some(t) = target {
            self.transport.locate(t);
            self.transport.follow_paused = false;
        }
    }

    /// The flags, on the strip's upper part. Called after the loop brace,
    /// so a flag wins where the two meet.
    pub(crate) fn locator_flags(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        at: &Layout,
    ) {
        let selected = self.locators.selected;
        if selected.is_some_and(|id| !self.document.project().markers.iter().any(|m| m.id == id)) {
            self.locators.selected = None;
        }
        let markers = self.document.project().markers.clone();
        let palette = theme::p();
        let font = FontId::proportional(theme::size::CAPTION);
        for marker in &markers {
            let x = at.view.x(at.canvas.left(), marker.at.as_quarters());
            let name_width = painter
                .layout_no_wrap(marker.name.clone(), font.clone(), palette.text)
                .size()
                .x;
            let flag = Rect::from_min_max(
                pos2(x, at.strip.top()),
                pos2(
                    x + FLAG_POINT + NAME_GAP + name_width + NAME_GAP,
                    at.strip.top() + FLAG_HEIGHT,
                ),
            );
            if flag.right() < at.strip.left() || flag.left() > at.strip.right() {
                continue;
            }
            let id = ui.id().with(("locator", marker.id));
            if let Some((renaming, text)) = &mut self.locators.renaming
                && *renaming == marker.id
            {
                let rect = Rect::from_min_size(
                    pos2(x + FLAG_POINT, at.strip.top()),
                    egui::vec2(140.0, at.strip.height()),
                );
                let response = ui.put(
                    rect,
                    egui::TextEdit::singleline(text)
                        .id(id.with("rename"))
                        .font(font.clone())
                        .margin(egui::vec2(3.0, 1.0)),
                );
                if !response.has_focus() && !response.lost_focus() {
                    response.request_focus();
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.locators.renaming = None;
                } else if response.lost_focus() {
                    let name = text.clone();
                    self.locators.renaming = None;
                    self.document.rename_marker(marker.id, &name);
                }
                continue;
            }

            let response = ui
                .interact(flag, id, Sense::click_and_drag())
                .on_hover_text(
                    "Click to jump here; drag to move (Alt places freely); double-click or F2 \
                     to rename; Delete removes it",
                );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Locator {}", marker.name),
                )
            });
            if response.hovered() && !response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if response.drag_started() {
                self.locators.selected = Some(marker.id);
                self.arrangement.selected.clear();
                self.arrangement.clips.clear();
                self.arrangement.range = None;
                // Where on the flag it was grabbed, so it doesn't jump.
                let grabbed = ui
                    .input(|i| i.pointer.press_origin())
                    .map_or(0.0, |p| p.x - x);
                ui.data_mut(|d| d.insert_temp(id, grabbed));
            }
            if response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                let free = ui.input(|i| i.modifiers.alt);
                let grabbed: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
                let to = self.snapped(
                    ruler::ticks_at(at.view.beat_at(at.canvas.left(), p.x - grabbed)),
                    free,
                );
                self.document
                    .move_marker(marker.id, to, Some("locator drag"));
                draw_snap_guide(
                    painter,
                    at.view.x(at.canvas.left(), to.as_quarters()),
                    at.guides(),
                );
            }
            if response.double_clicked() {
                self.locators.renaming = Some((marker.id, marker.name.clone()));
            } else if response.clicked() {
                self.locators.selected = Some(marker.id);
                self.arrangement.selected.clear();
                self.arrangement.clips.clear();
                self.arrangement.range = None;
                self.transport.locate(marker.at);
                self.transport.follow_paused = false;
            }
            let id_now = marker.id;
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    self.locators.renaming = Some((id_now, marker.name.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    self.document.delete_marker(id_now);
                    ui.close();
                }
            });

            let lit = self.locators.selected == Some(marker.id);
            let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
            let colour = if lit {
                palette.accent
            } else {
                palette.start_marker
            };
            // The post down through the strip, the flag's point, the name.
            painter.line_segment(
                [pos2(x, at.strip.top()), pos2(x, at.strip.bottom())],
                Stroke::new(1.0, colour),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x, at.strip.top() + 1.0),
                    pos2(x + FLAG_POINT, at.strip.top() + FLAG_HEIGHT / 2.0),
                    pos2(x, at.strip.top() + FLAG_HEIGHT - 1.0),
                ],
                colour,
                Stroke::NONE,
            ));
            if hover > 0.0 {
                painter.rect_filled(
                    flag.with_min_x(x + FLAG_POINT),
                    theme::radius::SMALL,
                    palette.control_hover.gamma_multiply(hover),
                );
            }
            painter.text(
                pos2(x + FLAG_POINT + NAME_GAP, flag.center().y),
                Align2::LEFT_CENTER,
                &marker.name,
                font.clone(),
                if lit {
                    palette.text
                } else {
                    palette.text_secondary
                },
            );
        }
    }

    /// The strip's corner over the track headers: ◂ Set ▸.
    pub(crate) fn locator_buttons(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let palette = theme::p();
        let here = self.transport.state().ticks;
        let markers = &self.document.project().markers;
        let (has_previous, has_next) = (
            previous(markers, here).is_some(),
            next(markers, here).is_some(),
        );
        let there = at(markers, here).is_some();
        let width = rect.height();
        let right = Rect::from_min_max(pos2(rect.right() - width, rect.top()), rect.max);
        let set = Rect::from_min_max(
            pos2(right.left() - 36.0, rect.top()),
            pos2(right.left(), rect.bottom()),
        );
        let left = Rect::from_min_max(pos2(set.left() - width, rect.top()), set.left_bottom());
        let buttons = [
            (
                left,
                "Previous locator",
                "Previous locator",
                Command::PreviousLocator,
                has_previous,
            ),
            (
                set,
                "Set locator",
                if there {
                    "Remove the locator at the playhead"
                } else {
                    "Add a locator at the playhead"
                },
                Command::SetLocator,
                true,
            ),
            (
                right,
                "Next locator",
                "Next locator",
                Command::NextLocator,
                has_next,
            ),
        ];
        painter_label(ui, rect, "Locators");
        for (i, (r, name, tip, command, enabled)) in buttons.into_iter().enumerate() {
            let response = ui.interact(r, ui.id().with(("locator button", i)), Sense::click());
            let keys = self.keymap.keys(command);
            let response = widgets::tip(response, tip, keys.as_ref(), None);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
            let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
            if hover > 0.0 && enabled {
                ui.painter().rect_filled(
                    r.shrink(2.0),
                    theme::radius::SMALL,
                    palette.control_hover.gamma_multiply(hover),
                );
            }
            let colour = if enabled {
                palette.text_secondary
            } else {
                palette.text_hint.gamma_multiply(0.5)
            };
            match i {
                0 => widgets::draw_arrow(ui.painter(), r.center(), 2.0, colour),
                2 => widgets::draw_arrow(ui.painter(), r.center(), 0.0, colour),
                _ => {
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "Set",
                        FontId::proportional(theme::size::CAPTION),
                        if there { palette.accent } else { colour },
                    );
                }
            }
            if response.clicked() && enabled {
                match i {
                    0 => self.jump_to_locator(false),
                    2 => self.jump_to_locator(true),
                    _ => self.set_locator(),
                }
            }
        }
    }
}

fn painter_label(ui: &egui::Ui, rect: Rect, text: &str) {
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(theme::size::CAPTION),
        theme::p().text_hint,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markers(at: &[i64]) -> Vec<Marker> {
        at.iter()
            .enumerate()
            .map(|(i, q)| Marker {
                id: Id(i as u64 + 1),
                at: Ticks::quarters(*q),
                name: format!("L{i}"),
            })
            .collect()
    }

    #[test]
    fn previous_and_next_skip_the_locator_you_are_on() {
        let m = markers(&[0, 4, 8]);
        assert_eq!(previous(&m, Ticks::quarters(4)), Some(Ticks::ZERO));
        assert_eq!(next(&m, Ticks::quarters(4)), Some(Ticks::quarters(8)));
        assert_eq!(previous(&m, Ticks::quarters(5)), Some(Ticks::quarters(4)));
        assert_eq!(previous(&m, Ticks::ZERO), None);
        assert_eq!(next(&m, Ticks::quarters(8)), None);
        assert_eq!(next(&[], Ticks::ZERO), None);
    }

    #[test]
    fn a_locator_is_found_only_exactly_where_it_is() {
        let m = markers(&[0, 4]);
        assert_eq!(at(&m, Ticks::quarters(4)), Some(Id(2)));
        assert_eq!(at(&m, Ticks(Ticks::quarters(4).0 + 1)), None);
    }
}
