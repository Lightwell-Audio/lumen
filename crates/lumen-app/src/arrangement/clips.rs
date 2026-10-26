//! The lanes and their clips (M3 slice 4): dark clips with a
//! track-coloured title bar, the armed and soloed lane hints, dimmed clips
//! on tracks that cannot be heard, and the hover lookup for the file card.
//!
//! Clip bodies are drawn on the GPU (slice 4b, ADR-0026). The boxes are
//! uploaded only when the project, the media or the theme change; where
//! rows are (scrolling, folding, resizing) goes up as a small table each
//! frame. Names, and the few clips still loading for import, are egui text
//! and shapes, drawn only where they are visible. The selection is drawn
//! by `selection_look`.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_media::Peaks;
use lumen_project::model::{Clip, ClipContent, Project, TrackKind};
use lumen_render::{BoxInstance, RowSlot, WaveInstance, WaveStyle, beats, rgba};

use super::recording_look::{self, Paint, RecordingLook};
use super::samples::{Geometry, Pixels, Strips};
use super::waves::{Source, wave_instances};
use super::{Arrangement, Row, RowKind, TITLE, Zone, clip_color, color_of};
use crate::canvas::{self, ClipFrame};
use crate::document::{Document, audible};
use crate::lanes::{Landing, Lanes};
use crate::media::{Item, State};
use crate::timeview::TimeView;
use crate::widgets::{draw_edge_glow, gradient};
use crate::{icons, theme};

/// Space between a lane's edges and its clips.
pub(super) const INSET: f32 = 3.0;
/// The opacity of clips on tracks that cannot be heard.
const DIMMED: f32 = 0.42;
/// How much of a clip shows through the automation lane over it (M7 slice
/// A1, the owner's L1 B: the clips step back while a lane is shown).
const UNDER_LANE: f32 = 0.45;
/// Space between the title bar and the waveform, and below it.
pub(super) const WAVE_GAP: f32 = 2.0;
/// Below this height (in points), stereo is drawn as one waveform.
pub(super) const SPLIT_BELOW: f32 = 40.0;
/// Clips narrower than this show no name: a letter or two is noise, and
/// at wide zooms thousands of names cost more than the clips.
const NAME_MIN_WIDTH: f32 = 24.0;
/// Width of a clip still loading for import, before it has a length.
const PENDING_WIDTH: f32 = 160.0;
/// How opaque a clip's body is: the grid shows faintly through it
/// (the owner's G1, MT-APP-14 run 2). The title bar and waveform
/// stay solid.
const BODY_OPACITY: f32 = 0.55;
/// A locked clip's padlock, and the narrowest title that shows it.
const LOCK_ICON: f32 = 9.0;
const LOCK_MIN_WIDTH: f32 = 30.0;
/// A clip's title bar shows which take plays from this width.
const TAKES_MIN_WIDTH: f32 = 72.0;
/// A locked clip's clamps: bars this wide at both ends (the owner's K3).
pub(super) const CLAMP: f32 = 3.0;
/// The "REC" tag's dot: its radius, and the room it takes before the name.
const TAG_DOT: f32 = 3.0;
const TAG_ROOM: f32 = 11.0;

/// A clip's left and right edges in `view`.
pub(super) fn span(view: TimeView, canvas: Rect, clip: &Clip) -> (f32, f32) {
    (
        view.x(canvas.left(), clip.start.as_quarters()),
        view.x(canvas.left(), (clip.start + clip.length).as_quarters()),
    )
}

/// The x range of a span of the timeline.
pub(super) fn span_of(view: TimeView, canvas: Rect, from: Ticks, to: Ticks) -> (f32, f32) {
    (
        view.x(canvas.left(), from.as_quarters()),
        view.x(canvas.left(), to.as_quarters()),
    )
}

/// The body and title of an audio take being recorded (M9 slice R2), in
/// record red; its waveform is drawn over it by `audio_take_look`. The body is
/// a tint, so an opaque base goes under it: a clip the take is recording over
/// is hidden exactly as far as the take has gone, however fast it grows.
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
    };
    draw_body(painter, rect, &look);
    draw_name(painter, rect, lane, &look);
}

