//! The audio editor's range and edit cursor (M9.5 S2b, the owner's WD A and
//! CU A, D15, D23). A drag on the part the clip plays picks a time range on
//! the clip's track, which is the arrangement's own range, so its commands
//! act on it; a click puts the edit cursor there. Both are cut to the heard
//! stretch: only there is the beat in the editor the beat the song plays the
//! frame at. With Zero crossings on, an edge then moves within 5 ms to the
//! frame where the louder channel is nearest zero.

use eframe::egui::{self, CursorIcon, Painter, Pos2, Rect, Stroke, pos2};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::Id;

use super::gestures::WaveArea;
use crate::App;
use crate::arrangement::select::{Gesture, TimeRange};
use crate::audio_editor::paint::beat_of_frame;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::{AudioEditor, SampleCache, crossing};
use crate::theme;
use crate::timeview::TimeView;

/// Beat `q` as ticks.
pub(super) fn ticks_of(q: f64) -> Ticks {
    Ticks((q * PPQ as f64).round() as i64)
}

/// The heard stretch, in ticks.
pub(super) fn heard(file: &WholeFile) -> (Ticks, Ticks) {
    (ticks_of(file.heard.0), ticks_of(file.heard.1))
}

impl App {
    /// The index and id of the track that holds clip `id`.
    pub(super) fn clip_track_index(&self, id: Id) -> Option<(usize, Id)> {
        self.document
            .project()
            .tracks
            .iter()
            .enumerate()
            .find(|(_, t)| t.clips.iter().any(|c| c.id == id))
            .map(|(i, t)| (i, t.id))
    }

    /// Where an edge at `to` lands: on the nearest quiet frame within 5 ms
    /// when Zero crossings is on and `free` (Alt) is not held. The second
    /// value says it moved onto a crossing, so its guide lights. A file
    /// played from disk answers on a later frame: until then the edge stays
    /// where the grid put it (D23).
    pub(super) fn crossing_at(
        &self,
        ctx: &egui::Context,
        ed: &mut AudioEditor,
        area: &WaveArea<'_>,
        to: Ticks,
        free: bool,
    ) -> (Ticks, bool) {
        if free || !self.settings.editing.zero_crossings {
            return (to, false);
        }
        let Some(frame) = area.file.frame_at(to.as_quarters()) else {
            return (to, false);
        };
        let candidate = frame.round() as i64;
        let reach = crossing::reach(area.rate);
        let first = (candidate - reach).max(0);
        let count = (candidate + reach + 1 - first).max(1);
        let length = area.audio.frames() as u64;
        let covers = |c: &SampleCache| {
            c.key == area.key
                && c.length == length
                && c.first <= first
                && first + count <= c.first + c.frames.len() as i64
        };
        if !ed.crossings.last.as_ref().is_some_and(covers)
            && let Some(frames) = ed
                .crossings
                .reader
                .samples(area.audio, first, count as u32, ctx)
        {
            ed.crossings.last = Some(SampleCache {
                key: area.key,
                length,
                first,
                frames,
            });
        }
        let Some(cache) = ed.crossings.last.as_ref().filter(|c| covers(c)) else {
            return (to, false);
        };
        crossing::best(&cache.frames, cache.first, candidate, reach)
            .and_then(|f| beat_of_frame(area.file, f as f64))
            .map_or((to, false), |q| (ticks_of(q), true))
    }

    /// An edge at `at`: on the editor's grid (Alt: free), then on a
    /// crossing, kept to the heard stretch.
    pub(super) fn wave_edge(
        &self,
        ctx: &egui::Context,
        ed: &mut AudioEditor,
        area: &WaveArea<'_>,
        at: Ticks,
        free: bool,
    ) -> (Ticks, bool) {
        let (from, to) = heard(area.file);
        let snapped = self.snapped_at_zoom(at, free, ed.time.zoom).clamp(from, to);
        let (edge, crossed) = self.crossing_at(ctx, ed, area, snapped, free);
        (edge.clamp(from, to), crossed)
    }

    /// A press on the part played that becomes a drag: a range from there.
    pub(super) fn start_wave_range(
        &mut self,
        id: Id,
        origin: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) -> Option<Gesture> {
        let (track, _) = self.clip_track_index(id)?;
        self.arrangement.clips.clear();
        self.arrangement.range = None;
        self.locators.selected = None;
        ed.cursor = None;
        Some(Gesture::Range {
            at: ticks_of(ed.time.beat_at(area.body.left(), origin.x)),
            track,
        })
    }

