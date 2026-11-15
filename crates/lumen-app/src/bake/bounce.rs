use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_devices::{Guard, HostedInstrument, Status};
use lumen_engine::{
    Clip, Ended, InstrumentSpec, Layered, NodeAutomation, NodeSpec, OFFLINE_WAIT, Source,
    SourceRender, Timeline, clip_player_for_render, midi_player, mixer, render_source,
};
use lumen_hosting::{Happened, Manager};
use lumen_media::wav::{Sample, Spec, Writer};
use lumen_project::model::{Id, Media, Project, TrackKind};

use crate::export::render_audio::Again;
use crate::hosting::{HOSTED_KIND, OfflinePlugin, stand_in};
use crate::media::streams::Unread;
use crate::midi::play::{self as midi_play, Hosts, Inputs, PortTable};
use crate::midi::runs::Cache;
use crate::routing::{self, Devices};

const LOAD_WAIT: Duration = Duration::from_secs(60);
const LOAD_POLL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bake {
    Plain,
    Sound,
    Mix,
}

pub struct Job {
    pub project: Project,
    pub track: Id,
    pub clips: Vec<Clip>,
    pub plugins: Vec<OfflinePlugin>,
    pub bake: Bake,
    pub mono: bool,
    pub tail: bool,
    pub timeline: Timeline,
    pub rate: u32,
    pub from: i64,
    pub frames: u64,
    pub file: PathBuf,
    pub media: Id,
    pub unread: Unread,

    pub samples: Vec<(Id, crate::builtin::samples::Source)>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub track: Id,
    pub media: Media,
    pub tail_frames: u64,
}

#[derive(Debug)]
pub enum Poll {
    Running(f32),
    Done(Vec<Rendered>),
    Stopped,
    Failed(String),
}

pub struct Render {
    results: Receiver<Result<Vec<Rendered>, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl Drop for Render {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct OneInstrument {
    track: Id,
    instrument: Option<InstrumentSpec>,
}

impl Hosts for OneInstrument {
    fn instrument(&mut self, track: Id) -> InstrumentSpec {
        if track == self.track
            && let Some(spec) = self.instrument.take()
        {
            return spec;
        }
        stand_in()
    }
}

struct Running {
    plugin: OfflinePlugin,
    linked: lumen_hosting::Linked,
}

fn start_hosts(
    manager: &mut Manager,
    plugins: Vec<OfflinePlugin>,
    cancel: &AtomicBool,
) -> Result<Option<Vec<Running>>, String> {
    for plugin in &plugins {
        manager
            .want(plugin.device.0, plugin.start.clone())
            .map_err(|e| format!("The host for {} could not be started: {e}", plugin.name))?;
    }
    let mut waiting = plugins;
    let mut running = Vec::with_capacity(waiting.len());
    let end = Instant::now() + LOAD_WAIT;
    while !waiting.is_empty() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        for event in manager.poll() {
            let Some(at) = waiting.iter().position(|p| p.device.0 == event.device) else {
                continue;
            };
            match event.happened {
                Happened::Linked(linked) => {
                    running.push(Running {
                        plugin: waiting.remove(at),
                        linked: *linked,
                    });
                }
                Happened::Failed(why) => {
                    return Err(format!("{} could not start: {why}", waiting[at].name));
                }
                _ => {}
            }
        }
        if Instant::now() > end {
            return Err(format!("{} took too long to load", waiting[0].name));
        }
        std::thread::sleep(LOAD_POLL);
    }
    Ok(Some(running))
}

fn plain(node: &mut NodeSpec) {
    let plain = NodeSpec::unity(node.id, node.output);
    *node = NodeSpec {
        source: node.source,
        ..plain
    };
}

fn sound(node: &mut NodeSpec) {
    node.volume = 1.0;
    node.pan = 0.0;
    node.law = lumen_core::pan::PanLaw::Balance;
    node.audible = true;
    if let Some(lanes) = &mut node.automation {
        lanes.volume = None;
        lanes.pan = None;
        lanes.mute = None;
        lanes.sends.clear();
    }
    if node
        .automation
        .as_ref()
        .is_some_and(NodeAutomation::is_empty)
    {
        node.automation = None;
    }
}

pub fn render_job(
    job: Job,
    cancel: &AtomicBool,
    done: &mut dyn FnMut(f32),
) -> Result<Option<Rendered>, String> {
    let Job {
        mut project,
        track,
        clips,
        plugins,
        bake,
        mono,
        tail,
        timeline,
        rate,
        from,
        frames,
        file,
        media,
        unread,
        samples: decoded,
    } = job;
    if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not make {}: {e}", parent.display()))?;
    }
    let effects = bake != Bake::Plain;
    let Some(held) = project.tracks.iter().find(|t| t.id == track) else {
        return Err("The track is gone".to_owned());
    };
    let is_midi = held.kind == TrackKind::Midi;
    if is_midi && held.midi_to.is_some() {
        return Err("A track sent to a MIDI output has no sound of its own to bounce".to_owned());
    }
    let mut manager = Manager::new(|| {});
    let Some(running) = start_hosts(&mut manager, plugins, cancel)? else {
        return Ok(None);
    };

