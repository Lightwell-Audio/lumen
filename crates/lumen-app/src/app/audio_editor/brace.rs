//! The clip's loop brace under the editor's ruler (M9.5 S2b, the owner's LB A
//! and LS A, D21). A click on the band loops the clip, or stops it looping;
//! the brace's ends set where the loop starts and ends in the file. The file
//! does not move and the clip stays where it is on the arrangement, so every
//! frame from the clip's start to its first repeat stays the same. The edit
//! is `edit::clip_loop` through `Document::set_clip_loop`; a drag is one
//! undo step.

use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Painter, PointerButton, Pos2, Rect, Response, Stroke, pos2};
use lumen_core::time::Ticks;
use lumen_project::edit::{self, clip_loop::LoopEdit};
use lumen_project::model::Clip;

use super::gestures::WaveArea;
use super::range::ticks_of;
use crate::App;
use crate::app::info::{Held, Info, Mouse};
use crate::audio_editor::brace::{Brace, BracePart};
use crate::audio_editor::{AudioEditor, BraceDrag, handles};
use crate::commands::Command;
use crate::widgets::raised;
use crate::{theme, widgets};

/// What the chip says while the loop's start holds at the clip's start (LS A).
const STOPS_AT_START: &str = "The loop starts at the clip's start at the latest";

impl App {
    /// The brace's pointer work: hover, the click that loops or stops, and
    /// a drag of an end.
    pub(super) fn brace_pointer(
        &mut self,
        response: &Response,
        clip: &Clip,
        brace: &Brace,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let ctx = response.ctx.clone();
        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
            && let Some(part @ (BracePart::Start | BracePart::End)) = brace.hit(origin)
            && self.opened_clip_takes_edits(clip.id)
            && let Some(turn) = edit::clip_loop::turn(&self.transport.timeline.tempo, clip)
        {
            let end = match part {
                BracePart::Start => turn.origin,
                _ => turn.end,
            };
            let under = ticks_of(ed.time.beat_at(brace.band.left(), origin.x));
            ed.brace = Some(BraceDrag {
                part,
                base: Arc::new(self.document.project().clone()),
                grab: end - under,
            });
            self.select_opened_clip(clip.id);
        }
        if let Some(drag) = ed.brace.clone() {
            let down = ctx.input(|i| i.pointer.primary_down());
            if down && let Some(p) = ctx.input(|i| i.pointer.latest_pos()) {
                self.step_brace(&ctx, clip, &drag, p, area, ed);
            }
            if !down || response.drag_stopped() {
                ed.brace = None;
                ed.brace_note = None;
            }
            return;
        }
        if response.clicked() {
            self.toggle_clip_loop(clip);
        }
        ed.brace_hover = response.hover_pos().and_then(|p| brace.hit(p));
        if let Some(part) = ed.brace_hover {
            ctx.set_cursor_icon(match part {
                BracePart::Band => CursorIcon::PointingHand,
                _ => CursorIcon::ResizeHorizontal,
            });
            let looping = brace.ends.is_some();
            let info = match part {
                BracePart::Band if looping => Info::new("Clip loop")
                    .on(Held::NONE, Mouse::Click, "stop looping")
                    .key(Command::AudioEditorClipLoop, "stop looping"),
                BracePart::Band => Info::new("Clip loop \u{00B7} off")
                    .on(Held::NONE, Mouse::Click, "loop the clip")
                    .key(Command::AudioEditorClipLoop, "loop the clip"),
                BracePart::Start => Info::new("Loop start")
                    .on(Held::NONE, Mouse::Drag, "where the loop begins in the file")
                    .on(Held::ALT, Mouse::Drag, "ignore the grid"),
                BracePart::End => Info::new("Loop end")
                    .on(Held::NONE, Mouse::Drag, "how long the loop is")
                    .on(Held::ALT, Mouse::Drag, "ignore the grid"),
            };
            self.set_info(info);
        }
    }

    /// The opened clip loops, or stops looping (a click on the band, or the
    /// command). Says why when it cannot.
    pub(crate) fn toggle_clip_loop(&mut self, clip: &Clip) {
        if !self.opened_clip_takes_edits(clip.id) {
            return;
        }
        let looping = clip.loop_length.is_some_and(|p| p > Ticks::ZERO);
        let change = if looping { LoopEdit::Off } else { LoopEdit::On };
        let base = self.document.project().clone();
        if !self.document.set_clip_loop(&base, clip.id, change, None) {
            self.say(
                "This clip can't loop: its file has too little audio left after the clip's start"
                    .to_owned(),
            );
        }
    }

