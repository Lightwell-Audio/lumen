//! The audio a render plays, at the project's sample rate (M9 slice R4,
//! ADR-0105). Joined, bounced and frozen audio is made at the rate the project
//! says (Project settings), whatever the audio device runs at. The clip player
//! holds every file converted to the device's rate, and converting that copy
//! again would be a second conversion, so when the two rates differ the
//! render's worker reads each file it plays from disk again and converts it
//! once, to the project's rate (reversed clips play the same file, mirrored).
//! When the rates are the same the loaded audio is used as it is, and nothing
//! is read twice. The model-to-engine translation is the same pure function
//! playback uses (`lanes::engine_clips`), so the file is what plays.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use lumen_engine::{Clip, ClipAudio};
use lumen_media::MediaError;
use lumen_project::model::{self, ClipContent, Id, Project};

use crate::lanes::{Lanes, engine_clips};
use crate::media::streams::Unread;
use crate::media::{mirror, reversed_key};

/// One file a render plays, and which of its forms: as it is, and mirrored.
struct File {
    media: u64,
    path: PathBuf,
    forward: bool,
    reversed: bool,
}

/// The clips of a render made again at the project's rate, on the worker.
pub struct Again {
    project: Project,
    rate: u32,
    files: Vec<File>,
    /// The result, kept for the next job of the same render (a bounce of
    /// several tracks reads each file once).
    made: Mutex<Option<Vec<Clip>>>,
}

impl Again {
    /// The clips as they play at the project's rate, reading each file again
    /// and converting it once. `Ok(None)` if `cancel` was set meanwhile; an
    /// error says which file could not be read.
    pub fn clips(&self, cancel: &AtomicBool) -> Result<Option<Vec<Clip>>, String> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(clips) = &*made {
            return Ok(Some(clips.clone()));
        }
        let mut audio: HashMap<u64, (Arc<ClipAudio>, u32)> = HashMap::new();
        for file in &self.files {
            match self.read(file, cancel) {
                Ok(Some((own, native))) => {
                    if file.reversed {
                        audio.insert(reversed_key(file.media), (Arc::new(mirror(&own)), native));
                    }
                    if file.forward {
                        audio.insert(file.media, (Arc::new(own), native));
                    }
                }
                Ok(None) => return Ok(None),
                Err(why) => return Err(why),
            }
        }
        let clips = engine_clips(&self.project, self.rate, |key| audio.get(&key.0).cloned());
        *made = Some(clips.clone());
        Ok(Some(clips))
    }

    /// One file at the project's rate, with the rate it was made at.
    fn read(&self, file: &File, cancel: &AtomicBool) -> Result<Option<(ClipAudio, u32)>, String> {
        let name = file
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let again = |why: MediaError| match why {
            MediaError::Cancelled => Ok(None),
            why => Err(format!(
                "Could not read {name} again at {} Hz: {why}",
                self.rate
            )),
        };
        let decoded = match lumen_media::decode(&file.path, cancel, &mut |_| {}) {
            Ok(decoded) => decoded,
            Err(why) => return again(why),
        };
        let native = decoded.sample_rate;
        match lumen_media::resample(&decoded, self.rate, cancel) {
            Ok(converted) => Ok(Some((
                ClipAudio::Memory {
                    sample_rate: converted.sample_rate,
                    channels: converted.into_channels(),
                },
                native,
            ))),
            Err(why) => again(why),
        }
    }
}

/// What a render plays and the rate it renders at.
pub struct RenderAudio {
    /// The project's rate: the rate of the files the render writes.
    pub rate: u32,
    /// The clips the engine has loaded, at the device's rate.
    loaded: Vec<Clip>,
    /// Present when the device's rate is not the project's.
    again: Option<Arc<Again>>,
    /// What the render could not read from disk, for every job.
    unread: Unread,
}

