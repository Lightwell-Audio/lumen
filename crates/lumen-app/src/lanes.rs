//! The project's tracks on the simple timeline, one lane each (M3 slice 3;
//! track headers and the arrangement at scale arrive in slice 4).
//!
//! Dropped files load in the background as placeholders that are not part
//! of the project. A file joins the project, as one undo step, once it has
//! loaded and its exact length is known; a failed or cancelled load never
//! touches it. The engine's clip list is derived from the project after
//! every change, undo and redo.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Painter, Pos2, Rect, RichText, Stroke, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipsHandle};
use lumen_project::model::{ClipContent, Id, Project};

use crate::document::{Document, Imported};
use crate::media::{self, Item, Library, State};
use crate::theme;

pub const LANE_HEIGHT: f32 = 56.0;
const LANE_GAP: f32 = 4.0;

#[derive(Default)]
pub struct Lanes {
    pub library: Library,
    /// Files loading for import: library id and where they were dropped.
    pending: Vec<(u64, Ticks)>,
    /// The project state the engine's clip list was built from.
    built: Option<u64>,
    /// The engine's clip list needs rebuilding.
    dirty: bool,
    /// A message about files that could not be added.
    notice: Option<String>,
    /// When each clip was first drawn (egui time), for its fade-in.
    seen: HashMap<u64, f64>,
}

/// One clip as drawn.
struct Drawn<'a> {
    rect: Rect,
    item: Option<&'a Item>,
    name: String,
    color: Color32,
    /// Identity for the fade-in: the media id for audio, so a loading
    /// placeholder that becomes its clip does not fade in twice.
    key: u64,
}