/// The clip bodies and waveforms on the GPU, and what they were built
/// from.
#[derive(Debug, Default)]
pub(super) struct GpuClips {
    boxes: Arc<Vec<BoxInstance>>,
    waves: Arc<Vec<WaveInstance>>,
    /// Each wave's exact place, and the samples of those drawn sample by
    /// sample (slice 4d).
    geometry: Vec<Geometry>,
    strips: Strips,
    /// The notes inside MIDI clips, and the tracks their bands belong to.
    mini: Arc<Vec<BoxInstance>>,
    mini_tracks: Vec<usize>,
    generation: u64,
    /// Peak slots by the peaks they hold (files with the same peaks share
    /// one), and the list the GPU side uploads from.
    slots: HashMap<usize, (u32, Arc<Peaks>)>,
    next_slot: u32,
    peaks: Arc<Vec<(u32, Arc<Peaks>)>>,
    /// The document state, media revision, device rate and palette the
    /// boxes show.
    built: Option<(u64, u64, u32, theme::Palette)>,
    /// The recording the boxes show (none while nothing records, so the
    /// check costs nothing then).
    built_recording: Option<RecordingLook>,
    /// A clip is fading in or loading: rebuild next frame too.
    moving: bool,
    /// The tracks left out of the boxes above because they are being
    /// recorded: their clips and notes are `live` and `live_mini`, rebuilt
    /// every frame from the take as far as it has gone (after MT-MIDI-7).
    built_live: Vec<lumen_project::model::Id>,
    /// While recording: the other tracks the boxes were built from, by
    /// identity.
    built_others: Vec<usize>,
    live: Arc<Vec<BoxInstance>>,
    live_mini: Arc<Vec<BoxInstance>>,
    live_mini_tracks: Vec<usize>,
    live_generation: u64,
}

