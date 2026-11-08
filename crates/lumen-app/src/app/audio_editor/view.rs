//! The audio editor's view: how far it zooms and scrolls, Follow, the zoom
//! keys and the wheel, fitting the clip or the file, and the ruler's info
//! line.

use eframe::egui::{self, Pos2, Rect};
use lumen_core::time::Ticks;

use crate::App;
use crate::app::info::{Held, Info, Mouse};
use crate::app::navigate::KEY_ZOOM;
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::{AudioEditor, Fit};
use crate::note_editor::EditorZoom;
use crate::timeview::{Limits, TimeView};

/// Room kept past the file's ends when fitting it (a fraction of its span).
const FIT_ROOM: f64 = 0.02;

impl App {
    /// How far the editor zooms and scrolls: out to the whole file (or the
    /// song, if longer) with room after it, in to the arrangement's deepest.
    pub(super) fn file_limits(&self, file: &WholeFile, width: f32) -> Limits {
        let song = self.zoom_limits_at(width, file.played.first.0);
        let end = (file.to + (file.to - file.from) * 0.1).max(song.end);
        Limits {
            min: (f64::from(width) / end).min(song.max),
            end,
            ..song
        }
    }

    /// The view follows the playhead while playing, as the arrangement's
    /// does (`navigate::follow_view`: Page turns a page at the right edge,
    /// Continuous scrolls under a playhead that stays in place), unless
    /// looking around paused it.
    pub(super) fn audio_follow(&self, ed: &mut AudioEditor, body: Rect) {
        if !self.transport.state().playing || self.transport.follow_paused {
            return;
        }
        let head = self.transport.playhead().as_quarters();
        if let Some(view) = crate::app::navigate::follow_view(
            ed.time,
            self.transport.follow,
            head,
            f64::from(body.width()),
        ) {
            ed.time = view;
        }
    }

    /// What the info line says over the ruler.
    pub(super) fn audio_ruler_info(
        &mut self,
        file: &WholeFile,
        time: TimeView,
        body: Rect,
        p: Pos2,
        rate: u32,
    ) {
        let q = time.beat_at(body.left(), p.x);
        let place = file.frame_at(q).map_or_else(
            || "Outside the file".to_owned(),
            |frame| {
                format!(
                    "File {}",
                    paint::seconds_text(frame / f64::from(rate.max(1)), 0.001)
                )
            },
        );
        self.set_info(
            Info::new(format!("Editor ruler \u{00B7} {place}"))
                .on(Held::NONE, Mouse::Click, "move the playhead here")
                .on(Held::ALT, Mouse::Click, "ignore the grid"),
        );
    }
}

/// The tick at `x` of a view whose left edge is at `origin`.
pub(super) fn ticks_at(time: TimeView, origin: f32, x: f32) -> Ticks {
    Ticks((time.beat_at(origin, x) * lumen_core::time::PPQ as f64).round() as i64)
}

/// The view for `fit` across `width` points.
pub(super) fn fitted(file: &WholeFile, fit: Fit, width: f32, limits: Limits) -> TimeView {
    let (from, to) = match fit {
        Fit::Clip | Fit::Back => file.played.extent(),
        Fit::File => (file.from, file.to),
    };
    let room = (to - from) * FIT_ROOM;
    let span = (to - from + 2.0 * room).max(f64::EPSILON);
    let zoom = (f64::from(width) / span).clamp(limits.min, limits.max);
    TimeView {
        left: (from - room).max(0.0),
        zoom,
    }
}

/// The zoom keys: around the pointer over the wave, else the playhead when
/// it is in view, else the middle.
pub(super) fn audio_zoom_keys(
    ed: &mut AudioEditor,
    body: Rect,
    pointer: Option<Pos2>,
    playhead: f64,
    limits: Limits,
) -> bool {
    if ed.zooms.is_empty() {
        return false;
    }
    let anchor = pointer.filter(|p| body.contains(*p)).map_or_else(
        || {
            let x = ed.time.x(body.left(), playhead);
            if body.x_range().contains(x) {
                x
            } else {
                body.center().x
            }
        },
        |p| p.x,
    );
    for zoom in ed.zooms.drain(..) {
        let factor = match zoom {
            EditorZoom::In => KEY_ZOOM,
            EditorZoom::Out => 1.0 / KEY_ZOOM,
        };
        ed.time = ed.time.zoomed(anchor - body.left(), factor, limits);
    }
    true
}

/// The wheel over the editor: Ctrl+wheel (or a pinch) zooms around the
/// pointer, the wheel or a sideways swipe scrolls time. The view is the
/// editor's own: the arrangement does not move.
pub(super) fn audio_wheel(
    ctx: &egui::Context,
    over: Option<Pos2>,
    body: Rect,
    time: &mut TimeView,
    limits: Limits,
) -> bool {
    let Some(pointer) = over else {
        return false;
    };
    let (delta, zoom) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
    if zoom != 1.0 {
        *time = time.zoomed(pointer.x - body.left(), f64::from(zoom), limits);
        return true;
    }
    let scroll = if delta.x != 0.0 { delta.x } else { delta.y };
    if scroll != 0.0 {
        *time = time.scrolled(scroll, limits);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_editor::source::Played;

    fn file(window: (f64, f64), from: f64, to: f64) -> WholeFile {
        WholeFile {
            spans: Vec::new(),
            from,
            to,
            loop_window: window,
            played: Played {
                first: window,
                second: None,
            },
            heard: window,
        }
    }

    fn limits() -> Limits {
        Limits::new(1_000.0, 1_000.0, 24_000.0)
    }

    #[test]
    fn zoom_to_clip_frames_the_window_and_whole_file_the_file() {
        let f = file((8.0, 12.0), 6.0, 26.0);
        let clip = fitted(&f, Fit::Clip, 1_000.0, limits());
        assert!(clip.x(0.0, 8.0) > 0.0 && clip.x(0.0, 12.0) < 1_000.0);
        assert!(clip.x(0.0, 8.0) < 30.0, "{}", clip.x(0.0, 8.0));
        let whole = fitted(&f, Fit::File, 1_000.0, limits());
        assert!(whole.x(0.0, 6.0) >= 0.0 && whole.x(0.0, 26.0) <= 1_000.0);
        assert!(whole.zoom < clip.zoom);
    }
}
