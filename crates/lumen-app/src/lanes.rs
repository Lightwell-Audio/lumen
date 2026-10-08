//! Audio for the project's clips (M3 slices 3–4): files loading for import,
//! and the engine's clip list derived from the project. Drawing is in
//! `arrangement.rs`.
//!
//! Dropped files load in the background as placeholders that are not part
//! of the project. A file joins the project, as one undo step, once it has
//! loaded and its exact length is known; a failed or cancelled load never
//! touches it. The engine's clip list is derived from the project after
//! every change, undo and redo.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Pos2, RichText};
use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipsHandle};
use lumen_project::model::{ClipContent, Id, Project};

use crate::document::{Document, Imported};
use crate::media::{self, Item, Library, State};
use crate::theme;

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
    /// Counts changes to what media is loaded, so drawings built from it
    /// know when to rebuild.
    revision: u64,
    /// The device rate at the last update: the rate audio is held at.
    rate: u32,
}

/// What the engine plays (audit M4: the model-to-engine translation, kept
/// pure so it is tested): every unmuted clip on a track that can be heard
/// whose audio is loaded (`audio` gives the audio at the device rate and the
/// file's own rate), for the shorter of the clip and the file, with clip and
/// track gain and the track's pan.
pub fn engine_clips(
    project: &Project,
    rate: u32,
    audio: impl Fn(Id) -> Option<(std::sync::Arc<lumen_engine::ClipAudio>, u32)>,
) -> Vec<Clip> {
    let tempo = lumen_core::time::TempoMap::new(project.tempo.clone()).ok();
    let audible = crate::document::audible(project);
    let mut list = Vec::new();
    for track in &project.tracks {
        let heard = audible(track);
        for clip in &track.clips {
            let ClipContent::Audio {
                media,
                offset,
                gain_db,
            } = &clip.content
            else {
                continue;
            };
            let Some((samples, native)) = audio(*media) else {
                continue;
            };
            // The offset is in the file's frames; the audio is at the
            // device rate.
            let offset =
                (*offset as f64 * f64::from(samples.sample_rate) / f64::from(native.max(1))) as i64;
            // Frames between two musical positions at the device rate.
            let frames = |a: Ticks, b: Ticks| {
                tempo
                    .as_ref()
                    .map_or(0, |t| t.sample_at(b, rate) - t.sample_at(a, rate))
            };
            let end = clip.start + clip.length;
            let region = if tempo.is_some() {
                frames(clip.start, end)
            } else {
                i64::MAX
            };
            // A looping clip repeats for its whole length; otherwise it
            // stops where the file does.
            let length = if clip.loop_length.is_some() {
                region
            } else {
                (samples.frames() as i64 - offset).min(region)
            };
            let shape = lumen_engine::ClipShape {
                fade_in: lumen_engine::FadeFrames {
                    frames: frames(clip.start, clip.start + clip.fade_in.length),
                    shape: clip.fade_in.shape,
                },
                fade_out: lumen_engine::FadeFrames {
                    frames: frames(end - clip.fade_out.length, end),
                    shape: clip.fade_out.shape,
                },
                period: clip
                    .loop_length
                    .map(|l| frames(clip.start, clip.start + l))
                    .filter(|p| *p > 0),
            };
            if length > 0 {
                list.push(Clip {
                    id: clip.id.0,
                    start: clip.start,
                    offset,
                    length,
                    // Track volume and pan (M3 slice 4); the mixer takes
                    // over in M4.
                    // Muted and unheard clips stay at gain 0, so they fade
                    // out and back in rather than clicking.
                    gain: if heard && !clip.muted {
                        db_to_gain(*gain_db) * db_to_gain(track.gain_db)
                    } else {
                        0.0
                    },
                    pan: track.pan,
                    audio: samples,
                    shape,
                });
            }
        }
    }
    list
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Lanes {
    /// Files are still loading for import.
    /// Files dropped for import are still loading (not an opened
    /// project's media: `library.is_loading()` covers every load).
    pub fn is_loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Files loading for import: library id and where they were dropped.
    pub fn pending(&self) -> &[(u64, Ticks)] {
        &self.pending
    }

    /// How far a clip has faded in (motion: BASE), by a stable key (the
    /// media id for audio, so a loading placeholder that becomes its clip
    /// does not fade in twice).
    pub fn shown(&mut self, key: u64, ctx: &egui::Context) -> f32 {
        let now = ctx.input(|i| i.time);
        let since = *self.seen.entry(key).or_insert(now);
        crate::motion::appear(ctx, since)
    }

    /// The device rate audio is held at (as of the last update).
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Changes whenever loaded media changes (see `revision`).
    pub fn revision(&self) -> u64 {
        self.revision
    }

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
                "Not added: {}. {} imports WAV, AIFF, FLAC, MP3 and Ogg Vorbis files",
                rejected.join(", "),
                lumen_core::product::PRODUCT_NAME
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
        self.rate = rate;
        if self.library.update(rate, ctx) {
            self.dirty = true;
            self.revision += 1;
        }

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
                self.revision += 1;
            }
        }

        if self.dirty || self.built != Some(doc.state()) {
            // A full queue means the engine has not caught up; retry next frame.
            let list = self.engine_clips(doc.project(), rate);
            self.dirty = clips.set(list).is_err();
            self.built = Some(doc.state());
        }
    }

    /// What the engine plays, from the files that are loaded.
    fn engine_clips(&self, project: &Project, rate: u32) -> Vec<Clip> {
        engine_clips(project, rate, |media| {
            let item = self.library.get(media.0)?;
            let State::Ready(audio) = &item.state else {
                return None;
            };
            Some((
                audio.clone(),
                item.info.sample_rate.unwrap_or(audio.sample_rate),
            ))
        })
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
    // Every gesture on a clip, so none stays hidden (owner, after 6c: slip
    // was not discoverable).
    ui.separator();
    for line in [
        "Top half: drag to move · Ctrl copies · Ctrl+Alt slips the audio",
        "Bottom half: drag to select time",
        "Sides: drag to trim · Shift loops",
        "Right-click: cut, copy, paste, duplicate, delete",
    ] {
        ui.label(RichText::new(line).small().weak());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::document::{Document, Imported};
    use lumen_engine::ClipAudio;
    use lumen_project::model::TrackKind;

    /// A document with one two-second clip on each of `n` tracks, and the
    /// audio for it at 48 kHz.
    fn setup(n: usize) -> (Document, Arc<ClipAudio>) {
        let mut doc = Document::new(crate::empty_project());
        let files = (0..n)
            .map(|i| Imported {
                media: Id(1000 + i as u64),
                path: PathBuf::from(format!("C:/a/{i}.wav")),
                name: format!("T{i}"),
                sample_rate: 48_000,
                channels: 2,
                frames: 96_000,
                size_bytes: 0,
                at: Ticks::ZERO,
            })
            .collect();
        doc.import(files);
        let audio = Arc::new(ClipAudio {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000], vec![0.5; 96_000]],
        });
        (doc, audio)
    }

    fn played(doc: &Document, audio: &Arc<ClipAudio>, native: u32) -> Vec<Clip> {
        engine_clips(doc.project(), 48_000, |_| Some((audio.clone(), native)))
    }

    #[test]
    fn clips_play_whole_with_track_gain_and_pan() {
        let (mut doc, audio) = setup(1);
        let id = doc.project().tracks[0].id;
        doc.set_volume(id, -6.0);
        doc.set_pan(id, -0.5);
        let clips = played(&doc, &audio, 48_000);
        assert_eq!(clips.len(), 1);
        assert_eq!((clips[0].offset, clips[0].length), (0, 96_000));
        assert!((clips[0].gain - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert_eq!(clips[0].pan, -0.5);
    }

    #[test]
    fn fades_and_loops_reach_the_engine_in_frames() {
        let (mut doc, audio) = setup(1);
        // A two-second file (four quarters at 120 BPM), stretched to eight
        // quarters and looped every quarter, with fades.
        doc.apply("shape", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            let clip = Arc::make_mut(&mut track.clips[0]);
            clip.length = Ticks::quarters(8);
            clip.loop_length = Some(Ticks::quarters(1));
            clip.fade_in.length = Ticks(lumen_core::time::PPQ / 2);
            clip.fade_out.length = Ticks::quarters(1);
            clip.fade_out.shape = lumen_core::fade::FadeShape::SCurve;
        });
        let clip = &played(&doc, &audio, 48_000)[0];
        assert_eq!(clip.length, 192_000, "repeats past the file's end");
        assert_eq!(clip.shape.period, Some(24_000));
        assert_eq!(clip.shape.fade_in.frames, 12_000);
        assert_eq!(clip.shape.fade_out.frames, 24_000);
        assert_eq!(
            clip.shape.fade_out.shape,
            lumen_core::fade::FadeShape::SCurve
        );
        // Without the loop it stops where the file does.
        doc.apply("unloop", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            Arc::make_mut(&mut track.clips[0]).loop_length = None;
        });
        assert_eq!(played(&doc, &audio, 48_000)[0].length, 96_000);
    }

    #[test]
    fn mute_solo_and_missing_audio_decide_what_plays() {
        let (mut doc, audio) = setup(3);
        let ids: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        // Every clip is sent; the ones that cannot be heard at gain 0, so
        // the engine fades them rather than cutting them off.
        let heard = |doc: &Document| {
            played(doc, &audio, 48_000)
                .iter()
                .map(|c| c.gain > 0.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(heard(&doc), [true, true, true]);
        doc.set_mute(&ids[..1], true);
        assert_eq!(heard(&doc), [false, true, true]);
        doc.set_solo(&ids[2..], true, true);
        assert_eq!(heard(&doc), [false, false, true], "only the soloed track");
        // Audio not loaded (or missing): nothing to play for it.
        let none = engine_clips(doc.project(), 48_000, |_| None);
        assert!(none.is_empty());
    }

    #[test]
    fn offsets_follow_the_device_rate_and_the_clip_region_caps_the_length() {
        let (mut doc, audio) = setup(1);
        // The file was 44.1 kHz; the audio has been converted to 48 kHz.
        doc.apply("Trim", None, |p| {
            let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
            if let ClipContent::Audio { offset, .. } = &mut clip.content {
                *offset = 44_100;
            }
            // One beat at 120 BPM: half a second.
            clip.length = Ticks::quarters(1);
        });
        let clips = played(&doc, &audio, 44_100);
        assert_eq!(clips[0].offset, 48_000, "one second of the file, at 48 kHz");
        assert_eq!(clips[0].length, 24_000, "capped to the clip's half second");
    }

    #[test]
    fn midi_is_not_sent_as_audio_and_muted_clips_are_silent() {
        let (mut doc, audio) = setup(1);
        doc.add_track(TrackKind::Midi, None);
        doc.apply("Mute clip", None, |p| {
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).muted = true;
        });
        let clips = played(&doc, &audio, 48_000);
        assert_eq!(clips.len(), 1, "the audio clip only");
        assert_eq!(clips[0].gain, 0.0);
        assert_eq!(clips[0].id, doc.project().tracks[0].clips[0].id.0);
    }
}
