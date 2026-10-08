//! The scrub strip (M3 slice 5e, as in Ableton): the strip's upper part,
//! around the locator flags. Click to jump there (snapped; Alt places
//! freely). Drag to hear the song under the pointer: by default the grid
//! step under it loops at normal pitch (Ableton-style); with Alt, the audio
//! follows the hand like tape. While playing, a drag jumps playback from
//! step to step instead.

use eframe::egui::{self, CursorIcon, Rect, Sense};
use lumen_core::time::Ticks;
use lumen_engine::ScrubStyle;

use super::grid::draw_snap_guide;
use super::rulers::{BRACE_BAND, Layout};
use crate::{App, ruler, theme, trace};

/// What a scrub drag last sent.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sent {
    at: Ticks,
    style: ScrubStyle,
}

impl App {
    /// The scrub strip's interaction. Called before the brace and the
    /// flags, so they win where they are.
    pub(crate) fn scrub_zone(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, at: &Layout) {
        let zone = Rect::from_x_y_ranges(
            at.strip.x_range(),
            at.strip.top()..=at.strip.bottom() - BRACE_BAND,
        );
        let id = ui.id().with("scrub");
        let response = ui
            .interact(zone, id, Sense::click_and_drag())
            .on_hover_text(
                "Click to jump here. Drag to hear the beat under the pointer; hold Alt to \
                 scrub like tape",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Scrub strip: click to jump, drag to listen",
            )
        });
        let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) else {
            return;
        };
        let free = ui.input(|i| i.modifiers.alt);
        let raw = ruler::ticks_at(at.view.beat_at(at.canvas.left(), pointer.x)).max(Ticks::ZERO);
        let x = |t: Ticks| at.view.x(at.canvas.left(), t.as_quarters());

        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            let sent: Option<Sent> = ui.data(|d| d.get_temp(id));
            let next = if self.transport.state().playing {
                // Playing: jump from step to step, as a click would.
                let target = self.snapped(raw, free);
                if sent.is_none_or(|s| s.at != target) {
                    self.transport.locate(target);
                }
                Sent {
                    at: target,
                    style: ScrubStyle::Tape,
                }
            } else if free {
                if sent.is_none_or(|s| s.at != raw) {
                    self.transport.scrub(raw, ScrubStyle::Tape);
                }
                Sent {
                    at: raw,
                    style: ScrubStyle::Tape,
                }
            } else {
                let (start, end) = self.grid_cell(raw);
                let style = ScrubStyle::Loop {
                    length: end - start,
                };
                let next = Sent { at: start, style };
                if sent != Some(next) {
                    self.transport.scrub(start, style);
                }
                // The step that plays, lit on the strip.
                painter.rect_filled(
                    Rect::from_x_y_ranges(x(start)..=x(end), zone.y_range()),
                    0.0,
                    theme::p().accent_muted,
                );
                next
            };
            ui.data_mut(|d| d.insert_temp(id, next));
            draw_snap_guide(painter, x(next.at), at.guides());
            return;
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<Sent>(id));
            self.transport.end_scrub();
            return;
        }
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let target = self.snapped(raw, free);
        draw_snap_guide(painter, x(target), at.guides());
        if response.clicked() {
            trace::log(|| format!("scrub strip: locate {target:?}"));
            self.transport.locate(target);
            self.transport.follow_paused = true;
        }
    }
}
