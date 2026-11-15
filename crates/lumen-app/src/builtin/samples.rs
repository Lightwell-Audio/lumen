use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};

use lumen_devices::sampler::{Face, Pad, Region, Sample, Sound, Zone};
use lumen_project::model::{Device, DrumMap, Id, Media, Sampler};

#[derive(Debug, Clone)]
pub struct Source {
    pub sample: Arc<Sample>,

    pub file_rate: u32,

    pub frames: u64,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub media: Id,
    pub path: PathBuf,
    pub name: String,
    pub source: Source,

    pub size_bytes: u64,
}

const WORKERS: usize = 4;

#[derive(Debug, Default)]
pub struct Loading {
    waiting: Vec<Waiting>,
}

#[derive(Debug)]
struct Waiting {
    device: Id,
    wanted: Vec<(Id, String)>,

    since: std::time::Instant,
}

const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug)]
pub struct Arrived {
    pub device: Id,
    pub files: Vec<Loaded>,
    pub failed: Vec<String>,
}

impl Loading {
    pub fn start(&mut self, device: Id, wanted: Vec<(Id, String)>) {
        self.waiting.push(Waiting {
            device,
            wanted,
            since: std::time::Instant::now(),
        });
    }

    pub fn is_loading(&self) -> bool {
        !self.waiting.is_empty()
    }

    pub fn take_ready(&mut self, library: &mut Library) -> Option<Arrived> {
        let waiting = self.waiting.first()?;
        let mut files = Vec::new();
        let mut later = Vec::new();
        for loaded in library.take_ready() {
            if waiting.wanted.iter().any(|(id, _)| *id == loaded.media) {
                files.push(loaded);
            } else {
                later.push(loaded);
            }
        }

        for file in later {
            library.ready.push(file);
        }
        let mut failed = Vec::new();
        let mut left = Vec::new();
        for (id, name) in &waiting.wanted {
            if files.iter().any(|f| f.media == *id) {
                continue;
            }
            match library.failed(*id) {
                Some(why) => failed.push(format!("{name}: {why}")),
                None => left.push(name.clone()),
            }
        }
        let gave_up = waiting.since.elapsed() > PATIENCE;
        if !left.is_empty() && !gave_up {
            for file in files {
                library.ready.push(file);
            }
            return None;
        }
        if !left.is_empty() {
            failed.push(format!(
                "{} did not arrive in time: {}.",
                if left.len() == 1 {
                    "This sample"
                } else {
                    "These samples"
                },
                left.join(", ")
            ));
        }
        let device = waiting.device;
        self.waiting.remove(0);
        Some(Arrived {
            device,
            files,
            failed,
        })
    }
}

struct Working {
    media: Id,
    path: PathBuf,
    rate: u32,
    rx: Receiver<Result<Loaded, (PathBuf, String)>>,
}

#[derive(Default)]
pub struct Library {
    rate: u32,

    loaded: BTreeMap<Id, Source>,

    working: Vec<Working>,
    asked: BTreeSet<Id>,

    ready: Vec<Loaded>,

    failed: Vec<(Id, PathBuf, String)>,

    arrived: bool,

    stale: Vec<BTreeMap<Id, Source>>,
}

impl Library {
    pub fn snapshot(&self) -> Vec<(Id, Source)> {
        self.loaded
            .iter()
            .map(|(id, source)| (*id, source.clone()))
            .collect()
    }

    pub fn from_snapshot(rate: u32, sources: Vec<(Id, Source)>) -> Self {
        Self {
            rate,
            loaded: sources.into_iter().collect(),
            ..Self::default()
        }
    }

