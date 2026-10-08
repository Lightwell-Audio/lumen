//! Moving around the arrangement (M3 slice 4d): scrolling; zooming with
//! the wheel, a pinch or the keys; fitting; stepping back; and Alt+wheel
//! track heights. Keys as in Ableton: `+`/`-` zoom, X back, W fit width,
//! H fit height. The view itself is `crate::timeview`.

use eframe::egui::{self, Key, Modifiers, Rect};
use lumen_core::time::{PPQ, Ticks};

use crate::settings::Follow;
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
/// Page follow: the page turns when the playhead passes this fraction of
/// the width, and the playhead lands this far in.
const PAGE_TURN: f64 = 0.95;
const PAGE_IN: f64 = 0.02;
/// Continuous follow: where the playhead stays.
const CONTINUOUS_AT: f64 = 0.4;
/// Dragging the ruler 100 points down zooms in by e (about 2.7 times).
const RULER_ZOOM_PER_POINT: f64 = 0.01;

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
    /// Playing at the last frame (Follow resumes when playback starts).
    was_playing: bool,
    /// The beat grabbed by a ruler drag in progress.
    ruler_drag: Option<f64>,
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
    /// Dragging the bar ruler, as in Ableton: down zooms in, up zooms out,
    /// sideways scrolls, and the grabbed beat stays under the pointer. True
    /// while a drag is moving the view.
    pub(crate) fn drag_ruler(&mut self, response: &egui::Response, canvas: Rect) -> bool {
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.nav.remember();
            self.nav.glide = None;
            self.nav.ruler_drag = Some(self.nav.view.beat_at(canvas.left(), p.x));
            // Looking around pauses Follow.
            self.transport.follow_paused = true;
        }
        if response.drag_stopped() {
            self.nav.ruler_drag = None;
        }
        let (Some(beat), Some(p)) = (self.nav.ruler_drag, response.interact_pointer_pos()) else {
            return false;
        };
        let limits = self.zoom_limits(canvas.width());
        let zoom =
            self.nav.view.zoom * (f64::from(response.drag_delta().y) * RULER_ZOOM_PER_POINT).exp();
        self.nav
            .set(TimeView::holding(beat, p.x - canvas.left(), zoom, limits));
        true
    }

    /// How far the view may zoom and scroll: out to the song (as in
    /// Ableton), in to single samples at the tempo where the view is.
    fn zoom_limits(&self, width: f32) -> Limits {
        let state = self.transport.state();
        let rate = state.sample_rate;
        let tempo = &self.transport.timeline.tempo;
        let at = Ticks((self.nav.view.left * PPQ as f64) as i64).max(Ticks::ZERO);
        let frames = tempo.sample_at(at + Ticks(PPQ), rate) - tempo.sample_at(at, rate);
        // The furthest beat in use: clips, the playhead, the start marker
        // and the loop.
        let looped = self.transport.loop_region;
        let song = [
            self.song_end(),
            state.ticks.as_quarters(),
            state.marker.as_quarters(),
            if looped.enabled {
                looped.end.as_quarters()
            } else {
                0.0
            },
        ]
        .into_iter()
        .fold(0.0, f64::max);
        Limits::new(song, width, frames as f64)
    }

    /// Wheel, pinch and keys over the arrangement (`over` is where they
    /// count: the headers and the lanes; `canvas`, the lanes).
    pub(crate) fn navigate(&mut self, ui: &egui::Ui, over: &egui::Response, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if self.nav.advance(now) {
            ctx.request_repaint();
        }
        let limits = self.zoom_limits(canvas.width());
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
                self.transport.follow_paused = true;
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
                    let view = self.nav.target().scrolled(delta.x, limits);
                    self.nav.set(view);
                    self.transport.follow_paused = true;
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
        if zoom_in || zoom_out || fit_width || back {
            // Looking around pauses Follow, as in Ableton.
            self.transport.follow_paused = true;
        }
        if zoom_in || zoom_out || fit_width || back || fit_height {
            ctx.request_repaint();
        }
    }

    /// Keeps the playhead in view while playing (Follow: page or
    /// continuous). Resumes when playback starts.
    pub(crate) fn follow(&mut self, canvas: Rect) {
        let state = self.transport.state();
        if state.playing && !self.nav.was_playing {
            self.transport.follow_paused = false;
        }
        self.nav.was_playing = state.playing;
        if !state.playing || self.transport.follow_paused || self.nav.glide.is_some() {
            return;
        }
        let head = state.ticks.as_quarters();
        if let Some(view) = follow_view(
            self.nav.view,
            self.transport.follow,
            head,
            f64::from(canvas.width()),
        ) {
            self.nav.set(view);
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

/// Where `follow` puts a view showing `width` points so the playhead at
/// `head` stays in sight; `None` when it need not move.
fn follow_view(view: TimeView, follow: Follow, head: f64, width: f64) -> Option<TimeView> {
    let span = width / view.zoom;
    let left = match follow {
        Follow::Off => return None,
        Follow::Page => {
            let at = (head - view.left) / span;
            if (0.0..PAGE_TURN).contains(&at) {
                return None;
            }
            head - span * PAGE_IN
        }
        Follow::Continuous => head - span * CONTINUOUS_AT,
    };
    let left = left.max(0.0);
    (left != view.left).then_some(TimeView { left, ..view })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: TimeView = TimeView {
        left: 0.0,
        zoom: 10.0,
    };

    #[test]
    fn page_follow_turns_the_page_near_the_right_edge() {
        // 1,000 points at 10 per beat: 100 beats in view.
        assert_eq!(follow_view(VIEW, Follow::Page, 50.0, 1_000.0), None);
        let turned = follow_view(VIEW, Follow::Page, 96.0, 1_000.0).unwrap();
        assert_eq!(turned.left, 94.0, "the playhead lands 2% in");
        assert_eq!(turned.zoom, VIEW.zoom);
        // A playhead behind the view (after a loop wrap) brings it back.
        let behind = TimeView {
            left: 200.0,
            ..VIEW
        };
        assert_eq!(
            follow_view(behind, Follow::Page, 10.0, 1_000.0)
                .unwrap()
                .left,
            8.0
        );
    }

    #[test]
    fn continuous_follow_keeps_the_playhead_in_place() {
        let view = follow_view(VIEW, Follow::Continuous, 140.0, 1_000.0).unwrap();
        assert_eq!(view.left, 100.0, "40% in");
        // Near the start the view stays at the start.
        assert_eq!(follow_view(VIEW, Follow::Continuous, 10.0, 1_000.0), None);
    }

    #[test]
    fn off_never_moves() {
        assert_eq!(follow_view(VIEW, Follow::Off, 500.0, 1_000.0), None);
    }
}
