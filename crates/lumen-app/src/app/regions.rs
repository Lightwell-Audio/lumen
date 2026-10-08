//! Editing the loop brace on the locator strip (M3 slice 5c, as in
//! Ableton): drag inside it to move it, drag an edge to resize it, drag in
//! empty space to set a new loop, click it to turn looping on or off, and
//! right-click for punch in and out. Every change snaps to the grid (Alt
//! places freely) and is one undo step per drag.

use eframe::egui::{self, CursorIcon, Rect, Sense, Stroke, pos2};
use lumen_core::time::Ticks;

use super::grid::draw_snap_guide;
use super::rulers::Layout;
use crate::{App, ruler, theme, transport, widgets};

/// Within this many points of an edge, a drag resizes (at most a third of
/// the brace, so a short loop can still be moved). Widened from 5 after
/// MT-APP-13 run 1: the edges were hard to catch.
const EDGE: f32 = 8.0;

/// What a loop drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grab {
    Move,
    Start,
    End,
    New,
}

/// A loop drag in progress: what was grabbed, where (unsnapped), and the
/// loop as it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LoopDrag {
    pub grab: Grab,
    pub at: Ticks,
    pub start: Ticks,
    pub end: Ticks,
    pub enabled: bool,
}

/// The loop after dragging to `to` (unsnapped), with `snap` applied; `None`
/// when it would be empty or start before zero.
pub(crate) fn dragged(
    drag: &LoopDrag,
    to: Ticks,
    snap: impl Fn(Ticks) -> Ticks,
) -> Option<(Ticks, Ticks)> {
    let (start, end) = match drag.grab {
        Grab::Move => {
            let start = snap(drag.start + (to - drag.at)).max(Ticks::ZERO);
            (start, start + (drag.end - drag.start))
        }
        Grab::Start => (snap(to).max(Ticks::ZERO), drag.end),
        Grab::End => (drag.start, snap(to)),
        Grab::New => {
            let (a, b) = (snap(drag.at).max(Ticks::ZERO), snap(to).max(Ticks::ZERO));
            (a.min(b), a.max(b))
        }
    };
    (start < end).then_some((start, end))
}

impl App {
    /// The loop brace's interaction, on the strip's bottom band. Returns
    /// what a drag holds or would hold, and where the pointer is, for
    /// [`draw_brace_hint`].
    pub(crate) fn loop_brace(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        at: &Layout,
    ) -> Option<(Grab, f32)> {
        let band = Rect::from_x_y_ranges(
            at.strip.x_range(),
            at.strip.bottom() - super::rulers::BRACE_BAND..=at.strip.bottom(),
        );
        let id = ui.id().with("loop brace");
        let response = ui
            .interact(band, id, Sense::click_and_drag())
            .on_hover_text(
                "The loop: drag it to move, drag an edge to resize, drag elsewhere to set a new \
             one, click to turn looping on or off. Right-click for punch in and out",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Loop brace: drag to set the loop, click to turn it on or off",
            )
        });
        let looped = self.document.project().transport;
        let (a, b) = (
            at.view.x(at.canvas.left(), looped.loop_start.as_quarters()),
            at.view.x(at.canvas.left(), looped.loop_end.as_quarters()),
        );
        let edge = EDGE.min((b - a) / 3.0);
        let grab_at = |x: f32| {
            if (x - a).abs() <= edge {
                Grab::Start
            } else if (x - b).abs() <= edge {
                Grab::End
            } else if (a..=b).contains(&x) {
                Grab::Move
            } else {
                Grab::New
            }
        };
        let ticks_at = |x: f32| ruler::ticks_at(at.view.beat_at(at.canvas.left(), x));

        let mut hint = None;
        if let Some(p) = response.hover_pos()
            && !response.dragged()
        {
            let grab = grab_at(p.x);
            ui.ctx().set_cursor_icon(match grab {
                Grab::Start | Grab::End => CursorIcon::ResizeHorizontal,
                Grab::Move => CursorIcon::Grab,
                Grab::New => CursorIcon::Crosshair,
            });
            hint = Some((grab, p.x));
        }
        // What was grabbed is decided where the button went down: egui
        // reports a drag only after the pointer has moved a few points,
        // which could carry it off an edge (MT-APP-13 run 1).
        if response.drag_started()
            && let Some(p) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            let drag = LoopDrag {
                grab: grab_at(p.x),
                at: ticks_at(p.x),
                start: looped.loop_start,
                end: looped.loop_end,
                enabled: looped.loop_enabled,
            };
            ui.data_mut(|d| d.insert_temp(id, drag));
        }
        let drag: Option<LoopDrag> = ui.data(|d| d.get_temp(id));
        if let Some(drag) = drag
            && let Some(p) = response.interact_pointer_pos()
            && response.dragged()
        {
            ui.ctx().set_cursor_icon(match drag.grab {
                Grab::Start | Grab::End => CursorIcon::ResizeHorizontal,
                Grab::Move => CursorIcon::Grabbing,
                Grab::New => CursorIcon::Crosshair,
            });
            hint = Some((drag.grab, p.x));
            let free = ui.input(|i| i.modifiers.alt);
            if let Some((start, end)) = dragged(&drag, ticks_at(p.x), |t| self.snapped(t, free)) {
                let (label, enabled) = match drag.grab {
                    Grab::Move => ("Move loop", drag.enabled),
                    Grab::Start | Grab::End => ("Resize loop", drag.enabled),
                    Grab::New => ("Set loop", true),
                };
                self.document
                    .set_loop(label, Some("loop drag"), start, end, enabled);
                let guides = at.guides();
                let x = |t: Ticks| at.view.x(at.canvas.left(), t.as_quarters());
                match drag.grab {
                    Grab::Start => draw_snap_guide(painter, x(start), guides),
                    Grab::End => draw_snap_guide(painter, x(end), guides),
                    Grab::Move | Grab::New => {
                        draw_snap_guide(painter, x(start), guides);
                        draw_snap_guide(painter, x(end), guides);
                    }
                }
            }
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<LoopDrag>(id));
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && (a..=b).contains(&p.x)
        {
            self.document.set_loop_enabled(!looped.loop_enabled);
        }
        response.context_menu(|ui| transport::punch_menu(ui, &mut self.document));
        hint
    }
}