impl GpuClips {
    /// The waves of every audio clip whose peaks are ready at `rate`, giving
    /// each set of peaks a slot (shared peaks share one).
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
            if audio.sample_rate != rate {
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
                    native_rate: item.info.sample_rate.unwrap_or(audio.sample_rate),
                },
            );
        }
        // Peaks no file holds any more (replaced after a rate change).
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
    /// The lanes and clips in `canvas`.
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
        // Clips moved by an edit are drawn on their way (M4.5, M1).
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
        // While recording, the take as far as it has gone (nothing glides
        // then).
        let preview = self.preview.clone();
        let shown = match &preview {
            Some(preview) => std::borrow::Cow::Borrowed(&**preview),
            None => self.glides.shown(doc.project(), now),
        };
        let project: &Project = &shown;
        let audible = audible(project);
        // The returns and master pinned at the bottom (slice 4) draw their
        // own lanes; everything below works in the scrolling tracks' zone.
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

        // Lane hints, under the clips.
        for row in &rows {
            let (Some(lane), RowKind::Track(index)) = (lane_of(row), row.kind) else {
                continue;
            };
            let track = &project.tracks[index];
            if track.armed {
                // A red wash, strongest at the left, with thin red edges
                // (owner's choice: "glow and breathe").
                let fade = Rect::from_min_size(
                    lane.min,
                    vec2((lane.width() * 0.18).min(220.0), lane.height()),
                );
                painter.rect_filled(lane, 0.0, palette.record.gamma_multiply(0.05));
                gradient(
                    painter,
                    fade,
                    palette.record.gamma_multiply(0.12),
                    Color32::TRANSPARENT,
                );
                let edge = Stroke::new(1.0, palette.record.gamma_multiply(0.45));
                painter.hline(canvas.x_range(), lane.top() + 0.5, edge);
                painter.hline(canvas.x_range(), lane.bottom() - 1.5, edge);
            } else if track.solo {
                painter.rect_filled(lane, 0.0, palette.accent.gamma_multiply(0.06));
            }
            if track.kind == TrackKind::Group {
                super::nesting::draw_group_lane(painter, lane, canvas, project, index, time);
            }
        }

        // Clip bodies, on the GPU.
        let gliding = self.glides.moving();
        self.refresh_boxes(ctx, doc.state(), project, lanes, gliding);
        self.refresh_live(ctx, project, lanes);
        let ppp = ctx.pixels_per_point();
        // One slot per track, by its index (boxes and waves name their
        // track's index); tracks hidden in a folded group draw nothing.
        let mut slots = vec![RowSlot::HIDDEN; project.tracks.len()];
        for row in &rows {
            if let RowKind::Track(index) = row.kind {
                let mut alpha = if audible(&project.tracks[index]) {
                    1.0
                } else {
                    DIMMED
                };
                if self.auto.dims(project.tracks[index].id) {
                    alpha *= UNDER_LANE;
                }
                slots[index] = RowSlot::new(row.y * ppp, row.height * ppp, alpha);
            }
        }
        // Deep zoom: the visible samples of waves past the peaks' detail,
        // between the first and last tracks in view.
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

        // Names; clips loading for import; lane edges.
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
                        // What undo or redo changed, washed in the accent
                        // (M4.5, M4).
                        let wash = self.glides.wash(clip.id, now);
                        if wash > 0.0 {
                            painter.rect_filled(
                                rect,
                                theme::radius::SMALL,
                                palette.accent.gamma_multiply(wash),
                            );
                        }
                        if x1 - x0 < NAME_MIN_WIDTH {
                            continue;
                        }
                        let paint = recording_look::paint(
                            self.recording.as_ref(),
                            track.id,
                            clip,
                            &palette,
                        );
                        let mut look = look_of(clip, track, paint, lanes, ctx);
                        if !heard {
                            look.alpha *= DIMMED;
                        }
                        if self.auto.dims(track.id) {
                            look.alpha *= UNDER_LANE;
                        }
                        if row.clipped {
                            // Folding shut: the name stays inside the lane.
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
                    // Files dropped on this track, while they load (slice
                    // 6e), side by side from where they were dropped.
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
                    };
                    draw_body(painter, rect, &look);
                    draw_name(painter, rect, lane, &look);
                }
                RowKind::Master => {}
            }
            painter.hline(
                canvas.x_range(),
                lane.bottom() - 0.5,
                Stroke::new(1.0, palette.border.gamma_multiply(0.6)),
            );
            // The resize glow continues across the lane.
            if let (RowKind::Track(index), Some((hot, t))) = (row.kind, self.hot_edge)
                && project.tracks[index].id == hot
            {
                draw_edge_glow(painter, canvas.x_range(), lane.bottom(), t);
            }
        }
        // The drop line of a track drag continues across the lanes.
        self.draw_drop_line(painter, canvas, false);
        self.draw_selection(painter, canvas, project, &rows, time);
        // A device from the browser, over a track's lane.
        for row in &rows {
            if let (RowKind::Track(index), Some(lane)) = (row.kind, lane_of(row)) {
                self.browser_drop_target(ctx, painter, lane, project.tracks[index].id);
            }
        }
    }

    /// Rebuilds the GPU boxes if the project, the media or the theme
    /// changed, or a clip is still fading in, loading or gliding.
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
        // While a take grows, commits change only the tracks being recorded,
        // which these boxes leave out: the others are compared by identity
        // instead of the document's state, so a commit rebuilds nothing here.
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
        // Any file loading (an import, or an opened project's media) has a
        // progress fill to keep moving.
        // A glide rebuilds once more after it lands.
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

    /// The tracks drawn on the live layers: the ones being recorded, while
    /// the take is drawn as it grows (none otherwise).
    fn live_tracks(&self) -> Vec<lumen_project::model::Id> {
        match (&self.preview, &self.recording) {
            (Some(_), Some(look)) => look.tracks.clone(),
            _ => Vec::new(),
        }
    }

    /// Rebuilds the live layers: every frame while a take grows, from the
    /// tracks being recorded only, so the cost is theirs, not the
    /// project's. Emptied once when it ends.
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

    /// The file of the audio clip under `pointer`, for the hover card.
    pub fn item_at<'a>(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        lanes: &'a Lanes,
        time: TimeView,
    ) -> Option<&'a Item> {
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
                    ClipContent::Audio { media, .. } if (x0..=x1).contains(&pointer.x) => {
                        lanes.library.get(media.0)
                    }
                    _ => None,
                }
            }),
            RowKind::Pending(i) => {
                let &(id, at) = lanes.pending().get(i)?;
                let x0 = time.x(canvas.left(), at.as_quarters());
                (x0..=x0 + PENDING_WIDTH)
                    .contains(&pointer.x)
                    .then(|| lanes.library.get(id))
                    .flatten()
            }
            RowKind::Master => None,
        }
    }
}