    /// One frame of a range drag: from where it began to the pointer, cut to
    /// the heard stretch, on the clip's track.
    pub(super) fn step_wave_range(
        &mut self,
        ctx: &egui::Context,
        overlay: &Painter,
        (press, track): (Ticks, usize),
        p: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let free = ctx.input(|i| i.modifiers.alt);
        let (a, _) = self.wave_edge(ctx, ed, area, press, free);
        let here = ticks_of(ed.time.beat_at(area.body.left(), p.x));
        let (b, crossed) = self.wave_edge(ctx, ed, area, here, free);
        let project = self.document.project();
        let Some(id) = project.tracks.get(track).map(|t| t.id) else {
            return;
        };
        let range = TimeRange::over(project, a, b, &[id]);
        self.arrangement.selected = range.as_ref().map(|r| r.tracks.clone()).unwrap_or_default();
        self.arrangement.range = range;
        let x = ed.time.x(area.body.left(), b.as_quarters());
        draw_edge_guide(overlay, x, area.body, crossed);
        ed.crossed = crossed;
        ctx.set_cursor_icon(CursorIcon::Text);
    }

    /// A click on the part played: the edit cursor goes there (Alt: off the
    /// grid), the clip is selected, and a range goes.
    pub(super) fn place_wave_cursor(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        p: Pos2,
        area: &WaveArea<'_>,
        ed: &mut AudioEditor,
    ) {
        let free = ctx.input(|i| i.modifiers.alt);
        let here = ticks_of(ed.time.beat_at(area.body.left(), p.x));
        let (at, _) = self.wave_edge(ctx, ed, area, here, free);
        ed.cursor = Some(at);
        ed.anchor = None;
        self.select_opened_clip(id);
    }

    /// The arrangement's range on the opened clip's track, cut to the heard
    /// stretch, in beats.
    pub(super) fn range_in_editor(&self, id: Id, file: &WholeFile) -> Option<(f64, f64)> {
        let range = self.arrangement.range.as_ref()?;
        let (_, track) = self.clip_track_index(id)?;
        if !range.tracks.contains(&track) {
            return None;
        }
        let a = range.start.as_quarters().max(file.heard.0);
        let b = range.end.as_quarters().min(file.heard.1);
        (b > a).then_some((a, b))
    }

    /// The range over the wave and the ruler, and the edit cursor.
    pub(super) fn paint_range_and_cursor(
        &self,
        painter: &Painter,
        (body, ruler_top): (Rect, f32),
        time: TimeView,
        id: Id,
        file: &WholeFile,
        cursor: Option<Ticks>,
    ) {
        let palette = theme::p();
        let painter = painter.with_clip_rect(
            Rect::from_x_y_ranges(body.x_range(), ruler_top..=body.bottom())
                .intersect(painter.clip_rect()),
        );
        if let Some((a, b)) = self.range_in_editor(id, file) {
            let (xa, xb) = (time.x(body.left(), a), time.x(body.left(), b));
            let (xa, xb) = (xa.max(body.left() - 1.0), xb.min(body.right() + 1.0));
            if xb > xa {
                // The arrangement's look (`selection_look.rs`): a wash, its
                // edges, and a bracket along its top.
                painter.rect_filled(
                    Rect::from_x_y_ranges(xa..=xb, body.y_range()),
                    0.0,
                    palette.accent.gamma_multiply(0.16),
                );
                let edge = Stroke::new(theme::stroke::HAIRLINE, palette.accent.gamma_multiply(0.6));
                painter.vline(xa, body.y_range(), edge);
                painter.vline(xb, body.y_range(), edge);
                let stroke = Stroke::new(theme::stroke::EDGE, palette.accent);
                let y = body.top() + 1.0;
                painter.line_segment([pos2(xa, y), pos2(xb, y)], stroke);
                for x in [xa, xb] {
                    painter.line_segment([pos2(x, y), pos2(x, y + 5.0)], stroke);
                }
            }
        }
        if let Some(at) = cursor {
            let x = time.x(body.left(), at.as_quarters());
            if body.x_range().contains(x) {
                let ink = palette.text;
                painter.vline(x, body.y_range(), Stroke::new(theme::stroke::HAIRLINE, ink));
                let cap = theme::size::CURSOR_CAP;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(x - cap, body.top()),
                        pos2(x + cap, body.top()),
                        pos2(x, body.top() + cap),
                    ],
                    ink,
                    Stroke::NONE,
                ));
            }
        }
    }
}

/// The guide at an edge being placed: the accent when it landed on a
/// crossing (D23), so the hand can tell.
pub(super) fn draw_edge_guide(painter: &Painter, x: f32, body: Rect, crossed: bool) {
    if crossed {
        painter.vline(
            x,
            body.y_range(),
            Stroke::new(theme::stroke::LIT, theme::p().accent),
        );
    } else {
        crate::app::grid::draw_snap_guide(painter, x, body);
    }
}

impl App {
    /// Esc in the audio editor (D28): the envelope's picks go first; a range
    /// is left to Deselect, and the keyboard's cursor comes back after it;
    /// then the cursor goes. Returns whether it used the key.
    pub(crate) fn audio_editor_deselect(&mut self) -> bool {
        let ed = &mut self.audio_editor;
        if ed.hands.picks.take().is_some() {
            return true;
        }
        if self.arrangement.range.is_some() {
            if let Some(anchor) = ed.anchor {
                ed.cursor = Some(anchor);
            }
            return false;
        }
        ed.anchor = None;
        ed.cursor.take().is_some()
    }
}
