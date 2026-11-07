//! The audio clip editor in the Clip tab (M9.5 S1): the clip card's rows in
//! a column at the left (until S3 moves them into the strip, the owner's L
//! B), and at the right the clip editors' shared header over the editor: a
//! ruler of bars and file time, and the clip's whole file with the part it
//! plays bright. The drawing is `audio_editor::paint`; the mapping
//! `audio_editor::source`; the handles are `gestures` (the pointer) and `keys`
//! (the keyboard) here, with `audio_editor::handles` for where they are.

mod gestures;
mod header;
pub(crate) mod keys;

use std::sync::Arc;

use eframe::egui::{self, Pos2, Rect, Sense, Ui, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_engine::ClipAudio;
use lumen_media::Peaks;
use lumen_project::model::{Clip, ClipContent, Id};
use lumen_project::routing::all_tracks;

use self::gestures::WaveArea;
use crate::app::clip_header::HEADER;
use crate::app::info::{Held, Info, Mouse};
use crate::app::navigate::KEY_ZOOM;
use crate::arrangement::{self, waves};
use crate::audio_editor::handles::{self as handle_marks, EditorHandles};
use crate::audio_editor::header::folds;
use crate::audio_editor::paint::{
    self, RULER, SAMPLES_BELOW, SPLIT_FROM, Samples, WaveInput, WaveLook,
};
use crate::audio_editor::source::{self, WholeFile};
use crate::audio_editor::{AudioEditor, Drawn, Fit};
use crate::commands::Command;
use crate::media::State;
use crate::note_editor::EditorZoom;
use crate::timeview::{Limits, TimeView};
use crate::widgets::{self, raised};
use crate::{App, theme};

/// The properties column at the left: the clip card's rows, with its
/// padding.
const PROPS_WIDTH: f32 = crate::app::clip_card::CARD_WIDTH + 2.0 * theme::space::M;
/// The editor needs at least this much wave area to be drawn.
const MIN_BODY: f32 = 24.0;
/// The most samples read for one frame's drawing (a screen's width at
/// 8 points a sample on a wide display).
const MOST_SAMPLES: i64 = 16_384;
/// Room kept past the file's ends when fitting it (a fraction of its span).
const FIT_ROOM: f64 = 0.02;

/// A loaded file: what its wave needs.
struct Loaded {
    source: waves::Source,
    peaks: Arc<Peaks>,
    audio: Arc<ClipAudio>,
}

impl App {
    /// An opened audio clip: its properties beside the editor.
    pub(crate) fn audio_clip_view(&mut self, ui: &mut Ui, area: Rect, clip: Id) {
        // Too narrow for both, the wave has the room: Clip properties is
        // then in the header's menu (M9.5 D24).
        let folded = folds(area.width(), PROPS_WIDTH);
        self.audio_editor.folded = folded;
        if folded {
            let mut ed = std::mem::take(&mut self.audio_editor);
            self.draw_audio_editor(ui, area, clip, &mut ed);
            self.audio_editor = ed;
            return;
        }
        let palette = *theme::p();
        let column =
            Rect::from_min_size(area.min, vec2(PROPS_WIDTH.min(area.width()), area.height()));
        ui.painter().rect_filled(column, 0.0, palette.app);
        ui.painter().vline(
            column.right() - theme::stroke::HAIRLINE / 2.0,
            column.y_range(),
            egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        let mut props = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(column.shrink(theme::space::M))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .id_salt("docked clip properties")
            .auto_shrink([false, false])
            .show(&mut props, |ui| {
                ui.set_width(crate::app::clip_card::CARD_WIDTH.min(ui.available_width()));
                self.docked_clip_props(ui, clip);
            });
        let editor = Rect::from_min_max(pos2(column.right(), area.top()), area.max);
        if editor.width() > 2.0 * theme::space::L {
            let mut ed = std::mem::take(&mut self.audio_editor);
            self.draw_audio_editor(ui, editor, clip, &mut ed);
            self.audio_editor = ed;
        }
    }

    /// The file of `key` (a media id, or a reversed twin's) when it is loaded
    /// at the device rate with its peaks.
    fn loaded(&self, key: u64) -> Option<Loaded> {
        let rate = self.lanes.rate();
        self.lanes
            .library
            .with_twins()
            .find(|i| i.id == key)
            .and_then(|item| {
                let (State::Ready(audio), Some(peaks)) = (&item.state, &item.peaks) else {
                    return None;
                };
                (audio.sample_rate() == rate).then(|| Loaded {
                    source: waves::Source {
                        slot: 0,
                        channels: peaks.channels as u32,
                        native_rate: item.info.sample_rate.unwrap_or(audio.sample_rate()),
                    },
                    peaks: Arc::clone(peaks),
                    audio: Arc::clone(audio),
                })
            })
    }

    /// The key of the file `clip` plays (its reversed twin when reversed).
    fn file_key(clip: &Clip) -> Option<u64> {
        let ClipContent::Audio { media, .. } = &clip.content else {
            return None;
        };
        Some(if clip.reversed {
            crate::media::reversed_key(media.0)
        } else {
            media.0
        })
    }

    fn draw_audio_editor(&mut self, ui: &mut Ui, area: Rect, id: Id, ed: &mut AudioEditor) {
        let ctx = ui.ctx().clone();
        let Some((clip, colour)) = self.opened_clip_look(id) else {
            return;
        };
        let rate = self.lanes.rate();
        let key = Self::file_key(&clip);
        let loaded = key.and_then(|key| self.loaded(key));
        let file = loaded.as_ref().and_then(|l| self.file_of(&clip, l, ed));

        let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER));
        let ruler = Rect::from_min_max(
            pos2(area.left(), header.bottom()),
            pos2(area.right(), header.bottom() + RULER),
        );
        let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
        if let Some(file) = &file {
            // Opening a clip frames it with nothing to go back to: the view
            // before belongs to another clip, or to this one opened before.
            let opening = ed.framed_for != Some(clip.id);
            if opening {
                ed.framed_for = Some(clip.id);
                ed.fit = Some(Fit::Clip);
            }
            if let Some(fit) = ed.fit.take() {
                let before = ed.time;
                ed.time = match (fit, ed.back) {
                    (Fit::Back, Some(back)) => back,
                    (Fit::Back, None) => before,
                    _ => fitted(
                        file,
                        fit,
                        body.width(),
                        self.file_limits(file, body.width()),
                    ),
                };
                ed.back = (!opening).then_some(before);
                self.transport.follow_paused = true;
            }
        }
        self.audio_header(ui, header, (&clip, colour), file.as_ref(), ed);
        let is_loaded = loaded.is_some();
        let (Some(file), Some(loaded)) = (file, loaded) else {
            // Nothing to zoom: keys pressed now do not wait for a file.
            ed.zooms.clear();
            ed.hovered = false;
            ed.last = None;
            self.audio_empty_state(ui, Rect::from_min_max(ruler.min, area.max), key, is_loaded);
            return;
        };
        if body.height() < MIN_BODY || body.width() < MIN_BODY {
            ed.zooms.clear();
            return;
        }

        let limits = self.file_limits(&file, body.width());
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let layer = ui.layer_id();
        let on_top = |p: Pos2| ctx.layer_id_at(p) == Some(layer);
        let over =
            pointer.filter(|p| Rect::from_min_max(ruler.min, area.max).contains(*p) && on_top(*p));
        let zoomed = audio_zoom_keys(
            ed,
            body,
            pointer,
            self.transport.playhead().as_quarters(),
            limits,
        );
        let wheeled = audio_wheel(&ctx, over, body, &mut ed.time, limits);
        if zoomed || wheeled {
            // Looking around pauses Follow, as in the arrangement.
            self.transport.follow_paused = true;
        }
        self.audio_follow(ed, body);
        ed.hovered = over.is_some();
        // A handle that took the keyboard or moved by a key comes into view.
        if let Some(part) = ed.reveal.take()
            && let Some(q) = self
                .shown_clip(id)
                .and_then(|c| handle_marks::beat_of(&c, part, ed.time, body))
        {
            let shown = ed.time.showing(q, q, limits);
            if shown != ed.time {
                ed.time = shown;
                self.transport.follow_paused = true;
            }
        }

        // The areas that take the pointer. The handles' Tab stops come last,
        // after the header's and the ruler's.
        let base = ui.id().with(("audio editor", clip.id.0));
        let wave_response = ui.interact(body, base.with("wave"), Sense::CLICK | Sense::DRAG);
        wave_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                format!("Audio editor: {}", clip.name),
            )
        });
        let ruler_response = ui.interact(ruler, base.with("ruler"), Sense::click_and_drag());
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Editor ruler: click to move the playhead",
            )
        });
        // Stopped, a drag moves the playhead with the pointer; playing, it
        // moves it where the drag ends, so playback is not sent to a new
        // place every frame.
        let playing = self.transport.state().playing;
        let moved = ruler_response.clicked()
            || ruler_response.drag_stopped()
            || (!playing && (ruler_response.dragged() || ruler_response.drag_started()));
        if moved && let Some(p) = ruler_response.interact_pointer_pos() {
            let free = ctx.input(|i| i.modifiers.alt);
            let at = self.snapped_at_zoom(ticks_at(ed.time, body.left(), p.x), free, ed.time.zoom);
            self.transport.locate(at.max(Ticks::ZERO));
            self.transport.follow_paused = true;
        }

        // The pointer's edits, then the clip and file as they left them, so
        // what is drawn is never a frame behind the hand.
        let hit = self.audio_wave_pointer(
            &wave_response,
            id,
            &WaveArea {
                body,
                file: &file,
                limits,
                rate,
            },
            ed,
        );
        let (clip, file) = match self.opened_clip_look(id) {
            Some((now, _)) if !Arc::ptr_eq(&now, &clip) => {
                let file = self.file_of(&now, &loaded, ed).unwrap_or(file);
                (now, file)
            }
            _ => (clip, file),
        };

        // What to draw.
        let painter = ui.painter().clone();
        let meter = &self.transport.timeline.meter;
        let from = ticks_at(ed.time, body.left(), body.left()).max(Ticks::ZERO);
        let to = ticks_at(ed.time, body.left(), body.right()).max(from);
        let lines = self.grid.lines(meter, from, to, ed.time.zoom);
        let palette = theme::p();
        painter.rect_filled(body, 0.0, palette.canvas);
        for line in &lines {
            let x = ed.time.x(body.left(), line.at.as_quarters());
            let ink = match line.level {
                lumen_core::grid::Level::Bar => palette.grid_bar,
                lumen_core::grid::Level::Beat => palette.grid_beat,
                lumen_core::grid::Level::Sub => palette.grid_sub,
            };
            painter.vline(
                x,
                body.y_range(),
                egui::Stroke::new(theme::stroke::HAIRLINE, ink),
            );
        }
        let ppp = ctx.pixels_per_point();
        let middle_q = ed.time.beat_at(body.left(), body.center().x);
        let deep = paint::frames_per_pixel(&file, ed.time, middle_q, ppp)
            .or_else(|| paint::frames_per_pixel(&file, ed.time, file.played.first.0, ppp))
            .is_some_and(|f| f < SAMPLES_BELOW);
        let wanted = if deep {
            paint::visible_frames(&file, ed.time, body)
        } else {
            None
        };
        if let (Some((a, b)), Some(key)) = (wanted, key) {
            self.audio_samples(ed, &loaded, key, (a, b), &ctx);
        }
        let samples = wanted.and_then(|(a, b)| {
            let cache = ed.samples.as_ref()?;
            let len = cache.frames.len() as i64;
            let covers = key == Some(cache.key)
                && cache.length == loaded.audio.frames() as u64
                && cache.first <= a
                && b < cache.first + len;
            covers.then(|| Samples {
                first: a,
                frames: &cache.frames[(a - cache.first) as usize..=(b - cache.first) as usize],
            })
        });
        let drew_samples = samples.is_some();
        let look = WaveLook {
            bright: theme::wave(colour),
            dim: palette.text_hint.gamma_multiply(theme::WAVE_OUTSIDE),
        };
        let input = WaveInput {
            file: &file,
            peaks: &loaded.peaks,
            clip: &clip,
            time: ed.time,
            look,
            split: loaded.source.channels > 1 && body.height() >= SPLIT_FROM,
            samples,
        };
        paint::wave(&painter, body, &input);
        paint::ruler(&painter, ruler, ed.time, &lines, meter, &file, rate);
        // Each handle's Tab stop, then the handles over the wave.
        let handles = self
            .shown_clip(id)
            .and_then(|c| EditorHandles::of(&c, body, ed.time));
        if let Some(handles) = &handles {
            let stop = self.audio_stops(ui, id, handles, ed);
            let hot = Self::audio_hot_part(ed, hit, stop.as_ref().map(|(part, _)| *part));
            let inside = painter.with_clip_rect(body.intersect(painter.clip_rect()));
            handle_marks::draw(
                &inside,
                handles,
                handle_marks::Look {
                    hot,
                    fixed: self.opened_clip_fixed(id),
                    crossing: self.audio_crossing_colours(id),
                },
            );
            if let Some((part, response)) = &stop {
                raised::focus_ring(&inside, response, handles.stop_rect(*part));
            }
        }
        self.paint_editor_playhead(&painter, ed.time, body, ruler.top());
        ed.last = Some(Drawn {
            clip: clip.id,
            body,
            time: ed.time,
            window: file.played.extent(),
            file: (file.from, file.to),
            samples: drew_samples,
            handles,
            played: file.played,
        });
        if let Some(p) = pointer.filter(|p| (body.contains(*p) || ruler.contains(*p)) && on_top(*p))
            && !egui::Popup::is_any_open(&ctx)
        {
            let area = WaveArea {
                body,
                file: &file,
                limits,
                rate,
            };
            if ruler.contains(p) {
                self.audio_ruler_info(&file, ed.time, body, p, rate);
            } else if ed.gesture.is_none() {
                self.audio_wave_info(&ctx, id, hit, p, &area, ed.time);
            }
        }
    }

    /// The opened clip and its colour.
    fn opened_clip_look(&self, id: Id) -> Option<(Arc<Clip>, egui::Color32)> {
        all_tracks(self.document.project()).find_map(|t| {
            let clip = t.clips.iter().find(|c| c.id == id)?;
            Some((Arc::clone(clip), arrangement::clip_color(t, clip)))
        })
    }

    /// The colours of the crossfade out of the opened clip: its own and the
    /// next clip's.
    fn audio_crossing_colours(&self, id: Id) -> Option<(egui::Color32, egui::Color32)> {
        let shown = self.shown_clip(id).filter(|c| c.crossfade)?;
        let end = shown.start + shown.length;
        all_tracks(self.document.project()).find_map(|t| {
            let clip = t.clips.iter().find(|c| c.id == id)?;
            let next = t.clips.iter().find(|c| c.id != id && c.start == end)?;
            Some((
                arrangement::clip_color(t, clip),
                arrangement::clip_color(t, next),
            ))
        })
    }

    /// The file's place on the timeline, worked out again only when the
    /// clip, the tempo, the rate or the file changes.
    fn file_of(
        &self,
        clip: &Arc<Clip>,
        loaded: &Loaded,
        ed: &mut AudioEditor,
    ) -> Option<WholeFile> {
        let rate = self.lanes.rate();
        let tempo = &self.transport.timeline.tempo;
        let frames = loaded.audio.frames() as u64;
        let fresh = ed.file.as_ref().is_some_and(|c| {
            Arc::ptr_eq(&c.clip, clip)
                && c.rate == rate
                && c.frames == frames
                && c.tempo.as_slice() == tempo.points()
        });
        if !fresh {
            let source = loaded.source;
            ed.file =
                source::whole_file(clip, tempo, rate, frames, move |_| Some(source)).map(|file| {
                    crate::audio_editor::FileCache {
                        clip: Arc::clone(clip),
                        tempo: tempo.points().to_vec(),
                        rate,
                        frames,
                        file,
                    }
                });
        }
        ed.file.as_ref().map(|c| c.file.clone())
    }

    /// Keeps the samples for `want` (the frames in view) at hand: they are
    /// asked for with a screen's width to spare on either side, and kept
    /// while the view stays inside them, so a file played from disk is not
    /// read again every frame.
    fn audio_samples(
        &self,
        ed: &mut AudioEditor,
        loaded: &Loaded,
        key: u64,
        (a, b): (i64, i64),
        ctx: &egui::Context,
    ) {
        let length = loaded.audio.frames() as u64;
        let covered = ed.samples.as_ref().is_some_and(|c| {
            c.key == key
                && c.length == length
                && c.first <= a
                && b < c.first + c.frames.len() as i64
        });
        if covered {
            return;
        }
        let width = (b - a + 1).clamp(1, MOST_SAMPLES);
        let first = (a - width).max(0);
        let count = (3 * width) as u32;
        if let Some(frames) = ed.reader.samples(&loaded.audio, first, count, ctx) {
            ed.samples = Some(crate::audio_editor::SampleCache {
                key,
                length,
                first,
                frames,
            });
        }
    }

    /// What the editor says when it cannot draw the clip's file.
    /// `loaded`: the file is ready at the device rate with its peaks.
    fn audio_empty_state(&mut self, ui: &mut Ui, rect: Rect, key: Option<u64>, loaded: bool) {
        let item = key.and_then(|key| self.lanes.library.with_twins().find(|i| i.id == key));
        let (title, hint, pool) = match item.map(|i| &i.state) {
            None => (
                "This clip's file is not in the project",
                "The media pool lists the project's files and can find a missing one.",
                true,
            ),
            Some(State::Failed(_)) => (
                "This clip's file cannot be read",
                "It may have moved, or it is not a kind of audio file that can be read. The media pool can find it again.",
                true,
            ),
            Some(State::Ready(_)) if loaded => (
                "This clip's wave cannot be shown",
                "Its file is loaded but does not reach the part the clip plays. The clip still plays as it is.",
                false,
            ),
            Some(State::Ready(_)) => (
                "Preparing the wave",
                "The file is ready; its wave appears here in a moment.",
                false,
            ),
            Some(State::Loading) => (
                "Reading the file",
                "Its wave appears here as soon as it is read.",
                false,
            ),
        };
        let keys = self.keymap.keys(Command::MediaPool);
        let state = widgets::EmptyState {
            icon: None,
            icon_size: theme::space::L,
            title,
            hint,
            action: pool.then_some(widgets::EmptyAction {
                label: "Open the media pool",
                keys: keys.as_ref(),
                tip: "Lists every file the project uses, with Locate for the missing ones",
            }),
        };
        if widgets::empty_state(ui, rect, &state) {
            self.show_media_pool();
        }
    }

    /// How far the editor zooms and scrolls: out to the whole file (or the
    /// song, if longer) with room after it, in to the arrangement's deepest.
    fn file_limits(&self, file: &WholeFile, width: f32) -> Limits {
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
    fn audio_follow(&self, ed: &mut AudioEditor, body: Rect) {
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
    fn audio_ruler_info(
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

    /// The editor's keys are live: the pointer is over it, or the detail
    /// panel has the keyboard on the Clip tab of an audio clip.
    pub(crate) fn audio_editor_live(&self, ctx: &egui::Context) -> bool {
        self.detail_audio_clip().is_some()
            && (self.audio_editor.hovered_before
                || keys::focused_stop(ctx).is_some()
                || (self.show_detail
                    && self.detail_tab == crate::app::detail::DetailTab::Clip
                    && crate::app::panes::focused(ctx) == Some(crate::app::panes::Pane::Detail)))
    }

    /// Once a frame, before the keys.
    pub(crate) fn refresh_audio_editor(&mut self) {
        let ed = &mut self.audio_editor;
        ed.hovered_before = ed.hovered;
        ed.hovered = false;
    }

    /// The playhead through an editor's ruler and body, as the arrangement
    /// draws it: a beam while playing (a white core in the accent, its
    /// halo, light cast on the content and a cap on the ruler), a plain line
    /// when stopped. Shared by both clip editors.
    pub(crate) fn paint_editor_playhead(
        &self,
        painter: &egui::Painter,
        time: TimeView,
        body: Rect,
        ruler_top: f32,
    ) {
        let at = self.transport.playhead().as_quarters();
        let x = time.x(body.left(), at);
        if x < body.left() || x > body.right() {
            return;
        }
        let y = egui::Rangef::new(ruler_top, body.bottom());
        let colour = theme::p().accent;
        let painter = painter.with_clip_rect(
            Rect::from_x_y_ranges(body.x_range(), ruler_top..=body.bottom())
                .intersect(painter.clip_rect()),
        );
        if self.transport.state().playing {
            crate::light::cast(
                &painter,
                crate::light::BEAM,
                x,
                egui::Rangef::new(body.top(), body.bottom()),
                colour,
            );
            crate::light::vline(&painter, crate::light::BEAM, x, y, colour, 1.0);
            crate::light::cap(&painter, x, ruler_top, colour);
        } else {
            painter.vline(x, y, egui::Stroke::new(1.0, colour));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - 5.0, ruler_top),
                    pos2(x + 5.0, ruler_top),
                    pos2(x, ruler_top + 7.0),
                ],
                colour,
                egui::Stroke::NONE,
            ));
        }
    }
}

/// The tick at `x` of a view whose left edge is at `origin`.
fn ticks_at(time: TimeView, origin: f32, x: f32) -> Ticks {
    Ticks((time.beat_at(origin, x) * lumen_core::time::PPQ as f64).round() as i64)
}

/// The view for `fit` across `width` points.
fn fitted(file: &WholeFile, fit: Fit, width: f32, limits: Limits) -> TimeView {
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
fn audio_zoom_keys(
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
fn audio_wheel(
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