/// What a clip's audio is doing, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClipState {
    Ready,
    Loading,
    Failed,
    Missing,
}

fn item_state(item: &Item) -> ClipState {
    match item.state {
        State::Ready(_) => ClipState::Ready,
        State::Loading => ClipState::Loading,
        State::Failed(_) => ClipState::Failed,
    }
}

/// How a clip looks: its name, track colour and load state, and `alpha`,
/// which fades it in and dims it when the track cannot be heard.
struct ClipLook<'a> {
    name: &'a str,
    /// The take playing and how many there are (counted from 1), for a clip
    /// that keeps the passes of a loop recording.
    takes: Option<(u32, u32)>,
    color: Color32,
    state: ClipState,
    progress: Option<f32>,
    alpha: f32,
    /// Locked (the owner's K3, slice 6e): a padlock ends the title bar.
    locked: bool,
    /// What a recording gives the clip (3-A): none for a clip not recording.
    paint: Paint,
}

/// A project clip's look on `track`, before dimming for its track.
fn look_of<'a>(
    clip: &'a Clip,
    track: &lumen_project::model::Track,
    paint: Paint,
    lanes: &mut Lanes,
    ctx: &egui::Context,
) -> ClipLook<'a> {
    let (state, progress, key) = match &clip.content {
        ClipContent::Audio { media, .. } => (
            lanes
                .library
                .get(media.0)
                .map_or(ClipState::Missing, item_state),
            lanes.library.progress(media.0),
            media.0,
        ),
        ClipContent::Midi { .. } => (ClipState::Ready, None, clip.id.0),
    };
    ClipLook {
        name: &clip.name,
        takes: clip.takes.as_ref().map(|t| (t.active + 1, t.count)),
        color: clip_color(track, clip),
        state,
        progress,
        alpha: lanes.shown(key, ctx),
        // A frozen track's clips cannot change, so they show the padlock too.
        locked: clip.locked || track.frozen.is_some(),
        paint,
    }
}

/// A clip body's colour at `alpha`: the clip body at the grid's opacity,
/// unless a recording gives it a colour (the record red take).
fn body_color(paint: &Paint, palette: &theme::Palette, alpha: f32) -> Color32 {
    match paint.body {
        Some(color) => color.gamma_multiply(alpha),
        None => palette.clip_body.gamma_multiply(BODY_OPACITY * alpha),
    }
}

/// A clip's outline at `alpha`: the track colour at 60%, unless a recording
/// gives it the record red.
fn outline_color(paint: &Paint, track: Color32, alpha: f32) -> Color32 {
    match paint.outline {
        Some(color) => color.gamma_multiply(alpha),
        None => track.gamma_multiply(0.6 * alpha),
    }
}

/// The title bar's colour and text for a clip in `state`.
fn bar_and_label(look: &ClipLook<'_>, palette: &theme::Palette) -> (Color32, String) {
    let name = look.name;
    match look.state {
        ClipState::Ready => (look.color, name.to_owned()),
        ClipState::Loading => (
            look.color.gamma_multiply(0.6),
            format!(
                "{name} \u{00B7} loading {:.0}%",
                look.progress.unwrap_or(0.0) * 100.0
            ),
        ),
        ClipState::Failed => (palette.record, format!("{name} \u{00B7} could not load")),
        ClipState::Missing => (palette.record, format!("{name} \u{00B7} file missing")),
    }
}

