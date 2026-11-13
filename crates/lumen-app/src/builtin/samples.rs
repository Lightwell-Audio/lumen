//! The sampler's samples in the app (M8 slice B5): decoding them off the audio
//! thread, keeping the buffers, and building the sound the engine is handed.
//!
//! A file is decoded and converted to the device's rate by `lumen-media` on a
//! worker, exactly as an imported clip is. What the engine gets is
//! [`lumen_devices::sampler::Sound`]: an immutable bank of the regions, zones
//! and pads that face plays, every one holding a share of a decoded buffer.
//!
//! **Who owns a buffer.** This library holds one share of every sample it has
//! decoded for as long as the document is open, and it drops them on the
//! interface thread. The engine's shares travel in the sound (freed off the
//! audio thread with the track list that held it) and in each sounding voice,
//! so the audio thread is never the last owner of a buffer and never frees
//! one. The allocation-guard test in `lumen-devices` proves the render path
//! allocates and frees nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};

use lumen_devices::sampler::{Face, Pad, Region, Sample, Sound, Zone};
use lumen_project::model::{Device, DrumMap, Id, Media, Sampler};

/// One sample decoded for the engine, at the rate it runs at.
#[derive(Debug, Clone)]
pub struct Source {
    /// The file, at the device's rate.
    pub sample: Arc<Sample>,
    /// The rate the file is in, which its regions are measured in.
    pub file_rate: u32,
    /// Frames of the file.
    pub frames: u64,
}

/// A file that finished decoding, ready to join a sampler.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The media entry it becomes.
    pub media: Id,
    pub path: PathBuf,
    pub name: String,
    pub source: Source,
    /// Where `path` came from, for the media pool's original path.
    pub size_bytes: u64,
}

/// How many files decode at once.
const WORKERS: usize = 4;

/// Files a person asked for, on their way into a sampler (M8 slice B5): the
/// ids asked for and the device they go to, until every one has answered.
#[derive(Debug, Default)]
pub struct Loading {
    waiting: Option<Waiting>,
}

#[derive(Debug)]
struct Waiting {
    device: Id,
    ids: Vec<Id>,
    /// When the ask began: a worker that never answers is said so after a
    /// while rather than waited on for ever.
    since: std::time::Instant,
}

/// What happened to one ask: the files that arrived, and why the others did
/// not, and the device they were for.
#[derive(Debug)]
pub struct Arrived {
    pub device: Id,
    pub files: Vec<Loaded>,
    pub failed: Vec<String>,
}

impl Loading {
    /// Starts waiting for `ids`, which go to `device`.
    pub fn start(&mut self, device: Id, ids: Vec<Id>) {
        self.waiting = Some(Waiting {
            device,
            ids,
            since: std::time::Instant::now(),
        });
    }

    /// Whether files are still on their way.
    pub fn is_loading(&self) -> bool {
        self.waiting.is_some()
    }

    /// Takes what finished. `Some` once every file asked for has answered (or
    /// one could not); `None` while they are still making up their minds.
    pub fn take_ready(&mut self, library: &mut Library) -> Option<Arrived> {
        let waiting = self.waiting.as_ref()?;
        let mut files = Vec::new();
        let mut failed = Vec::new();
        for loaded in library.take_ready() {
            if waiting.ids.contains(&loaded.media) {
                files.push(loaded);
            }
        }
        let mut left = 0;
        for id in &waiting.ids {
            if files.iter().any(|f| f.media == *id) {
                continue;
            }
            match library.failed(*id) {
                Some(why) => failed.push(why.to_owned()),
                None => left += 1,
            }
        }
        // A worker that never answers is given up on after a while: a sample
        // that will not arrive never holds the person's edit open.
        let gave_up = waiting.since.elapsed() > std::time::Duration::from_secs(30);
        if left > 0 && !gave_up {
            // Keep the files that did arrive for the next frame.
            for file in files {
                library.ready.push(file);
            }
            return None;
        }
        let device = waiting.device;
        self.waiting = None;
        Some(Arrived {
            device,
            files,
            failed,
        })
    }
}

/// A decode under way.
struct Working {
    media: Id,
    rx: Receiver<Result<Loaded, (PathBuf, String)>>,
}

/// What a document's samplers play, decoded.
#[derive(Default)]
pub struct Library {
    /// The rate the buffers are at.
    rate: u32,
    /// Every sample decoded, by the media entry it came from.
    loaded: BTreeMap<Id, Source>,
    /// Files decoding now, and the ids asked for (so one file is decoded
    /// once).
    working: Vec<Working>,
    asked: BTreeSet<Id>,
    /// What finished and has not been taken yet.
    ready: Vec<Loaded>,
    /// Why a file could not be decoded, by media id and the path tried, until
    /// it is asked for at another place (a relink).
    failed: Vec<(Id, PathBuf, String)>,
    /// A sample arrived since this was last read, so what the engine plays is
    /// built again (a relinked file reaches the audio through this).
    arrived: bool,
}

