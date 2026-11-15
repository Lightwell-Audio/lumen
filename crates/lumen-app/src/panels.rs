use eframe::egui::{self, Id, Rect, Ui, UiBuilder, pos2, vec2};

use crate::motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Bottom,
}

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
        let name = format!("Resize the {id} panel");
        ctx.accesskit_node_builder(Id::new(id).with("__resize"), |node| {
            node.set_label(name);
        });
        return;
    }
    let full = size.clamp(min, max);
    make(Id::new(id).with("sliding"))
        .resizable(false)
        .exact_size(full * shown)
        .show(ui, |ui| {
            let inner = ui.max_rect();
            let margin = full * shown - extent(inner);
            let content = match side {
                Side::Left => {
                    let width = full - margin;
                    Rect::from_min_size(
                        pos2(inner.right() - width, inner.top()),
                        vec2(width, inner.height()),
                    )
                }
                Side::Bottom => Rect::from_min_size(inner.min, vec2(inner.width(), full - margin)),
            };
            ui.set_clip_rect(inner.intersect(ui.clip_rect()));
            ui.scope_builder(UiBuilder::new().max_rect(content), add_contents);
        });
}