    let mut devices = Devices::default();
    for r in &running {
        devices.hosted.insert(r.plugin.device, r.linked.latency);
    }
    devices.builtin = crate::builtin::latencies(&project);
    let Some(mut routing) = routing::bounce_routing(&project, track, &devices, rate) else {
        return Err("The track is gone".to_owned());
    };
    if let Some(node) = routing.nodes.iter_mut().find(|n| n.id == track.0) {
        match bake {
            Bake::Plain => plain(node),
            Bake::Sound => sound(node),
            Bake::Mix => {}
        }
    }
    project.tracks.retain(|t| t.id == track);
    let mut statuses: Vec<(String, Arc<Status>)> = Vec::new();
    let mut instrument = None;
    let (mut clip_handle, clip_play) = clip_player_for_render();
    let (mut midi_handle, midi_play) = midi_player();
    let (mut mixer_handle, mut source) = mixer(Layered(clip_play, midi_play));
    crate::builtin::attach_all(&project, &routing, &mut mixer_handle, || {
        source.idle(rate, 256)
    })?;
    for Running { plugin, linked } in running {
        let latency = linked.latency;
        let guard = Guard::new(Box::new(linked.link), latency);
        statuses.push((plugin.name.clone(), guard.status()));
        if plugin.instrument {
            instrument = Some(InstrumentSpec {
                kind: HOSTED_KIND,
                preset: 0,
                values: Vec::new(),
                instrument: Box::new(HostedInstrument::new(guard)),
            });
        } else if mixer_handle.attach_host(plugin.device.0, guard).is_err() {
            return Err("Too many plugins to bounce at once".to_owned());
        }
    }
    let mut hosts = OneInstrument { track, instrument };
    let mut midi_tracks = midi_play::engine_midi_with(
        &project,
        &Inputs::default(),
        &mut PortTable::default(),
        &mut Cache::default(),
        &mut hosts,
    );
    if effects {
        midi_play::with_lanes(&mut midi_tracks, &project, &Default::default(), rate);
    }
    let samples = crate::builtin::samples::Library::from_snapshot(rate, decoded);
    crate::midi::play::attach_sounds(&mut midi_tracks, &project, &|device, map| {
        samples.sound(device, map)
    });
    let mine: Vec<Clip> = clips.into_iter().filter(|c| c.track == track.0).collect();
    if clip_handle.set(mine).is_err() || midi_handle.set(midi_tracks).is_err() {
        return Err("The engine could not take the track".to_owned());
    }
    if !mixer_handle
        .set(&routing)
        .map_err(|e| format!("The track cannot be routed: {e}"))?
    {
        return Err("The engine could not take the routing".to_owned());
    }