impl Library {
    /// Every sample this library holds, for a render that has no interface
    /// behind it (M8 slice B5): the buffers are shared, never copied.
    pub fn snapshot(&self) -> Vec<(Id, Source)> {
        self.loaded
            .iter()
            .map(|(id, source)| (*id, source.clone()))
            .collect()
    }

    /// A library holding `sources`, for a render (M8 slice B5).
    pub fn from_snapshot(rate: u32, sources: Vec<(Id, Source)>) -> Self {
        Self {
            rate,
            loaded: sources.into_iter().collect(),
            ..Self::default()
        }
    }

    /// The sample of a media entry, when it is loaded.
    pub fn get(&self, media: Id) -> Option<&Source> {
        self.loaded.get(&media)
    }

    /// Asks for `media` to be decoded from `path` at `rate`. A file already
    /// loaded or being decoded is left alone; one that failed is tried again
    /// only at another path (a relinked file).
    pub fn load(&mut self, media: Id, path: PathBuf, rate: u32) {
        if self.loaded.contains_key(&media) || self.asked.contains(&media) {
            return;
        }
        if self
            .failed
            .iter()
            .any(|(id, tried, _)| *id == media && *tried == path)
        {
            return;
        }
        self.failed.retain(|(id, _, _)| *id != media);
        if self.working.len() >= WORKERS {
            // Asked for again next frame: the document's own loop keeps
            // asking for the files it plays.
            return;
        }
        let (tx, rx) = mpsc::channel();
        let name = path.file_stem().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let size_bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        let worker_name = format!("lumen-sampler-{:x}", media.0);
        let tries = path.clone();
        let spawned = crate::spawn::spawn_named(&worker_name, move || {
            let result = decode(&path, rate, media)
                .map(|mut loaded| {
                    loaded.name = name;
                    loaded.size_bytes = size_bytes;
                    loaded
                })
                .map_err(|why| (tries, why));
            let _ = tx.send(result);
        });
        match spawned {
            Ok(handle) => {
                drop(handle);
                self.asked.insert(media);
                self.working.push(Working { media, rx });
            }
            Err(error) => self.failed.push((
                media,
                PathBuf::new(),
                crate::spawn::start_failed("the sample's decoder", &error),
            )),
        }
    }