fn color_of(c: lumen_project::model::Color) -> Color32 {
    Color32::from_rgb((c.0 >> 16) as u8, (c.0 >> 8) as u8, c.0 as u8)
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Lanes {
    pub fn is_empty(&self, project: &Project) -> bool {
        project.tracks.is_empty() && self.pending.is_empty()
    }

    /// Starts loading each audio file, to be added at `at`; anything else is
    /// reported, not added.
    pub fn add_files(
        &mut self,
        paths: Vec<PathBuf>,
        at: Ticks,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
    ) {
        let mut rejected = Vec::new();
        for path in paths {
            if media::is_audio_file(&path) {
                let id = doc.fresh_id().0;
                self.library.import_as(id, path, rate, ctx);
                self.pending.push((id, at));
            } else {
                rejected.push(
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                );
            }
        }
        if !rejected.is_empty() {
            self.notice = Some(format!(
                "Not added: {}. Lumen imports WAV, AIFF, FLAC, MP3 and Ogg Vorbis files",
                rejected.join(", ")
            ));
        }
    }

    /// Applies finished loads (adding them to the project), loads media the
    /// project needs, and keeps the engine's clips in step. Call every
    /// frame.
    pub fn update(
        &mut self,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
        clips: &mut ClipsHandle,
    ) {
        self.dirty |= self.library.update(rate, ctx);

        // Loads that finished become tracks, in drop order, as one step.
        let mut imported = Vec::new();
        let library = &self.library;
        self.pending.retain(|&(id, at)| match library.get(id) {
            Some(item) => match (&item.state, item.info.frames, item.info.sample_rate) {
                (State::Ready(_), Some(frames), Some(sample_rate)) => {
                    imported.push(Imported {
                        media: Id(id),
                        path: item.path.clone(),
                        name: item.name.clone(),
                        sample_rate,
                        channels: item.info.channels.unwrap_or(2) as u16,
                        frames,
                        size_bytes: item.info.size_bytes,
                        at,
                    });
                    false
                }
                (State::Failed(_), ..) => false,
                _ => true,
            },
            // Cancelled.
            None => false,
        });
        doc.import(imported);

        // Media the project refers to but that is not loaded (an opened
        // project, slice 3c).
        for media in &doc.project().media {
            if self.library.get(media.id.0).is_none()
                && let Some(path) = doc.media_files.get(&media.id)
            {
                self.library.import_as(media.id.0, path.clone(), rate, ctx);
            }
        }

        if self.dirty || self.built != Some(doc.state()) {
            // A full queue means the engine has not caught up; retry next frame.
            let list = self.engine_clips(doc.project(), rate);
            self.dirty = clips.set(list).is_err();
            self.built = Some(doc.state());
        }
    }

    /// What the engine plays: every unmuted audio clip whose file is loaded,
    /// for the shorter of the clip and the file.
    fn engine_clips(&self, project: &Project, rate: u32) -> Vec<Clip> {
        let tempo = lumen_core::time::TempoMap::new(project.tempo.clone()).ok();
        let mut list = Vec::new();
        for track in project.tracks.iter().filter(|t| !t.mute) {
            for clip in track.clips.iter().filter(|c| !c.muted) {
                let ClipContent::Audio {
                    media,
                    offset,
                    gain_db,
                } = &clip.content
                else {
                    continue;
                };
                let Some(item) = self.library.get(media.0) else {
                    continue;
                };
                let State::Ready(audio) = &item.state else {
                    continue;
                };
                // The offset is in the file's frames; the audio is at the
                // device rate.
                let native = item.info.sample_rate.unwrap_or(audio.sample_rate).max(1);
                let offset =
                    (*offset as f64 * f64::from(audio.sample_rate) / f64::from(native)) as i64;
                let region = tempo.as_ref().map_or(i64::MAX, |t| {
                    t.sample_at(clip.start + clip.length, rate) - t.sample_at(clip.start, rate)
                });
                let length = (audio.frames() as i64 - offset).min(region);
                if length > 0 {
                    list.push(Clip {
                        start: clip.start,
                        offset,
                        length,
                        gain: db_to_gain(*gain_db),
                        audio: audio.clone(),
                    });
                }
            }
        }
        list
    }

    fn lane_rect(canvas: Rect, index: usize) -> Rect {
        let top = canvas.top() + LANE_GAP + index as f32 * (LANE_HEIGHT + LANE_GAP);
        Rect::from_min_size(pos2(canvas.left(), top), vec2(canvas.width(), LANE_HEIGHT))
    }

    /// Every clip with its lane and where it is drawn: the project's tracks
    /// first, then the files still loading.
    fn drawn<'a>(
        &'a self,
        project: &Project,
        canvas: Rect,
        x_of: &impl Fn(f32) -> f32,
    ) -> Vec<(usize, Drawn<'a>)> {
        let mut out = Vec::new();
        for (row, track) in project.tracks.iter().enumerate() {
            let lane = Self::lane_rect(canvas, row).shrink2(vec2(0.0, 2.0));
            for clip in &track.clips {
                let (item, key) = match &clip.content {
                    ClipContent::Audio { media, .. } => (self.library.get(media.0), media.0),
                    ClipContent::Midi { .. } => (None, clip.id.0),
                };
                let x0 = x_of(clip.start.as_quarters() as f32);
                let x1 = x_of((clip.start + clip.length).as_quarters() as f32);
                out.push((
                    row,
                    Drawn {
                        rect: Rect::from_x_y_ranges(x0..=x1.max(x0 + 2.0), lane.y_range()),
                        item,
                        name: clip.name.clone(),
                        color: color_of(track.color),
                        key,
                    },
                ));
            }
        }
        for (i, &(id, at)) in self.pending.iter().enumerate() {
            let row = project.tracks.len() + i;
            let lane = Self::lane_rect(canvas, row).shrink2(vec2(0.0, 2.0));
            let x0 = x_of(at.as_quarters() as f32);
            let item = self.library.get(id);
            out.push((
                row,
                Drawn {
                    rect: Rect::from_x_y_ranges(x0..=x0 + 160.0, lane.y_range()),
                    name: item.map_or_else(String::new, |i| i.name.clone()),
                    item,
                    color: theme::CLIP_COLORS[row % theme::CLIP_COLORS.len()],
                    key: id,
                },
            ));
        }
        out
    }

    /// The file of the clip under `pointer`.
    pub fn file_at(
        &self,
        project: &Project,
        pointer: Pos2,
        canvas: Rect,
        x_of: impl Fn(f32) -> f32,
    ) -> Option<&Item> {
        self.drawn(project, canvas, &x_of)
            .into_iter()
            .find(|(_, d)| d.rect.contains(pointer))
            .and_then(|(_, d)| d.item)
    }

    /// Draws the lanes and their clips. `x_of` maps quarter notes to x.
    pub fn paint(
        &mut self,
        painter: &Painter,
        canvas: Rect,
        project: &Project,
        x_of: impl Fn(f32) -> f32,
    ) {
        let rows = project.tracks.len() + self.pending.len();
        for row in 0..rows {
            let lane = Self::lane_rect(canvas, row);
            if lane.top() > canvas.bottom() {
                break;
            }
            painter.rect_filled(lane, 0.0, theme::p().lane);
        }
        let ctx = painter.ctx();
        let now = ctx.input(|i| i.time);
        let mut seen = std::mem::take(&mut self.seen);
        let drawn = self.drawn(project, canvas, &x_of);
        for (_, d) in &drawn {
            seen.entry(d.key).or_insert(now);
        }
        for (row, drawn) in drawn {
            // A new clip fades in (motion: BASE).
            let shown = crate::motion::appear(ctx, seen[&drawn.key]);
            let lane = Self::lane_rect(canvas, row);
            let visible = drawn.rect.intersect(lane);
            if lane.top() > canvas.bottom() || visible.width() <= 0.0 {
                continue;
            }
            let missing = theme::p().record.gamma_multiply(0.4);
            let (fill, label) = match drawn.item.map(|i| &i.state) {
                Some(State::Ready(_)) => (drawn.color, drawn.name.clone()),
                Some(State::Loading) => {
                    let p = drawn
                        .item
                        .and_then(|i| self.library.progress(i.id))
                        .unwrap_or(0.0);
                    (
                        drawn.color.gamma_multiply(0.45),
                        format!("{} · loading {:.0}%", drawn.name, p * 100.0),
                    )
                }
                Some(State::Failed(_)) => (missing, format!("{} · could not load", drawn.name)),
                // The project refers to a file that is not available.
                None => (missing, format!("{} · file missing", drawn.name)),
            };
            let fill = fill.gamma_multiply(shown);
            let radius = theme::radius::SMALL;
            painter.rect_filled(drawn.rect, radius, fill);
            if let Some(item) = drawn.item
                && let (State::Loading, Some(p)) = (&item.state, self.library.progress(item.id))
            {
                // The progress fill glides between updates rather than jumping.
                let p = ctx.animate_value_with_time(
                    egui::Id::new(("load progress", item.id)),
                    p,
                    crate::motion::time(crate::motion::BASE),
                );
                let done = Rect::from_min_size(
                    drawn.rect.min,
                    vec2(drawn.rect.width() * p, drawn.rect.height()),
                );
                painter.rect_filled(done, radius, drawn.color.gamma_multiply(0.8 * shown));
            }
            painter.rect_stroke(
                drawn.rect,
                radius,
                Stroke::new(1.0, fill.gamma_multiply(1.4)),
                egui::StrokeKind::Inside,
            );
            let text_at = Pos2::new(
                visible.left().max(drawn.rect.left()) + 6.0,
                drawn.rect.top() + 4.0,
            );
            painter.with_clip_rect(visible).text(
                text_at,
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(theme::size::BODY - 1.0),
                theme::CLIP_TEXT.gamma_multiply(shown),
            );
        }
        self.seen = seen;
    }

    /// The panel for loads in progress (with Cancel), failures (with
    /// Dismiss) and notices, in the arrangement's bottom-left corner.
    pub fn panel(&mut self, ctx: &egui::Context, corner: Pos2) {
        let loading: Vec<(String, f32)> = self
            .library
            .items()
            .iter()
            .filter_map(|i| Some((i.name.clone(), self.library.progress(i.id)?)))
            .collect();
        let failed: Vec<(u64, String, String)> = self
            .library
            .items()
            .iter()
            .filter_map(|i| match &i.state {
                State::Failed(why) => Some((i.id, i.name.clone(), why.clone())),
                _ => None,
            })
            .collect();
        if loading.is_empty() && failed.is_empty() && self.notice.is_none() {
            return;
        }
        egui::Area::new(egui::Id::new("imports"))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(corner)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    if !loading.is_empty() {
                        ui.horizontal(|ui| {
                            let total: f32 = loading.iter().map(|(_, p)| p).sum();
                            ui.add(
                                egui::ProgressBar::new(total / loading.len() as f32)
                                    .desired_width(160.0)
                                    .show_percentage(),
                            );
                            ui.label(match loading.as_slice() {
                                [(name, _)] => format!("Loading {name}"),
                                many => format!("Loading {} files", many.len()),
                            });
                            if ui.button("Cancel").on_hover_text("Stop loading").clicked() {
                                self.library.cancel_all();
                            }
                        });
                    }
                    for (id, name, why) in failed {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⚠").color(theme::p().warning));
                            ui.label(format!("{name}: {why}"));
                            if ui.small_button("Dismiss").clicked() {
                                self.library.dismiss(id);
                                self.dirty = true;
                            }
                        });
                    }
                    if let Some(notice) = self.notice.clone() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("ⓘ").color(theme::p().text_secondary));
                            ui.label(notice);
                            if ui.small_button("Dismiss").clicked() {
                                self.notice = None;
                            }
                        });
                    }
                });
            });
    }
}

/// The hover card for a clip's file.
pub fn file_card(ui: &mut egui::Ui, item: &Item) {
    ui.label(RichText::new(&item.name).strong());
    let info = &item.info;
    let mut facts = vec![info.format.clone()];
    if let Some(rate) = info.sample_rate {
        facts.push(format!("{:.1} kHz", f64::from(rate) / 1000.0));
    }
    if let Some(channels) = info.channels {
        facts.push(match channels {
            1 => "mono".into(),
            2 => "stereo".into(),
            n => format!("{n} channels"),
        });
    }
    if let Some(seconds) = info.seconds {
        facts.push(crate::position::format_time(seconds));
    }
    facts.push(format!("{:.1} MB", info.size_bytes as f64 / 1_048_576.0));
    ui.label(facts.join(" · "));
    match &item.state {
        State::Ready(audio) if Some(audio.sample_rate) != info.sample_rate => {
            ui.label(format!(
                "Playing at {:.1} kHz (converted)",
                f64::from(audio.sample_rate) / 1000.0
            ));
        }
        State::Failed(why) => {
            ui.label(RichText::new(why).color(theme::p().warning));
        }
        _ => {}
    }
    ui.label(
        RichText::new(item.path.display().to_string())
            .small()
            .weak(),
    );
}
