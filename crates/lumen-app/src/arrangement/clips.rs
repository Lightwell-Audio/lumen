use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_media::Peaks;
use lumen_project::model::{Clip, ClipContent, Id, Project, TrackKind};
use lumen_render::{BoxInstance, RowSlot, WaveInstance, WaveStyle};

use super::clip_look::{ClipLook, ClipState, clip_box, draw_body, draw_name, item_state, look_of};
use super::clip_marks::Marks;
use super::recording_look::{self, Paint, RecordingLook};
use super::samples::{Geometry, Pixels, Strips};
use super::waves::{Source, wave_instances};
use super::{Arrangement, Row, RowKind, TITLE, Zone, clip_color, color_of};
use crate::canvas::{self, ClipFrame};
use crate::document::{Document, audible};
use crate::lanes::{Landing, Lanes};
use crate::media::{Item, State};
use crate::theme;
use crate::timeview::TimeView;
use crate::widgets::{draw_edge_glow, gradient};

pub(super) const INSET: f32 = 3.0;
pub(super) const WAVE_GAP: f32 = 2.0;
pub(super) const SPLIT_BELOW: f32 = 40.0;
const NAME_MIN_WIDTH: f32 = 24.0;
const PENDING_WIDTH: f32 = 160.0;
const WARP_TICK: f32 = 7.0;
pub(super) const CLAMP: f32 = 3.0;

pub(super) fn span(view: TimeView, canvas: Rect, clip: &Clip) -> (f32, f32) {
    (
        view.x(canvas.left(), clip.start.as_quarters()),
        view.x(canvas.left(), (clip.start + clip.length).as_quarters()),
    )
}

pub(super) fn span_of(view: TimeView, canvas: Rect, from: Ticks, to: Ticks) -> (f32, f32) {
    (
        view.x(canvas.left(), from.as_quarters()),
        view.x(canvas.left(), to.as_quarters()),
    )
}

pub(super) fn draw_recording_take(painter: &Painter, rect: Rect, lane: Rect, name: &str) {
    painter.rect_filled(rect, theme::radius::SMALL, theme::p().lane);
    let look = ClipLook {
        name,
        takes: None,
        color: theme::p().record,
        state: ClipState::Ready,
        progress: None,
        alpha: 1.0,
        locked: false,
        paint: recording_look::take_paint(theme::p()),
        marks: Marks::default(),
    };
    draw_body(painter, rect, &look);
    draw_name(painter, rect, lane, &look);
}

#[derive(Debug, Default)]
pub(super) struct GpuClips {
    boxes: Arc<Vec<BoxInstance>>,
    waves: Arc<Vec<WaveInstance>>,
    geometry: Vec<Geometry>,
    strips: Strips,
    mini: Arc<Vec<BoxInstance>>,
    mini_tracks: Vec<usize>,
    generation: u64,
    slots: HashMap<usize, (u32, Arc<Peaks>)>,
    next_slot: u32,
    peaks: Arc<Vec<(u32, Arc<Peaks>)>>,
    built: Option<(u64, u64, u32, theme::Palette)>,
    built_recording: Option<RecordingLook>,
    moving: bool,
    built_live: Vec<lumen_project::model::Id>,
    built_others: Vec<usize>,
    live: Arc<Vec<BoxInstance>>,
    live_mini: Arc<Vec<BoxInstance>>,
    live_mini_tracks: Vec<usize>,
    live_generation: u64,
}

impl GpuClips {
    fn waves_for(
        &mut self,
        project: &Project,
        rate: u32,
        lanes: &Lanes,
    ) -> Vec<super::waves::Wave> {
        let mut sources = HashMap::new();
        for item in lanes.library.with_twins() {
            let (State::Ready(audio), Some(peaks)) = (&item.state, &item.peaks) else {
                continue;
            };
            if audio.sample_rate() != rate {
                continue;
            }
            let key = Arc::as_ptr(peaks) as usize;
            let next = &mut self.next_slot;
            let (slot, _) = self.slots.entry(key).or_insert_with(|| {
                *next += 1;
                (*next - 1, Arc::clone(peaks))
            });
            sources.insert(
                item.id,
                Source {
                    slot: *slot,
                    channels: peaks.channels as u32,
                    native_rate: item.info.sample_rate.unwrap_or(audio.sample_rate()),
                },
            );
        }
        self.slots
            .retain(|_, (_, peaks)| Arc::strong_count(peaks) > 1);
        self.peaks = Arc::new(
            self.slots
                .values()
                .map(|(slot, peaks)| (*slot, Arc::clone(peaks)))
                .collect(),
        );
        wave_instances(
            project,
            rate,
            |media| sources.get(&media).copied(),
            |track, clip| theme::wave(clip_color(track, clip)),
        )
    }
}