    let mut writer = Writer::create(
        &file,
        Spec {
            sample_rate: rate,
            channels: 2,
            sample: Sample::F32,
        },
    )
    .map_err(|e| e.to_string())?;
    let twin_file = file.with_extension("mono.wav");
    let mut twin = if mono {
        Some(
            Writer::create(
                &twin_file,
                Spec {
                    sample_rate: rate,
                    channels: 1,
                    sample: Sample::F32,
                },
            )
            .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let mut same = true;
    let mut write_failed = None;
    let mut written = 0u64;
    let mut late = None;
    let ended = render_source(
        &mut source,
        SourceRender {
            timeline: &timeline,
            sample_rate: rate,
            from,
            frames,
            most_tail: if tail {
                crate::builtin::track_tail_frames(&project, track, rate)
            } else {
                0
            },
            block: lumen_engine::MOST_BLOCK,
            wait: OFFLINE_WAIT,
        },
        cancel,
        &mut || {
            late = statuses
                .iter()
                .find(|(_, s)| s.failed() || s.late_blocks() > 0)
                .map(|(name, _)| name.clone());
            late.is_some()
        },
        done,
        &mut |left, right| {
            if write_failed.is_some() {
                return;
            }
            match writer.write(&[left, right]) {
                Ok(()) => written += left.len() as u64,
                Err(e) => write_failed = Some(e.to_string()),
            }
            if let Some(twin) = &mut twin {
                same &= left == right;
                if same && let Err(e) = twin.write(&[left]) {
                    write_failed = Some(e.to_string());
                }
            }
        },
    );
    drop(source);
    manager.shutdown(Duration::from_secs(2));
    let done = if bake == Bake::Sound {
        "frozen"
    } else {
        "bounced"
    };
    let mut discard = |writer: Writer| {
        drop(writer);
        drop(twin.take());
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&twin_file);
    };
    if let Some(error) = write_failed {
        discard(writer);
        return Err(error);
    }
    match ended {
        Ended::Cancelled => {
            discard(writer);
            return Ok(None);
        }
        Ended::Failed => {
            discard(writer);
            let name = late.unwrap_or_else(|| "A plugin".to_owned());
            return Err(format!(
                "{name} stopped answering. Nothing was {done} and the project is unchanged"
            ));
        }
        Ended::Done => {}
    }
    if let Err(why) = unread.check() {
        discard(writer);
        return Err(format!(
            "{why}. Nothing was {done} and the project is unchanged"
        ));
    }
    let frames_written = writer.finish().map_err(|e| e.to_string())?;
    let mut channels = 2;
    if let Some(twin) = twin.take() {
        let kept = same && twin.finish().is_ok() && std::fs::rename(&twin_file, &file).is_ok();
        if kept {
            channels = 1;
        } else {
            let _ = std::fs::remove_file(&twin_file);
        }
    }
    let size_bytes = std::fs::metadata(&file).map_or(0, |m| m.len());
    Ok(Some(Rendered {
        track,
        media: Media {
            id: media,
            path: file.to_string_lossy().into_owned(),
            original_path: None,
            frames: frames_written,
            channels,
            sample_rate: rate,
            size_bytes,
        },
        tail_frames: frames_written.saturating_sub(frames),
    }))
}

impl Render {
    pub fn start(jobs: Vec<Job>, again: Option<Arc<Again>>, ctx: &egui::Context) -> Self {
        let (tx, results) = channel();
        let report = tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (stop, shown, ctx) = (Arc::clone(&cancel), Arc::clone(&progress), ctx.clone());
        let count = jobs.len().max(1) as f32;
        let spawned = crate::spawn::spawn_named("lumen-bounce", move || {
            let mut made: Vec<Rendered> = Vec::with_capacity(jobs.len());
            for mut job in jobs {
                if let Some(again) = &again {
                    match again.clips(&stop) {
                        Ok(Some(clips)) => job.clips = clips,
                        Ok(None) => {
                            remove_files(&made);
                            return;
                        }
                        Err(e) => {
                            remove_files(&made);
                            let _ = tx.send(Err(e));
                            ctx.request_repaint();
                            return;
                        }
                    }
                }
                let before = made.len() as f32;
                let result = render_job(job, &stop, &mut |part| {
                    let fraction = (before + part) / count;
                    shown.store(fraction.min(1.0).to_bits(), Ordering::Relaxed);
                    ctx.request_repaint();
                });
                match result {
                    Ok(Some(done)) => made.push(done),
                    Ok(None) => {
                        remove_files(&made);
                        return;
                    }
                    Err(e) => {
                        remove_files(&made);
                        let _ = tx.send(Err(e));
                        ctx.request_repaint();
                        return;
                    }
                }
            }
            let _ = tx.send(Ok(made));
            ctx.request_repaint();
        });
        if let Err(error) = spawned {
            let _ = report.send(Err(crate::spawn::start_failed("the bounce", &error)));
        }
        Self {
            results,
            cancel,
            progress,
        }
    }

