//! The audio clip editor in the Clip tab (M9.5 S1): the clip editors' shared
//! header, the properties strip under it (S3, `strip`), then the editor: a
//! ruler of bars and file time, and the clip's whole file with the part it
//! plays bright. The drawing is `audio_editor::paint`; the mapping
//! `audio_editor::source`; the handles are `gestures` (the pointer) and `keys`
//! (the keyboard) here, with `audio_editor::handles` for where they are.

mod brace;
mod envelope;
mod gestures;
mod header;
mod hearing;
pub(crate) mod keys;
mod pitch;
mod range;
pub(crate) mod stops;
mod strip;
mod strip_chips;
mod view;
mod warp;
pub(crate) mod warp_markers;
mod window;

use std::sync::Arc;

use eframe::egui::{self, Pos2, Rect, Sense, Ui, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_engine::ClipAudio;
use lumen_media::Peaks;
use lumen_project::model::{Clip, Id};
use lumen_project::routing::all_tracks;

use self::gestures::WaveArea;
use self::view::{audio_wheel, audio_zoom_keys, fitted, ticks_at};
use crate::app::clip_header::HEADER;
use crate::arrangement::{self, waves};
use crate::audio_editor::brace::RULER;
use crate::audio_editor::handles::{self as handle_marks, EditorHandles};
use crate::audio_editor::paint::{self, SAMPLES_BELOW, SPLIT_FROM, Samples, WaveInput, WaveLook};
use crate::audio_editor::source::{self, WholeFile};
use crate::audio_editor::warp_strip::{self, WarpStrip};
use crate::audio_editor::{AudioEditor, Drawn, Fit, WarpEditor};
use crate::commands::Command;
use crate::media::State;
use crate::timeview::TimeView;
use crate::widgets::{self, raised};
use crate::{App, theme};

/// The editor needs at least this much wave area to be drawn.
const MIN_BODY: f32 = 24.0;
/// The most samples read for one frame's drawing (a screen's width at
/// 8 points a sample on a wide display).
const MOST_SAMPLES: i64 = 16_384;

/// A loaded file: what its wave needs.
struct Loaded {
    source: waves::Source,
    peaks: Arc<Peaks>,
    audio: Arc<ClipAudio>,
}

impl App {
    /// An opened audio clip: its editor, with the header and the properties
    /// strip over it.
    pub(crate) fn audio_clip_view(&mut self, ui: &mut Ui, area: Rect, clip: Id) {
        if self.detail.audio_popped {
            self.popped_note(ui, area);
            return;
        }
        let mut ed = std::mem::take(&mut self.audio_editor);
        self.draw_audio_editor(ui, area, clip, &mut ed);
        self.audio_editor = ed;
    }

    /// The note that stands where the editor was while it is in its own
    /// window, with the way to dock it back.
    fn popped_note(&mut self, ui: &mut Ui, rect: Rect) {
        let state = widgets::EmptyState {
            icon: None,
            icon_size: theme::space::L,
            title: "The clip editor is in its own window",
            hint: "Close that window, or dock it here",
            action: Some(widgets::EmptyAction {
                label: "Dock editor",
                keys: None,
                tip: "Put the editor back in the detail panel",
            }),
        };
        if widgets::empty_state(ui, rect, &state) {
            self.detail.audio_popped = false;
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

    fn draw_audio_editor(&mut self, ui: &mut Ui, area: Rect, id: Id, ed: &mut AudioEditor) {
        let ctx = ui.ctx().clone();
        let Some((clip, colour)) = self.opened_clip_look(id) else {
            return;
        };
        let rate = self.lanes.rate();
        let key = crate::audio_editor::detect::file_key(&clip);
        let loaded = key.and_then(|key| self.loaded(key));
        let file = loaded.as_ref().and_then(|l| self.file_of(&clip, l, ed));

        let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER));
        let strip = Rect::from_min_size(
            pos2(area.left(), header.bottom()),
            vec2(area.width(), theme::size::EDITOR_STRIP),
        );
        let ruler = Rect::from_min_max(
            pos2(area.left(), strip.bottom()),
            pos2(area.right(), strip.bottom() + RULER),
        );
        let (scale, band) = self::brace::split_ruler(ruler);
        // A warped clip has its markers' strip between the ruler and the wave
        // (S5b, P A).
        let strip_rect = warp_strip::rect_under(ruler, &clip);
        let body = Rect::from_min_max(
            pos2(
                area.left(),
                strip_rect.map_or(ruler.bottom(), |r| r.bottom()),
            ),
            area.max,
        );
        // Another clip opened: what was picked in the last one goes.
        if ed.framed_for != Some(clip.id) {
            ed.cursor = None;
            ed.anchor = None;
            ed.hands.picks = None;
            ed.keys.clear();
            ed.markers = WarpEditor::default();
        }
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
        self.drive_detection(&ctx, &clip, ed);
        self.audio_header(ui, header, (&clip, colour), file.as_ref(), ed);
        self.audio_strip(ui, strip, (&clip, colour), ed);
        let is_loaded = loaded.is_some();
        let (Some(file), Some(loaded)) = (file, loaded) else {
            // Nothing to zoom: keys pressed now do not wait for a file.
            ed.zooms.clear();
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
        ed.hovered |= over.is_some();
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
        let ruler_response = ui.interact(scale, base.with("ruler"), Sense::CLICK | Sense::DRAG);
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Editor ruler: click to move the playhead",
            )
        });
        self.editor_scrub(&ctx, &ruler_response, ed, body.left());

        let band_response = ui.interact(band, base.with("brace"), Sense::CLICK | Sense::DRAG);

        // The pointer's edits, then the clip and file as they left them, so
        // what is drawn is never a frame behind the hand: the brace, the
        // envelope, then the wave.
        let area_now = WaveArea {
            body,
            file: &file,
            limits,
            rate,
            audio: &loaded.audio,
            key: key.unwrap_or(0),
        };
        let brace_now = crate::audio_editor::brace::Brace::of(&clip, &file, ed.time, band);
        self.brace_pointer(&band_response, &clip, &brace_now, &area_now, ed);
        if let Some(rect) = strip_rect {
            let strip_now = WarpStrip::of(&clip, &file, (rate, ed.time), rect);
            let response = ui.interact(rect, base.with("warp strip"), Sense::CLICK | Sense::DRAG);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Other,
                    true,
                    "Warp strip: double-click to add a warp marker",
                )
            });
            self.warp_strip_pointer(&response, &clip, &strip_now, &area_now, ed);
        } else {
            ed.markers.ids.clear();
            ed.markers.drag = None;
            ed.markers.hover = None;
        }
        let taken = match self
            .shown_clip(id)
            .and_then(|c| EditorHandles::of(&c, body, ed.time))
        {
            Some(handles) => {
                let frame = self.envelope_frame(&clip, body, ed.time);
                let handles = handles.hiding_gain(ed.pill);
                self.envelope_pointer(&wave_response, &clip, (&frame, &handles), &area_now, ed)
            }
            None => false,
        };
        let hit = self.audio_wave_pointer(&wave_response, id, &area_now, ed, taken);
        // The stops' keys, with the file and the envelope at hand.
        if !ed.keys.is_empty()
            && let Some(now) = self.shown_clip(id)
        {
            let frame = self.envelope_frame(&now, body, ed.time);
            self.run_stop_keys(&ctx, &now, (&area_now, &frame), ed);
        }
        if !ed.markers.keys.is_empty()
            && let Some(now) = self.shown_clip(id)
        {
            self.run_marker_keys(&now, ed);
        }
        let (clip, file) = match self.opened_clip_look(id) {
            Some((now, _)) if !Arc::ptr_eq(&now, &clip) => {
                let file = self.file_of(&now, &loaded, ed).unwrap_or(file);
                (now, file)
            }
            _ => (clip, file),
        };
        // The markers as the edits left them.
        let strip = strip_rect.map(|rect| WarpStrip::of(&clip, &file, (rate, ed.time), rect));
        if let Some(strip) = &strip {
            self.reveal_marker(strip, limits, ed);
        }

        // What to draw.
        let painter = ui.painter().clone();
        let meter = &self.transport.timeline.meter;
        let from = ticks_at(ed.time, body.left(), body.left()).max(Ticks::ZERO);
        let to = ticks_at(ed.time, body.left(), body.right()).max(from);
        let lines = self.grid.lines(meter, from, to, ed.time.zoom);
        let palette = theme::p();
        painter.rect_filled(body, 0.0, palette.canvas);
        // The same lines again over the clip's body, where the wave draws.
        let over_clip: Vec<(f32, lumen_core::grid::Level)> = lines
            .iter()
            .map(|l| (ed.time.x(body.left(), l.at.as_quarters()), l.level))
            .collect();
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
            grid: &over_clip,
        };
        paint::wave(&painter, body, &input);
        if let Some(strip) = &strip {
            Self::paint_strip_ground(&painter, strip.rect);
        }
        let previewing = std::mem::take(&mut ed.detect.previewing);
        if let Some(key) = crate::audio_editor::detect::file_key(&clip)
            && let Some(found) = ed.detect.analysis(key)
        {
            // A warped clip shows them as triangles in its strip; one that
            // is not shows them as faint lines on the wave only while the
            // Warp popover is open.
            let at = (ed.time, rate);
            match (
                crate::audio_editor::detect::Shown::of(strip.is_some(), previewing),
                &strip,
            ) {
                (crate::audio_editor::detect::Shown::Strip, Some(strip)) => {
                    Self::paint_transient_ticks(
                        &painter,
                        strip,
                        &file,
                        at,
                        (found, ed.detect.sensitivity),
                    );
                }
                (crate::audio_editor::detect::Shown::Wave, _) => {
                    Self::paint_transients(
                        &painter,
                        body,
                        &file,
                        at,
                        (found, ed.detect.sensitivity),
                    );
                }
                _ => {}
            }
        }
        paint::ruler(&painter, scale, ed.time, &lines, meter, &file, rate);
        // The wave's Tab stop, then the envelope and the range over the
        // wave, each handle's stop and the handles, then the envelope's and
        // the loop's stops and the brace.
        let ruler_stop = self.ruler_stop(ui, scale);
        let wave_stop = self.wave_stop(ui, body, &clip, ed);
        let shown = self.shown_clip(id);
        if let Some(shown) = &shown {
            let frame = self.envelope_frame(shown, body, ed.time);
            self.paint_envelope(&painter, shown, &frame, &file, ed);
        }
        let cursor = ed
            .cursor
            .filter(|_| self.range_in_editor(id, &file).is_none());
        self.paint_range_and_cursor(&painter, (body, scale.top()), ed.time, id, &file, cursor);
        if let Some(response) = &ruler_stop {
            raised::focus_ring(&painter, response, scale.shrink(raised::FOCUS_OUTSET));
        }
        if let Some(response) = &wave_stop {
            raised::focus_ring(&painter, response, body.shrink(raised::FOCUS_OUTSET));
        }
        let handles = shown
            .as_ref()
            .and_then(|c| EditorHandles::of(c, body, ed.time))
            .map(|h| h.hiding_gain(ed.pill));
        if let Some(handles) = &handles {
            let stop = self.audio_stops(ui, id, handles, strip.as_ref(), ed);
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
        if let Some(strip) = &strip {
            let ring = ed.markers.ring.take();
            self.paint_warp_strip(&painter, strip, body, ed, ring.as_ref());
        }
        let brace = crate::audio_editor::brace::Brace::of(&clip, &file, ed.time, band);
        let later = self.later_stops(ui, &clip, (body, &brace), ed);
        let brace_ring = match &later {
            Some((stops::Stop::Envelope, response)) => {
                raised::focus_ring(&painter, response, body.shrink(raised::FOCUS_OUTSET));
                None
            }
            Some((stop, response)) => Some((stops::brace_part(*stop), response)),
            None => None,
        };
        self.paint_brace(&painter, &brace, ed, brace_ring);
        self.paint_editor_playhead(&painter, ed.time, body, ruler.top());
        ed.markers.drawn = strip;
        ed.last = Some(Drawn {
            clip: clip.id,
            body,
            time: ed.time,
            window: file.played.extent(),
            file: (file.from, file.to),
            samples: drew_samples,
            handles,
            played: file.played,
            band,
            brace: brace.ends,
        });
        if let Some(p) = pointer.filter(|p| (body.contains(*p) || ruler.contains(*p)) && on_top(*p))
            && !egui::Popup::is_any_open(&ctx)
        {
            let area = WaveArea {
                body,
                file: &file,
                limits,
                rate,
                audio: &loaded.audio,
                key: key.unwrap_or(0),
            };
            if scale.contains(p) {
                self.audio_ruler_info(&file, ed.time, body, p, rate);
            } else if ed.gesture.is_none()
                && ed.hands.active.is_none()
                && !ed.envelope_hover
                && ed.brace.is_none()
                && ed.brace_hover.is_none()
                && !band.contains(p)
            {
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

    /// The editor's keys are live: the pointer is over it, the detail panel
    /// has the keyboard on the Clip tab of an audio clip, or the keys are
    /// read for the editor's own window (all of its keyboard is the editor's).
    pub(crate) fn audio_editor_live(&self, ctx: &egui::Context) -> bool {
        self.detail_audio_clip().is_some()
            && (window::its_keyboard(self.detail.audio_popped, ctx.viewport_id())
                || self.audio_editor.hovered_before
                || keys::focused_stop(ctx).is_some()
                || stops::focused(ctx).is_some()
                || warp_markers::focused(ctx, &self.audio_editor.markers.ids).is_some()
                || (self.show_detail
                    && self.detail_tab == crate::app::detail::DetailTab::Clip
                    && crate::app::panes::focused(ctx) == Some(crate::app::panes::Pane::Detail)))
    }

    /// Once a frame, before the keys.
    pub(crate) fn refresh_audio_editor(&mut self) {
        let ed = &mut self.audio_editor;
        ed.hovered_before = ed.hovered;
        ed.hovered = false;
        // A scrub whose ruler stopped being drawn (the panel was hidden under
        // the held pointer) never sees its release: let go of it here.
        if ed.scrub_sent.is_some() {
            if ed.scrub_drawn {
                ed.scrub_drawn = false;
            } else {
                ed.scrub_sent = None;
                if !self.transport.state().playing {
                    self.transport.end_scrub();
                }
            }
        }
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