    pub fn get(&self, media: Id) -> Option<&Source> {
        self.loaded.get(&media)
    }

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
            return;
        }
        let (tx, rx) = mpsc::channel();
        let name = path.file_stem().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let size_bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        let worker_name = format!("lumen-sampler-{:x}", media.0);
        let asked_at = path.clone();
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
                self.working.push(Working {
                    media,
                    path: asked_at,
                    rate,
                    rx,
                });
            }
            Err(error) => self.failed.push((
                media,
                PathBuf::new(),
                crate::spawn::start_failed("the sample's decoder", &error),
            )),
        }
    }

    pub fn update(&mut self, rate: u32) {
        if rate != 0 && rate != self.rate {
            if self.rate != 0 {
                if !self.loaded.is_empty() {
                    self.stale.push(std::mem::take(&mut self.loaded));
                }
                self.asked.clear();
                self.failed.clear();
                self.ready.clear();
            }
            self.rate = rate;
        }
        let mut still = Vec::new();
        for working in std::mem::take(&mut self.working) {
            match working.rx.try_recv() {
                Ok(result) => {
                    self.asked.remove(&working.media);
                    if working.rate != self.rate {
                        self.failed.push((
                            working.media,
                            working.path,
                            "the device's sample rate changed while it decoded".to_owned(),
                        ));
                        continue;
                    }
                    match result {
                        Ok(loaded) => {
                            self.loaded.insert(loaded.media, loaded.source.clone());
                            self.ready.push(loaded);
                            self.arrived = true;
                        }
                        Err((path, why)) => self.failed.push((working.media, path, why)),
                    }
                }

                Err(mpsc::TryRecvError::Empty) => still.push(working),

                Err(mpsc::TryRecvError::Disconnected) => {
                    self.asked.remove(&working.media);
                    self.failed.push((
                        working.media,
                        working.path,
                        "its decoder stopped without an answer".to_owned(),
                    ));
                }
            }
        }
        self.working = still;
    }

    pub fn has_stale(&self) -> bool {
        !self.stale.is_empty()
    }

    pub fn release_stale(&mut self) -> usize {
        let mut freed = 0;
        for generation in &mut self.stale {
            generation.retain(|_, source| {
                if Arc::strong_count(&source.sample) > 1 {
                    return true;
                }
                freed += source.sample.memory();
                false
            });
        }
        self.stale.retain(|generation| !generation.is_empty());
        freed
    }

    pub fn take_arrived(&mut self) -> bool {
        std::mem::take(&mut self.arrived)
    }

    pub fn take_ready(&mut self) -> Vec<Loaded> {
        std::mem::take(&mut self.ready)
    }

    pub fn failed(&self, media: Id) -> Option<&str> {
        self.failed
            .iter()
            .find(|(id, _, _)| *id == media)
            .map(|(_, _, why)| why.as_str())
    }

    pub fn sound(&self, device: &Device, map: Option<&DrumMap>) -> Option<Arc<Sound>> {
        let held = device.sampler()?;
        Some(Arc::new(self.build(held, map)))
    }

    pub fn memory(&self) -> usize {
        self.loaded.values().map(|s| s.sample.memory()).sum()
    }

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

    fn region(&self, region: &lumen_project::model::Region) -> Option<Region> {
        self.region_of(region.media, region.start, region.end, region.root)
    }

    fn region_of(&self, media: Id, start: u64, end: Option<u64>, root: u8) -> Option<Region> {
        let source = self.loaded.get(&media)?;
        let rate = u64::from(source.sample.rate().max(1));
        let file_rate = u64::from(source.file_rate.max(1));
        let frames = source.sample.frames() as u64;

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

pub fn file_name(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn db_to_gain(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        (10.0_f64).powf(f64::from(db) / 20.0) as f32
    }
}

fn decode(path: &std::path::Path, rate: u32, media: Id) -> Result<Loaded, String> {
    let cancel = AtomicBool::new(false);

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

pub fn place_by_name_or_order(named: &[(u8, Option<&str>)], files: &[String]) -> Vec<(u8, usize)> {
    let mut placed: Vec<(u8, usize)> = Vec::new();
    let mut taken = [false; 256];

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

fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

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

        assert!(
            (sample.frames() as i64 - 48_000).abs() < 200,
            "{}",
            sample.frames()
        );

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

        let placed = place_by_name_or_order(
            &named,
            &["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
        );
        assert_eq!(placed, [(36, 0), (38, 1), (42, 2), (46, 3)]);

        let placed = place_by_name_or_order(&named, &["take".into(), "open_hat".into()]);
        assert_eq!(placed, [(36, 0), (46, 1)]);

        let placed = place_by_name_or_order(&named, &["Kick 808.wav".into()]);
        assert_eq!(placed, [(36, 0)]);

        let unnamed: [(u8, Option<&str>); 2] = [(36, None), (38, None)];
        assert_eq!(
            place_by_name_or_order(&unnamed, &["x".into(), "y".into()]),
            [(36, 0), (38, 1)]
        );
    }

    fn arrived(media: u64, name: &str) -> Loaded {
        Loaded {
            media: Id(media),
            path: PathBuf::from(format!("Samples/{name}.wav")),
            name: name.to_owned(),
            source: Source {
                sample: Arc::new(Sample::mono(48_000, vec![0.1; 100])),
                file_rate: 48_000,
                frames: 100,
            },
            size_bytes: 400,
        }
    }

    #[test]
    fn a_second_choice_while_the_first_loads_gets_both_in_order() {
        let mut library = Library::default();
        let mut loading = Loading::default();
        loading.start(Id(1), vec![(Id(10), "first".into())]);
        loading.start(Id(2), vec![(Id(20), "second".into())]);
        assert!(loading.is_loading());

        library.ready.push(arrived(20, "second"));
        assert!(
            loading.take_ready(&mut library).is_none(),
            "the first ask still waits"
        );
        assert_eq!(library.ready.len(), 1, "the early file was not dropped");

        library.ready.push(arrived(10, "first"));
        let first = loading.take_ready(&mut library).expect("the first ask");
        assert_eq!(first.device, Id(1));
        assert_eq!(
            first.files.iter().map(|f| f.media).collect::<Vec<_>>(),
            [Id(10)]
        );
        assert!(first.failed.is_empty());

        let second = loading.take_ready(&mut library).expect("the second ask");
        assert_eq!(second.device, Id(2));
        assert_eq!(
            second.files.iter().map(|f| f.media).collect::<Vec<_>>(),
            [Id(20)]
        );
        assert!(!loading.is_loading(), "both asks are done");
    }

    #[test]
    fn a_file_that_does_not_arrive_is_named_after_the_ask_gives_up() {
        let mut library = Library::default();
        let mut loading = Loading::default();
        loading.start(
            Id(1),
            vec![(Id(10), "intro".into()), (Id(11), "verse".into())],
        );
        library.ready.push(arrived(10, "intro"));

        loading.waiting[0].since =
            std::time::Instant::now() - PATIENCE - std::time::Duration::from_secs(1);
        let arrived = loading.take_ready(&mut library).expect("the ask ends");
        assert_eq!(arrived.files.len(), 1, "the file that came");
        assert!(
            arrived
                .failed
                .iter()
                .any(|why| why.contains("verse") && why.contains("did not arrive")),
            "{:?}",
            arrived.failed
        );
        assert!(!loading.is_loading());
    }

    #[test]
    fn a_decoder_that_vanishes_names_its_file() {
        let mut library = Library::default();
        let mut loading = Loading::default();

        let (tx, rx) = mpsc::channel();
        drop(tx);
        library.working.push(Working {
            media: Id(10),
            path: PathBuf::from("Samples/gone.wav"),
            rate: 48_000,
            rx,
        });
        library.update(48_000);
        loading.start(Id(1), vec![(Id(10), "gone".into())]);
        let arrived = loading.take_ready(&mut library).expect("the ask ends");
        assert!(arrived.files.is_empty());
        assert!(
            arrived
                .failed
                .iter()
                .any(|why| why.contains("gone") && why.contains("stopped without an answer")),
            "{:?}",
            arrived.failed
        );
    }
}