    /// Takes the files that finished decoding: the app joins them to the
    /// project, as one undo step. Answers that arrived are read here, so call
    /// it every frame.
    pub fn update(&mut self, rate: u32) {
        if rate != 0 && rate != self.rate {
            // The device runs at another rate: every buffer is made again.
            if self.rate != 0 {
                self.loaded.clear();
                self.asked.clear();
                self.failed.clear();
            }
            self.rate = rate;
        }
        let mut still = Vec::new();
        for working in std::mem::take(&mut self.working) {
            match working.rx.try_recv() {
                Ok(result) => {
                    self.asked.remove(&working.media);
                    match result {
                        Ok(loaded) => {
                            self.loaded.insert(loaded.media, loaded.source.clone());
                            self.ready.push(loaded);
                            self.arrived = true;
                        }
                        Err((path, why)) => self.failed.push((working.media, path, why)),
                    }
                }
                // Still decoding. It ended without an answer only if its
                // thread is gone, which the spawn would have refused.
                Err(mpsc::TryRecvError::Empty) => still.push(working),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.asked.remove(&working.media);
                }
            }
        }
        self.working = still;
    }

    /// Whether a sample arrived since this was last asked, which is when what
    /// the engine plays is built again.
    pub fn take_arrived(&mut self) -> bool {
        std::mem::take(&mut self.arrived)
    }

    /// The files that finished decoding, taken once.
    pub fn take_ready(&mut self) -> Vec<Loaded> {
        std::mem::take(&mut self.ready)
    }

    /// Why a sample could not be decoded, if it could not.
    pub fn failed(&self, media: Id) -> Option<&str> {
        self.failed
            .iter()
            .find(|(id, _, _)| *id == media)
            .map(|(_, _, why)| why.as_str())
    }

    /// What `device` plays now, built from the model and the buffers here
    /// (M8 slice B5): the face it shows, and that face's regions, zones and
    /// pads. `map` is the drum map of the track it is on, whose choke groups
    /// its pads answer to.
    ///
    /// A region whose file is not loaded is left out, so a missing sample
    /// plays nothing rather than the wrong sound; the media pool is where the
    /// person finds it and relinks it.
    pub fn sound(&self, device: &Device, map: Option<&DrumMap>) -> Option<Arc<Sound>> {
        let held = device.sampler()?;
        Some(Arc::new(self.build(held, map)))
    }

    /// How many bytes of samples are held.
    pub fn memory(&self) -> usize {
        self.loaded.values().map(|s| s.sample.memory()).sum()
    }

    /// [`Library::sound`] for a sampler's sound without a device.
    pub fn build(&self, held: &Sampler, map: Option<&DrumMap>) -> Sound {
        let mut sound = Sound {
            face: match held.face {
                lumen_project::model::Face::OneShot => Face::OneShot,
                lumen_project::model::Face::Zones => Face::Zones,
                lumen_project::model::Face::Pads => Face::Pads,
            },
            ..Sound::default()
        };
        if let Some(region) = held.one_shot.region {
            sound.one_shot = self.region(&region).map(|mut region| {
                region.looping = held.one_shot.looping;
                region.reversed = held.one_shot.reversed;
                region
            });
        }
        for zone in &held.zones {
            if let Some(region) = self.region_of(zone.media, 0, None, zone.root) {
                sound.zones.push(Zone {
                    region,
                    low: zone.low,
                    high: zone.high,
                    low_velocity: zone.low_velocity,
                    high_velocity: zone.high_velocity,
                });
            }
        }
        for pad in &held.pads {
            let Some(media) = pad.media else {
                continue;
            };
            let Some(region) = self.region_of(media, 0, None, pad.root()) else {
                continue;
            };
            sound.pads.push(Pad {
                note: pad.note,
                region: Some(region),
                gain: db_to_gain(pad.level_db),
                pan: pad.pan,
                tune: pad.tune,
                output: pad.output,
                choke_fade: pad.choke_fade,
                choke: map
                    .and_then(|map| map.sound(pad.note))
                    .and_then(|s| s.choke),
            });
        }
        sound
    }

    /// The region of `region`, in frames of the engine's rate.
    fn region(&self, region: &lumen_project::model::Region) -> Option<Region> {
        self.region_of(region.media, region.start, region.end, region.root)
    }

    /// The region `start..end` of `media`, in frames of the engine's rate,
    /// played at `root`.
    fn region_of(&self, media: Id, start: u64, end: Option<u64>, root: u8) -> Option<Region> {
        let source = self.loaded.get(&media)?;
        let rate = u64::from(source.sample.rate().max(1));
        let file_rate = u64::from(source.file_rate.max(1));
        let frames = source.sample.frames() as u64;
        // The region is measured in frames of the file, as a waveform shows
        // it; the buffer is at the engine's rate, so both ends move with it.
        let at = |frame: u64| ((frame * rate) / file_rate).min(frames);
        let start = at(start);
        let end = end.map_or(frames, at);
        if end <= start {
            return None;
        }
        Some(Region {
            sample: Arc::clone(&source.sample),
            start: start as usize,
            end: end as usize,
            root,
            looping: false,
            reversed: false,
        })
    }
}

/// A level in decibels as a gain.
fn db_to_gain(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        (10.0_f64).powf(f64::from(db) / 20.0) as f32
    }
}

/// Decodes `path` at `rate` into a sample (on a worker).
fn decode(path: &std::path::Path, rate: u32, media: Id) -> Result<Loaded, String> {
    let cancel = AtomicBool::new(false);
    // The file's own rate, which its regions are measured in, and its length
    // as its header says it (a format that does not say falls back to what
    // was decoded).
    let header = lumen_media::probe(path).ok();
    let file_rate = header
        .as_ref()
        .and_then(|h| h.sample_rate)
        .unwrap_or(rate)
        .max(1);
    let audio = lumen_media::load(path, rate, &cancel, &mut |_| {})
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let channels = audio.channels().min(2);
    let frames = audio.frames();
    // Channel-major, as the engine reads it; a file of more than two channels
    // plays its first two.
    let mut data = Vec::with_capacity(frames * channels);
    for channel in 0..channels {
        data.extend_from_slice(audio.channel(channel));
    }
    let sample = Sample::new(rate, channels as u8, frames, data)
        .ok_or_else(|| format!("{}: the decoder gave no frames", path.display()))?;
    let file_frames = header
        .as_ref()
        .and_then(|h| h.frames)
        .unwrap_or((frames as u64 * u64::from(file_rate)) / u64::from(rate.max(1)));
    Ok(Loaded {
        media,
        path: path.to_path_buf(),
        name: String::new(),
        source: Source {
            sample: Arc::new(sample),
            file_rate,
            frames: file_frames,
        },
        size_bytes: 0,
    })
}

