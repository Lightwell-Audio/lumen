//! Audio clips in the arrangement, the simple timeline of M2 slice 6: one
//! lane per imported file, each holding one clip at a musical position.
//! Tracks with headers, editing, undo and save/load arrive in M3, where these
//! lanes become project tracks.

use std::path::PathBuf;

use eframe::egui::{self, Color32, Painter, Pos2, Rect, RichText, Stroke, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipsHandle, Timeline};

use crate::media::{self, Item, Library, State};
use crate::theme;

pub const LANE_HEIGHT: f32 = 56.0;
const LANE_GAP: f32 = 4.0;

pub struct Lane {
    pub media: u64,
    pub start: Ticks,
    color: Color32,
    /// When it was added (egui input time), for its fade-in.
    added: f64,
}

#[derive(Default)]
pub struct Lanes {
    pub lanes: Vec<Lane>,
    pub library: Library,
    /// The engine's clip list needs rebuilding.
    dirty: bool,
    /// A message about files that could not be added.
    notice: Option<String>,
}

impl Lanes {
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// Adds each audio file on a new lane at `at`; anything else is
    /// reported, not added.
    pub fn add_files(&mut self, paths: Vec<PathBuf>, at: Ticks, rate: u32, ctx: &egui::Context) {
        let mut rejected = Vec::new();
        for path in paths {
            if media::is_audio_file(&path) {
                let id = self.library.import(path, rate, ctx);
                let color = theme::CLIP_COLORS[self.lanes.len() % theme::CLIP_COLORS.len()];
                self.lanes.push(Lane {
                    media: id,
                    start: at,
                    color,
                    added: ctx.input(|i| i.time),
                });
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
        self.dirty = true;
    }

    /// Applies finished loads and keeps the engine's clips in step. Call
    /// every frame.
    pub fn update(&mut self, rate: u32, ctx: &egui::Context, clips: &mut ClipsHandle) {
        self.dirty |= self.library.update(rate, ctx);
        // Lanes whose file was cancelled or dismissed go too.
        let library = &self.library;
        let before = self.lanes.len();
        self.lanes.retain(|l| library.get(l.media).is_some());
        self.dirty |= self.lanes.len() != before;
        if self.dirty {
            let list: Vec<Clip> = self
                .lanes
                .iter()
                .filter_map(|lane| match &self.library.get(lane.media)?.state {
                    State::Ready(audio) => Some(Clip {
                        start: lane.start,
                        offset: 0,
                        length: audio.frames() as i64,
                        gain: 1.0,
                        audio: audio.clone(),
                    }),
                    _ => None,
                })
                .collect();
            // A full queue means the engine has not caught up; retry next frame.
            self.dirty = clips.set(list).is_err();
        }
    }

    fn lane_rect(canvas: Rect, index: usize) -> Rect {
        let top = canvas.top() + LANE_GAP + index as f32 * (LANE_HEIGHT + LANE_GAP);
        Rect::from_min_size(pos2(canvas.left(), top), vec2(canvas.width(), LANE_HEIGHT))
    }

    /// Where lane `index`'s clip is drawn, with its file.
    fn clip(
        &self,
        index: usize,
        canvas: Rect,
        timeline: &Timeline,
        x_of: &impl Fn(f32) -> f32,
    ) -> Option<(Rect, &Item)> {
        let lane = self.lanes.get(index)?;
        let item = self.library.get(lane.media)?;
        let row = Self::lane_rect(canvas, index);
        let x0 = x_of(lane.start.as_quarters() as f32);
        let x1 = match &item.state {
            State::Ready(audio) => {
                let start = timeline.tempo.seconds_at(lane.start);
                let seconds = audio.frames() as f64 / f64::from(audio.sample_rate);
                let end = timeline.tempo.ticks_at_seconds(start + seconds);
                x_of(end.as_quarters() as f32)
            }
            _ => x0 + 160.0,
        };
        let rect =
            Rect::from_x_y_ranges(x0..=x1.max(x0 + 2.0), row.shrink2(vec2(0.0, 2.0)).y_range());
        Some((rect, item))
    }

    /// The file whose clip is under `pointer`.
    pub fn file_at(
        &self,
        pointer: Pos2,
        canvas: Rect,
        timeline: &Timeline,
        x_of: impl Fn(f32) -> f32,
    ) -> Option<&Item> {
        (0..self.lanes.len())
            .filter_map(|i| self.clip(i, canvas, timeline, &x_of))
            .find(|(rect, _)| rect.contains(pointer))
            .map(|(_, item)| item)
    }

    /// Draws the lanes and their clips. `x_of` maps quarter notes to x.
    pub fn paint(
        &self,
        painter: &Painter,
        canvas: Rect,
        timeline: &Timeline,
        x_of: impl Fn(f32) -> f32,
    ) {
        for (index, lane) in self.lanes.iter().enumerate() {
            let row = Self::lane_rect(canvas, index);
            if row.top() > canvas.bottom() {
                break;
            }
            painter.rect_filled(row, 0.0, theme::p().lane);
            let Some((clip, item)) = self.clip(index, canvas, timeline, &x_of) else {
                continue;
            };
            let visible = clip.intersect(row);
            if visible.width() <= 0.0 {
                continue;
            }
            let (fill, label) = match &item.state {
                State::Ready(_) => (lane.color, item.name.clone()),
                State::Loading => {
                    let p = self.library.progress(item.id).unwrap_or(0.0);
                    (
                        lane.color.gamma_multiply(0.45),
                        format!("{} · loading {:.0}%", item.name, p * 100.0),
                    )
                }
                State::Failed(_) => (
                    theme::p().record.gamma_multiply(0.4),
                    format!("{} · could not load", item.name),
                ),
            };
            // A new clip fades in (motion: BASE).
            let ctx = painter.ctx();
            let shown = crate::motion::appear(ctx, lane.added);
            let fill = fill.gamma_multiply(shown);
            let radius = theme::radius::SMALL;
            painter.rect_filled(clip, radius, fill);
            if let (State::Loading, Some(p)) = (&item.state, self.library.progress(item.id)) {
                // The progress fill glides between updates rather than jumping.
                let p = ctx.animate_value_with_time(
                    egui::Id::new(("load progress", item.id)),
                    p,
                    crate::motion::time(crate::motion::BASE),
                );
                let done = Rect::from_min_size(clip.min, vec2(clip.width() * p, clip.height()));
                painter.rect_filled(done, radius, lane.color.gamma_multiply(0.8 * shown));
            }
            painter.rect_stroke(
                clip,
                radius,
                Stroke::new(1.0, fill.gamma_multiply(1.4)),
                egui::StrokeKind::Inside,
            );
            let text_at = Pos2::new(visible.left().max(clip.left()) + 6.0, clip.top() + 4.0);
            painter.with_clip_rect(visible).text(
                text_at,
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(theme::size::BODY - 1.0),
                theme::CLIP_TEXT.gamma_multiply(shown),
            );
        }
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
