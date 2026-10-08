//! Editing the loop brace on the locator strip (M3 slice 5c, as in
//! Ableton): drag inside it to move it, drag an edge to resize it, drag in
//! empty space to set a new loop, click it to turn looping on or off, and
//! right-click for punch in and out. Every change snaps to the grid (Alt
//! places freely) and is one undo step per drag.

use eframe::egui::{self, CursorIcon, Rect, Sense, Stroke, pos2};
use lumen_core::time::Ticks;

use super::grid::draw_snap_guide;
use super::rulers::Layout;
use crate::{App, ruler, theme, transport};

/// Within this many points of an edge, a drag resizes.
const EDGE: f32 = 5.0;

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
    /// The loop brace's interaction, on the strip's lower half.
    pub(crate) fn loop_brace(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, at: &Layout) {
        let band =
            Rect::from_x_y_ranges(at.strip.x_range(), at.strip.center().y..=at.strip.bottom());
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
        let grab_at = |x: f32| {
            if (x - a).abs() <= EDGE {
                Grab::Start
            } else if (x - b).abs() <= EDGE {
                Grab::End
            } else if (a..=b).contains(&x) {
                Grab::Move
            } else {
                Grab::New
            }
        };
        let ticks_at = |x: f32| ruler::ticks_at(at.view.beat_at(at.canvas.left(), x));

        if let Some(p) = response.hover_pos()
            && !response.dragged()
        {
            ui.ctx().set_cursor_icon(match grab_at(p.x) {
                Grab::Start | Grab::End => CursorIcon::ResizeHorizontal,
                Grab::Move => CursorIcon::Grab,
                Grab::New => CursorIcon::Crosshair,
            });
        }
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
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
    }
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
