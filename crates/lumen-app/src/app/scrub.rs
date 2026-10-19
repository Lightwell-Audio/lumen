//! The scrub strip (M3 slice 5e): the strip's upper part,
//! around the locator flags. Click to jump there (snapped; Alt places
//! freely). Drag to hear the song under the pointer: by default the grid
//! step under it loops at normal pitch; with Alt, the audio
//! follows the hand like tape. While playing, a drag jumps playback from
//! step to step instead. Right-click adds a locator there.

use eframe::egui::{self, CursorIcon, Rect, Sense, pos2};
use lumen_core::time::Ticks;
use lumen_engine::{MAX_SCRUB_SPEED, ScrubStyle};

use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use super::rulers::{BRACE_BAND, Layout};
use crate::commands::Command;
use crate::{App, ruler, theme, trace, widgets};

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
                 scrub like tape. Right-click to add a locator here",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Scrub strip: click to jump, drag to listen",
            )
        });
        // Right-click: add a locator where it was clicked, leaving the
        // playhead alone (MT-APP-13 run 1).
        let menu_at = id.with("menu at");
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let free = ui.input(|i| i.modifiers.alt);
            let t = ruler::ticks_at(at.view.beat_at(at.canvas.left(), p.x)).max(Ticks::ZERO);
            let t = self.snapped(t, free);
            ui.data_mut(|d| d.insert_temp(menu_at, t));
        }
        widgets::Menu::context(&response).show(|ui| {
            let t: Ticks = ui.data(|d| d.get_temp(menu_at)).unwrap_or_default();
            if ui.button("Add locator here").clicked() {
                let id = self.document.add_marker(t, None);
                self.locators.selected = Some(id);
                self.arrangement.selected.clear();
                self.arrangement.clips.clear();
                self.arrangement.range = None;
                ui.close();
            }
        });
        let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) else {
            return;
        };
        let free = ui.input(|i| i.modifiers.alt);
        let raw = ruler::ticks_at(at.view.beat_at(at.canvas.left(), pointer.x)).max(Ticks::ZERO);
        let x = |t: Ticks| at.view.x(at.canvas.left(), t.as_quarters());

        let palette = theme::p();
        let row = zone.center().y;
        let playing = self.transport.state().playing;

        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            let sent: Option<Sent> = ui.data(|d| d.get_temp(id));
            let next = if playing {
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
                // The tape's speed, as the hand sets it.
                let speed = self.tape_speed(ui, id, raw);
                let text = format!("tape {speed:.1}×");
                widgets::draw_caption(
                    painter,
                    pos2(pointer.x + 12.0, row),
                    &text,
                    palette.accent,
                    at.strip,
                );
                self.hint = Some(text);
                ui.ctx().request_repaint();
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
                    palette.accent_muted,
                );
                next
            };
            ui.data_mut(|d| d.insert_temp(id, next));
            draw_snap_guide(painter, x(next.at), at.guides());
            return;
        }
        if response.drag_stopped() {
            ui.data_mut(|d| {
                d.remove::<Sent>(id);
                d.remove::<Speed>(id);
            });
            self.transport.end_scrub();
            return;
        }
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let target = self.snapped(raw, free);
        // What a drag would do (MT-APP-13 run 1, the owner's choice B2):
        // the step it would play, washed; the words are in the info line
        // (M4 slice 0).
        let info = Info::new("Scrub strip").on(Held::NONE, Mouse::Click, "jump");
        let info = if playing {
            info.on(Held::NONE, Mouse::Drag, "jump along")
        } else {
            if !free {
                let (start, end) = self.grid_cell(raw);
                painter.rect_filled(
                    Rect::from_x_y_ranges(x(start)..=x(end), zone.y_range()),
                    0.0,
                    palette.accent_muted.gamma_multiply(0.6),
                );
            }
            info.on(Held::NONE, Mouse::Drag, "listen to each step").on(
                Held::ALT,
                Mouse::Drag,
                "scrub like tape",
            )
        };
        self.set_info(
            info.on(Held::NONE, Mouse::RightClick, "add a locator")
                .key(Command::SetLocator, "locator at the playhead"),
        );
        draw_snap_guide(painter, x(target), at.guides());
        if response.clicked() {
            trace::log(|| format!("scrub strip: locate {target:?}"));
            self.transport.locate(target);
            self.transport.follow_paused = true;
        }
    }

    /// How fast a tape scrub plays, times normal: the song time the hand
    /// covers per second, smoothed, within the engine's limit.
    fn tape_speed(&self, ui: &egui::Ui, id: egui::Id, at: Ticks) -> f64 {
        let rate = self.transport.state().sample_rate.max(1);
        let song = self.transport.timeline.sample_at(at, rate) as f64 / f64::from(rate);
        let now = ui.input(|i| i.time);
        let last: Option<Speed> = ui.data(|d| d.get_temp(id));
        let speed = match last {
            Some(last) if now > last.time => {
                let instant = (song - last.song) / (now - last.time);
                last.speed + (instant - last.speed) * 0.3
            }
            Some(last) => last.speed,
            None => 0.0,
        }
        .clamp(-MAX_SCRUB_SPEED, MAX_SCRUB_SPEED);
        ui.data_mut(|d| {
            d.insert_temp(
                id,
                Speed {
                    song,
                    time: now,
                    speed,
                },
            )
        });
        speed
    }
}

/// A tape scrub's speed, carried from frame to frame.
#[derive(Debug, Clone, Copy)]
struct Speed {
    /// Where the hand was, in seconds of song, and when.
    song: f64,
    time: f64,
    speed: f64,
}
