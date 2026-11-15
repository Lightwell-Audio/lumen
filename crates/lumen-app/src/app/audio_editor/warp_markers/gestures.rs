use std::sync::Arc;

use eframe::egui::{self, CursorIcon, PointerButton, Pos2, Rect, Response, Ui, pos2};

use lumen_media::detect::Analysis;
use lumen_project::edit::warp::{Refused, origin};
use lumen_project::edit::warp_markers::{MarkerEdit, held_at};
use lumen_project::model::{Clip, Id};

use super::span_words;
use super::{AT_LIMIT, OUTSIDE_FILE, stop_id};
use crate::App;
use crate::app::audio_editor::gestures::WaveArea;
use crate::app::audio_editor::range::ticks_of;
use crate::app::info::{Held, Info, Mouse};
use crate::audio_editor::warp_strip::{self, Ghost, WarpStrip};
use crate::audio_editor::{AudioEditor, MarkerDrag, Slide, StripHover, StripMenu, handles};
use crate::commands::Command;
use crate::document::warp::with_marker_edit;
use crate::theme;
use crate::widgets;

impl App {
    fn clip_in(project: &lumen_project::model::Project, id: Id) -> Option<&Clip> {
        project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == id)
            .map(|c| &**c)
    }

    pub(in crate::app::audio_editor) fn warp_strip_pointer(
        &mut self,
        response: &Response,
        clip: &Clip,
        strip: &WarpStrip,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let ctx = response.ctx.clone();
        let left = strip.rect.left();
        let found = ed.detect.found(clip).map(|(_, a, s)| (a, s));
        let ghost_at = |x: f32, ed: &AudioEditor| -> Option<Ghost> {
            warp_strip::ghost(
                area.file,
                (area.rate, ed.time),
                left,
                x,
                found.as_ref().map(|(a, s)| (&**a, *s)),
            )
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(press) = ctx.input(|i| i.pointer.press_origin())
            && strip.rect.contains(press)
            && self.opened_clip_takes_edits(clip.id)
        {
            let shift = ctx.input(|i| i.modifiers.shift);
            let ghost =
                ghost_at(press.x, ed).map(|g| self.ghost_on_the_grid(&ctx, g, (area, left), ed));
            if let Some(drag) =
                self.start_marker_drag((clip, strip), (press, shift), ghost, area, ed)
            {
                ed.markers.picked = Some(drag.index);
                ed.markers.drag = Some(drag);
                ed.markers.held = None;
                self.select_opened_clip(clip.id);
            }
        }
        if let Some(drag) = ed.markers.drag.clone() {
            let down = ctx.input(|i| i.pointer.primary_down());
            if down && let Some(p) = ctx.input(|i| i.pointer.latest_pos()) {
                self.step_marker_drag(&ctx, clip, &drag, p, area, ed);
            }
            if !down || response.drag_stopped() {
                ed.markers.drag = None;
                ed.markers.held = None;
            }
            return;
        }

        let pointer = response.hover_pos();
        ed.markers.hover = pointer.and_then(|p| match strip.hit(p) {
            Some(i) => Some(StripHover::Marker(i)),
            None => ghost_at(p.x, ed)
                .map(|g| self.ghost_on_the_grid(&ctx, g, (area, left), ed))
                .map(StripHover::Ghost),
        });
        if response.double_clicked() && self.opened_clip_takes_edits(clip.id) {
            match ed.markers.hover {
                Some(StripHover::Marker(i)) => self.remove_warp_marker(clip.id, i, ed),
                Some(StripHover::Ghost(g)) => {
                    self.select_opened_clip(clip.id);
                    self.add_warp_marker(clip.id, MarkerEdit::AddOn(g.source), ed);
                }
                None => {}
            }
        } else if response.clicked()
            && let Some(StripHover::Marker(i)) = ed.markers.hover
        {
            ed.markers.picked = Some(i);
            ctx.memory_mut(|m| m.request_focus(stop_id(i)));
        }
        if response.secondary_clicked() {
            ed.markers.menu = match ed.markers.hover {
                Some(StripHover::Marker(i)) => Some(StripMenu::Marker(i)),
                Some(StripHover::Ghost(g)) if g.on_transient => {
                    Some(StripMenu::Hit { source: g.source })
                }
                _ => None,
            };
        }
        let menu = ed.markers.menu;
        let seen = found.as_ref().map(|(a, s)| (&**a, *s));
        widgets::Menu::context(response).show(|ui| self.warp_strip_menu(ui, clip, menu, seen, ed));
        self.warp_hover_info(&ctx, ed);
    }

    fn warp_strip_menu(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        menu: Option<StripMenu>,
        found: Option<(&Analysis, u8)>,
        ed: &mut AudioEditor,
    ) {
        if let Some(StripMenu::Marker(i)) = menu {
            widgets::menu_caption(ui, "Warp marker");
            if widgets::menu_item(ui, "Delete marker", None, true).clicked() {
                self.remove_warp_marker(clip.id, i, ed);
                ui.close();
            }
        }
        if let Some(StripMenu::Hit { source }) = menu
            && let Some(sides) = super::markers::fix_target(clip, source, found)
        {
            let keys = self.keymap.keys(Command::FixWarpHit);
            if widgets::menu_item(ui, "Fix this hit", keys.as_ref(), true).clicked() {
                self.fix_warp_hit(clip.id, source, sides, ed);
                ui.close();
            }
        }
        let many = clip.warp.as_ref().is_some_and(|w| w.markers.len() > 1);
        let keys = self.keymap.keys(Command::ClearWarpMarkers);
        if widgets::menu_item(ui, "Remove all warp markers", keys.as_ref(), many).clicked() {
            self.clear_warp_markers(clip.id, ed);
            ui.close();
        }
    }

    fn warp_hover_info(&mut self, ctx: &egui::Context, ed: &AudioEditor) {
        match ed.markers.hover {
            Some(StripHover::Marker(_)) => {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                self.set_info(
                    Info::new("Warp marker")
                        .on(Held::NONE, Mouse::Drag, "stretch the audio beside it")
                        .on(Held::SHIFT, Mouse::Drag, "slide the audio under it")
                        .on(Held::ALT, Mouse::Drag, "ignore the grid")
                        .on(Held::NONE, Mouse::DoubleClick, "delete")
                        .on(Held::NONE, Mouse::RightClick, "marker menu"),
                );
            }
            Some(StripHover::Ghost(g)) => {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
                let what = if g.on_transient {
                    "Warp marker \u{00B7} on a transient"
                } else {
                    "Warp marker"
                };
                self.set_info(
                    Info::new(what)
                        .on(Held::NONE, Mouse::Drag, "make a marker and move it")
                        .on(Held::NONE, Mouse::DoubleClick, "add a marker here")
                        .key(Command::AddWarpMarker, "add one at the cursor"),
                );
            }
            None => {}
        }
    }

    fn start_marker_drag(
        &mut self,
        (clip, strip): (&Clip, &WarpStrip),
        (press, shift): (Pos2, bool),
        ghost: Option<Ghost>,
        area: &WaveArea<'_>,
        ed: &AudioEditor,
    ) -> Option<MarkerDrag> {
        let (index, base, slide) = match strip.hit(press) {
            Some(i) => {
                let marker = *clip.warp.as_ref()?.markers.get(i)?;
                let slide = shift.then(|| {
                    let beat = ed.time.beat_at(strip.rect.left(), press.x);
                    Slide {
                        source: marker.source,
                        beat,
                        per_beat: seconds_per_beat(area, strip.flags[i], strip.rect.left(), ed),
                    }
                });
                (i, Arc::new(self.document.project().clone()), slide)
            }
            None => {
                let ghost = ghost?;
                let (made, index) = with_marker_edit(
                    self.document.project(),
                    clip.id,
                    MarkerEdit::AddOn(ghost.source),
                )?;
                (index?, Arc::new(made), None)
            }
        };
        let held = Self::clip_in(&base, clip.id)?;
        let at = held.warp.as_ref()?.markers.get(index)?.at;
        let under = ticks_of(ed.time.beat_at(strip.rect.left(), press.x));
        let grab = origin(held) + at - under;
        Some(MarkerDrag {
            index,
            base,
            grab,
            slide,
        })
    }

    fn step_marker_drag(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        drag: &MarkerDrag,
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
        let left = area.body.left();
        let Some(held) = Self::clip_in(&drag.base, clip.id) else {
            return;
        };
        let mut on_line = None;
        let change = match drag.slide {
            Some(slide) => {
                let now = ed.time.beat_at(left, p.x);
                MarkerEdit::Slide {
                    index: drag.index,
                    source: slide.source - (now - slide.beat) * slide.per_beat,
                }
            }
            None => {
                let pointer = ticks_of(ed.time.beat_at(left, p.x));
                let target = if self.snaps_here(ctx) {
                    let line = self.snapped_at_zoom(pointer, false, ed.time.zoom);
                    on_line = Some(line);
                    line
                } else {
                    pointer + drag.grab
                };
                MarkerEdit::Move {
                    index: drag.index,
                    to: target - origin(held),
                }
            }
        };
        match self
            .document
            .edit_warp_marker(&drag.base, clip.id, change, Some("warp marker"))
        {
            Ok(done) => ed.markers.held = done.held.then_some(AT_LIMIT),
            Err(Refused::Unchanged) => {
                if let MarkerEdit::Move { index, to } = change
                    && let Some(now) = self.shown_clip(clip.id)
                {
                    ed.markers.held = held_at(&now, index, to).then_some(AT_LIMIT);
                }
            }
            Err(_) => ed.markers.held = Some(OUTSIDE_FILE),
        }
        ctx.set_cursor_icon(if drag.slide.is_some() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::ResizeHorizontal
        });
        let now = self.shown_clip(clip.id);
        let words = now.as_ref().and_then(|c| span_words(c, drag.index));
        let overlay = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("audio editor warp marker"),
            ))
            .with_clip_rect(area.body.union(Rect::from_min_max(
                pos2(area.body.left(), area.body.top() - theme::size::WARP_STRIP),
                area.body.max,
            )));
        if let Some(line) = on_line {
            let x = ed
                .time
                .x(left, line.0 as f64 / lumen_core::time::PPQ as f64);
            crate::app::grid::draw_snap_guide(&overlay, x, area.body);
        }
        if let Some(words) = words {
            let text = match ed.markers.held {
                Some(_) => format!("{words} \u{00B7} limit"),
                None => words,
            };
            widgets::draw_hint_tone(
                &overlay,
                pos2(p.x + 14.0, p.y + 14.0),
                &text,
                area.body,
                ed.markers.held.is_some(),
            );
        }
        if let Some(why) = ed.markers.held {
            self.set_info(Info::new(format!("Warp marker \u{00B7} {why}")));
        } else if drag.slide.is_some() {
            self.set_info(Info::new("Warp marker").on(
                Held::SHIFT,
                Mouse::Drag,
                "slide the audio under it",
            ));
        }
    }
}

fn seconds_per_beat(area: &WaveArea<'_>, x: f32, left: f32, ed: &AudioEditor) -> f64 {
    let beat = ed.time.beat_at(left, x);
    area.file
        .span_at(beat)
        .map_or(0.5, |s| s.frames_per_beat / f64::from(area.rate.max(1)))
}
