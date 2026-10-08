//! The editing grid in the app (M3 slice 5a): snapping, Ableton's keys, the
//! grid menu (right-click the lanes, or the grid button in the ruler's
//! corner) and the snap guide. The grid itself is `lumen_core::grid`.

use eframe::egui::{self, Align2, FontId, Key, Modifiers, Rect, Sense, Stroke, pos2};
use lumen_core::grid::{Feel, Size, Step, Width};
use lumen_core::time::Ticks;

use crate::{App, motion, ruler, theme};

impl App {
    /// `t` snapped to the grid at the current zoom, unless `free` (Alt held)
    /// or snapping is off.
    pub(crate) fn snapped(&self, t: Ticks, free: bool) -> Ticks {
        if free {
            return t;
        }
        self.grid
            .snap(&self.transport.timeline.meter, t, self.nav.view.zoom)
    }

    /// The grid step `t` falls in, at the current zoom (scrubbing).
    pub(crate) fn grid_cell(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid
            .cell(&self.transport.timeline.meter, t, self.nav.view.zoom)
    }

    /// The step showing now, where the view starts.
    fn grid_step(&self) -> Step {
        let map = &self.transport.timeline.meter;
        let meter = map.meter_at(ruler::ticks_at(self.nav.view.left));
        self.grid.step(meter, self.nav.view.zoom)
    }

    /// What the grid is, for the screen: "1/16", "1/8T", "4 bars", "Off".
    pub(crate) fn grid_label(&self) -> String {
        self.grid.label(self.grid_step())
    }

    /// Ableton's grid keys: Ctrl+1 narrower, Ctrl+2 wider, Ctrl+3 triplet,
    /// Ctrl+4 snap on or off, Ctrl+5 fixed or adaptive.
    pub(crate) fn grid_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key));
        let step = self.grid_step();
        if pressed(Key::Num1) {
            self.grid.narrower();
        } else if pressed(Key::Num2) {
            self.grid.wider();
        } else if pressed(Key::Num3) {
            self.grid.toggle_triplet();
        } else if pressed(Key::Num4) {
            self.grid.snap = !self.grid.snap;
        } else if pressed(Key::Num5) {
            self.grid.toggle_fixed(step);
        } else {
            return;
        }
        ctx.request_repaint();
    }

    /// The grid menu: adaptive widths, fixed sizes, triplet, dotted, snap.
    pub(crate) fn grid_menu(&mut self, ui: &mut egui::Ui) {
        let grid = &mut self.grid;
        let item = |ui: &mut egui::Ui, on: bool, text: String, keys: &str| {
            let button = egui::Button::selectable(on, text);
            let button = if keys.is_empty() {
                button
            } else {
                button.shortcut_text(keys)
            };
            ui.add(button).clicked()
        };
        ui.label(egui::RichText::new("Adaptive grid").weak().small());
        for width in Width::all() {
            let keys = match width {
                Width::Narrowest => "Ctrl+1 narrower",
                Width::Widest => "Ctrl+2 wider",
                _ => "",
            };
            if item(
                ui,
                !grid.fixed && grid.width == width,
                width.name().into(),
                keys,
            ) {
                grid.fixed = false;
                grid.width = width;
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Fixed grid").weak().small());
        for size in Size::all() {
            if item(ui, grid.fixed && grid.size == size, size.name(), "") {
                grid.fixed = true;
                grid.size = size;
            }
        }
        ui.separator();
        if item(ui, grid.feel == Feel::Triplet, "Triplet".into(), "Ctrl+3") {
            grid.toggle_triplet();
        }
        if item(ui, grid.feel == Feel::Dotted, "Dotted".into(), "") {
            grid.feel = if grid.feel == Feel::Dotted {
                Feel::Straight
            } else {
                Feel::Dotted
            };
        }
        if item(ui, grid.snap, "Snap to grid".into(), "Ctrl+4") {
            grid.snap = !grid.snap;
        }
        ui.label(
            egui::RichText::new("Hold Alt to place freely; Ctrl+5 fixed or adaptive")
                .weak()
                .small(),
        );
    }

    /// The grid button in the ruler's corner: what the grid is now, and the
    /// grid menu on click.
    pub(crate) fn grid_button(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let label = self.grid_label();
        let response = ui
            .interact(rect, ui.id().with("grid button"), Sense::click())
            .on_hover_text("Grid and snapping (Ctrl+1 to Ctrl+5; right-click the lanes too)");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Grid: {label}"))
        });
        let palette = theme::p();
        let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect.shrink(2.0),
                theme::radius::SMALL,
                palette.control_hover.gamma_multiply(hover),
            );
        }
        let painter = ui.painter();
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            "Grid",
            FontId::proportional(theme::size::CAPTION),
            palette.text_hint,
        );
        painter.text(
            pos2(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            &label,
            FontId::monospace(theme::size::CAPTION),
            if self.grid.snap {
                palette.text_secondary
            } else {
                palette.text_hint
            },
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.grid_menu(ui));
    }
}

/// The snap guide: where a click or a drop will land, across `area`.
pub(crate) fn draw_snap_guide(painter: &egui::Painter, x: f32, area: Rect) {
    if (area.left()..=area.right()).contains(&x) {
        painter.line_segment(
            [pos2(x, area.top()), pos2(x, area.bottom())],
            Stroke::new(1.0, theme::p().accent.gamma_multiply(0.5)),
        );
    }
}
