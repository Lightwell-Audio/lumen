//! Moving around the arrangement (M3 slice 4d): scrolling; zooming with
//! the wheel, a pinch or the keys; fitting; stepping back; and Alt+wheel
//! track heights. Keys as in Ableton: `+`/`-` zoom, X back, W fit width,
//! H fit height. The view itself is `crate::timeview`.

use eframe::egui::{self, Key, Modifiers, Rect};
use lumen_core::time::{PPQ, Ticks};

use crate::timeview::{Limits, TimeView};
use crate::{App, motion};

/// Earlier views kept for X.
const BACK: usize = 32;
/// Wheel zooms closer together than this (seconds) are one step for X.
const GESTURE: f64 = 0.5;
/// Alt+wheel: this much scroll moves a track one height step.
const NOTCH: f32 = 40.0;
/// The keys zoom by this factor.
const KEY_ZOOM: f64 = 2.0;

/// Where the arrangement looks, how it got there, and where it was.
#[derive(Debug, Default)]
pub struct Nav {
    pub view: TimeView,
    back: Vec<TimeView>,
    glide: Option<Glide>,
    /// When the last wheel zoom happened (egui time).
    wheel_zoom: Option<f64>,
    /// Alt+wheel scroll not yet turned into a height step.
    height_wheel: f32,
}

/// A view on its way to another (key zooms, fits, X).
#[derive(Debug, Clone, Copy)]
struct Glide {
    from: TimeView,
    to: TimeView,
    anchor: Option<f32>,
    since: f64,
}

impl Nav {
    /// Jumps to `view` (opening a project, the benchmark).
    pub fn set(&mut self, view: TimeView) {
        self.view = view;
        self.glide = None;
    }

    /// Where the view is going: further steps build on it.
    fn target(&self) -> TimeView {
        self.glide.map_or(self.view, |g| g.to)
    }

    fn remember(&mut self) {
        let here = self.target();
        if self.back.last() != Some(&here) {
            self.back.push(here);
            if self.back.len() > BACK {
                self.back.remove(0);
            }
        }
    }

    fn glide_to(&mut self, to: TimeView, anchor: Option<f32>, now: f64) {
        if motion::time(motion::GLIDE) == 0.0 {
            self.set(to);
        } else {
            self.glide = Some(Glide {
                from: self.view,
                to,
                anchor,
                since: now,
            });
        }
    }

    /// Moves a glide on; true while one runs.
    fn advance(&mut self, now: f64) -> bool {
        let Some(glide) = self.glide else {
            return false;
        };
        let duration = f64::from(motion::time(motion::GLIDE)).max(1e-6);
        let t = ((now - glide.since) / duration).min(1.0);
        let eased = f64::from(motion::ease_out(t as f32));
        self.view = glide.from.between(glide.to, eased, glide.anchor);
        if t >= 1.0 {
            self.set(glide.to);
            return false;
        }
        true
    }
}

impl App {
    /// How far the view may zoom: in to single samples at the tempo where
    /// the view is.
    fn zoom_limits(&self) -> Limits {
        let rate = self.transport.state().sample_rate;
        let tempo = &self.transport.timeline.tempo;
        let at = Ticks((self.nav.view.left * PPQ as f64) as i64).max(Ticks::ZERO);
        let frames = tempo.sample_at(at + Ticks(PPQ), rate) - tempo.sample_at(at, rate);
        Limits::for_audio(frames as f64)
    }

