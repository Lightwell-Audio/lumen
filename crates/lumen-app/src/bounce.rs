//! The worker behind Bounce (M7b slice E4, ADR-0096). One track is played
//! through the mixer without a device, as playback plays it, into a 32-bit
//! float WAV: its clips or its notes through the instrument, and with
//! `effects` its devices, trim, polarity, fader, pan and own automation lanes
//! (the owner's B2; not sends, its group or the master). Plugins run in host
//! processes of the bounce's own, started from the states the project holds,
//! and are waited for rather than raced. A plugin that stops answering ends
//! the bounce with its name, and no file is left.
//!
//! A freeze (M7b E5) is the same render with [`Bake::Sound`]: the track's
//! instrument, devices, trim and polarity, but not its fader, pan, mute or
//! sends, which stay live on a frozen track (the owner's FR1).
//!
//! What a bounce does to the project is `Document::bounce_tracks` and a freeze
//! `Document::freeze_tracks`; this module only turns a track into a file, off
//! the UI thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_devices::{Guard, HostedInstrument, Status};
use lumen_engine::{
    Clip, Ended, InstrumentSpec, Layered, NodeAutomation, NodeSpec, OFFLINE_WAIT, SourceRender,
    Timeline, clip_player, midi_player, mixer, render_source,
};
use lumen_hosting::{Happened, Manager};
use lumen_media::wav::{Sample, Spec, Writer};
use lumen_project::model::{Id, Media, Project, TrackKind};

use crate::hosting::{HOSTED_KIND, OfflinePlugin, stand_in};
use crate::midi_play::{self, Hosts, Inputs, PortTable};
use crate::midi_runs::Cache;
use crate::render_audio::Again;
use crate::routing::{self, Devices};

/// How long a plugin's host may take to load before the bounce gives up.
const LOAD_WAIT: Duration = Duration::from_secs(60);

/// The longest tail a bounce adds after its span.
pub const MOST_TAIL_SECONDS: u32 = 10;

/// How much of the track's processing is in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bake {
    /// Its own sound, unprocessed (an instrument still plays).
    Plain,
    /// Its instrument, devices, trim and polarity, and the lanes for them: a
    /// freeze (the fader, pan, mute and sends stay live).
    Sound,
    /// Also its fader, pan and own lanes: a bounce with effects.
    Mix,
}

/// One track to render.
pub struct Job {
    /// The project as it was when the bounce began, with the plugins' states
    /// stamped in.
    pub project: Project,
    pub track: Id,
    /// The clips the engine plays, of any track (this track's are used).
    pub clips: Vec<Clip>,
    /// The plugin devices to host: the instrument, and the effects when
    /// `effects` is on.
    pub plugins: Vec<OfflinePlugin>,
    pub bake: Bake,
    /// The track plays mono material: a mono file is kept too, and used when
    /// the two channels came out the same, so the track keeps its pan law.
    pub mono: bool,
    /// Add what rings on after the span, until it is quiet.
    pub tail: bool,
    pub timeline: Timeline,
    pub rate: u32,
    /// Timeline sample of the file's first frame, and the span's length.
    pub from: i64,
    pub frames: u64,
    /// Where the file goes, and the media id it will have.
    pub file: PathBuf,
    pub media: Id,
}

/// What a finished job made.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub track: Id,
    pub media: Media,
    /// Frames after the span the tail added.
    pub tail_frames: u64,
}

/// Where a bounce stands.
#[derive(Debug)]
pub enum Poll {
    /// Still rendering, this far (0.0 to 1.0).
    Running(f32),
    Done(Vec<Rendered>),
    /// Stopped before it finished; the files are removed.
    Stopped,
    Failed(String),
}

/// A bounce on its worker. Dropping it stops the worker.
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

/// The plugin instrument of the track being rendered, handed to
/// [`midi_play::engine_midi_with`] in place of the live hosts'.
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

/// A plugin running in a host of the bounce's own.
struct Running {
    plugin: OfflinePlugin,
    linked: lumen_hosting::Linked,
}

/// Starts a host for each plugin and waits for them all to be linked.
/// `Ok(None)` if `cancel` was set meanwhile.
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
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(Some(running))
}

/// The node of the bounced track with only its own sound: no trim, polarity,
/// fader, pan, devices or lanes (a plugin instrument keeps its time).
fn plain(node: &mut NodeSpec) {
    let plain = NodeSpec::unity(node.id, node.output);
    *node = NodeSpec {
        source: node.source,
        ..plain
    };
}

/// The node of a frozen track: its trim, polarity, devices and the lanes for them,
/// but a fader at unity, pan centred with the balance law and no mute, send or
/// fader lane (the owner's FR1: those stay live on the track).
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

/// Renders one track to its file. `done` is told how far (0.0 to 1.0) after
/// each block. `None` means `cancel` stopped it and no file is left.
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

    // What the engine is given: the time each plugin reported, a routing of
    // this track alone, its clips and notes.
    let mut devices = Devices::default();
    for r in &running {
        devices.hosted.insert(r.plugin.device, r.linked.latency);
    }
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
    // MIDI is built from this track alone: another track's instrument is
    // not started.
    project.tracks.retain(|t| t.id == track);
    let mut statuses: Vec<(String, Arc<Status>)> = Vec::new();
    let mut instrument = None;
    let (mut clip_handle, clip_play) = clip_player();
    let (mut midi_handle, midi_play) = midi_player();
    let (mut mixer_handle, mut source) = mixer(Layered(clip_play, midi_play));
    for Running { plugin, linked } in running {
        let latency = linked.latency;
        let guard = Guard::new(Box::new(linked.link), latency);
        statuses.push((plugin.name.clone(), guard.status()));
        if plugin.instrument {
            instrument = Some(InstrumentSpec {
                kind: HOSTED_KIND,
                preset: 0,
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
    // A track that plays mono material also gets a mono file, kept if the two
    // channels never differed.
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
                u64::from(MOST_TAIL_SECONDS) * u64::from(rate)
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
    let frames_written = writer.finish().map_err(|e| e.to_string())?;
    let mut channels = 2;
    if let Some(twin) = twin.take() {
        // The mono file replaces the stereo one only if every block matched.
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
    /// Starts rendering `jobs` one after another on a worker; `ctx` is woken
    /// as it goes. `again` makes the jobs' clips on the worker, at the jobs'
    /// rate, when the loaded audio is at another one (`render_audio`).
    pub fn start(jobs: Vec<Job>, again: Option<Arc<Again>>, ctx: &egui::Context) -> Self {
        let (tx, results) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (stop, shown, ctx) = (Arc::clone(&cancel), Arc::clone(&progress), ctx.clone());
        let count = jobs.len().max(1) as f32;
        std::thread::Builder::new()
            .name("lumen-bounce".into())
            .spawn(move || {
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
                        // Stopped: drop the files already made and say nothing.
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
            })
            .expect("spawn bounce render");
        Self {
            results,
            cancel,
            progress,
        }
    }

    /// Where the bounce stands; a finished one answers `Done` once.
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

    /// Asks the worker to stop.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Removes the files of renders that will not be used.
pub fn remove_files(made: &[Rendered]) {
    for done in made {
        let _ = std::fs::remove_file(&done.media.path);
    }
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

    /// A job for a track that plays the ramp from the start.
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
        // Once the mixer's level has settled, the left holds the ramp at the
        // fader and the right nothing.
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

        // A pan lane-free track whose right channel differs stays stereo.
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
