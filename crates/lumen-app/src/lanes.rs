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
use lumen_engine::{Clip, ClipsHandle, MidiHandle, MixerHandle};
use lumen_project::model::{ClipContent, Id, Project};

use crate::document::{Document, DropInto, Imported};
use crate::media::{self, Item, Library, State};
use crate::theme;

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

#[derive(Default)]
pub struct Lanes {
    pub library: Library,
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
    /// The project state the engine's MIDI tracks were built from, and
    /// whether they need sending again (M5 slice 1).
    midi_built: Option<(u64, crate::midi_play::Inputs, u32)>,
    midi_dirty: bool,
    midi_cache: crate::midi_play::Cache,
    /// The MIDI outputs tracks name, by the numbers the engine uses.
    pub(crate) midi_ports: crate::midi_play::PortTable,
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
    /// The device rate at the last update: the rate audio is held at.
    rate: u32,
}

/// What the engine plays (audit M4: the model-to-engine translation, kept
/// pure so it is tested): every audio clip whose audio is loaded (`audio`
/// gives the audio at the device rate and the file's own rate), for the
/// shorter of the clip and the file, at its clip gain, on its track. The
/// track's level, pan, mute and solo are in the routing (`routing.rs`).
pub fn engine_clips(
    project: &Project,
    rate: u32,
    audio: impl Fn(Id) -> Option<(std::sync::Arc<lumen_engine::ClipAudio>, u32)>,
) -> Vec<Clip> {
    let tempo = lumen_core::time::TempoMap::new(project.tempo.clone()).ok();
    let mut list = Vec::new();
    for track in &project.tracks {
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
                    curve: clip.fade_in.curve,
                },
                fade_out: lumen_engine::FadeFrames {
                    frames: frames(end - clip.fade_out.length, end),
                    curve: clip.fade_out.curve,
                },
                period: clip
                    .loop_length
                    .map(|l| frames(clip.start, clip.start + l))
                    .filter(|p| *p > 0),
            };
            if length > 0 {
                list.push(Clip {
                    id: clip.id.0,
                    track: track.id.0,
                    start: clip.start,
                    offset,
                    length,
                    // Muted clips stay at gain 0, so they fade out and
                    // back in rather than clicking.
                    gain: if clip.muted {
                        0.0
                    } else {
                        crate::routing::db_to_gain(*gain_db)
                    },
                    audio: samples,
                    shape,
                });
            }
        }
    }
    list
}

impl Lanes {
    /// A plugin instrument is ready for the next MIDI track list (M6
    /// slice 2c).
    pub fn midi_changed(&mut self, changed: bool) {
        self.midi_dirty |= changed;
    }

    /// Files are still loading for import.
    /// Files dropped for import are still loading (not an opened
    /// project's media: `library.is_loading()` covers every load).
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
        inputs: &crate::midi_play::Inputs,
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
        // Sent again when the project, the selection or the inputs change
        // (which tracks listen to what), never every frame.
        // The lanes of plugin instruments are baked for the device's rate.
        let midi_wanted = (doc.state(), inputs, rate);
        if self.midi_dirty
            || self
                .midi_built
                .as_ref()
                .is_none_or(|(s, i, r)| (*s, i, *r) != midi_wanted)
        {
            let mut tracks = crate::midi_play::engine_midi_with(
                doc.project(),
                inputs,
                &mut self.midi_ports,
                &mut self.midi_cache,
                hosting,
            );
            crate::midi_play::with_lanes(&mut tracks, doc.project(), rate);
            self.midi_dirty = match midi.set(tracks) {
                Ok(()) => false,
                // Full: a plugin instrument in the list is kept for the
                // next one, not lost with it.
                Err(tracks) => {
                    hosting.reclaim(tracks);
                    true
                }
            };
            self.midi_built = Some((doc.state(), inputs.clone(), rate));
        }
        // The routing follows the clips, never ahead of them: a deleted
        // track's clips fade out while its node is still there, and a new
        // track's clips wait for their node, then fade in.
        // Lanes are baked for the device's rate, so a new rate is a new routing.
        let wanted = (doc.state(), cue.clone(), self.devices.clone(), rate);
        if !self.dirty && self.routed.as_ref() != Some(&wanted) {
            match mixer.set(&crate::routing::routing(
                doc.project(),
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
                at,
                into,
            })
        }
        (State::Failed(_), ..) => Load::Gone,
        _ => Load::Loading,
    }
}