    /// One frame of a brace drag at `p`: the view scrolls at the wave's side
    /// (D29), then the edit is redone from where the drag began.
    fn step_brace(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        drag: &BraceDrag,
        p: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let dt = ctx.input(|i| i.stable_dt);
        let from = ctx.input(|i| i.pointer.press_origin()).map_or(p.x, |o| o.x);
        let pull = handles::edge_scroll(p.x, from, area.body.x_range(), dt);
        let moved = ed.time.scrolled(pull, area.limits);
        if moved != ed.time {
            ed.time = moved;
            self.transport.follow_paused = true;
            ctx.request_repaint();
        }
        let free = ctx.input(|i| i.modifiers.alt);
        let under = ticks_of(ed.time.beat_at(area.body.left(), p.x)) + drag.grab;
        let to = self.snapped_at_zoom(under, free, ed.time.zoom);
        let change = match drag.part {
            BracePart::Start => LoopEdit::Start(to),
            _ => LoopEdit::End(to),
        };
        // The base's clip: the drag redoes its edit from there each frame.
        let tempo = edit::tempo_of(&drag.base);
        let held = drag
            .base
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip.id);
        let limits = held.and_then(|c| edit::clip_loop::loop_limits(&drag.base, &tempo, c));
        ed.brace_note = match (drag.part, limits, held) {
            (BracePart::Start, Some(l), Some(c)) if to > l.start.1 && l.start.1 == c.start => {
                Some(STOPS_AT_START)
            }
            _ => None,
        };
        self.document
            .set_clip_loop(&drag.base, clip.id, change, Some("clip loop"));
        ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        let overlay = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("audio editor brace"),
            ))
            .with_clip_rect(area.body.union(Rect::from_min_max(
                pos2(
                    area.body.left(),
                    area.body.top() - theme::size::EDITOR_RULER,
                ),
                area.body.max,
            )));
        if let Some(note) = ed.brace_note {
            widgets::draw_hint(&overlay, pos2(p.x + 14.0, p.y + 12.0), note, area.body);
            self.set_info(Info::new(note.to_owned()).on(
                Held::NONE,
                Mouse::Drag,
                "trim the clip's start first to loop from later",
            ));
        }
    }

    /// The brace: the thin mark of the clip's window until it loops, then the
    /// loop between its ends, with grips, and the clip start's flag.
    pub(super) fn paint_brace(
        &self,
        painter: &Painter,
        brace: &Brace,
        ed: &AudioEditor,
        focused: Option<(BracePart, &Response)>,
    ) {
        let palette = theme::p();
        let band = brace.band;
        let painter = painter.with_clip_rect(band.intersect(painter.clip_rect()));
        painter.rect_filled(band, 0.0, palette.raised);
        let view = band.x_range();
        let Some((a, b)) = brace.ends else {
            let (a, b) = (brace.window.0.max(view.min), brace.window.1.min(view.max));
            if b > a {
                let y = band.center().y;
                let hot = ed.brace_hover == Some(BracePart::Band);
                let ink = if hot {
                    palette.accent
                } else {
                    palette.accent_muted
                };
                painter.rect_filled(Rect::from_x_y_ranges(a..=b, y - 1.0..=y + 1.0), 1.0, ink);
            }
            if let Some((BracePart::Band, response)) = focused {
                raised::focus_ring(&painter, response, band.shrink(1.0));
            }
            return;
        };
        let (left, right) = (a.max(view.min - 2.0), b.min(view.max + 2.0));
        if right > left {
            let lit = ed.brace.is_some() || ed.brace_hover == Some(BracePart::Band);
            let fill = if lit {
                palette.accent
            } else {
                palette.accent.gamma_multiply(0.75)
            };
            let body = Rect::from_x_y_ranges(left..=right, band.top() + 2.0..=band.bottom() - 2.0);
            painter.rect_filled(body, theme::radius::for_height(body.height()), fill);
        }
        for (part, x) in [(BracePart::Start, a), (BracePart::End, b)] {
            if !view.contains(x) {
                continue;
            }
            let hot =
                ed.brace_hover == Some(part) || ed.brace.as_ref().is_some_and(|d| d.part == part);
            let width = if hot {
                theme::stroke::LIT
            } else {
                theme::stroke::HAIRLINE
            };
            painter.vline(x, band.y_range(), Stroke::new(width, palette.text));
        }
        if let Some(x) = brace.flag.filter(|x| view.contains(*x)) {
            let size = theme::size::BRACE_FLAG;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x, band.top()),
                    pos2(x + size, band.top() + size / 2.0),
                    pos2(x, band.top() + size),
                ],
                palette.text,
                Stroke::NONE,
            ));
            painter.vline(
                x,
                band.y_range(),
                Stroke::new(theme::stroke::HAIRLINE, palette.text),
            );
        }
        if let Some((part, response)) = focused
            && let Some(rect) = brace.end_rect(part)
        {
            raised::focus_ring(&painter, response, rect);
        }
    }
}

/// The ruler's part above the band, and the band.
pub(super) fn split_ruler(ruler: Rect) -> (Rect, Rect) {
    let top = ruler.bottom() - theme::size::BRACE_BAND;
    (
        Rect::from_min_max(ruler.min, pos2(ruler.right(), top)),
        Rect::from_min_max(pos2(ruler.left(), top), ruler.max),
    )
}
