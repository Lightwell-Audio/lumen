//! The editing grid in the app (M3 slice 5a): snapping, its keys, the
//! grid menu (right-click the lanes, or the grid button in the ruler's
//! corner) and the snap guide. The grid itself is `lumen_core::grid`.

use eframe::egui::{self, Align2, FontId, Rect, Stroke, pos2};
use lumen_core::grid::{Feel, Size, Step, Width};
use lumen_core::time::Ticks;

use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::{App, ruler, theme, widgets};

impl App {
    /// `t` snapped to the grid at the current zoom, unless `free` (Alt held)
    /// or snapping is off.
    pub(crate) fn snapped(&self, t: Ticks, free: bool) -> Ticks {
        self.snapped_at_zoom(t, free, self.nav.view.zoom)
    }

    /// [`Self::snapped`] for a view at `zoom` points a beat: the clip
    /// editor's own zoom (its grid lines are drawn at it, so it snaps there).
    pub(crate) fn snapped_at_zoom(&self, t: Ticks, free: bool, zoom: f64) -> Ticks {
        if free {
            return t;
        }
        self.grid.snap(&self.transport.timeline.meter, t, zoom)
    }

    /// The grid step `t` falls in, at the current zoom (scrubbing).
    pub(crate) fn grid_cell(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid_cell_at_zoom(t, self.nav.view.zoom)
    }

    /// [`Self::grid_cell`] at `zoom` points a beat.
    pub(crate) fn grid_cell_at_zoom(&self, t: Ticks, zoom: f64) -> (Ticks, Ticks) {
        self.grid.cell(&self.transport.timeline.meter, t, zoom)
    }

    /// The step showing now, where the view starts.
    pub(crate) fn grid_step(&self) -> Step {
        self.grid_step_at(self.nav.view.left, self.nav.view.zoom)
    }

    /// The step showing in a view whose left edge is at beat `left` and
    /// that takes `zoom` points a beat.
    pub(crate) fn grid_step_at(&self, left: f64, zoom: f64) -> Step {
        let map = &self.transport.timeline.meter;
        let meter = map.meter_at(ruler::ticks_at(left));
        self.grid.step(meter, zoom)
    }

    /// What the grid is, for the screen: "1/16", "1/8T", "4 bars".
    pub(crate) fn grid_label(&self) -> String {
        self.grid.label(self.grid_step())
    }

    /// [`Self::grid_label`] for a view at `left` and `zoom` (the clip
    /// editor's toolbar shows the grid its roll is drawn with).
    pub(crate) fn grid_label_at(&self, left: f64, zoom: f64) -> String {
        self.grid.label(self.grid_step_at(left, zoom))
    }