    pub fn poll(&self) -> Poll {
        match self.results.try_recv() {
            Ok(Ok(made)) => Poll::Done(made),
            Ok(Err(why)) => Poll::Failed(why),
            Err(TryRecvError::Empty) => {
                Poll::Running(f32::from_bits(self.progress.load(Ordering::Relaxed)))
            }
            Err(TryRecvError::Disconnected) => Poll::Stopped,
        }
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn remove_files(made: &[Rendered]) {
    crate::recording::take_notes::delete_later(
        made.iter()
            .map(|done| PathBuf::from(&done.media.path))
            .collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use lumen_core::time::Ticks;
    use lumen_engine::ClipAudio;
    use lumen_project::testing::TempDir;

    const RATE: u32 = 48_000;

    fn ramp() -> Vec<f32> {
        (0..48_000)
            .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
            .collect()
    }

    fn job(doc: &Document, track: Id, dir: &std::path::Path, from: i64, frames: u64) -> Job {
        let samples = ramp();
        let audio = Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![samples.clone(), samples],
        });
        let clip = Clip {
            envelope: Default::default(),
            id: 1,
            track: track.0,
            start: Ticks::ZERO,
            offset: 0,
            length: audio.frames() as i64,
            gain: 1.0,
            audio,
            shape: Default::default(),
        };
        Job {
            project: doc.project().clone(),
            track,
            clips: vec![clip],
            plugins: Vec::new(),
            bake: Bake::Mix,
            mono: false,
            tail: false,
            timeline: Timeline::default(),
            rate: RATE,
            from,
            frames,
            file: dir.join("bounce.wav"),
            media: Id(777),
            unread: Default::default(),
            samples: Vec::new(),
        }
    }

    fn audio_track() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let id = doc.add_track(TrackKind::Audio, None);
        (doc, id)
    }

