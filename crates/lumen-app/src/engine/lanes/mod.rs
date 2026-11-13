//! Audio for the project's clips (M3 slices 3–4): files loading for import,
//! and the engine's clip list derived from the project. Drawing is in
//! `arrangement.rs`.
//!
//! Dropped files load in the background as placeholders that are not part
//! of the project. A file joins the project, as one undo step, once it has
//! loaded and its exact length is known, in the order the files were
//! dropped; a failed or cancelled load never touches it. The engine's clip
//! list is derived from the project after every change, undo and redo.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Pos2, RichText};
use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipAudio, ClipsHandle, MidiHandle, MixerHandle};
use lumen_project::model::{Id, Project};

use crate::document::{Document, DropInto, Imported};
use crate::media::warped::WarpedStreams;
use crate::media::{self, Library, State};
use crate::theme;

mod cards;
mod clips;

pub use cards::{file_card, file_facts};
#[cfg(test)]
pub use clips::engine_clips;
pub use clips::engine_clips_with;
use clips::{audio_replacing, reversed_media, without_replaced};

/// A file loading for import onto a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    pub id: u64,
    pub at: Ticks,
    pub track: Id,
    drop: u64,
}

/// The handles that replace what the engine plays.
pub struct Handles<'a> {
    pub clips: &'a mut ClipsHandle,
    pub midi: &'a mut MidiHandle,
    pub mixer: &'a mut MixerHandle,
    /// The plugin devices' hosts, for the instruments in the MIDI tracks.
    pub hosting: &'a mut crate::hosting::Hosting,
}

/// The longest a list waits for a warped clip's audio to be prepared.
const HOLD_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Default)]
pub struct Lanes {
    pub library: Library,
    /// What the project's samplers play, decoded off the audio thread (M8
    /// slice B5).
    pub samples: crate::builtin::samples::Library,
    /// Files loading for import onto new tracks, in the order they were
    /// dropped: library id and where they were dropped. Each shows as a row
    /// below the tracks until it joins the project.
    pending: Vec<(u64, Ticks)>,
    /// Where each of those new tracks goes (slice 6e), and the drop it
    /// came in.
    before: HashMap<u64, (DropInto, u64)>,
    /// Files loading for import onto a track (slice 6e), drawn in its lane
    /// while they load. Files dropped together import together, once none
    /// of them is still loading, so they can follow one another.
    landing: Vec<Landing>,
    /// Counts drops, to keep files dropped together together.
    drops: u64,
    /// The project state the engine's clip list was built from.
    built: Option<u64>,
    /// The engine's clip list needs rebuilding.
    dirty: bool,
    /// The streamed clips of the list last sent, and a count of the lists
    /// sent, for the disk thread's scene (M9 slice R6).
    streamed: (u64, Vec<Clip>),
    /// The streamed clips of the list sent, and the warped ones of lists
    /// held back; `streamed` is the two together.
    sent_streams: Vec<Clip>,
    waiting: Vec<Clip>,
    /// The playhead while the transport plays, and when the list was first
    /// held back for a warped clip's audio.
    playhead: Option<i64>,
    held_since: Option<std::time::Instant>,
    /// The streams warped clips play as (M9.5 S4b), made in the pool of the
    /// streaming that was on when they were. Behind a lock because the lists
    /// built for a render are built from `&self`.
    warped: std::sync::Mutex<Option<WarpedStreams>>,
    /// The audio tracks being recorded over that the clip list was built
    /// with, so it is built again when a take begins or ends.
    masked: Vec<crate::midi::play::Replacing>,
    /// The project state the engine's MIDI tracks were built from, and
    /// whether they need sending again (M5 slice 1).
    midi_built: Option<(
        u64,
        crate::midi::play::Inputs,
        u32,
        crate::document::Overrides,
    )>,
    midi_dirty: bool,
    midi_cache: crate::midi::play::Cache,
    /// The MIDI outputs tracks name, by the numbers the engine uses.
    pub(crate) midi_ports: crate::midi::play::PortTable,
    /// The project state, cue and devices the engine's routing was built
    /// from.
    routed: Option<(u64, crate::routing::Cue, crate::routing::Devices, u32)>,
    /// Devices on tracks, until devices join the project (M6).
    pub(crate) devices: crate::routing::Devices,
    /// A message about files that could not be added.
    notice: Option<String>,
    /// When each clip was first drawn (egui time), for its fade-in.
    seen: HashMap<u64, f64>,
    /// Counts changes to what media is loaded, so drawings built from it
    /// know when to rebuild.
    revision: u64,
    /// The project state and media revision the reversed twins were last
    /// matched to.
    reversed_built: Option<(u64, u64)>,
    /// The device rate at the last update: the rate audio is held at.
    rate: u32,
}

impl Lanes {
    /// A plugin instrument is ready for the next MIDI track list (M6
    /// slice 2c).
    pub fn midi_changed(&mut self, changed: bool) {
        self.midi_dirty |= changed;
    }

    /// The samples the project's samplers play (M8 slice B5): files that
    /// finished decoding are read here, and every file the project names is
    /// asked for if it is not loaded — an opened project decodes its samples
    /// as it decodes its clips, off the audio thread.
    fn follow_samples(&mut self, doc: &Document, rate: u32) {
        self.samples.update(rate);
        for media in &doc.project().media {
            if self.samples.get(media.id).is_some() {
                continue;
            }
            if let Some(path) = doc.media_files.get(&media.id) {
                self.samples.load(media.id, path.clone(), rate);
            }
        }
    }

    /// Files are still loading for import.
    /// Files dropped for import are still loading (not an opened
    /// project's media: `library.is_loading()` covers every load). The audio tracks whose clips the engine plays cut for a take under
    /// way (test seam).
    pub fn masked_tracks(&self) -> Vec<Id> {
        self.masked.iter().map(|r| r.track).collect()
    }

    /// The span cut from `track`'s clips in that list (test seam).
    pub fn masked_span(&self, track: Id) -> Option<(Ticks, Option<Ticks>)> {
        self.masked
            .iter()
            .find(|r| r.track == track)
            .map(|r| (r.from, r.until))
    }