/// Which notes a drop of `files` fills: a file whose name is a sound's takes
/// that pad, and the rest go into the pads left free, in order (M8 slice B5:
/// "map them by name or by order"). `named` is each pad's note and the name
/// the track's drum map gives it, when it gives one.
pub fn place_by_name_or_order(named: &[(u8, Option<&str>)], files: &[String]) -> Vec<(u8, usize)> {
    let mut placed: Vec<(u8, usize)> = Vec::new();
    let mut taken = [false; 256];
    // The names first: a file that says which sound it is goes there.
    let mut spare: Vec<usize> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let key = normalise(file);
        let found = named.iter().find(|(note, name)| {
            !taken[usize::from(*note)]
                && name.is_some_and(|name| {
                    let name = normalise(name);
                    !name.is_empty() && (name == key || key.contains(&name))
                })
        });
        match found {
            Some(&(note, _)) => {
                taken[usize::from(note)] = true;
                placed.push((note, index));
            }
            None => spare.push(index),
        }
    }
    // Then the rest, in order, into the pads nothing has taken.
    for (note, _) in named {
        let Some(index) = spare.first().copied() else {
            break;
        };
        if taken[usize::from(*note)] {
            continue;
        }
        taken[usize::from(*note)] = true;
        placed.push((*note, index));
        spare.remove(0);
    }
    placed.sort_by_key(|(note, _)| *note);
    placed
}

