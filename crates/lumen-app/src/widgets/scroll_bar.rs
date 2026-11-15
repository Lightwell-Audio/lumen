use eframe::egui::{self, Id as EguiId, Rect, Sense, Ui, pos2, vec2};

use crate::{motion, theme};

const LINGER: f64 = motion::SCROLL_BAR_LINGER;
const NEAR: f32 = 24.0;
const THICKNESS: f32 = 6.0;
const INSET: f32 = 3.0;
const MIN_HANDLE: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy)]
pub struct ScrollBar {
    pub area: Rect,
    pub axis: Axis,
    pub content: f32,
    pub offset: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    offset: f32,
    active_at: f64,
    held: bool,
}

pub fn scroll_bar(ui: &mut Ui, id: EguiId, bar: ScrollBar) -> Option<f32> {
    let ScrollBar {
        area,
        axis,
        content,
        offset,
    } = bar;
    let along = |r: Rect| match axis {
        Axis::Vertical => r.y_range(),
        Axis::Horizontal => r.x_range(),
    };
    let length = along(area).span();
    if content <= length + 0.5 {
        return None;
    }
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let mut state: State = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let track = match axis {
        Axis::Vertical => Rect::from_min_max(
            pos2(area.right() - INSET - THICKNESS, area.top() + INSET),
            pos2(area.right() - INSET, area.bottom() - INSET),
        ),
        Axis::Horizontal => Rect::from_min_max(
            pos2(area.left() + INSET, area.bottom() - INSET - THICKNESS),
            pos2(area.right() - INSET, area.bottom() - INSET),
        ),
    };
    let run = along(track).span();
    let handle_len = (length / content * run).clamp(MIN_HANDLE.min(run), run);
    let range = (content - length).max(1.0);
    let at = |offset: f32| along(track).min + (offset / range).clamp(0.0, 1.0) * (run - handle_len);
    let handle_rect = |start: f32| match axis {
        Axis::Vertical => {
            Rect::from_min_size(pos2(track.left(), start), vec2(THICKNESS, handle_len))
        }
        Axis::Horizontal => {
            Rect::from_min_size(pos2(start, track.top()), vec2(handle_len, THICKNESS))
        }
    };
    let handle = handle_rect(at(offset));

    let near = ctx
        .pointer_hover_pos()
        .is_some_and(|p| area.contains(p) && track.expand(NEAR).contains(p));
    if (offset - state.offset).abs() > 0.5 || near || state.held {
        state.active_at = now;
    }
    state.offset = offset;
    let on = now - state.active_at < LINGER;
    let time = motion::time(if on { motion::FAST } else { motion::BASE });
    let shown = ctx.animate_bool_with_time(id.with("shown"), on, time);
    if on {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            LINGER - (now - state.active_at) + 0.01,
        ));
    }

    let mut out = None;
    if shown > 0.5 || state.held {
        let response = ui.interact(handle.expand(2.0), id.with("handle"), Sense::drag());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Scroll bar"));
        state.held = response.dragged();
        if state.held {
            let moved = match axis {
                Axis::Vertical => response.drag_delta().y,
                Axis::Horizontal => response.drag_delta().x,
            };
            out = Some((offset + moved * range / (run - handle_len).max(1.0)).clamp(0.0, range));
        }
        if shown > 0.0 {
            let palette = theme::p();
            let lit = response.hovered() || state.held;
            let alpha = shown * if lit { 0.8 } else { 0.55 };
            ui.painter().rect_filled(
                handle,
                THICKNESS / 2.0,
                palette.text_secondary.gamma_multiply(alpha),
            );
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, state));
    out
}
