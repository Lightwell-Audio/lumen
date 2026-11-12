//! The palette's window, when open: the search field, the rows, the strip
//! that explains the chosen one, and the keys.

use eframe::egui::{self, Align2, FontId, Key, Modifiers, Sense, Stroke, Vec2, vec2};

use super::{Entry, TOP, WIDTH};
use crate::{App, icons, theme, widgets};

impl App {
    /// The palette, when open: the search field, the rows, the strip that
    /// explains the chosen one, and the keys. Esc, Ctrl+K or a click
    /// elsewhere closes it.
    pub(crate) fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        // Templates are rows too: a template added in the file manager shows
        // up within a moment of the palette opening.
        self.templates.listing.ask(ctx, self.templates_dir(), false);
        // The list's keys, before the field sees them.
        // Ctrl+Enter records new keys for the chosen command (the owner's
        // E3); while recording, `capture_chord` has taken the keys.
        let (rekey, up, down, enter, close) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape)
                    || i.consume_key(Modifiers::COMMAND, Key::K),
            )
        });
        if close {
            self.close_palette();
            return;
        }
        let rows = self.palette_rows(ctx);
        let last = rows.len().saturating_sub(1);
        if up {
            self.palette.chosen = self.palette.chosen.checked_sub(1).unwrap_or(last);
        }
        if down {
            self.palette.chosen = if self.palette.chosen >= last {
                0
            } else {
                self.palette.chosen + 1
            };
        }
        self.palette.chosen = self.palette.chosen.min(last);
        if up || down {
            self.palette.notice = None;
        }
        if rekey
            && let Some(Entry::Command(command)) = rows.get(self.palette.chosen).map(|r| &r.entry)
        {
            let slot = (!self.keymap.chords(*command).is_empty()).then_some(0);
            self.start_rebind(*command, slot);
        }
        if enter && let Some(row) = rows.get(self.palette.chosen) {
            self.run_row(ctx, row);
            return;
        }

        let palette = *theme::p();
        // The click that opened it (the transport button) is not a click
        // elsewhere.
        let fresh = self.palette.focus;
        let mut picked = None;
        let shown = egui::Area::new(egui::Id::new("command palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, TOP))
            .show(ctx, |ui| {
                // A window-sized floating surface (the Palette card):
                // `radius::XL`, `overlay`, `border_strong` and the edge
                // light along its top inside edge.
                let frame = egui::Frame::popup(ui.style())
                    .fill(palette.overlay)
                    .stroke(Stroke::new(theme::stroke::HAIRLINE, palette.border_strong))
                    .corner_radius(theme::radius::XL)
                    .inner_margin(theme::space::S)
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        self.palette_field(ui);
                        ui.separator();
                        picked = self.palette_list(ui, &rows, up || down);
                        ui.separator();
                        self.palette_strip(ui, rows.get(self.palette.chosen));
                    });
                widgets::raised::lip(
                    ui.painter(),
                    frame.response.rect,
                    theme::radius::XL,
                    palette.edge_light,
                );
            });
        if let Some(i) = picked
            && let Some(row) = rows.get(i)
        {
            self.run_row(ctx, row);
        } else if !fresh && shown.response.clicked_elsewhere() {
            self.close_palette();
        }
    }

    fn palette_field(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::splat(theme::size::CONTROL_SMALL), Sense::hover());
            ui.painter().extend(icons::shapes(
                icons::Icon::Search,
                rect,
                theme::p().text_hint,
            ));
            let before = self.palette.query.clone();
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.palette.query)
                    .id(egui::Id::new("command palette field"))
                    .frame(egui::Frame::NONE)
                    .font(FontId::proportional(theme::size::TITLE))
                    .hint_text("Search commands, or type a tempo, a bar or a track")
                    .desired_width(f32::INFINITY),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search commands")
            });
            if std::mem::take(&mut self.palette.focus) || !field.has_focus() {
                field.request_focus();
            }
            if self.palette.query != before {
                self.palette.chosen = 0;
                self.palette.notice = None;
            }
        });
    }
}