    /// The streamed clips the engine plays, and a count that changes with
    /// every list sent (M9 slice R6).
    pub fn streamed(&self) -> (u64, &[Clip]) {
        (self.streamed.0, &self.streamed.1)
    }

    pub fn is_loading(&self) -> bool {
        !self.pending.is_empty() || !self.landing.is_empty()
    }

    /// Files loading for import onto tracks (slice 6e).
    pub fn landing(&self) -> &[Landing] {
        &self.landing
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
        project.tracks.is_empty() && !self.is_loading()
    }

    /// Starts loading each audio file, to be added at `at`, `into` new
    /// tracks or onto a track; anything else is reported, not added.
    pub fn add_files(
        &mut self,
        paths: Vec<PathBuf>,
        at: Ticks,
        into: DropInto,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
    ) {
        let mut rejected = Vec::new();
        self.drops += 1;
        for path in paths {
            if media::is_audio_file(&path) {
                let id = doc.fresh_id().0;
                self.library.import_as(id, path, rate, ctx);
                match into {
                    DropInto::NewTracks { .. } => {
                        self.pending.push((id, at));
                        self.before.insert(id, (into, self.drops));
                    }
                    DropInto::Track(track) => self.landing.push(Landing {
                        id,
                        at,
                        track,
                        drop: self.drops,
                    }),
                }
            } else {
                rejected.push(
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                );
            }
        }
        if !rejected.is_empty() {
            self.notice = Some(format!(
                "Not added: {}. {} imports WAV, AIFF, FLAC, MP3, Ogg Vorbis and MIDI files",
                rejected.join(", "),
                lumen_core::product::PRODUCT_NAME
            ));
        }
    }