impl Arrangement {
    pub(super) fn prune_drum_cache(
        &mut self,
        drawn: &std::collections::BTreeMap<Id, (usize, usize, f32)>,
    ) {
        self.drum_cache.retain(|id, _| drawn.contains_key(id));
    }

    pub fn lanes(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        canvas: Rect,
        doc: &Document,
        lanes: &mut Lanes,
        time: TimeView,
    ) {
        let palette = *theme::p();
        let mut drum_cache = std::mem::take(&mut self.drum_cache);
        let now = ctx.input(|i| i.time);
        self.glides.update(
            now,
            (doc.session(), doc.state()),
            doc.project(),
            (self.gesture.is_some(), self.recording.is_some()),
        );
        if self.glides.moving() {
            ctx.request_repaint();
        }
        let preview = self.preview.clone();
        let shown = match &preview {
            Some(preview) => std::borrow::Cow::Borrowed(&**preview),
            None => self.glides.shown(doc.project(), now),
        };
        let project: &Project = &shown;
        let audible = audible(project);
        let all = self.rows(ctx, project, lanes.pending().len());
        self.pinned_lanes(ctx, painter, canvas, project, &all);
        let canvas = Rect::from_x_y_ranges(
            canvas.x_range(),
            self.zone_span(ctx, project, Zone::Main, canvas.y_range()),
        );
        let clipped = painter.with_clip_rect(canvas.intersect(painter.clip_rect()));
        let painter = &clipped;
        let rows: Vec<Row> = all.into_iter().filter(|r| r.zone == Zone::Main).collect();
        let top = self.top;
        let lane_of = |row: &Row| {
            let lane = Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - top),
                vec2(canvas.width(), row.height),
            );
            (lane.bottom() >= canvas.top() && lane.top() <= canvas.bottom()).then_some(lane)
        };

        for row in &rows {
            let (Some(lane), RowKind::Track(index)) = (lane_of(row), row.kind) else {
                continue;
            };
            let track = &project.tracks[index];
            if track.armed {
                let fade = Rect::from_min_size(
                    lane.min,
                    vec2((lane.width() * 0.18).min(220.0), lane.height()),
                );
                painter.rect_filled(
                    lane,
                    0.0,
                    palette.record.gamma_multiply(theme::alpha::WASH_FAINT),
                );
                gradient(
                    painter,
                    fade,
                    palette.record.gamma_multiply(theme::alpha::WASH_SELECTED),
                    Color32::TRANSPARENT,
                );
                let edge = Stroke::new(
                    theme::stroke::HAIRLINE,
                    palette.record.gamma_multiply(theme::alpha::EDGE_SOFT),
                );
                painter.hline(canvas.x_range(), lane.top() + 0.5, edge);
                painter.hline(canvas.x_range(), lane.bottom() - 1.5, edge);
            } else if track.solo {
                painter.rect_filled(
                    lane,
                    0.0,
                    palette.accent.gamma_multiply(theme::alpha::WASH_LANE),
                );
            }
            if track.kind == TrackKind::Group {
                super::nesting::draw_group_lane(painter, lane, canvas, project, index, time);
            }
        }

        let gliding = self.glides.moving();
        self.refresh_boxes(ctx, doc.state(), project, lanes, gliding);
        self.refresh_live(ctx, project, lanes);
        let ppp = ctx.pixels_per_point();
        let mut slots = vec![RowSlot::HIDDEN; project.tracks.len()];
        for row in &rows {
            if let RowKind::Track(index) = row.kind {
                let mut alpha = if audible(&project.tracks[index]) {
                    1.0
                } else {
                    theme::CLIP_DIMMED
                };
                if self.auto.dims(project.tracks[index].id) {
                    alpha *= theme::UNDER_LANE;
                }
                slots[index] = RowSlot::new(row.y * ppp, row.height * ppp, alpha);
            }
        }
        let top = self.top;
        let shown = |r: &Row| r.y - top < canvas.height() && r.y + r.height - top > 0.0;
        let indices = rows
            .iter()
            .filter(|r| shown(r))
            .filter_map(|r| match r.kind {
                RowKind::Track(index) => Some(index as u32),
                RowKind::Pending(_) | RowKind::Master => None,
            });
        let visible = indices
            .clone()
            .min()
            .zip(indices.max())
            .map_or(0..0, |(a, b)| a..b + 1);
        let left_px = f64::from(canvas.left() * ppp);
        let strips = self.gpu.strips.update(
            &self.gpu.geometry,
            visible,
            Pixels {
                left: time.left,
                per_beat: time.zoom * f64::from(ppp),
                origin: left_px - left_px.round(),
                width: f64::from(canvas.width() * ppp),
            },
            ctx,
            |media| match lanes.library.get(media).map(|i| &i.state) {
                Some(State::Ready(audio)) => Some(Arc::clone(audio)),
                _ => None,
            },
        );
        let mini_rows = super::mini_notes::rows(&self.gpu.mini_tracks, &slots, ppp);
        let live_mini_rows = super::mini_notes::rows(&self.gpu.live_mini_tracks, &slots, ppp);
        painter.add(canvas::clips_shape(
            canvas,
            ppp,
            ClipFrame {
                strips,
                boxes: Arc::clone(&self.gpu.boxes),
                waves: Arc::clone(&self.gpu.waves),
                peaks: Arc::clone(&self.gpu.peaks),
                generation: self.gpu.generation,
                rows: slots,
                time,
                top: self.top,
                style: canvas::box_style(ppp, INSET, TITLE),
                mini: Arc::clone(&self.gpu.mini),
                mini_rows,
                mini_style: canvas::mini_style(ppp),
                live: Arc::clone(&self.gpu.live),
                live_mini: Arc::clone(&self.gpu.live_mini),
                live_mini_rows,
                live_generation: self.gpu.live_generation,
                wave_style: WaveStyle {
                    top: (INSET + TITLE + WAVE_GAP) * ppp,
                    bottom: (INSET + WAVE_GAP) * ppp,
                    side: ppp,
                    split_below: SPLIT_BELOW * ppp,
                },
            },
        ));

        let mut drum_pictures = std::collections::BTreeMap::new();
        for row in &rows {
            let Some(lane) = lane_of(row) else {
                continue;
            };
            match row.kind {
                RowKind::Track(index) => {
                    let track = &project.tracks[index];
                    let heard = audible(track);
                    for clip in &track.clips {
                        let (x0, x1) = span(time, canvas, clip);
                        if x1 < canvas.left() || x0 > canvas.right() {
                            continue;
                        }
                        let rect = Rect::from_x_y_ranges(
                            x0..=x1,
                            lane.top() + INSET..=lane.bottom() - INSET,
                        );
                        let wash = self.glides.wash(clip.id, now);
                        if wash > 0.0 {
                            painter.rect_filled(
                                rect,
                                theme::radius::SMALL,
                                palette.accent.gamma_multiply(wash),
                            );
                        }
                        if let Some(warp) = &clip.warp {
                            let ticks = super::warp_ticks::xs(
                                time,
                                canvas.left(),
                                &super::warp_ticks::Map {
                                    origin: lumen_project::edit::warp::origin(clip),
                                    markers: &warp.markers,
                                    period: clip.loop_length,
                                    clip: (x0, x1),
                                },
                                (canvas.left(), canvas.right()),
                            );
                            let stroke = Stroke::new(
                                theme::stroke::HAIRLINE,
                                theme::CLIP_TEXT.gamma_multiply(if heard {
                                    theme::alpha::INK_DIM
                                } else {
                                    theme::alpha::INK_TRACE
                                }),
                            );
                            let top = rect.top() + TITLE.min(rect.height());
                            let bottom = (top + WARP_TICK).min(rect.bottom());
                            let lane_clip =
                                painter.with_clip_rect(lane.intersect(painter.clip_rect()));
                            for x in ticks {
                                lane_clip.vline(x, top..=bottom, stroke);
                            }
                        }
                        if x1 - x0 < NAME_MIN_WIDTH {
                            continue;
                        }
                        if let Some(map) = project.drum_map_of(track) {
                            let body = Rect::from_min_max(
                                pos2(x0 + INSET, rect.top() + TITLE + WAVE_GAP),
                                pos2(x1 - INSET, rect.bottom() - WAVE_GAP),
                            );
                            let dim = if heard { 1.0 } else { theme::CLIP_DIMMED };
                            let picture = drum_cache.entry(clip.id).or_default();
                            let drawn = crate::drum_editor::arrangement::paint(
                                painter,
                                crate::drum_editor::arrangement::MapKey {
                                    map,
                                    revision: doc.state(),
                                },
                                clip,
                                body,
                                time,
                                dim,
                                picture,
                            );
                            drum_pictures.insert(clip.id, drawn);
                        }
                        let paint = recording_look::paint(
                            self.recording.as_ref(),
                            track.id,
                            clip,
                            &palette,
                        );
                        let mut look = look_of(clip, track, paint, lanes, ctx);
                        if !heard {
                            look.alpha *= theme::CLIP_DIMMED;
                        }
                        if self.auto.dims(track.id) {
                            look.alpha *= theme::UNDER_LANE;
                        }
                        if row.clipped {
                            draw_name(
                                &painter.with_clip_rect(lane.intersect(painter.clip_rect())),
                                rect,
                                lane,
                                &look,
                            );
                        } else {
                            draw_name(painter, rect, lane, &look);
                        }
                    }
                    super::audio_take_look::draw(
                        painter,
                        &self.audio_takes,
                        track.id,
                        lane,
                        time,
                        canvas,
                    );
                    let landing: Vec<Landing> = lanes
                        .landing()
                        .iter()
                        .filter(|l| l.track == track.id)
                        .copied()
                        .collect();
                    for (k, Landing { id, at, .. }) in landing.into_iter().enumerate() {
                        let x0 = time.x(canvas.left(), at.as_quarters())
                            + k as f32 * (PENDING_WIDTH + INSET);
                        let rect = Rect::from_x_y_ranges(
                            x0..=x0 + PENDING_WIDTH,
                            lane.top() + INSET..=lane.bottom() - INSET,
                        );
                        let name = lanes
                            .library
                            .get(id)
                            .map_or_else(String::new, |i| i.name.clone());
                        let look = ClipLook {
                            name: &name,
                            takes: None,
                            color: color_of(track.color),
                            state: lanes.library.get(id).map_or(ClipState::Loading, item_state),
                            progress: lanes.library.progress(id),
                            alpha: lanes.shown(id, ctx),
                            locked: false,
                            paint: Paint::default(),
                            marks: Marks::default(),
                        };
                        draw_body(painter, rect, &look);
                        draw_name(painter, rect, lane, &look);
                    }
                }
                RowKind::Pending(i) => {
                    let Some(&(id, at)) = lanes.pending().get(i) else {
                        continue;
                    };
                    let x0 = time.x(canvas.left(), at.as_quarters());
                    let rect = Rect::from_x_y_ranges(
                        x0..=x0 + PENDING_WIDTH,
                        lane.top() + INSET..=lane.bottom() - INSET,
                    );
                    let name = lanes
                        .library
                        .get(id)
                        .map_or_else(String::new, |i| i.name.clone());
                    let state = lanes.library.get(id).map_or(ClipState::Loading, item_state);
                    let progress = lanes.library.progress(id);
                    let shown = lanes.shown(id, ctx);
                    let color =
                        theme::CLIP_COLORS[(project.tracks.len() + i) % theme::CLIP_COLORS.len()];
                    let look = ClipLook {
                        name: &name,
                        takes: None,
                        color,
                        state,
                        progress,
                        alpha: shown,
                        locked: false,
                        paint: Paint::default(),
                        marks: Marks::default(),
                    };
                    draw_body(painter, rect, &look);
                    draw_name(painter, rect, lane, &look);
                }
                RowKind::Master => {}
            }
            painter.hline(
                canvas.x_range(),
                lane.bottom() - 0.5,
                Stroke::new(
                    theme::stroke::HAIRLINE,
                    palette.border.gamma_multiply(theme::alpha::EDGE_DIM),
                ),
            );
            if let (RowKind::Track(index), Some((hot, t))) = (row.kind, self.hot_edge)
                && project.tracks[index].id == hot
            {
                draw_edge_glow(painter, canvas.x_range(), lane.bottom(), t);
            }
        }
        self.prune_drum_cache(&drum_pictures);
        self.drum_pictures = drum_pictures;
        self.drum_cache = drum_cache;
        self.draw_drop_line(painter, canvas, false);
        self.draw_selection(painter, canvas, project, &rows, time);
        for row in &rows {
            if let (RowKind::Track(index), Some(lane)) = (row.kind, lane_of(row)) {
                self.browser_drop_target(ctx, painter, lane, project.tracks[index].id);
            }
        }
    }

    fn refresh_boxes(
        &mut self,
        ctx: &egui::Context,
        state: u64,
        project: &Project,
        lanes: &mut Lanes,
        gliding: bool,
    ) {
        let palette = *theme::p();
        let rate = lanes.rate();
        let key = (state, lanes.revision(), rate, palette);
        let recording = self.recording.as_ref();
        let live = self.live_tracks();
        let others: Vec<usize> = if live.is_empty() {
            Vec::new()
        } else {
            project
                .tracks
                .iter()
                .filter(|t| !live.contains(&t.id))
                .map(|t| Arc::as_ptr(t) as usize)
                .collect()
        };
        let same = if live.is_empty() {
            self.gpu.built == Some(key) && self.gpu.built_recording.as_ref() == recording
        } else {
            self.gpu
                .built
                .map(|(_, revision, rate, palette)| (revision, rate, palette))
                == Some((key.1, key.2, key.3))
                && self.gpu.built_others == others
        };
        if same && self.gpu.built_live == live && !self.gpu.moving && !gliding {
            return;
        }
        let recording = recording.cloned();
        let mut moving = lanes.library.is_loading() || gliding;
        let count = project.tracks.iter().map(|t| t.clips.len()).sum();
        let mut boxes = Vec::with_capacity(count);
        for (row, track) in project.tracks.iter().enumerate() {
            if live.contains(&track.id) {
                continue;
            }
            for clip in &track.clips {
                let paint = recording_look::paint(recording.as_ref(), track.id, clip, &palette);
                let look = look_of(clip, track, paint, lanes, ctx);
                moving |= look.alpha < 1.0;
                boxes.push(clip_box(clip, row as u32, &look, &palette));
            }
        }
        let waves = self.gpu.waves_for(project, rate, lanes);
        let gpu = &mut self.gpu;
        gpu.boxes = Arc::new(boxes);
        gpu.geometry = waves.iter().map(|w| w.geometry).collect();
        gpu.waves = Arc::new(waves.into_iter().map(|w| w.instance).collect());
        gpu.strips.reset();
        let mini = super::mini_notes::build(project, recording.as_ref(), |t| !live.contains(&t.id));
        gpu.mini = Arc::new(mini.boxes);
        gpu.mini_tracks = mini.tracks;
        gpu.generation += 1;
        gpu.built = Some(key);
        gpu.built_recording = recording;
        gpu.built_live = live;
        gpu.built_others = others;
        gpu.moving = moving;
    }

    fn live_tracks(&self) -> Vec<lumen_project::model::Id> {
        match (&self.preview, &self.recording) {
            (Some(_), Some(look)) => look.tracks.clone(),
            _ => Vec::new(),
        }
    }

    fn refresh_live(&mut self, ctx: &egui::Context, project: &Project, lanes: &mut Lanes) {
        let live = self.live_tracks();
        if live.is_empty() {
            if !self.gpu.live.is_empty() || !self.gpu.live_mini.is_empty() {
                self.gpu.live = Arc::new(Vec::new());
                self.gpu.live_mini = Arc::new(Vec::new());
                self.gpu.live_mini_tracks.clear();
                self.gpu.live_generation += 1;
            }
            return;
        }
        let palette = *theme::p();
        let recording = self.recording.as_ref();
        let mut boxes = Vec::new();
        for (row, track) in project.tracks.iter().enumerate() {
            if !live.contains(&track.id) {
                continue;
            }
            for clip in &track.clips {
                let paint = recording_look::paint(recording, track.id, clip, &palette);
                let look = look_of(clip, track, paint, lanes, ctx);
                boxes.push(clip_box(clip, row as u32, &look, &palette));
            }
        }
        let mini = super::mini_notes::build(project, recording, |t| live.contains(&t.id));
        let gpu = &mut self.gpu;
        gpu.live = Arc::new(boxes);
        gpu.live_mini = Arc::new(mini.boxes);
        gpu.live_mini_tracks = mini.tracks;
        gpu.live_generation += 1;
    }

    pub fn item_at<'a>(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        lanes: &'a Lanes,
        time: TimeView,
    ) -> Option<(&'a Item, Marks)> {
        let zone = self.zone_at(ctx, project, pointer.y - canvas.top())?;
        let rows = self.rows(ctx, project, lanes.pending().len());
        let row = rows.iter().filter(|r| r.zone == zone).find(|r| {
            let y = canvas.top() + r.y - self.top;
            (y..y + r.height).contains(&pointer.y)
        })?;
        match row.kind {
            RowKind::Track(index) => project.tracks[index].clips.iter().find_map(|clip| {
                let (x0, x1) = span(time, canvas, clip);
                match &clip.content {
                    ClipContent::Audio { media, .. } if (x0..=x1).contains(&pointer.x) => lanes
                        .library
                        .get(media.0)
                        .map(|item| (item, Marks::of(clip))),
                    _ => None,
                }
            }),
            RowKind::Pending(i) => {
                let &(id, at) = lanes.pending().get(i)?;
                let x0 = time.x(canvas.left(), at.as_quarters());
                (x0..=x0 + PENDING_WIDTH)
                    .contains(&pointer.x)
                    .then(|| lanes.library.get(id).map(|item| (item, Marks::default())))
                    .flatten()
            }
            RowKind::Master => None,
        }
    }
}