    /// The grid menu: adaptive widths, fixed sizes, triplet, dotted, snap.
    pub(crate) fn grid_menu(&mut self, ui: &mut egui::Ui) {
        let keys = |command| self.keymap.keys(command);
        let (narrower, wider, triplet, snap, fixed) = (
            keys(Command::GridNarrower),
            keys(Command::GridWider),
            keys(Command::TripletGrid),
            keys(Command::SnapToGrid),
            self.keymap.text(Command::FixedGrid),
        );
        let grid = &mut self.grid;
        let item = |ui: &mut egui::Ui, on: bool, text: &str, keys: Option<&widgets::Keys>| {
            widgets::menu_toggle(ui, text, keys, on).clicked()
        };
        ui.label(egui::RichText::new("Adaptive grid").weak().small());
        if widgets::menu_toggle(ui, "Narrower", narrower.as_ref(), false).clicked() {
            grid.narrower();
        }
        if widgets::menu_toggle(ui, "Wider", wider.as_ref(), false).clicked() {
            grid.wider();
        }
        for width in Width::all() {
            if item(ui, !grid.fixed && grid.width == width, width.name(), None) {
                grid.fixed = false;
                grid.width = width;
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Fixed grid").weak().small());
        for size in Size::all() {
            if item(ui, grid.fixed && grid.size == size, &size.name(), None) {
                grid.fixed = true;
                grid.size = size;
            }
        }
        ui.separator();
        if item(ui, grid.feel == Feel::Triplet, "Triplet", triplet.as_ref()) {
            grid.toggle_triplet();
        }
        if item(ui, grid.feel == Feel::Dotted, "Dotted", None) {
            grid.feel = if grid.feel == Feel::Dotted {
                Feel::Straight
            } else {
                Feel::Dotted
            };
        }
        if item(ui, grid.snap, "Snap to grid", snap.as_ref()) {
            grid.snap = !grid.snap;
        }
        let fixed = fixed.map_or_else(String::new, |k| format!("; {k} fixed or adaptive"));
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("Hold Alt to place freely{fixed}"))
                    .weak()
                    .small(),
            )
            .wrap(),
        );
    }

    /// How wide the grid's value piece is: its size and the menu's arrow.
    pub(crate) fn grid_value_width(&self, ui: &egui::Ui) -> f32 {
        let text = ui
            .painter()
            .layout_no_wrap(
                self.grid_label(),
                FontId::monospace(theme::size::CAPTION),
                theme::p().text,
            )
            .size()
            .x;
        text + theme::space::XS + widgets::ARROW_SIZE + 2.0 * theme::space::XS
    }

    /// The grid in the ruler corner's well (the interface audit's H1): the
    /// magnet, its own piece, turns snapping on and off; the value piece
    /// opens the grid menu.
    pub(crate) fn grid_pieces(&mut self, ui: &mut egui::Ui, magnet: Rect, value: Rect) {
        let label = self.grid_label();
        let palette = theme::p();
        let keys = self.keymap.keys(Command::SnapToGrid);

        let (switch, ink) = widgets::well::button(
            ui,
            magnet,
            ui.id().with("snap switch"),
            (0.0, theme::Tone::On),
            true,
        );
        let switch = if self.grid.snap {
            widgets::tip(
                switch,
                "Snapping is on: click to turn it off",
                keys.as_ref(),
                Some("Hold Alt to place freely"),
            )
        } else {
            widgets::tip(
                switch,
                "Snapping is off: click to turn it on",
                keys.as_ref(),
                None,
            )
        };
        let snap = self.grid.snap;
        switch.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, snap, "Snap to grid")
        });
        // Lit while snapping, struck through when not (MT-APP-13 run 1).
        let (icon, colour) = if snap {
            (Icon::Magnet, palette.accent)
        } else {
            (Icon::MagnetOff, ink)
        };
        let side = theme::size::CAPTION + 3.0;
        let square = Rect::from_center_size(magnet.center(), egui::vec2(side, side));
        ui.painter().extend(icons::shapes(icon, square, colour));
        if switch.clicked() {
            self.grid.snap = !self.grid.snap;
        }

        let (response, ink) = widgets::well::button(
            ui,
            value,
            ui.id().with("grid button"),
            (0.0, theme::Tone::On),
            true,
        );
        let response = widgets::tip(
            response,
            "Grid and snapping",
            keys.as_ref(),
            Some("Right-click the lanes for the grid too"),
        );
        response.widget_info(|| {
            let snap = if snap { "" } else { ", snap off" };
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Grid: {label}{snap}"),
            )
        });
        let painter = ui.painter().with_clip_rect(value);
        let arrow_x = value.right() - theme::space::XS - widgets::ARROW_SIZE / 2.0;
        painter.text(
            pos2(value.left() + theme::space::XS, value.center().y),
            Align2::LEFT_CENTER,
            &label,
            FontId::monospace(theme::size::CAPTION),
            if snap { ink } else { palette.text_hint },
        );
        widgets::draw_arrow(
            &painter,
            pos2(arrow_x, value.center().y),
            1.0,
            palette.text_hint,
        );
        widgets::Menu::below(&response).show(|ui| self.grid_menu(ui));
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