/// A name reduced to what a person would type: letters and digits, lower
/// case, so "Closed Hat" and "closed_hat" are the same.
fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A media entry for a loaded file, so the pool, Relink and Collect know it.
pub fn media_entry(loaded: &Loaded) -> Media {
    Media {
        id: loaded.media,
        path: loaded.path.to_string_lossy().into_owned(),
        original_path: None,
        frames: loaded.source.frames,
        channels: loaded.source.sample.channels().into(),
        sample_rate: loaded.source.file_rate,
        size_bytes: loaded.size_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::{Face as ModelFace, OneShot, Pad as ModelPad};
    use lumen_project::testing::TempDir;

    fn wav(dir: &TempDir, name: &str, frames: usize, rate: u32) -> PathBuf {
        let path = dir.path().join(name);
        let spec = lumen_media::wav::Spec {
            channels: 1,
            sample_rate: rate,
            sample: lumen_media::wav::Sample::F32,
        };
        let mut writer = lumen_media::wav::Writer::create(&path, spec).expect("a file");
        let data: Vec<f32> = (0..frames).map(|k| (k as f32) * 0.001).collect();
        writer.write(&[&data]).expect("the frames");
        writer.finish().expect("the file");
        path
    }

    fn settled(library: &mut Library, rate: u32) -> Vec<Loaded> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            library.update(rate);
            let ready = library.take_ready();
            if !ready.is_empty() {
                return ready;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the file decoded in time"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_file_decodes_to_a_sample_at_the_engine_s_rate() {
        let dir = TempDir::new("sampler-decode");
        let file = wav(&dir, "one.wav", 4_800, 48_000);
        let mut library = Library::default();
        library.load(Id(1), file, 48_000);
        let ready = settled(&mut library, 48_000);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].source.frames, 4_800);
        assert_eq!(ready[0].source.file_rate, 48_000);
        assert_eq!(ready[0].source.sample.frames(), 4_800);
        assert_eq!(ready[0].source.sample.rate(), 48_000);
        assert!(library.get(Id(1)).is_some());
    }

    #[test]
    fn a_file_at_another_rate_is_converted_and_its_regions_keep_their_place() {
        let dir = TempDir::new("sampler-resample");
        let file = wav(&dir, "one.wav", 24_000, 48_000);
        let mut library = Library::default();
        library.load(Id(1), file, 96_000);
        let ready = settled(&mut library, 96_000);
        assert_eq!(ready[0].source.file_rate, 48_000, "the file's own rate");
        let sample = &ready[0].source.sample;
        assert_eq!(sample.rate(), 96_000);
        // Half a second at either rate.
        assert!(
            (sample.frames() as i64 - 48_000).abs() < 200,
            "{}",
            sample.frames()
        );
        // A region at 12,000 frames of the file is at 24,000 of the buffer.
        library.loaded.insert(Id(1), ready[0].source.clone());
        let held = Sampler {
            face: ModelFace::OneShot,
            one_shot: OneShot {
                region: Some(lumen_project::model::Region {
                    media: Id(1),
                    start: 12_000,
                    end: Some(18_000),
                    root: 60,
                }),
                looping: false,
                reversed: false,
            },
            ..Sampler::default()
        };
        let sound = library.build(&held, None);
        let region = sound.one_shot.expect("the region");
        assert_eq!(region.start, 24_000);
        assert_eq!(region.end, 36_000);
    }

    #[test]
    fn a_missing_file_plays_nothing_and_says_why() {
        let mut library = Library::default();
        library.load(Id(9), PathBuf::from("no/such/file.wav"), 48_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while library.failed(Id(9)).is_none() {
            library.update(48_000);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(library.get(Id(9)).is_none());
        assert!(library.failed(Id(9)).is_some());
        let held = Sampler {
            one_shot: OneShot {
                region: Some(lumen_project::model::Region::whole(Id(9), 60)),
                ..OneShot::default()
            },
            ..Sampler::default()
        };
        assert!(library.build(&held, None).one_shot.is_none());
    }

    #[test]
    fn a_pad_takes_its_level_and_its_choke_group() {
        let mut library = Library::default();
        library.loaded.insert(
            Id(1),
            Source {
                sample: Arc::new(Sample::mono(48_000, vec![0.5; 100])),
                file_rate: 48_000,
                frames: 100,
            },
        );
        let held = Sampler {
            face: ModelFace::Pads,
            pads: vec![
                ModelPad {
                    level_db: -6.0,
                    pan: 0.5,
                    ..ModelPad::playing(36, Id(1))
                },
                ModelPad::playing(60, Id(1)),
            ],
            ..Sampler::default()
        };
        let mut map = lumen_project::drums::presets::Preset::EIGHT_OH_EIGHT.map(Id(7));
        for sound in &mut map.sounds {
            if sound.input == 36 {
                sound.choke = Some(2);
            }
        }
        let sound = library.build(&held, Some(&map));
        let kick = sound.pad(36).expect("the kick");
        assert!((kick.gain - 0.501_187).abs() < 0.001, "{}", kick.gain);
        assert_eq!(kick.pan, 0.5);
        assert_eq!(kick.choke, Some(2));
        assert_eq!(
            sound.pad(60).and_then(|p| p.choke),
            None,
            "a pad off the kit is alone"
        );
    }

    #[test]
    fn a_face_decides_what_a_note_plays() {
        let mut library = Library::default();
        for id in [1u64, 2, 3] {
            library.loaded.insert(
                Id(id),
                Source {
                    sample: Arc::new(Sample::mono(48_000, vec![id as f32; 100])),
                    file_rate: 48_000,
                    frames: 100,
                },
            );
        }
        let mut held = Sampler {
            face: ModelFace::Zones,
            zones: vec![lumen_project::model::Zone {
                media: Id(2),
                low: 60,
                high: 60,
                low_velocity: 1,
                high_velocity: 127,
                root: 60,
            }],
            ..Sampler::default()
        };
        held.one_shot = OneShot {
            region: Some(lumen_project::model::Region::whole(Id(1), 60)),
            ..OneShot::default()
        };
        held.pads.push(ModelPad::playing(60, Id(3)));
        let sound = library.build(&held, None);
        assert!(matches!(sound.face, Face::Zones));
        assert!(sound.zones.iter().any(|z| z.holds(60, 100)));
        // Every face keeps its own sound, whichever is showing.
        assert!(sound.one_shot.is_some());
        assert!(sound.pad(60).is_some());
    }

    #[test]
    fn files_fill_pads_by_their_names_and_then_by_order() {
        let named: [(u8, Option<&str>); 4] = [
            (36, Some("Kick")),
            (38, Some("Snare")),
            (42, Some("Closed Hat")),
            (46, Some("Open Hat")),
        ];
        // Nothing is named after them: in order.
        let placed = place_by_name_or_order(
            &named,
            &["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
        );
        assert_eq!(placed, [(36, 0), (38, 1), (42, 2), (46, 3)]);
        // Named files take their own pad, and the rest fill what is left.
        let placed = place_by_name_or_order(&named, &["take".into(), "open_hat".into()]);
        assert_eq!(placed, [(36, 0), (46, 1)]);
        // A name inside a longer one counts, in any case and punctuation.
        let placed = place_by_name_or_order(&named, &["Kick 808.wav".into()]);
        assert_eq!(placed, [(36, 0)]);
        // A map with no names is filled in order.
        let unnamed: [(u8, Option<&str>); 2] = [(36, None), (38, None)];
        assert_eq!(
            place_by_name_or_order(&unnamed, &["x".into(), "y".into()]),
            [(36, 0), (38, 1)]
        );
    }
}