    fn read(path: &std::path::Path) -> lumen_media::Audio {
        lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    #[test]
    fn an_unused_render_is_deleted_after_the_call_returns() {
        let dir = TempDir::new("bounce-unused");
        let path = dir.path().join("bounced.wav");
        std::fs::write(&path, b"a render nobody kept").unwrap();
        let made = Rendered {
            track: Id(1),
            media: Media {
                id: Id(2),
                path: path.to_string_lossy().into_owned(),
                original_path: None,
                frames: 0,
                channels: 2,
                sample_rate: RATE,
                size_bytes: 21,
            },
            tail_frames: 0,
        };
        remove_files(std::slice::from_ref(&made));
        lumen_testkit::wait_until("the unused render to be deleted", || !path.exists());
    }

    #[test]
    fn a_bounce_with_effects_has_the_fader_and_pan_in_the_file() {
        let (mut doc, track) = audio_track();
        doc.set_volume(track, -6.0);
        doc.set_pan(track, -1.0);
        let dir = TempDir::new("bounce-effects");
        let job = job(
            &doc,
            track,
            &dir.path().join("new-render-folder"),
            20_000,
            10_000,
        );
        let file = job.file.clone();
        let mut last = 0.0;
        let made = render_job(job, &AtomicBool::new(false), &mut |p| last = p)
            .unwrap()
            .expect("not cancelled");
        assert_eq!(last, 1.0);
        assert_eq!((made.media.frames, made.media.channels), (10_000, 2));
        assert_eq!(made.tail_frames, 0);
        assert_eq!(made.media.path, file.to_string_lossy());
        let back = read(&file);
        let ramp = ramp();
        let gain = 10f32.powf(-6.0 / 20.0);
        for k in 1_000..10_000 {
            let want = ramp[20_000 + k] * gain;
            assert!((back.channel(0)[k] - want).abs() < 1e-4, "frame {k}");
            assert!(back.channel(1)[k].abs() < 1e-6, "frame {k}");
        }
    }

    #[test]
    fn a_bounce_without_effects_is_the_tracks_own_sound() {
        let (mut doc, track) = audio_track();
        doc.set_volume(track, -6.0);
        doc.set_pan(track, -1.0);
        let dir = TempDir::new("bounce-plain");
        let mut job = job(&doc, track, dir.path(), 20_000, 10_000);
        job.bake = Bake::Plain;
        let file = job.file.clone();
        render_job(job, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        let back = read(&file);
        let ramp = ramp();
        for k in 0..10_000 {
            assert!(
                (back.channel(0)[k] - ramp[20_000 + k]).abs() < 1e-6,
                "frame {k}"
            );
            assert!(
                (back.channel(1)[k] - ramp[20_000 + k]).abs() < 1e-6,
                "frame {k}"
            );
        }
    }

    #[test]
    fn a_freeze_has_trim_and_polarity_in_the_file_but_not_the_fader_or_pan() {
        let (mut doc, track) = audio_track();
        doc.set_trim(track, 6.0);
        doc.set_invert(&[track], true);
        doc.set_volume(track, -6.0);
        doc.set_pan(track, -1.0);
        let dir = TempDir::new("freeze-sound");
        let mut job = job(&doc, track, dir.path(), 20_000, 10_000);
        job.bake = Bake::Sound;
        let file = job.file.clone();
        render_job(job, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        let back = read(&file);
        let ramp = ramp();
        let gain = -(10f32.powf(6.0 / 20.0));
        for k in 1_000..10_000 {
            let want = ramp[20_000 + k] * gain;
            assert!((back.channel(0)[k] - want).abs() < 1e-4, "left {k}");
            assert!((back.channel(1)[k] - want).abs() < 1e-4, "right {k}");
        }
    }

    #[test]
    fn mono_material_is_kept_as_a_mono_file_only_if_the_channels_never_differ() {
        let (doc, track) = audio_track();
        let dir = TempDir::new("freeze-mono");
        let mut same = job(&doc, track, dir.path(), 0, 10_000);
        same.bake = Bake::Sound;
        same.mono = true;
        let file = same.file.clone();
        let made = render_job(same, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        assert_eq!(made.media.channels, 1);
        let back = read(&file);
        assert_eq!(back.channels(), 1);
        assert!((back.channel(0)[500] - ramp()[500]).abs() < 1e-6);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no twin left behind");

        let mut apart = job(&doc, track, dir.path(), 0, 10_000);
        let left = ramp();
        let right: Vec<f32> = left.iter().map(|s| s * 0.5).collect();
        apart.clips[0].audio = Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![left, right],
        });
        apart.bake = Bake::Sound;
        apart.mono = true;
        apart.file = dir.path().join("apart.wav");
        let made = render_job(apart, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        assert_eq!(made.media.channels, 2);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_bounce_ignores_mute_and_other_tracks() {
        let (mut doc, track) = audio_track();
        let other = doc.add_track(TrackKind::Audio, None);
        doc.set_mute(&[track], true);
        doc.set_solo(&[other], true, false);
        let dir = TempDir::new("bounce-muted");
        let mut job = job(&doc, track, dir.path(), 20_000, 4_000);
        job.bake = Bake::Plain;
        let file = job.file.clone();
        render_job(job, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        let back = read(&file);
        assert!(back.channel(0).iter().any(|s| s.abs() > 0.1), "it sounds");
    }

    #[test]
    fn a_cancelled_bounce_leaves_no_file_and_a_missing_track_is_an_error() {
        let (doc, track) = audio_track();
        let dir = TempDir::new("bounce-cancel");
        let job1 = job(&doc, track, dir.path(), 0, 10_000);
        let done = render_job(job1, &AtomicBool::new(true), &mut |_| {}).unwrap();
        assert!(done.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        let job2 = job(&doc, Id(9_999), dir.path(), 0, 10_000);
        assert!(render_job(job2, &AtomicBool::new(false), &mut |_| {}).is_err());
    }

    #[test]
    fn a_synth_track_bounces_from_the_exact_sample_of_its_note() {
        use lumen_project::model::{Clip as ModelClip, ClipContent, Note};
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let q = Ticks::quarters;
        let clip = ModelClip {
            id: Id(900),
            name: "Part".into(),
            start: Ticks::ZERO,
            length: q(8),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: ClipContent::Midi {
                notes: vec![Note::new(q(1) + Ticks(77), q(1), 60, 100)],
                events: Vec::new(),
            },
        };
        doc.apply("add clip", None, |p| {
            Arc::make_mut(&mut p.tracks[0]).clips.push(Arc::new(clip));
        });
        let dir = TempDir::new("bounce-synth");
        let mut job = job(&doc, track, dir.path(), 0, 40_000);
        job.clips.clear();
        let file = job.file.clone();
        render_job(job, &AtomicBool::new(false), &mut |_| {})
            .unwrap()
            .unwrap();
        let back = read(&file);
        let note = Timeline::default().sample_at(q(1) + Ticks(77), RATE) as usize;
        let first = back
            .channel(0)
            .iter()
            .position(|s| s.abs() > 0.0)
            .expect("it sounds");
        assert_eq!(first, note);
    }
}