    /// Wheel, pinch and keys over the arrangement (`over` is where they
    /// count: the headers and the lanes; `canvas`, the lanes).
    pub(crate) fn navigate(&mut self, ui: &egui::Ui, over: &egui::Response, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if self.nav.advance(now) {
            ctx.request_repaint();
        }
        let limits = self.zoom_limits();
        let pointer = ctx.input(|i| i.pointer.hover_pos());

        // `contains_pointer`, not `hovered`: the ruler's own click area sits
        // on top, and scrolling and zooming must work over it too. The wheel
        // scrolls the tracks; Shift+wheel or a sideways swipe scrolls the
        // timeline; Ctrl+wheel (or a pinch) zooms; Alt+wheel sizes the track
        // under the pointer.
        if over.contains_pointer() {
            // egui turns Ctrl+wheel (and a pinch) into a zoom factor and
            // reports no scroll for it, so zoom must read `zoom_delta`.
            let (delta, zoom, alt) =
                ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt));
            if zoom != 1.0 {
                let anchor = pointer.map_or(canvas.center().x, |p| p.x) - canvas.left();
                // One wheel gesture is one step back for X.
                if self.nav.wheel_zoom.is_none_or(|t| now - t > GESTURE) {
                    self.nav.remember();
                }
                self.nav.wheel_zoom = Some(now);
                let view = self.nav.target().zoomed(anchor, f64::from(zoom), limits);
                self.nav.set(view);
            } else if alt && (delta.y != 0.0 || delta.x != 0.0) {
                // Alt can turn the wheel sideways on some systems.
                let wheel = if delta.y != 0.0 { delta.y } else { delta.x };
                self.nav.height_wheel += wheel;
                let steps = (self.nav.height_wheel / NOTCH).trunc();
                if steps != 0.0
                    && let Some(p) = pointer
                    && let Some(id) = self.arrangement.track_at(
                        &ctx,
                        self.document.project(),
                        self.lanes.pending().len(),
                        p.y - canvas.top(),
                    )
                {
                    self.nav.height_wheel -= steps * NOTCH;
                    self.arrangement.step_height(id, steps as i32);
                }
            } else {
                if delta.x != 0.0 {
                    let view = self.nav.target().scrolled(delta.x);
                    self.nav.set(view);
                }
                if delta.y != 0.0 {
                    let content = self.arrangement.height_of_content(
                        &ctx,
                        self.document.project(),
                        self.lanes.pending().len(),
                    );
                    self.arrangement.scroll(delta.y, content, canvas.height());
                }
            }
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (zoom_in, zoom_out, back, fit_width, fit_height) = ctx.input_mut(|i| {
            let plus = i.consume_key(Modifiers::NONE, Key::Plus)
                || i.consume_key(Modifiers::NONE, Key::Equals)
                || i.consume_key(Modifiers::SHIFT, Key::Equals);
            (
                plus,
                i.consume_key(Modifiers::NONE, Key::Minus),
                i.consume_key(Modifiers::NONE, Key::X),
                i.consume_key(Modifiers::NONE, Key::W),
                i.consume_key(Modifiers::NONE, Key::H),
            )
        });
        if zoom_in || zoom_out {
            // Around the pointer when it is over the lanes, else the
            // playhead when it is in view, else the middle.
            let target = self.nav.target();
            let head = target.x(canvas.left(), self.transport.state().ticks.as_quarters());
            let anchor = pointer
                .filter(|p| canvas.contains(*p))
                .map(|p| p.x)
                .or_else(|| canvas.x_range().contains(head).then_some(head))
                .unwrap_or(canvas.center().x)
                - canvas.left();
            let factor = if zoom_in { KEY_ZOOM } else { 1.0 / KEY_ZOOM };
            self.nav.remember();
            let to = target.zoomed(anchor, factor, limits);
            self.nav.glide_to(to, Some(anchor), now);
        }
        if fit_width {
            let end = self.song_end();
            self.nav.remember();
            let to = TimeView::fit(0.0, end, canvas.width(), limits);
            self.nav.glide_to(to, None, now);
        }
        if back && let Some(previous) = self.nav.back.pop() {
            self.nav.glide_to(previous, None, now);
        }
        if fit_height {
            self.arrangement
                .toggle_fit_heights(self.document.project(), canvas.height());
        }
        if zoom_in || zoom_out || fit_width || back || fit_height {
            ctx.request_repaint();
        }
    }

    /// The end of the last clip in beats (at least four bars), for W.
    fn song_end(&self) -> f64 {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| (c.start + c.length).as_quarters())
            .fold(16.0, f64::max)
    }
}