/// A clip's body for the GPU: the same colours as [`draw_body`].
fn clip_box(clip: &Clip, row: u32, look: &ClipLook<'_>, palette: &theme::Palette) -> BoxInstance {
    let straight = |c: Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        rgba(r, g, b, a)
    };
    let a = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    BoxInstance {
        start: beats(clip.start.as_quarters()),
        end: beats((clip.start + clip.length).as_quarters()),
        row,
        body: straight(body_color(&look.paint, palette, a)),
        bar: straight(bar.gamma_multiply(a)),
        border: straight(outline_color(&look.paint, look.color, a)),
        fill: straight(look.color.gamma_multiply(0.25 * a)),
        progress: match look.state {
            ClipState::Loading => look.progress.unwrap_or(0.0),
            _ => 0.0,
        },
    }
}

/// A clip body drawn by egui: dark body, track-coloured border and title
/// bar. Only clips still loading for import use it; the rest are on the
/// GPU ([`clip_box`]).
fn draw_body(painter: &Painter, rect: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let radius = theme::radius::SMALL;
    let alpha = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    painter.rect_filled(rect, radius, body_color(&look.paint, palette, alpha));
    if let (ClipState::Loading, Some(p)) = (look.state, look.progress) {
        let done = Rect::from_min_size(rect.min, vec2(rect.width() * p, rect.height()));
        painter.rect_filled(done, radius, look.color.gamma_multiply(0.25 * alpha));
    }
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    painter.rect_filled(
        title,
        egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        bar.gamma_multiply(alpha),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, outline_color(&look.paint, look.color, alpha)),
        egui::StrokeKind::Inside,
    );
}

/// A clip's name in its title bar, kept within the visible part of the
/// lane.
fn draw_name(painter: &Painter, rect: Rect, lane: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    let visible = title.intersect(lane);
    if visible.width() <= 8.0 {
        return;
    }
    let (_, label) = bar_and_label(look, palette);
    let alpha = look.alpha;
    let mut room = visible.shrink2(vec2(4.0, 0.0));
    // A recording's tag: a drawn dot (the UI font has no "●") and "REC" in
    // front of the name, so the state never rests on colour alone.
    let (label, text_left) = if look.paint.tag {
        painter.circle_filled(
            pos2(visible.left() + 5.0 + TAG_DOT, title.center().y),
            TAG_DOT,
            theme::CLIP_TEXT.gamma_multiply(alpha),
        );
        (
            format!("REC \u{00B7} {label}"),
            visible.left() + 5.0 + TAG_ROOM,
        )
    } else {
        (label, visible.left() + 5.0)
    };
    if look.locked && visible.width() >= LOCK_MIN_WIDTH {
        let side = LOCK_ICON;
        let at = Rect::from_center_size(
            pos2(visible.right() - 4.0 - side / 2.0, title.center().y),
            vec2(side, side),
        );
        painter.extend(icons::shapes(
            icons::Icon::Lock,
            at,
            theme::CLIP_TEXT.gamma_multiply(0.9 * alpha),
        ));
        room.max.x = at.left() - 3.0;
    }
    let ink = if alpha < 0.9 {
        palette.text_secondary.gamma_multiply(alpha.max(0.6))
    } else {
        theme::CLIP_TEXT.gamma_multiply(alpha)
    };
    // Which take plays, as "2/5" at the right end of the bar.
    if let Some((now, all)) = look.takes
        && visible.width() >= TAKES_MIN_WIDTH
    {
        let galley = painter.layout_no_wrap(
            format!("{now}/{all}"),
            FontId::proportional(theme::size::CAPTION),
            ink,
        );
        let width = galley.size().x;
        let right = room.max.x;
        painter.with_clip_rect(room).galley(
            pos2(right - width, title.center().y - galley.size().y / 2.0),
            galley,
            ink,
        );
        room.max.x = right - width - 4.0;
    }
    painter.with_clip_rect(room).text(
        pos2(text_left, title.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::size::CAPTION),
        // Dimmed clips keep a readable name in both themes.
        ink,
    );
}