/// The hover card for a clip's file; `properties` is the properties
/// card's shortcut, if it has one.
pub fn file_card(ui: &mut egui::Ui, item: &Item, properties: Option<&str>) {
    ui.label(RichText::new(&item.name).strong());
    file_facts(ui, item);
    // Every gesture on a clip, so none stays hidden (owner, after 6c: slip
    // was not discoverable).
    ui.separator();
    for line in [
        "Header: drag to move · Ctrl copies · Ctrl+Alt slips the audio",
        "Body: drag to select time · Ctrl+Shift selects clips",
        "Sides: drag to trim · Shift loops",
        "Top corners: fades · their middle dot bends them · the line sets gain",
        "Right-click: edits, colour, mute, lock and fade shapes",
    ] {
        ui.label(RichText::new(line).small().weak());
    }
    // The way into the properties card (slice 6e, the owner's P1), with
    // its shortcut as the keymap has it.
    if let Some(keys) = properties {
        ui.label(
            RichText::new(format!("{keys}: edit its properties"))
                .small()
                .color(theme::p().accent),
        );
    }
}

/// What a clip's file is: format, rate, channels, length, size, any
/// conversion or failure, and its path. The hover card and the clip's
/// properties card show it.
pub fn file_facts(ui: &mut egui::Ui, item: &Item) {
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
    // The size arrives with the load.
    if info.size_bytes > 0 {
        facts.push(format!("{:.1} MB", info.size_bytes as f64 / 1_048_576.0));
    }
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
    // Shortened in the middle, so the file's name always shows (the
    // interface audit's S6: cut at its end, the name was what went).
    let font = egui::FontId::proportional(theme::size::CAPTION);
    let room = ui.available_width();
    let shown = shorten_middle(&item.path, |text| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), theme::p().text_hint)
            .size()
            .x
            <= room
    });
    ui.add(egui::Label::new(RichText::new(shown).small().weak()).truncate())
        .on_hover_text(item.path.display().to_string());
}

/// `path` as it `fits`: whole if it can be, otherwise with the folders
/// nearest the file left out as "…", keeping the start and the file's name.
fn shorten_middle(path: &std::path::Path, fits: impl Fn(&str) -> bool) -> String {
    let whole = path.display().to_string();
    if fits(&whole) {
        return whole;
    }
    use std::path::Component;
    let sep = std::path::MAIN_SEPARATOR_STR;
    // The drive and root ("C:\", "/") lead the first folder; the rest are
    // names.
    let mut root = String::new();
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => root.push_str(&p.as_os_str().to_string_lossy()),
            Component::RootDir => root.push_str(sep),
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    let Some((name, folders)) = parts.split_last() else {
        return whole;
    };
    for keep in (1..folders.len()).rev() {
        let text = format!(
            "{root}{}{sep}\u{2026}{sep}{name}",
            folders[..keep].join(sep)
        );
        if fits(&text) {
            return text;
        }
    }
    format!("\u{2026}{sep}{name}")
}

#[cfg(test)]
mod path_tests {
    use super::shorten_middle;
    use std::path::PathBuf;

    #[test]
    fn a_long_path_keeps_its_start_and_its_file_name() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let path: PathBuf = ["base", "Users", "someone", "Music", "Samples", "kick.wav"]
            .iter()
            .collect();
        assert_eq!(shorten_middle(&path, |_| true), path.display().to_string());
        let short = shorten_middle(&path, |t| t.chars().count() <= 26);
        assert!(
            short.ends_with(&format!("\u{2026}{sep}kick.wav")),
            "{short}"
        );
        assert!(short.starts_with("base"), "{short}");
        assert!(short.chars().count() <= 26, "{short}");
        assert_eq!(
            shorten_middle(&path, |_| false),
            format!("\u{2026}{sep}kick.wav")
        );
        // From the root: one separator after it, never two.
        let rooted = PathBuf::from(sep).join(&path);
        let short = shorten_middle(&rooted, |t| t.chars().count() <= 27);
        assert!(short.starts_with(&format!("{sep}base")), "{short}");
        assert!(!short.contains(&format!("{sep}{sep}")), "{short}");
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
                into: DropInto::NewTracks {
                    before: None,
                    group: None,
                },
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
            clip.fade_in.length = Ticks(lumen_core::time::PPQ / 2);
            clip.fade_out.length = Ticks::quarters(1);
            clip.fade_out.curve = lumen_core::curve::Curve::S_CURVE;
        });
        let clip = &played(&doc, &audio, 48_000)[0];
        assert_eq!(clip.length, 192_000, "repeats past the file's end");
        assert_eq!(clip.shape.period, Some(24_000));
        assert_eq!(clip.shape.fade_in.frames, 12_000);
        assert_eq!(clip.shape.fade_out.frames, 24_000);
        assert_eq!(clip.shape.fade_out.curve, lumen_core::curve::Curve::S_CURVE);
        // Without the loop it stops where the file does.
        doc.apply("unloop", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            Arc::make_mut(&mut track.clips[0]).loop_length = None;
        });
        assert_eq!(played(&doc, &audio, 48_000)[0].length, 96_000);
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
        let audio = Arc::new(ClipAudio {
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
