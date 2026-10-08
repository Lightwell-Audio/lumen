//! The editing grid in the app (M3 slice 5a): snapping, Ableton's keys, the
//! grid menu (right-click the lanes, or the grid button in the ruler's
//! corner) and the snap guide. The grid itself is `lumen_core::grid`.

use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, pos2};
use lumen_core::grid::{Feel, Size, Step, Width};
use lumen_core::time::Ticks;

use crate::icons::{self, Icon};
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
    pub(crate) fn grid_step(&self) -> Step {
        let map = &self.transport.timeline.meter;
        let meter = map.meter_at(ruler::ticks_at(self.nav.view.left));
        self.grid.step(meter, self.nav.view.zoom)
    }

    /// What the grid is, for the screen: "1/16", "1/8T", "4 bars".
    pub(crate) fn grid_label(&self) -> String {
        self.grid.label(self.grid_step())
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
            .on_hover_text(
                "Grid and snapping: the magnet is lit while snapping is on (Ctrl+4). \n                 Ctrl+1 to Ctrl+5; right-click the lanes too",
            );
        response.widget_info(|| {
            let snap = if self.grid.snap { "" } else { ", snap off" };
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Grid: {label}{snap}"),
            )
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
        // The size, and a magnet: lit while snapping, struck through when
        // not (MT-APP-13 run 1).
        let size = painter.text(
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
        let (icon, colour) = if self.grid.snap {
            (Icon::Magnet, palette.accent)
        } else {
            (Icon::MagnetOff, palette.text_hint)
        };
        let side = theme::size::CAPTION + 3.0;
        let magnet = Rect::from_center_size(
            pos2(size.left() - 4.0 - side / 2.0, rect.center().y),
            egui::vec2(side, side),
        );
        // The magnet is its own switch (owner, after MT-APP-13 run 1): a
        // click turns snapping on or off; the rest of the button opens the
        // menu, which keeps "Snap to grid" too.
        let target =
            Rect::from_center_size(magnet.center(), egui::vec2(side + 8.0, rect.height() - 4.0));
        let switch = ui
            .interact(target, ui.id().with("snap switch"), Sense::click())
            .on_hover_text(if self.grid.snap {
                "Snapping is on: click to turn it off (Ctrl+4; hold Alt to place freely)"
            } else {
                "Snapping is off: click to turn it on (Ctrl+4)"
            });
        let snap = self.grid.snap;
        switch.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, snap, "Snap to grid")
        });
        let hover = motion::hover(ui.ctx(), switch.id.with("hover"), switch.hovered());
        let painter = ui.painter();
        if hover > 0.0 {
            painter.rect_filled(
                target,
                theme::radius::SMALL,
                palette.control_press.gamma_multiply(hover),
            );
        }
        painter.extend(icons::shapes(icon, magnet, colour));
        if switch.clicked() {
            self.grid.snap = !self.grid.snap;
        }
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