    /// Shows a notice in the imports panel (M5 slice 7: MIDI files that
    /// could not be added).
    pub fn say(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    /// Sends the engine its clips when they, or the tracks being recorded
    /// over, changed. Recording calls it before the transport starts, so
    /// the audio under a take is already cut when the take begins.
    pub fn send_clips(
        &mut self,
        doc: &Document,
        clips: &mut ClipsHandle,
        rate: u32,
        inputs: &crate::midi::play::Inputs,
    ) {
        let masked = audio_replacing(doc.project(), inputs);
        if self.dirty || self.built != Some(doc.state()) || self.masked != masked {
            // A full queue means the engine has not caught up; retry next frame.
            // A frozen track plays its file (M7b E5), not its clips.
            let sounding = lumen_project::edit::freeze::played(doc.project());
            let list = self.engine_clips(&without_replaced(sounding, &masked), rate);
            // A warped clip an edit just made is not played until its first
            // moments are prepared (below), so the old one plays on.
            if self.hold_for_warps(&list, doc.project(), rate) {
                self.dirty = true;
                return;
            }
            let streamed: Vec<Clip> = list
                .iter()
                .filter(|c| matches!(&*c.audio, ClipAudio::Stream(_)))
                .cloned()
                .collect();
            self.dirty = clips.set(list).is_err();
            if !self.dirty {
                self.forget_unplayed_warps(&streamed);
                self.sent_streams = streamed;
                self.waiting.clear();
                self.publish_streamed();
            }
            self.built = Some(doc.state());
            self.masked = masked;
        }
    }

    /// Tells the lanes where the playhead is while the transport plays
    /// (`None` when stopped), for [`Lanes::send_clips`] to hold back a clip
    /// whose audio is not prepared yet.
    pub fn set_playhead(&mut self, sample: Option<i64>) {
        self.playhead = sample;
    }

    /// Makes the hold in force look `by` older (a test seam).
    #[cfg(test)]
    pub(crate) fn age_hold(&mut self, by: std::time::Duration) {
        self.held_since = self.held_since.map(|since| since - by);
    }

    /// The streams of the clips sent and those waiting, for the disk
    /// thread's scene: it prepares the waiting ones first.
    fn publish_streamed(&mut self) {
        let mut all = self.sent_streams.clone();
        all.extend(self.waiting.iter().cloned());
        self.streamed = (self.streamed.0 + 1, all);
    }

    /// Whether `list` waits: while playing, a warped stream that the engine
    /// does not play yet and whose next quarter second is not all in memory
    /// holds the whole list back, so an edit made during playback is heard
    /// when its audio is ready and the old one plays on until then (M9.5 §2 E,
    /// "an edit made while playing"). The clips waiting are told to the disk
    /// thread. Never holds longer than [`HOLD_LIMIT`]: a disk thread that does
    /// not answer plays the edit anyway, with the dip a late page gives.
    pub(crate) fn hold_for_warps(&mut self, list: &[Clip], project: &Project, rate: u32) -> bool {
        let waiting = self.warps_not_ready(list, project, rate);
        if waiting.is_empty() {
            self.held_since = None;
            return false;
        }
        let since = *self.held_since.get_or_insert_with(std::time::Instant::now);
        if since.elapsed() > HOLD_LIMIT {
            self.held_since = None;
            return false;
        }
        // A clip is the same waiting clip only with the same stream: a second
        // edit makes another stream, which the disk thread must be told of.
        let ids = |clips: &[Clip]| -> Vec<(u64, Option<u64>)> {
            clips
                .iter()
                .map(|c| (c.id, c.audio.stream().map(|s| s.id())))
                .collect()
        };
        if ids(&waiting) != ids(&self.waiting) {
            self.waiting = waiting;
            self.publish_streamed();
        }
        true
    }

    /// The clips of `list` that play a warped stream not played yet, whose
    /// audio from the playhead on is not all in memory.
    fn warps_not_ready(&self, list: &[Clip], project: &Project, rate: u32) -> Vec<Clip> {
        let Some(at) = self.playhead else {
            return Vec::new();
        };
        let Ok(tempo) = lumen_core::time::TempoMap::new(project.tempo.clone()) else {
            return Vec::new();
        };
        let playing = |id: u64| {
            self.sent_streams
                .iter()
                .any(|c| c.audio.stream().is_some_and(|s| s.id() == id))
        };
        list.iter()
            .filter(|clip| {
                let Some(stream) = clip.audio.stream() else {
                    return false;
                };
                if playing(stream.id()) || media::streams::warped(stream.id()).is_none() {
                    return false;
                }
                let start = tempo.sample_at(clip.start, rate);
                let ahead = at..at + i64::from(rate / 4);
                !lumen_engine::for_each_page(clip, start, ahead, stream.frames(), &mut |page| {
                    stream.resident(page)
                })
            })
            .cloned()
            .collect()
    }

    /// Applies finished loads (adding them to the project), loads media the
    /// project needs, and keeps the engine's clips in step. Call every
    /// frame.
    pub fn update(
        &mut self,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
        engine: Handles<'_>,
        cue: &crate::routing::Cue,
        inputs: &crate::midi::play::Inputs,
    ) {
        let Handles {
            clips,
            midi,
            mixer,
            hosting,
        } = engine;
        self.rate = rate;
        if self.library.update(rate, ctx) {
            self.dirty = true;
            self.revision += 1;
        }

        self.join_loaded(doc);

        if self.reversed_built != Some((doc.state(), self.revision)) {
            let wanted = reversed_media(doc.project());
            if self.library.keep_reversed(&wanted, ctx) {
                self.dirty = true;
            }
            self.reversed_built = Some((doc.state(), self.revision));
        }

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

        self.send_clips(doc, clips, rate, inputs);
        // The samples the project's samplers play (M8 slice B5): the files the
        // project holds are decoded off the audio thread, and a sound is made
        // for the list where they are ready.
        self.follow_samples(doc, rate);
        // Sent again when the project, the selection or the inputs change
        // (which tracks listen to what), never every frame.
        // The lanes of plugin instruments are baked for the device's rate.
        let midi_wanted = (doc.state(), inputs, rate, &self.devices.overridden);
        if self.midi_dirty
            || self
                .midi_built
                .as_ref()
                .is_none_or(|(s, i, r, o)| (*s, i, *r, o) != midi_wanted)
        {
            let sounding = lumen_project::edit::freeze::played(doc.project());
            let mut tracks = crate::midi::play::engine_midi_with(
                &sounding,
                inputs,
                &mut self.midi_ports,
                &mut self.midi_cache,
                hosting,
            );
            crate::midi::play::with_lanes(&mut tracks, &sounding, &self.devices.overridden, rate);
            // What each sampler plays, from the decoded samples (M8 slice B5).
            let project = &sounding;
            crate::midi::play::attach_sounds(&mut tracks, project, &|device, map| {
                self.samples.sound(device, map)
            });
            self.midi_dirty = match midi.set(tracks) {
                Ok(()) => false,
                // Full: a plugin instrument in the list is kept for the
                // next one, not lost with it.
                Err(tracks) => {
                    hosting.reclaim(tracks);
                    true
                }
            };
            self.midi_built = Some((
                doc.state(),
                inputs.clone(),
                rate,
                self.devices.overridden.clone(),
            ));
        }
        // The routing follows the clips, never ahead of them: a deleted
        // track's clips fade out while its node is still there, and a new
        // track's clips wait for their node, then fade in.
        // Lanes are baked for the device's rate, so a new rate is a new routing.
        let wanted = (doc.state(), cue.clone(), self.devices.clone(), rate);
        if !self.dirty && self.routed.as_ref() != Some(&wanted) {
            let sounding = lumen_project::edit::freeze::played(doc.project());
            match mixer.set(&crate::routing::routing(
                &sounding,
                cue,
                &self.devices,
                rate,
            )) {
                Ok(true) => self.routed = Some(wanted),
                // The queue is full: the engine has not caught up.
                Ok(false) => {}
                // `routing` never builds one the engine refuses; if it
                // did, the last routing plays on.
                Err(e) => {
                    debug_assert!(false, "routing refused: {e}");
                    eprintln!("routing refused: {e}");
                    self.routed = Some(wanted);
                }
            }
        }
    }

    /// Loads that finished join the project as one step, once all the
    /// files dropped with them have, in the order they were dropped,
    /// whatever order their loads finish in (a short file loads first, and
    /// used to come first; the dropped order is kept). Files for new
    /// tracks also wait for every earlier drop, so tracks keep the order of
    /// the drops too; files dropped on a track only for their own drop.
    fn join_loaded(&mut self, doc: &mut Document) {
        let mut imported = Vec::new();
        let library = &self.library;
        let before = &mut self.before;
        // `pending` is in drop order: the files before the first drop with
        // a file still loading are ready to join.
        let drop_of = |id: &u64| before.get(id).map_or(0, |b| b.1);
        let loading = self
            .pending
            .iter()
            .find(|&&(id, _)| matches!(load(library, id), Load::Loading))
            .map(|(id, _)| drop_of(id));
        let ended = self
            .pending
            .iter()
            .take_while(|(id, _)| loading.is_none_or(|drop| drop_of(id) < drop))
            .count();
        for (id, at) in self.pending.drain(..ended) {
            let into = before.remove(&id).map_or(
                DropInto::NewTracks {
                    before: None,
                    group: None,
                },
                |b| b.0,
            );
            if let Load::Ready(file) = load(library, id) {
                imported.push(file(at, into));
            }
        }
        let waiting: Vec<u64> = self
            .landing
            .iter()
            .filter(|l| matches!(load(library, l.id), Load::Loading))
            .map(|l| l.drop)
            .collect();
        self.landing.retain(|l| {
            if waiting.contains(&l.drop) {
                return true;
            }
            if let Load::Ready(file) = load(library, l.id) {
                imported.push(file(l.at, DropInto::Track(l.track)));
            }
            false
        });
        doc.import(imported);
    }

    /// The file a loaded media id was read from, for a render that reads it
    /// again at another rate (`render_audio`).
    pub(crate) fn file_of(&self, media: u64) -> Option<PathBuf> {
        let item = self.library.get(media)?;
        matches!(item.state, State::Ready(_)).then(|| item.path.clone())
    }

    /// What the engine plays, from the files that are loaded.
    pub(crate) fn engine_clips(&self, project: &Project, rate: u32) -> Vec<Clip> {
        // Warped clips play from streams in the live pool, so they need
        // "Play audio from disk" (the default); without it they play as if
        // they were not warped.
        let mut warped = self.warped.lock().unwrap_or_else(|p| p.into_inner());
        let pool = self.library.streaming().map(|s| s.pool());
        if warped
            .as_ref()
            .is_some_and(|w| pool.is_none_or(|p| !std::sync::Arc::ptr_eq(w.pool(), p)))
        {
            *warped = None;
        }
        if warped.is_none() {
            *warped = pool.map(WarpedStreams::new);
        }
        self.clips_with(project, rate, &mut |request| {
            warped.as_mut()?.stream(request)
        })
    }

    /// What a render plays: the clips, with the warped ones as streams of
    /// their own that the render reads (M9.5 S4b). They live as long as the
    /// returned [`WarpedStreams`], which the render keeps until it is done. A
    /// render needs no "Play audio from disk": it loads the pages itself.
    pub(crate) fn render_clips(&self, project: &Project, rate: u32) -> (Vec<Clip>, WarpedStreams) {
        let mut warped = WarpedStreams::new(&lumen_engine::PagePool::new(1));
        let clips = self.clips_with(project, rate, &mut |request| warped.stream(request));
        (clips, warped)
    }

    fn clips_with(
        &self,
        project: &Project,
        rate: u32,
        warped: &mut dyn FnMut(&media::warped::Request<'_>) -> Option<std::sync::Arc<ClipAudio>>,
    ) -> Vec<Clip> {
        engine_clips_with(
            project,
            rate,
            |media| {
                let item = self.library.get(media.0)?;
                let State::Ready(audio) = &item.state else {
                    return None;
                };
                Some((
                    audio.clone(),
                    item.info.sample_rate.unwrap_or(audio.sample_rate()),
                ))
            },
            warped,
        )
    }

    /// Forgets the warped streams the engine no longer plays, after it was
    /// sent `sent`.
    fn forget_unplayed_warps(&self, sent: &[Clip]) {
        let mut warped = self.warped.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(warped) = warped.as_mut() {
            let playing = sent
                .iter()
                .filter_map(|c| c.audio.stream().map(|s| s.id()))
                .collect();
            warped.retain(&playing);
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
                            if crate::widgets::raised::button(ui, "Cancel")
                                .on_hover_text("Stop loading")
                                .clicked()
                            {
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

/// Where a file being imported is.
enum Load<F> {
    Loading,
    /// Failed or cancelled.
    Gone,
    /// Loaded: makes the import, given where it goes.
    Ready(F),
}

/// The state of the import `id`.
fn load(library: &Library, id: u64) -> Load<impl FnOnce(Ticks, DropInto) -> Imported + '_> {
    let Some(item) = library.get(id) else {
        return Load::Gone;
    };
    match (&item.state, item.info.frames, item.info.sample_rate) {
        (State::Ready(_), Some(frames), Some(sample_rate)) => {
            Load::Ready(move |at, into| Imported {
                media: Id(id),
                path: item.path.clone(),
                name: item.name.clone(),
                sample_rate,
                channels: item.info.channels.unwrap_or(2) as u16,
                frames,
                size_bytes: item.info.size_bytes,
                tempo: item.info.tempo,
                at,
                into,
            })
        }
        (State::Failed(_), ..) => Load::Gone,
        _ => Load::Loading,
    }
}
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::document::{Document, Imported};
    use lumen_engine::ClipAudio;
    use lumen_engine::{
        Command, Destination, NodeSpec, Routing, Timeline, clip_player, engine, mixer,
    };
    use lumen_project::model::{ClipContent, TrackKind};

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
                tempo: None,
                at: Ticks::ZERO,
                into: DropInto::NewTracks {
                    before: None,
                    group: None,
                },
            })
            .collect();
        doc.import(files);
        let audio = Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000], vec![0.5; 96_000]],
        });
        (doc, audio)
    }

    fn played(doc: &Document, audio: &Arc<ClipAudio>, native: u32) -> Vec<Clip> {
        engine_clips(doc.project(), 48_000, |_| Some((audio.clone(), native)))
    }

    #[test]
    fn a_take_recorded_over_a_clip_silences_it_from_where_the_take_begins() {
        use crate::midi::play::Replacing;
        let (doc, audio) = setup(2);
        let (first, second) = (doc.project().tracks[0].id, doc.project().tracks[1].id);
        let whole = played(&doc, &audio, 48_000);
        let one = |list: &[Clip], track: Id| list.iter().filter(|c| c.track == track.0).count();
        let length = |list: &[Clip], track: Id| {
            list.iter()
                .filter(|c| c.track == track.0)
                .map(|c| c.length)
                .sum::<i64>()
        };
        let cut = |until: Option<Ticks>| {
            let replacing = [Replacing {
                track: first,
                from: Ticks::quarters(1),
                until,
            }];
            let sounding = without_replaced(std::borrow::Cow::Borrowed(doc.project()), &replacing);
            engine_clips(&sounding, 48_000, |_| Some((audio.clone(), 48_000)))
        };
        // Recording with no end yet: only the second before it begins plays.
        let open = cut(None);
        assert_eq!(one(&open, first), 1);
        assert_eq!(length(&open, first), 24_000);
        // Punched: the clip plays on both sides of the punch.
        let punched = cut(Some(Ticks::quarters(3)));
        assert_eq!(one(&punched, first), 2);
        assert_eq!(length(&punched, first), 48_000);
        // The other track is not touched.
        assert_eq!(length(&open, second), length(&whole, second));
        // Nothing being recorded: the clips play whole.
        let none = without_replaced(std::borrow::Cow::Borrowed(doc.project()), &[]);
        assert!(matches!(none, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn only_audio_tracks_are_masked_for_audio() {
        use crate::midi::play::{Inputs, Replacing};
        let (doc, _) = setup(1);
        let track = doc.project().tracks[0].id;
        let mut inputs = Inputs::default();
        for id in [track, Id(9999)] {
            inputs.replacing.push(Replacing {
                track: id,
                from: Ticks::ZERO,
                until: None,
            });
        }
        let masked = audio_replacing(doc.project(), &inputs);
        assert_eq!(masked.len(), 1);
        assert_eq!(masked[0].track, track);
    }

    #[test]
    fn a_frozen_track_plays_its_file_in_place_of_its_clips() {
        use lumen_project::edit::freeze::{Baked, plan};
        let (mut doc, audio) = setup(2);
        let track = doc.project().tracks[0].id;
        let own = doc.project().tracks[0].clips[0].id;
        let media = doc.fresh_id();
        let baked: Vec<Baked> = plan(doc.project(), &[track])
            .parts
            .into_iter()
            .map(|part| Baked {
                end: part.end,
                part,
                media: lumen_project::model::Media {
                    id: media,
                    path: "frozen.wav".into(),
                    original_path: None,
                    frames: 96_000,
                    channels: 2,
                    sample_rate: 48_000,
                    size_bytes: 0,
                },
                devices: Vec::new(),
            })
            .collect();
        doc.freeze_tracks(&baked);
        let frozen = doc.project().tracks[0].frozen.clone().expect("frozen");
        let sounding = lumen_project::edit::freeze::played(doc.project());
        let list = engine_clips(&sounding, 48_000, |_| Some((audio.clone(), 48_000)));
        let mine: Vec<_> = list.iter().filter(|c| c.track == track.0).collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].id, frozen.id.0);
        assert_ne!(mine[0].id, own.0, "not the clip it was made from");
        let other = doc.project().tracks[1].id;
        assert_eq!(list.iter().filter(|c| c.track == other.0).count(), 1);
    }

    #[test]
    fn clips_play_whole_on_their_track_at_their_own_gain() {
        let (mut doc, audio) = setup(1);
        let id = doc.project().tracks[0].id;
        // The track's volume is the mixer's (`routing.rs`), not the clip's.
        doc.set_volume(id, -6.0);
        let clips = played(&doc, &audio, 48_000);
        assert_eq!(clips.len(), 1);
        assert_eq!((clips[0].offset, clips[0].length), (0, 96_000));
        assert_eq!((clips[0].gain, clips[0].track), (1.0, id.0));
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
            clip.loop_phase = Ticks(lumen_core::time::PPQ / 2);
            clip.fade_in.length = Ticks(lumen_core::time::PPQ / 2);
            clip.fade_in.offset = Ticks(lumen_core::time::PPQ / 4);
            clip.fade_in.span = Ticks::quarters(1);
            clip.fade_out.length = Ticks::quarters(1);
            clip.fade_out.offset = Ticks(lumen_core::time::PPQ / 4);
            clip.fade_out.span = Ticks::quarters(1);
            clip.fade_out.curve = lumen_core::curve::Curve::S_CURVE;
        });
        let clip = &played(&doc, &audio, 48_000)[0];
        assert_eq!(clip.length, 192_000, "repeats past the file's end");
        assert_eq!(clip.shape.period, Some(24_000));
        assert_eq!(clip.shape.phase, 12_000);
        assert_eq!(clip.shape.fade_in.frames, 12_000);
        assert_eq!(clip.shape.fade_in.offset, 6_000);
        assert_eq!(clip.shape.fade_in.span, 24_000);
        assert_eq!(clip.shape.fade_out.frames, 24_000);
        assert_eq!(clip.shape.fade_out.offset, 6_000);
        assert_eq!(clip.shape.fade_out.span, 24_000);
        assert_eq!(clip.shape.fade_out.curve, lumen_core::curve::Curve::S_CURVE);
        // Without the loop it stops where the file does.
        doc.apply("unloop", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            Arc::make_mut(&mut track.clips[0]).loop_length = None;
        });
        assert_eq!(played(&doc, &audio, 48_000)[0].length, 96_000);
    }

    #[test]
    fn split_loop_phase_uses_its_original_tempo_origin() {
        let (mut doc, audio) = setup(1);
        doc.apply("tempo and split", None, |p| {
            p.tempo = vec![
                lumen_core::time::TempoPoint {
                    at: Ticks::ZERO,
                    bpm: 120.0,
                    ramp: false,
                },
                lumen_core::time::TempoPoint {
                    at: Ticks::quarters(4),
                    bpm: 60.0,
                    ramp: false,
                },
            ];
            let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
            clip.start = Ticks::quarters(5);
            clip.length = Ticks::quarters(8);
            clip.loop_length = Some(Ticks::quarters(4));
            clip.loop_phase = Ticks::quarters(5);
        });
        let played = played(&doc, &audio, 48_000);
        assert_eq!(played[0].shape.period, Some(96_000));
        assert_eq!(played[0].shape.phase, 48_000);
    }

    #[test]
    fn a_split_loop_after_a_tempo_change_nulls_with_fades_and_envelopes() {
        let (mut doc, audio) = setup(1);
        let tempo_points = vec![
            lumen_core::time::TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            lumen_core::time::TempoPoint {
                at: Ticks::quarters(4),
                bpm: 60.0,
                ramp: false,
            },
        ];
        doc.apply("prepare loop", None, |p| {
            p.tempo = tempo_points.clone();
            let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
            clip.length = Ticks::quarters(12);
            clip.loop_length = Some(Ticks::quarters(4));
            clip.fade_in.length = Ticks::quarters(1);
            clip.fade_out.length = Ticks::quarters(1);
            clip.fade_out.curve = lumen_core::curve::Curve::S_CURVE;
            clip.envelopes.set(
                lumen_project::envelope::Kind::Gain,
                lumen_core::automation::Lane::new(vec![
                    lumen_core::automation::Point::new(Ticks::ZERO, -9.0),
                    lumen_core::automation::Point::new(Ticks::quarters(2), -1.0).with_shape(
                        lumen_core::automation::Shape::Curve(lumen_core::curve::Curve::S_CURVE),
                    ),
                    lumen_core::automation::Point::new(Ticks::quarters(4), -6.0),
                ])
                .unwrap(),
            );
            clip.envelopes.set(
                lumen_project::envelope::Kind::Pan,
                lumen_core::automation::Lane::new(vec![
                    lumen_core::automation::Point::new(Ticks::ZERO, -0.8),
                    lumen_core::automation::Point::new(Ticks::quarters(4), 0.7).with_shape(
                        lumen_core::automation::Shape::Curve(lumen_core::curve::Curve::FAST),
                    ),
                ])
                .unwrap(),
            );
        });
        let original = doc.project().clone();
        let tempo = lumen_core::time::TempoMap::new(original.tempo.clone()).unwrap();
        let cut = Ticks::quarters(5);
        let source = original.tracks[0].clips[0].as_ref();
        let left =
            lumen_project::edit::slice(&original, &tempo, source, source.start, cut).unwrap();
        let mut right = lumen_project::edit::slice(
            &original,
            &tempo,
            source,
            cut,
            source.start + source.length,
        )
        .unwrap();
        right.id = Id(99_999);
        let mut split = original.clone();
        Arc::make_mut(&mut split.tracks[0]).clips = vec![Arc::new(left.clone()), Arc::new(right)];
        let timeline = Timeline::new(tempo, original.meter_map());
        let render = |project: &Project, frames: usize, block: usize| {
            let list = engine_clips(project, 48_000, |_| Some((audio.clone(), 48_000)));
            let (mut clips, player) = clip_player();
            clips.set(list).unwrap();
            let (mut routing, source) = mixer(player);
            assert!(
                routing
                    .set(&Routing {
                        nodes: vec![NodeSpec::unity(
                            project.tracks[0].id.0,
                            Destination::Hardware(0)
                        )],
                        cue: None,
                        measure: None,
                    })
                    .unwrap()
            );
            let (mut handle, mut engine) = engine(timeline.clone(), source, 48_000);
            handle.send(Command::Play).unwrap();
            let mut data = vec![0.0; block * 2];
            let mut out = Vec::with_capacity(frames * 2);
            let mut done = 0;
            while done < frames {
                let n = block.min(frames - done);
                engine.process(48_000, &mut data[..2 * n], 2, n);
                out.extend_from_slice(&data[..2 * n]);
                done += n;
            }
            out
        };
        for block in [64, 128, 333, 4_096] {
            let frames = 500_000;
            let expected = render(&original, frames, block);
            let actual = render(&split, frames, block);
            if let Some(at) = actual.iter().zip(&expected).position(|(a, b)| a != b) {
                panic!(
                    "block {block} differs at interleaved sample {at}: {} != {}",
                    actual[at], expected[at]
                );
            }
        }
    }

    #[test]
    fn a_non_loop_split_keeps_curved_envelopes_exact() {
        use lumen_core::automation::{Point, Shape};
        use lumen_project::envelope::Kind;

        let (mut doc, audio) = setup(1);
        doc.apply("curved source envelope", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            let clip = Arc::make_mut(&mut track.clips[0]);
            clip.length = Ticks::quarters(8);
            clip.envelopes.set(
                Kind::Gain,
                lumen_core::automation::Lane::new(vec![
                    Point::new(Ticks::ZERO, -12.0)
                        .with_shape(Shape::Curve(lumen_core::curve::Curve::S_CURVE)),
                    Point::new(Ticks::quarters(4), -1.0),
                    Point::new(Ticks::quarters(8), -8.0),
                ])
                .unwrap(),
            );
            clip.envelopes.set(
                Kind::Pan,
                lumen_core::automation::Lane::new(vec![
                    Point::new(Ticks::ZERO, -0.9)
                        .with_shape(Shape::Curve(lumen_core::curve::Curve::FAST)),
                    Point::new(Ticks::quarters(8), 0.8),
                ])
                .unwrap(),
            );
        });
        let original = doc.project().clone();
        let tempo = lumen_core::time::TempoMap::new(original.tempo.clone()).unwrap();
        let source = original.tracks[0].clips[0].as_ref();
        let cut = Ticks::quarters(2);
        let left =
            lumen_project::edit::slice(&original, &tempo, source, source.start, cut).unwrap();
        let mut right = lumen_project::edit::slice(
            &original,
            &tempo,
            source,
            cut,
            source.start + source.length,
        )
        .unwrap();
        right.id = Id(99_998);
        let mut split = original.clone();
        Arc::make_mut(&mut split.tracks[0]).clips = vec![Arc::new(left), Arc::new(right)];
        let timeline = Timeline::new(tempo, original.meter_map());
        let render = |project: &Project, frames: usize, block: usize| {
            let list = engine_clips(project, 48_000, |_| Some((audio.clone(), 48_000)));
            let (mut clips, player) = clip_player();
            clips.set(list).unwrap();
            let (mut routing, source) = mixer(player);
            routing
                .set(&Routing {
                    nodes: vec![NodeSpec::unity(
                        project.tracks[0].id.0,
                        Destination::Hardware(0),
                    )],
                    cue: None,
                    measure: None,
                })
                .unwrap();
            let (mut handle, mut engine) = engine(timeline.clone(), source, 48_000);
            handle.send(Command::Play).unwrap();
            let mut data = vec![0.0; block * 2];
            let mut out = Vec::with_capacity(frames * 2);
            let mut done = 0;
            while done < frames {
                let n = block.min(frames - done);
                engine.process(48_000, &mut data[..2 * n], 2, n);
                out.extend_from_slice(&data[..2 * n]);
                done += n;
            }
            out
        };
        for block in [64, 128, 333, 4_096] {
            assert_eq!(
                render(&original, 100_000, block),
                render(&split, 100_000, block),
                "block {block}"
            );
        }
    }

    #[test]
    fn split_audio_keeps_the_exact_source_sample_at_44100_to_48000() {
        let (mut doc, _) = setup(1);
        let tempo_points = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 123.0,
            ramp: false,
        }];
        for (device_rate, native_rate) in [(48_000, 44_100), (44_100, 48_000)] {
            let samples: Vec<f32> = (0..520_000)
                .map(|i| ((i * 37 % 4_093) as f32 / 4_093.0) * 0.5)
                .collect();
            let audio = Arc::new(ClipAudio::Memory {
                sample_rate: device_rate,
                channels: vec![samples.clone(), samples],
            });
            for origin in [Ticks::ZERO, Ticks::quarters(3)] {
                doc.apply("prepare exact source split", None, |project| {
                    project.tempo = tempo_points.clone();
                    let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
                    clip.start = origin;
                    clip.length = Ticks::quarters(15);
                    if let ClipContent::Audio { offset, .. } = &mut clip.content {
                        *offset = 37;
                    }
                });
                let original = doc.project().clone();
                let tempo = lumen_core::time::TempoMap::new(original.tempo.clone()).unwrap();
                let source = original.tracks[0].clips[0].as_ref();
                let cut = origin + Ticks::quarters(7);
                let left =
                    lumen_project::edit::slice(&original, &tempo, source, origin, cut).unwrap();
                let mut right = lumen_project::edit::slice(
                    &original,
                    &tempo,
                    source,
                    cut,
                    origin + source.length,
                )
                .unwrap();
                right.id = Id(88_000 + origin.0 as u64);
                let mut split = original.clone();
                Arc::make_mut(&mut split.tracks[0]).clips = vec![Arc::new(left), Arc::new(right)];
                let render = |project: &Project, block: usize| {
                    let timeline = Timeline::new(
                        lumen_core::time::TempoMap::new(project.tempo.clone()).unwrap(),
                        project.meter_map(),
                    );
                    let list =
                        engine_clips(project, device_rate, |_| Some((audio.clone(), native_rate)));
                    let (mut clips, player) = clip_player();
                    clips.set(list).unwrap();
                    let (mut routing, source) = mixer(player);
                    routing
                        .set(&Routing {
                            nodes: vec![NodeSpec::unity(
                                project.tracks[0].id.0,
                                Destination::Hardware(0),
                            )],
                            cue: None,
                            measure: None,
                        })
                        .unwrap();
                    let (mut handle, mut engine) = engine(timeline, source, device_rate);
                    handle.send(Command::Play).unwrap();
                    let mut data = vec![0.0; block * 2];
                    let mut out = Vec::new();
                    for _ in 0..(480_000 / block) {
                        engine.process(device_rate, &mut data, 2, block);
                        out.extend_from_slice(&data);
                    }
                    out
                };
                for block in [64, 128, 333, 4_096] {
                    assert_eq!(
                        render(&original, block),
                        render(&split, block),
                        "origin {} block {block}",
                        origin.0
                    );
                }
            }
        }
    }

    fn with_envelopes(doc: &mut Document, edit: impl FnOnce(&mut lumen_project::model::Clip)) {
        doc.apply("envelope", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            edit(Arc::make_mut(&mut track.clips[0]));
        });
    }

    fn lane(points: &[(i64, f32)]) -> lumen_core::automation::Lane {
        lumen_core::automation::Lane::new(
            points
                .iter()
                .map(|&(q, v)| lumen_core::automation::Point::new(Ticks::quarters(q), v))
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn envelopes_reach_the_engine_on_the_clips_own_clock() {
        use lumen_project::envelope::Kind;
        let (mut doc, audio) = setup(1);
        // A clip from quarter 2 with a gain envelope from 0 to −12 dB over its
        // first two quarters (a second at 120 BPM), and a pan to the right.
        with_envelopes(&mut doc, |clip| {
            clip.start = Ticks::quarters(2);
            clip.envelopes
                .set(Kind::Gain, lane(&[(0, 0.0), (2, -12.0)]));
            clip.envelopes.set(Kind::Pan, lane(&[(0, 1.0)]));
        });
        let clip = &played(&doc, &audio, 48_000)[0];
        let gain = clip.envelope.gain.as_ref().unwrap();
        assert_eq!(gain.points()[0].at, 0, "frame 0 is the clip's first frame");
        assert_eq!(gain.value_at(48_000), -12.0);
        assert_eq!(gain.value_at(24_000), -6.0);
        // A stereo file balances.
        let (_, law) = clip.envelope.pan.as_ref().unwrap();
        assert_eq!(*law, lumen_core::pan::PanLaw::Balance);
        // A clip without envelopes plays the plain path.
        with_envelopes(&mut doc, |clip| clip.envelopes = Default::default());
        assert!(played(&doc, &audio, 48_000)[0].envelope.is_empty());
    }

    #[test]
    fn a_mono_clip_pans_by_the_projects_law() {
        use lumen_project::envelope::Kind;
        let (mut doc, _) = setup(1);
        with_envelopes(&mut doc, |clip| {
            clip.envelopes.set(Kind::Pan, lane(&[(0, -0.5)]));
        });
        let mono = Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000]],
        });
        let clip = &played(&doc, &mono, 48_000)[0];
        let (_, law) = clip.envelope.pan.as_ref().unwrap();
        assert_eq!(*law, doc.project().pan_law);
    }

    #[test]
    fn a_looping_clips_envelope_keeps_hidden_source_knots_and_tempo_shape() {
        use lumen_project::envelope::Kind;
        let (mut doc, audio) = setup(1);
        // Looped every two quarters, with points past the repeat.
        with_envelopes(&mut doc, |clip| {
            clip.length = Ticks::quarters(8);
            clip.loop_length = Some(Ticks::quarters(2));
            clip.envelopes
                .set(Kind::Gain, lane(&[(0, 0.0), (1, -20.0), (6, 0.0)]));
        });
        let gain = played(&doc, &audio, 48_000)[0]
            .envelope
            .gain
            .clone()
            .unwrap();
        // The engine repeats every 48,000 frames, while the baked source lane
        // keeps its hidden later knot so slices can preserve curve identity.
        assert!(gain.points().iter().any(|p| p.at == 144_000));
        assert_eq!(gain.value_at(24_000), -20.0);
        assert_eq!(gain.value_at(144_000), 0.0);
        // The same envelope under a slower tempo from the start of the clip
        // is twice as long in frames, and still passes through its points.
        doc.apply("slow", None, |p| p.tempo[0].bpm = 60.0);
        let slow = played(&doc, &audio, 48_000)[0]
            .envelope
            .gain
            .clone()
            .unwrap();
        assert_eq!(slow.value_at(48_000), -20.0);
    }

    #[test]
    fn missing_audio_plays_nothing_and_track_mutes_are_the_mixers() {
        let (mut doc, audio) = setup(3);
        let ids: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        // Mute and solo silence tracks in the mixer (`routing.rs`): every
        // clip is still sent at its own gain.
        doc.set_mute(&ids[..1], true);
        doc.set_solo(&ids[2..], true, true);
        let clips = played(&doc, &audio, 48_000);
        assert!(clips.iter().all(|c| c.gain == 1.0));
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

    #[test]
    fn files_dropped_together_join_in_drop_order_whatever_order_they_load_in() {
        let mut doc = Document::new(crate::empty_project());
        let mut lanes = Lanes::default();
        let names = ["kick", "snare", "hat"];
        let ids: Vec<u64> = names.iter().map(|_| doc.fresh_id().0).collect();
        // Kick and snare dropped together onto new tracks, then the hat in
        // a second drop, as `add_files` queues them.
        for (i, (&id, name)) in ids.iter().zip(names).enumerate() {
            let drop = if i < 2 { 1 } else { 2 };
            lanes.library.insert_loading(id, name, 48_000);
            lanes.pending.push((id, Ticks::ZERO));
            lanes.before.insert(
                id,
                (
                    DropInto::NewTracks {
                        before: None,
                        group: None,
                    },
                    drop,
                ),
            );
        }
        let channels = vec![vec![0.5; 96_000]; 2];
        let peaks = Arc::new(
            lumen_media::Peaks::build(&channels, &std::sync::atomic::AtomicBool::new(false))
                .expect("not cancelled"),
        );
        let audio = Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels,
        });
        let tracks = |doc: &Document| -> Vec<String> {
            doc.project()
                .tracks
                .iter()
                .map(|t| t.name.clone())
                .collect()
        };

        // The hat loads first: it waits for the earlier drop. Then the
        // kick: it waits for the snare, dropped with it.
        for (i, why) in [(2, "waits for the earlier drop"), (0, "waits for its drop")] {
            lanes
                .library
                .insert_ready(ids[i], names[i], audio.clone(), peaks.clone());
            lanes.join_loaded(&mut doc);
            assert!(tracks(&doc).is_empty(), "{why}");
            assert_eq!(lanes.pending().len(), 3, "still shown as loading");
        }
        lanes
            .library
            .insert_ready(ids[1], names[1], audio.clone(), peaks.clone());
        lanes.join_loaded(&mut doc);
        assert_eq!(tracks(&doc), names, "the order they were dropped in");
        assert!(lanes.pending().is_empty() && lanes.before.is_empty());
        assert_eq!(doc.history().steps().0.len(), 1, "one step");
    }

    /// The owner's MT-MIDI-2 finding: choosing a preset must reach the
    /// engine at once. Between the edit and the next block there is one
    /// `Lanes::update` (a frame) and nothing else: the first note after
    /// the edit is, sample for sample, the one the new preset plays.
    #[test]
    fn a_preset_change_reaches_the_engine_in_one_update() {
        use lumen_devices::Preset;
        use lumen_engine::{Command, Layered, Timeline, clip_player, engine, midi_player, mixer};
        use lumen_project::testing::{midi_track, project_of};
        let q = Ticks::quarters;
        let project = |preset: &str| {
            project_of(
                "Preset",
                vec![midi_track(
                    1,
                    "Keys",
                    preset,
                    (0, 8),
                    None,
                    vec![lumen_project::model::Note::new(q(1), q(1), 60, 100)],
                    vec![],
                )],
            )
        };
        let ctx = egui::Context::default();
        let render = |from: &str, to: &str| -> Vec<f32> {
            let mut doc = Document::new(project(from));
            let (mut clips, players) = clip_player();
            let (mut midi, player) = midi_player();
            let (mut routing, source) = mixer(Layered(players, player));
            let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
            let mut lanes = Lanes::default();
            let (cue, inputs) = (Default::default(), Default::default());
            let mut hosting = crate::hosting::Hosting::off();
            let mut update = |lanes: &mut Lanes, doc: &mut Document| {
                lanes.update(
                    48_000,
                    &ctx,
                    doc,
                    Handles {
                        clips: &mut clips,
                        midi: &mut midi,
                        mixer: &mut routing,
                        hosting: &mut hosting,
                    },
                    &cue,
                    &inputs,
                );
            };
            update(&mut lanes, &mut doc);
            transport.send(Command::Play).unwrap();
            let mut out = Vec::new();
            let mut data = vec![0.0; 256];
            let mut block = |out: &mut Vec<f32>| {
                engine.process(48_000, &mut data, 2, 128);
                out.extend((0..128).map(|i| data[2 * i]));
            };
            // Well before the note.
            for _ in 0..100 {
                block(&mut out);
            }
            let id = doc.project().tracks[0].id;
            let preset = Preset::ALL.iter().find(|p| p.id() == to).copied();
            doc.set_instrument(&[id], preset);
            // One frame: one update; then audio.
            update(&mut lanes, &mut doc);
            for _ in 0..(24_000 + 12_000) / 128 - 100 {
                block(&mut out);
            }
            out
        };
        let changed = render("poly", "pad");
        assert_eq!(changed, render("pad", "pad"), "the new sound, at once");
        assert_ne!(changed, render("poly", "poly"));
        assert!(changed.iter().any(|v| v.abs() > 0.01), "and it sounds");
    }
}
