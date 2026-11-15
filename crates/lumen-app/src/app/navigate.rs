use eframe::egui::{self, Rect};
use lumen_core::time::{PPQ, Ticks};

use crate::settings::Follow;
use crate::timeview::{Limits, TimeView};
use crate::{App, motion};

const BACK: usize = 32;
const GESTURE: f64 = 0.5;
const NOTCH: f32 = 40.0;
pub(crate) const KEY_ZOOM: f64 = 2.0;
const PAGE_TURN: f64 = 0.95;
const PAGE_IN: f64 = 0.02;
const CONTINUOUS_AT: f64 = 0.4;
const RULER_ZOOM_PER_POINT: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewStep {
    ZoomIn,
    ZoomOut,
    Back,
    FitWidth,
    FitHeight,
    Reveal(lumen_project::model::Id),
    ShowClip(lumen_project::model::Id),
    Fit(Ticks, Ticks),
}

#[derive(Debug, Default)]
pub struct Nav {
    pub view: TimeView,
    pub(crate) steps: Vec<ViewStep>,
    back: Vec<TimeView>,
    glide: Option<Glide>,
    wheel_zoom: Option<f64>,
    height_wheel: f32,
    was_playing: bool,
    ruler_drag: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    from: TimeView,
    to: TimeView,
    anchor: Option<f32>,
    since: f64,
}

impl Nav {
    pub fn set(&mut self, view: TimeView) {
        self.view = view;
        self.glide = None;
    }

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
    pub(crate) fn drag_ruler(&mut self, response: &egui::Response, canvas: Rect) -> bool {
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.nav.remember();
            self.nav.glide = None;
            self.nav.ruler_drag = Some(self.nav.view.beat_at(canvas.left(), p.x));
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

    pub(crate) fn view_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::Command;
        for (command, label) in [
            (Command::ZoomIn, "Zoom in"),
            (Command::ZoomOut, "Zoom out"),
            (Command::ZoomBack, "Zoom back"),
            (Command::ZoomToSelection, "Zoom to selection"),
            (Command::FitWidth, "Fit song to width"),
            (Command::FitLoop, "Fit loop"),
            (Command::FitHeight, "Fit tracks to height"),
        ] {
            self.menu_command(ui, command, label);
        }
        ui.separator();
        let following = self.transport.follow != Follow::Off && !self.transport.follow_paused;
        let keys = self.keymap.keys(Command::ToggleFollow);
        if crate::widgets::menu_toggle(ui, "Follow playhead", keys.as_ref(), following).clicked() {
            self.transport.toggle_follow();
        }
        let keys = self.keymap.keys(Command::SongLanesShow);
        if crate::widgets::menu_toggle(ui, "Tempo and meter lanes", keys.as_ref(), self.song.shown)
            .clicked()
        {
            self.song_lanes_toggle();
        }
    }

    pub(crate) fn zoom_limits(&self, width: f32) -> Limits {
        self.zoom_limits_at(width, self.nav.view.left)
    }