impl RenderAudio {
    /// For the audio clips in `wanted` (the clips the render plays), from the
    /// files `lanes` holds.
    pub fn new<'a>(
        lanes: &Lanes,
        project: &Project,
        wanted: impl IntoIterator<Item = &'a model::Clip>,
    ) -> Self {
        Self::build(
            project,
            lanes.rate(),
            lanes.engine_clips(project, lanes.rate()),
            |media| lanes.file_of(media),
            wanted,
        )
    }

    /// For every clip of `tracks`: what a bounce or a freeze plays.
    pub fn of_tracks<'a>(
        lanes: &Lanes,
        project: &Project,
        tracks: impl IntoIterator<Item = &'a model::Track>,
    ) -> Self {
        Self::new(
            lanes,
            project,
            tracks
                .into_iter()
                .flat_map(|track| track.clips.iter().map(|clip| &**clip)),
        )
    }

    /// [`Self::new`] with the loaded clips and the files given.
    pub(crate) fn build<'a>(
        project: &Project,
        device_rate: u32,
        loaded: Vec<Clip>,
        file_of: impl Fn(u64) -> Option<PathBuf>,
        wanted: impl IntoIterator<Item = &'a model::Clip>,
    ) -> Self {
        let rate = project.sample_rate;
        let again = (rate != device_rate).then(|| {
            let mut files: Vec<File> = Vec::new();
            for clip in wanted {
                let ClipContent::Audio { media, .. } = &clip.content else {
                    continue;
                };
                let held = files.iter_mut().find(|f| f.media == media.0);
                if let Some(held) = held {
                    held.forward |= !clip.reversed;
                    held.reversed |= clip.reversed;
                } else if let Some(path) = file_of(media.0) {
                    files.push(File {
                        media: media.0,
                        path,
                        forward: !clip.reversed,
                        reversed: clip.reversed,
                    });
                }
            }
            Arc::new(Again {
                project: project.clone(),
                rate,
                files,
                made: Mutex::new(None),
            })
        });
        Self {
            rate,
            loaded,
            again,
            unread: Unread::default(),
        }
    }

    /// Whether the audio of clip `id` is loaded, so the render can play it.
    pub fn has(&self, id: Id) -> bool {
        self.loaded.iter().any(|c| c.id == id.0)
    }

    /// The clips a job starts with: the loaded ones when they are at the
    /// project's rate, none when the worker makes them (`Self::again`). Clips
    /// played from disk read through a pool of the job's own, filled as the
    /// render reaches each page (M9 slice R6), so call this once per job:
    /// one job's pool is read by one thread.
    pub fn clips(&self) -> Vec<Clip> {
        if self.again.is_some() {
            Vec::new()
        } else {
            crate::media::streams::offline(self.loaded.clone(), &self.unread)
        }
    }

    /// What the jobs' clips could not read from disk: the worker checks it
    /// before it keeps a file.
    pub fn unread(&self) -> Unread {
        self.unread.clone()
    }

    /// What the worker makes the clips with, when the rates differ.
    pub fn again(&self) -> Option<Arc<Again>> {
        self.again.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use lumen_core::time::Ticks;
    use lumen_media::wav::{Sample, Spec, Writer};
    use lumen_project::model::{Clip as ModelClip, Color, Track, TrackKind};
    use lumen_project::testing::TempDir;

    /// A mono-ish stereo ramp file at `rate`, one second long.
    fn write(path: &Path, rate: u32) {
        let left: Vec<f32> = (0..rate)
            .map(|i| ((i as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        let right: Vec<f32> = left.iter().map(|v| -v).collect();
        let mut writer = Writer::create(
            path,
            Spec {
                sample_rate: rate,
                channels: 2,
                sample: Sample::F32,
            },
        )
        .unwrap();
        writer.write(&[&left, &right]).unwrap();
        writer.finish().unwrap();
    }

    fn model_clip(id: u64, media: u64, reversed: bool) -> ModelClip {
        ModelClip {
            id: Id(id),
            name: "Take".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(2),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed,
            inverted: false,
            takes: None,
            content: ClipContent::Audio {
                media: Id(media),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    /// A project at `rate` with one audio track holding `clips`.
    fn project_at(rate: u32, clips: Vec<ModelClip>) -> Project {
        let mut project = crate::empty_project();
        project.sample_rate = rate;
        let mut track = Track::new(Id(1), "Audio", TrackKind::Audio, Color(0x5E8C6A));
        track.clips = clips.into_iter().map(Arc::new).collect();
        project.tracks.push(Arc::new(track));
        project
    }

    /// What loading `path` for a device at `rate` leaves in the library.
    fn loaded_at(path: &Path, rate: u32) -> Arc<ClipAudio> {
        let never = AtomicBool::new(false);
        let decoded = lumen_media::decode(path, &never, &mut |_| {}).unwrap();
        let converted = lumen_media::resample(&decoded, rate, &never).unwrap();
        Arc::new(ClipAudio::Memory {
            sample_rate: converted.sample_rate,
            channels: converted.into_channels(),
        })
    }

    /// The engine's clips for `project` with the file loaded at `rate`.
    fn playing(project: &Project, file: &Path, media: u64, native: u32, rate: u32) -> Vec<Clip> {
        let audio = loaded_at(file, rate);
        engine_clips(project, rate, |key| {
            assert!(key.0 == media || key.0 == reversed_key(media));
            let audio = if key.0 == reversed_key(media) {
                Arc::new(mirror(&audio))
            } else {
                Arc::clone(&audio)
            };
            Some((audio, native))
        })
    }

    #[test]
    fn equal_rates_use_the_loaded_audio_and_read_nothing_again() {
        let dir = TempDir::new("render-audio-equal");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(48_000, vec![clip.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        assert_eq!(loaded.len(), 1);

        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        assert_eq!(audio.rate, 48_000);
        assert!(audio.again().is_none(), "no second decode");
        assert_eq!(audio.clips().len(), 1);
        assert!(audio.has(Id(1)));
        assert!(!audio.has(Id(2)));
    }

    #[test]
    fn another_rate_makes_the_clips_on_the_worker_from_the_file() {
        let dir = TempDir::new("render-audio-again");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(44_100, vec![clip.clone()]);
        // The device runs at 48 kHz: the loaded clips are at 48 kHz.
        let loaded = playing(&project, &file, 9, 48_000, 48_000);

        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        assert_eq!(audio.rate, 44_100, "the project's rate, not the device's");
        assert!(audio.has(Id(1)), "loaded audio says the render can start");
        assert!(audio.clips().is_empty(), "the worker makes them");

        let again = audio.again().expect("the rates differ");
        let made = again
            .clips(&AtomicBool::new(false))
            .unwrap()
            .expect("not cancelled");
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].audio.sample_rate(), 44_100);
        assert_eq!(made[0].audio.frames(), 44_100, "one second at the new rate");

        // The same project on a device that runs at 44.1 kHz plays exactly
        // what the worker made: the file was converted once, from the file.
        let direct = playing(&project, &file, 9, 48_000, 44_100);
        assert_eq!(made[0].audio.memory(), direct[0].audio.memory());
        assert_eq!(
            (made[0].start, made[0].offset, made[0].length),
            (direct[0].start, direct[0].offset, direct[0].length)
        );
    }

    /// Joins the clip `1` of `project` as a render does, with the audio
    /// `audio` gives, and returns the file it wrote.
    fn joined(project: &Project, audio: &RenderAudio, file: PathBuf) -> PathBuf {
        use crate::consolidate::{Job, Poll, Render};
        use lumen_engine::Timeline;
        use lumen_project::edit::consolidate::plan;
        let part = plan(project, &[Id(1)]).pop().expect("one part");
        let timeline = Timeline::default();
        let frames =
            timeline.sample_at(part.end, audio.rate) - timeline.sample_at(part.start, audio.rate);
        let clips = audio
            .clips()
            .into_iter()
            .filter(|c| part.clips.iter().any(|p| p.id.0 == c.id))
            .collect();
        let job = Job {
            part,
            clips,
            from: 0,
            frames: frames as u64,
            file: file.clone(),
            media: Id(70),
            unread: audio.unread(),
        };
        let ctx = eframe::egui::Context::default();
        let render = Render::start(vec![job], timeline, audio.rate, audio.again(), &ctx);
        let started = std::time::Instant::now();
        loop {
            match render.poll() {
                Poll::Running(_) => {
                    assert!(
                        started.elapsed().as_secs() < 30,
                        "the render did not finish"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Poll::Done(baked) => {
                    assert_eq!(baked[0].media.sample_rate, audio.rate);
                    return file;
                }
                other => panic!("the render ended with {other:?}"),
            }
        }
    }

    #[test]
    fn a_join_at_another_rate_matches_the_same_project_on_a_device_at_that_rate() {
        let dir = TempDir::new("render-audio-join");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(44_100, vec![clip.clone()]);
        let never = AtomicBool::new(false);

        // A device at 48 kHz: the worker reads the file again.
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        let apart = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        assert!(apart.again().is_some());
        let first = joined(&project, &apart, dir.path().join("apart.wav"));

        // A device at 44.1 kHz: the loaded audio is used as it is.
        let loaded = playing(&project, &file, 9, 48_000, 44_100);
        let same = RenderAudio::build(&project, 44_100, loaded, |_| Some(file.clone()), [&clip]);
        assert!(same.again().is_none());
        let second = joined(&project, &same, dir.path().join("same.wav"));

        let first = lumen_media::decode(&first, &never, &mut |_| {}).unwrap();
        let second = lumen_media::decode(&second, &never, &mut |_| {}).unwrap();
        assert_eq!(
            first.sample_rate, 44_100,
            "the file is at the project's rate"
        );
        assert_eq!(first.frames(), 44_100);
        assert_eq!(first.channel(0), second.channel(0));
        assert_eq!(first.channel(1), second.channel(1));
        assert!(
            first.channel(0).iter().any(|v| v.abs() > 0.1),
            "it is the tone"
        );
    }

    #[test]
    fn a_reversed_clip_plays_the_file_mirrored_at_the_project_rate() {
        let dir = TempDir::new("render-audio-reversed");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let (forward, backward) = (model_clip(1, 9, false), model_clip(2, 9, true));
        let project = project_at(44_100, vec![forward.clone(), backward.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        assert_eq!(loaded.len(), 2);

        let audio = RenderAudio::build(
            &project,
            48_000,
            loaded,
            |_| Some(file.clone()),
            [&forward, &backward],
        );
        let made = audio
            .again()
            .unwrap()
            .clips(&AtomicBool::new(false))
            .unwrap()
            .unwrap();
        let direct = playing(&project, &file, 9, 48_000, 44_100);
        for clip in &direct {
            let own = made.iter().find(|c| c.id == clip.id).expect("made");
            assert_eq!(own.audio.memory(), clip.audio.memory(), "clip {}", clip.id);
        }
        let first = made.iter().find(|c| c.id == 1).unwrap();
        let second = made.iter().find(|c| c.id == 2).unwrap();
        assert_ne!(first.audio.memory(), second.audio.memory());
    }

    #[test]
    fn the_files_are_read_once_for_all_the_jobs_of_a_render() {
        let dir = TempDir::new("render-audio-once");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(44_100, vec![clip.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        let again = audio.again().unwrap();
        let never = AtomicBool::new(false);
        let first = again.clips(&never).unwrap().unwrap();
        // The file is gone: a second job still has its clips.
        std::fs::remove_file(&file).unwrap();
        let second = again.clips(&never).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first[0].audio, &second[0].audio));
    }

    #[test]
    fn a_file_that_cannot_be_read_again_is_named() {
        let dir = TempDir::new("render-audio-gone");
        let file = dir.path().join("gone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(44_100, vec![clip.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        std::fs::remove_file(&file).unwrap();
        let why = audio
            .again()
            .unwrap()
            .clips(&AtomicBool::new(false))
            .unwrap_err();
        assert!(why.contains("gone.wav") && why.contains("44100"), "{why}");
    }

    #[test]
    fn a_cancelled_read_makes_nothing() {
        let dir = TempDir::new("render-audio-cancel");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let clip = model_clip(1, 9, false);
        let project = project_at(44_100, vec![clip.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        let made = audio
            .again()
            .unwrap()
            .clips(&AtomicBool::new(true))
            .unwrap();
        assert!(made.is_none());
    }
}
