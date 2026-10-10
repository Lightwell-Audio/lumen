//! Side and bottom panels that slide open and closed with the design
//! system's motion (`docs/DESIGN.md` §6).
//!
//! egui's own collapsible panels close with the mirror of their opening
//! curve: a slow start and an abrupt stop. Here both directions use the
//! critically damped spring (`motion::glide`): quick to start, soft to land.
//! While sliding, the contents keep their full size and slide out toward the
//! panel's fixed edge, so text does not re-wrap on every frame.

use eframe::egui::{self, Id, Rect, Ui, UiBuilder, pos2, vec2};

use crate::motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Bottom,
}

/// A resizable panel on `side`. `size` is its full size (width or height),
/// kept up to date while it is fully open, and used while it slides. It
/// stays within `range` (the smallest wins when the window is too small for
/// both ends).
pub fn sliding(
    ui: &mut Ui,
    id: &str,
    (side, range): (Side, egui::Rangef),
    open: bool,
    size: &mut f32,
    add_contents: impl FnOnce(&mut Ui),
) {
    let ctx = ui.ctx().clone();
    let shown = motion::glide(
        &ctx,
        Id::new(id).with("slide"),
        if open { 1.0 } else { 0.0 },
        true,
    );
    if shown <= 0.0 {
        return;
    }
    let (min, max) = (range.min, range.max.max(range.min));
    let make = |id: Id| match side {
        Side::Left => egui::Panel::left(id),
        Side::Bottom => egui::Panel::bottom(id),
    };
    let extent = |r: Rect| match side {
        Side::Left => r.width(),
        Side::Bottom => r.height(),
    };
    if shown >= 1.0 {
        // Fully open: the real, resizable panel, whose size egui remembers.
        // When `size` changed from outside since the last frame (a project
        // opened with its saved layout), that size is applied once: egui's
        // memory won before, and saved panel sizes never came back (found
        // in M4 slice 5).
        let given = Id::new(id).with("given");
        let last: Option<f32> = ctx.data(|d| d.get_temp(given));
        let mut panel = make(Id::new(id))
            .resizable(true)
            .default_size(size.clamp(min, max))
            .size_range(min..=max);
        if last.is_some_and(|last| (last - *size).abs() > 0.5) {
            panel = panel.exact_size(size.clamp(min, max));
        }
        let response = panel.show(ui, add_contents);
        *size = extent(response.response.rect);
        ctx.data_mut(|d| d.insert_temp(given, *size));
        // egui draws the resize edge; it gets a name here, for screen
        // readers (slice 8).
        let name = format!("Resize the {id} panel");
        ctx.accesskit_node_builder(Id::new(id).with("__resize"), |node| {
            node.set_label(name);
        });
        return;
    }
    // Sliding: a separate panel id, so the partial sizes are never
    // remembered as the panel's size.
    let full = size.clamp(min, max);
    make(Id::new(id).with("sliding"))
        .resizable(false)
        .exact_size(full * shown)
        .show(ui, |ui| {
            let inner = ui.max_rect();
            let margin = full * shown - extent(inner);
            let content = match side {
                // Anchored at the panel's inner edge, sliding out to the left.
                Side::Left => {
                    let width = full - margin;
                    Rect::from_min_size(
                        pos2(inner.right() - width, inner.top()),
                        vec2(width, inner.height()),
                    )
                }
                // Anchored at the panel's top, sliding down out of the window.
                Side::Bottom => Rect::from_min_size(inner.min, vec2(inner.width(), full - margin)),
            };
            ui.set_clip_rect(inner.intersect(ui.clip_rect()));
            ui.scope_builder(UiBuilder::new().max_rect(content), add_contents);
        });
}
