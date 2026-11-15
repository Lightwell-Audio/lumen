use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use lumen_engine::{Clip, ClipAudio};
use lumen_media::MediaError;
use lumen_project::model::{self, ClipContent, Id, Project};

use crate::lanes::{Lanes, engine_clips_with};
use crate::media::streams::Unread;
use crate::media::warped::WarpedStreams;
use crate::media::{mirror, reversed_key};

struct File {
    media: u64,
    path: PathBuf,
    forward: bool,
    reversed: bool,
}

pub struct Again {
    project: Project,
    rate: u32,
    files: Vec<File>,
    made: Mutex<Option<Vec<Clip>>>,
    warped: Mutex<WarpedStreams>,
    unread: Unread,
}

impl Again {
    pub fn clips(&self, cancel: &AtomicBool) -> Result<Option<Vec<Clip>>, String> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(clips) = &*made {
            return Ok(Some(crate::media::streams::offline(
                clips.clone(),
                &self.unread,
            )));
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
        let mut warped = self.warped.lock().unwrap_or_else(PoisonError::into_inner);
        let clips = engine_clips_with(
            &self.project,
            self.rate,
            |key| audio.get(&key.0).cloned(),
            &mut |request| warped.stream(request),
        );
        *made = Some(clips.clone());
        Ok(Some(crate::media::streams::offline(clips, &self.unread)))
    }

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

pub struct RenderAudio {
    pub rate: u32,
    loaded: Vec<Clip>,
    again: Option<Arc<Again>>,
    unread: Unread,
    warps: Option<Arc<WarpedStreams>>,
}

impl RenderAudio {
    pub fn new<'a>(
        lanes: &Lanes,
        project: &Project,
        wanted: impl IntoIterator<Item = &'a model::Clip>,
    ) -> Self {
        let (clips, warps) = lanes.render_clips(project, lanes.rate());
        let mut audio = Self::build(
            project,
            lanes.rate(),
            clips,
            |media| lanes.file_of(media),
            wanted,
        );
        audio.warps = Some(Arc::new(warps));
        audio
    }

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

    pub(crate) fn build<'a>(
        project: &Project,
        device_rate: u32,
        loaded: Vec<Clip>,
        file_of: impl Fn(u64) -> Option<PathBuf>,
        wanted: impl IntoIterator<Item = &'a model::Clip>,
    ) -> Self {
        let rate = project.sample_rate;
        let unread = Unread::default();
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
                warped: Mutex::new(WarpedStreams::new(&lumen_engine::PagePool::new(1))),
                unread: unread.clone(),
            })
        });
        Self {
            rate,
            loaded,
            again,
            unread,
            warps: None,
        }
    }

    pub fn has(&self, id: Id) -> bool {
        self.loaded.iter().any(|c| c.id == id.0)
    }

    pub fn clips(&self) -> Vec<Clip> {
        if self.again.is_some() {
            Vec::new()
        } else {
            crate::media::streams::offline(self.loaded.clone(), &self.unread)
        }
    }

    pub fn unread(&self) -> Unread {
        self.unread.clone()
    }

    pub fn warps(&self) -> Option<Arc<WarpedStreams>> {
        self.warps.clone()
    }

    pub fn again(&self) -> Option<Arc<Again>> {
        self.again.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::lanes::engine_clips;
    use lumen_core::time::Ticks;
    use lumen_media::wav::{Sample, Spec, Writer};
    use lumen_project::model::{Clip as ModelClip, Color, Track, TrackKind};
    use lumen_project::testing::TempDir;

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
            warp: None,
            content: ClipContent::Audio {
                media: Id(media),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    fn project_at(rate: u32, clips: Vec<ModelClip>) -> Project {
        let mut project = crate::empty_project();
        project.sample_rate = rate;
        let mut track = Track::new(Id(1), "Audio", TrackKind::Audio, Color(0x5E8C6A));
        track.clips = clips.into_iter().map(Arc::new).collect();
        project.tracks.push(Arc::new(track));
        project
    }

    fn loaded_at(path: &Path, rate: u32) -> Arc<ClipAudio> {
        let never = AtomicBool::new(false);
        let decoded = lumen_media::decode(path, &never, &mut |_| {}).unwrap();
        let converted = lumen_media::resample(&decoded, rate, &never).unwrap();
        Arc::new(ClipAudio::Memory {
            sample_rate: converted.sample_rate,
            channels: converted.into_channels(),
        })
    }

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

        let direct = playing(&project, &file, 9, 48_000, 44_100);
        assert_eq!(made[0].audio.memory(), direct[0].audio.memory());
        assert_eq!(
            (made[0].start, made[0].offset, made[0].length),
            (direct[0].start, direct[0].offset, direct[0].length)
        );
    }

    #[test]
    fn another_rate_makes_the_warped_clips_streams_with_a_loader_of_the_jobs_own() {
        let dir = TempDir::new("render-audio-again-warped");
        let file = dir.path().join("tone.wav");
        write(&file, 48_000);
        let mut clip = model_clip(1, 9, false);
        clip.warp = Some(lumen_project::model::Warp::new(100.0, 0.0));
        let project = project_at(44_100, vec![clip.clone()]);
        let loaded = playing(&project, &file, 9, 48_000, 48_000);

        let audio = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        let again = audio.again().expect("the rates differ");
        let made = again
            .clips(&AtomicBool::new(false))
            .unwrap()
            .expect("not cancelled");
        let stream = made[0].audio.stream().expect("a warped clip is a stream");
        assert!(
            stream.is_offline(),
            "the render loads its pages itself; with no loader it would be silent"
        );
        let heard = crate::media::streams::in_memory(made).unwrap();
        let wave = &heard[0].audio.memory().unwrap()[0];
        assert!(wave.iter().any(|v| v.abs() > 0.1));
    }

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

        let loaded = playing(&project, &file, 9, 48_000, 48_000);
        let apart = RenderAudio::build(&project, 48_000, loaded, |_| Some(file.clone()), [&clip]);
        assert!(apart.again().is_some());
        let first = joined(&project, &apart, dir.path().join("apart.wav"));

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

    #[test]
    fn a_job_that_holds_the_warped_streams_can_still_read_them_after_the_render_is_gone() {
        use lumen_engine::{ClipAudio, PagePool};
        use lumen_project::model::Warp;
        let source = Arc::new(ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![vec![0.25; 100_000]; 2],
        });
        let tempo = lumen_core::time::TempoMap::constant(120.0).unwrap();
        let warp = Warp::new(120.0, 0.0);
        let mut warps = WarpedStreams::new(&PagePool::new(1));
        let stream = warps
            .stream(&crate::media::warped::Request {
                warp: &warp,
                source: &source,
                tempo: &tempo,
                rate: 48_000,
                origin: Ticks::ZERO,
                reach: 48_000,
                pitch: None,
            })
            .unwrap();
        let clip = Clip {
            id: 1,
            track: 1,
            start: Ticks::ZERO,
            offset: 0,
            length: 48_000,
            gain: 1.0,
            audio: stream,
            shape: Default::default(),
            envelope: Default::default(),
        };
        let mut audio =
            RenderAudio::build(&crate::empty_project(), 48_000, vec![clip], |_| None, []);
        audio.warps = Some(Arc::new(warps));
        let keeper = audio.warps();
        let clips = audio.clips();
        drop(audio);
        let read = crate::media::streams::in_memory(clips.clone()).unwrap();
        assert_eq!(read[0].audio.memory().unwrap()[0].len(), 48_000);
        drop(keeper);
        assert!(crate::media::streams::in_memory(clips).is_err());
    }
}