/// What a drag on the brace will do (MT-APP-13 run 1, the owner's choice
/// A1): a bracket grows at the edge that will resize, or the brace lights
/// when it will move, and a chip names the action.
pub(crate) fn draw_brace_hint(
    painter: &egui::Painter,
    at: &Layout,
    state: &lumen_project::model::TransportState,
    (grab, pointer): (Grab, f32),
) -> &'static str {
    let palette = theme::p();
    let x = |t: Ticks| at.view.x(at.canvas.left(), t.as_quarters());
    let (a, b) = (x(state.loop_start), x(state.loop_end));
    let bottom = at.strip.bottom();
    let top = bottom - super::rulers::BRACE_BAND - 4.0;
    let stroke = Stroke::new(2.0, palette.accent);
    let bracket = |edge: f32, inward: f32| {
        painter.line_segment([pos2(edge, top), pos2(edge, bottom)], stroke);
        painter.line_segment([pos2(edge, top), pos2(edge + inward, top)], stroke);
    };
    let (mark, text) = match grab {
        Grab::Start => {
            bracket(a, 5.0);
            (widgets::HintMark::Resize, "Loop start")
        }
        Grab::End => {
            bracket(b, -5.0);
            (widgets::HintMark::Resize, "Loop end")
        }
        Grab::Move => {
            let brace = Rect::from_x_y_ranges(a..=b, bottom - 7.0..=bottom).intersect(at.strip);
            painter.rect_filled(brace, 0.0, palette.accent.gamma_multiply(0.35));
            (widgets::HintMark::Move, "Move loop · click: on or off")
        }
        Grab::New => (widgets::HintMark::None, "Drag to draw a loop"),
    };
    let row = at.strip.top() + (at.strip.height() - super::rulers::BRACE_BAND) / 2.0;
    widgets::draw_hint(painter, pos2(pointer + 12.0, row), mark, text, at.strip);
    text
}

/// Punch in and out: record-coloured posts at the loop's edges.
pub(crate) fn draw_punch(
    painter: &egui::Painter,
    at: &Layout,
    state: &lumen_project::model::TransportState,
) {
    let colour = theme::p().record;
    for (on, t) in [
        (state.punch_in, state.loop_start),
        (state.punch_out, state.loop_end),
    ] {
        let x = at.view.x(at.canvas.left(), t.as_quarters());
        if on && (at.strip.left()..=at.strip.right()).contains(&x) {
            painter.line_segment(
                [pos2(x, at.strip.top() + 4.0), pos2(x, at.strip.bottom())],
                Stroke::new(2.0, colour),
            );
            painter.circle_filled(pos2(x, at.strip.top() + 4.0), 3.0, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: Ticks = Ticks::quarters(4);

    fn drag(grab: Grab, at: Ticks) -> LoopDrag {
        LoopDrag {
            grab,
            at,
            start: BAR,
            end: Ticks(BAR.0 * 3),
            enabled: false,
        }
    }

    /// Snaps to whole bars.
    fn bars(t: Ticks) -> Ticks {
        Ticks((t.0 as f64 / BAR.0 as f64).round() as i64 * BAR.0)
    }

    #[test]
    fn moving_keeps_the_length_and_snaps_the_start() {
        // Grabbed at bar 2, dropped a bar and a bit later.
        let d = drag(Grab::Move, Ticks(BAR.0 * 2));
        let moved = dragged(&d, Ticks(BAR.0 * 3 + 100), bars);
        assert_eq!(moved, Some((Ticks(BAR.0 * 2), Ticks(BAR.0 * 4))));
        // Not before the start.
        assert_eq!(
            dragged(&d, Ticks(-BAR.0 * 9), bars),
            Some((Ticks::ZERO, Ticks(BAR.0 * 2)))
        );
    }

    #[test]
    fn edges_resize_but_never_past_each_other() {
        let start = drag(Grab::Start, BAR);
        assert_eq!(
            dragged(&start, Ticks(BAR.0 * 2), bars),
            Some((Ticks(BAR.0 * 2), Ticks(BAR.0 * 3)))
        );
        assert_eq!(dragged(&start, Ticks(BAR.0 * 5), bars), None);
        let end = drag(Grab::End, Ticks(BAR.0 * 3));
        assert_eq!(
            dragged(&end, Ticks(BAR.0 * 6), bars),
            Some((BAR, Ticks(BAR.0 * 6)))
        );
        assert_eq!(dragged(&end, BAR, bars), None);
    }

    #[test]
    fn a_new_loop_spans_the_drag_either_way() {
        let new = drag(Grab::New, Ticks(BAR.0 * 8));
        assert_eq!(
            dragged(&new, Ticks(BAR.0 * 5), bars),
            Some((Ticks(BAR.0 * 5), Ticks(BAR.0 * 8)))
        );
        assert_eq!(
            dragged(&new, Ticks(BAR.0 * 8 + 10), bars),
            None,
            "too short"
        );
        // Unsnapped (Alt): exactly where the pointer is.
        assert_eq!(
            dragged(&new, Ticks(BAR.0 * 9 + 7), |t| t),
            Some((Ticks(BAR.0 * 8), Ticks(BAR.0 * 9 + 7)))
        );
    }
}