    pub(crate) fn zoom_limits_at(&self, width: f32, left: f64) -> Limits {
        let state = self.transport.state();
        let rate = state.sample_rate;
        let tempo = &self.transport.timeline.tempo;
        let at = Ticks((left * PPQ as f64) as i64).max(Ticks::ZERO);
        let frames = tempo.sample_at(at + Ticks(PPQ), rate) - tempo.sample_at(at, rate);
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

    pub(crate) fn navigate(&mut self, ui: &egui::Ui, over: &egui::Response, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if self.nav.advance(now) {
            ctx.request_repaint();
        }
        let limits = self.zoom_limits(canvas.width());
        let pointer = ctx.input(|i| i.pointer.hover_pos());

        if over.contains_pointer() {
            let (delta, zoom, alt) =
                ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt));
            if zoom != 1.0 {
                let anchor = pointer.map_or(canvas.center().x, |p| p.x) - canvas.left();
                if self.nav.wheel_zoom.is_none_or(|t| now - t > GESTURE) {
                    self.nav.remember();
                }
                self.nav.wheel_zoom = Some(now);
                let view = self.nav.target().zoomed(anchor, f64::from(zoom), limits);
                self.nav.set(view);
                self.transport.follow_paused = true;
            } else if alt && (delta.y != 0.0 || delta.x != 0.0) {
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

        let steps = std::mem::take(&mut self.nav.steps);
        for step in &steps {
            match *step {
                ViewStep::Reveal(id) => self.arrangement.reveal(
                    &ctx,
                    self.document.project(),
                    self.lanes.pending().len(),
                    id,
                    canvas.height(),
                ),
                ViewStep::ShowClip(id) => {
                    let project = self.document.project();
                    let Some((track, clip)) = project
                        .tracks
                        .iter()
                        .find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t.id, c)))
                    else {
                        continue;
                    };
                    let (from, to) = (clip.start, clip.start + clip.length);
                    self.arrangement.scroll_to_track(
                        &ctx,
                        project,
                        self.lanes.pending().len(),
                        track,
                        canvas.height(),
                    );
                    let to = self
                        .nav
                        .view
                        .showing(from.as_quarters(), to.as_quarters(), limits);
                    if to != self.nav.view {
                        self.nav.glide_to(to, None, now);
                        self.transport.follow_paused = true;
                        ctx.request_repaint();
                    }
                }
                ViewStep::Fit(start, end) => {
                    self.nav.remember();
                    let to = TimeView::fit(
                        start.as_quarters(),
                        end.as_quarters(),
                        canvas.width(),
                        limits,
                    );
                    self.nav.glide_to(to, None, now);
                    self.transport.follow_paused = true;
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
        let ran = |step| steps.contains(&step);
        let (zoom_in, zoom_out, back, fit_width, fit_height) = (
            ran(ViewStep::ZoomIn),
            ran(ViewStep::ZoomOut),
            ran(ViewStep::Back),
            ran(ViewStep::FitWidth),
            ran(ViewStep::FitHeight),
        );
        if zoom_in || zoom_out {
            let target = self.nav.target();
            let head = target.x(canvas.left(), self.transport.playhead().as_quarters());
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
            let room = self
                .arrangement
                .main_height(&ctx, self.document.project(), canvas.height());
            self.arrangement
                .toggle_fit_heights(self.document.project(), room);
        }
        if zoom_in || zoom_out || fit_width || back {
            self.transport.follow_paused = true;
        }
        if zoom_in || zoom_out || fit_width || back || fit_height {
            ctx.request_repaint();
        }
    }

    pub(crate) fn follow(&mut self, canvas: Rect) {
        let state = self.transport.state();
        if state.playing && !self.nav.was_playing {
            self.transport.follow_paused = false;
        }
        self.nav.was_playing = state.playing;
        if !state.playing || self.transport.follow_paused || self.nav.glide.is_some() {
            return;
        }
        let head = self.transport.playhead().as_quarters();
        if let Some(view) = follow_view(
            self.nav.view,
            self.transport.follow,
            head,
            f64::from(canvas.width()),
        ) {
            self.nav.set(view);
        }
    }

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

pub(crate) fn end_of_song(project: &lumen_project::model::Project) -> Option<Ticks> {
    let last = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start + c.length)
        .max()?;
    let looped = &project.transport;
    Some(if looped.loop_enabled {
        last.max(looped.loop_end)
    } else {
        last
    })
}

pub(crate) fn follow_view(
    view: TimeView,
    follow: Follow,
    head: f64,
    width: f64,
) -> Option<TimeView> {
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
    fn the_end_of_the_song_is_the_last_clip_or_a_later_loop_end() {
        use lumen_project::testing::{midi_track, project_of};
        let q = Ticks::quarters;
        assert_eq!(end_of_song(&project_of("empty", Vec::new())), None);
        let mut project = project_of(
            "two",
            vec![
                midi_track(1000, "A", "Poly", (0, 8), None, Vec::new(), Vec::new()),
                midi_track(1010, "B", "Poly", (4, 12), None, Vec::new(), Vec::new()),
            ],
        );
        assert_eq!(end_of_song(&project), Some(q(16)), "the later clip's end");
        project.transport.loop_end = q(32);
        assert_eq!(
            end_of_song(&project),
            Some(q(16)),
            "an idle loop is not the song"
        );
        project.transport.loop_enabled = true;
        assert_eq!(
            end_of_song(&project),
            Some(q(32)),
            "a loop on, ending later"
        );
        project.transport.loop_end = q(8);
        assert_eq!(end_of_song(&project), Some(q(16)), "a loop ending earlier");
    }

    #[test]
    fn page_follow_turns_the_page_near_the_right_edge() {
        assert_eq!(follow_view(VIEW, Follow::Page, 50.0, 1_000.0), None);
        let turned = follow_view(VIEW, Follow::Page, 96.0, 1_000.0).unwrap();
        assert_eq!(turned.left, 94.0, "the playhead lands 2% in");
        assert_eq!(turned.zoom, VIEW.zoom);
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
        assert_eq!(follow_view(VIEW, Follow::Continuous, 10.0, 1_000.0), None);
    }

    #[test]
    fn off_never_moves() {
        assert_eq!(follow_view(VIEW, Follow::Off, 500.0, 1_000.0), None);
    }
}
