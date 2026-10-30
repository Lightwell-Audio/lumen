//! Standalone whole-project export rendering (M9 R5).
//!
//! The renderer builds the same clip, MIDI, routing and mixer sources used by
//! playback, but gives them to `render_source` without an audio device. Plugin
//! hosts belong to this render and remain alive in `Prepared` for its lifetime.
//! This module does not pace rendering or encode anything beyond float WAV.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lumen_devices::{Guard, HostedInstrument, Status};
use lumen_engine::{
    Destination, InstrumentSpec, OFFLINE_WAIT, Routing, Source, SourceRender, Timeline,
    clip_player, midi_player, mixer, render_source,
};
use lumen_hosting::{Happened, Manager};
use lumen_media::wav::{Sample, Spec, Writer};
use lumen_project::model::{Id, Project};

use crate::hosting::{HOSTED_KIND, OfflinePlugin, stand_in};
use crate::midi_play::{self, Hosts, Inputs, PortTable};
use crate::midi_runs::Cache;
use crate::routing::{self, Cue, Devices};

const LOAD_WAIT: Duration = Duration::from_secs(60);
const MAX_TAIL_SECONDS: u32 = 10;

/// A snapshot to render. `clips` must be the engine clips translated from this
/// project and already prepared at `rate`; the caller owns media decoding and
/// rate conversion. `stem=None` renders the master. A selected stem is its
/// post-device, post-fader bus output; a group includes its child buses, and a
/// return includes the sends routed into it. Sends leaving the selected bus do
/// not enter that stem. Sidechain sources are retained as dependencies but are
/// not added to the stem's audible output.
pub struct Job {
    pub project: Project,
    pub clips: Vec<lumen_engine::Clip>,
    pub plugins: Vec<OfflinePlugin>,
    pub rate: u32,
    pub from: i64,
    pub frames: u64,
    pub tail: bool,
    /// Reserved destination inside the caller's private staging directory.
    /// Existing files are refused; Writer publishes atomically by rename.
    pub file: PathBuf,
    pub stem: Option<Id>,
}

/// A prepared independent source, ready to be located and driven by either
/// offline rendering or a real audio callback. Keep `manager` alive as long as
/// `source` is in use. The source contains no device monitor input, live MIDI
/// ports or metronome.
pub struct Prepared {
    pub source: Box<dyn Source>,
    pub manager: Manager,
    pub statuses: Vec<(String, Arc<Status>)>,
    pub timeline: Timeline,
    pub rate: u32,
}

/// What the renderer produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub file: PathBuf,
    pub frames: u64,
    pub tail_frames: u64,
    pub rate: u32,
}

struct Running {
    plugin: OfflinePlugin,
    linked: lumen_hosting::Linked,
}

struct ProjectInstruments {
    instruments: HashMap<Id, InstrumentSpec>,
    outputs: HashMap<Id, u32>,
}

impl Hosts for ProjectInstruments {
    fn instrument(&mut self, track: Id) -> InstrumentSpec {
        self.instruments.remove(&track).unwrap_or_else(stand_in)
    }

    fn outputs_on(&self, device: Id) -> u32 {
        self.outputs.get(&device).copied().unwrap_or(0)
    }
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
                Happened::Linked(linked) => running.push(Running {
                    plugin: waiting.remove(at),
                    linked: *linked,
                }),
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

/// Makes an independent full-mixer source from the immutable job snapshot.
/// `Ok(None)` means cancellation while plugin hosts were starting.
pub fn prepare(job: &Job, cancel: &AtomicBool) -> Result<Option<Prepared>, String> {
    if job.rate == 0 {
        return Err("The export sample rate must be greater than zero".into());
    }
    let tempo = lumen_core::time::TempoMap::new(job.project.tempo.clone())
        .map_err(|e| format!("The project tempo map is invalid: {e}"))?;
    let meter = lumen_core::meter::MeterMap::new(job.project.meter.clone())
        .map_err(|e| format!("The project time-signature map is invalid: {e}"))?;
    let timeline = Timeline::new(tempo, meter);
    let mut manager = Manager::new(|| {});
    let Some(running) = start_hosts(
        &mut manager,
        job.plugins
            .iter()
            .map(|p| OfflinePlugin {
                device: p.device,
                name: p.name.clone(),
                instrument: p.instrument,
                start: p.start.clone(),
            })
            .collect(),
        cancel,
    )?
    else {
        return Ok(None);
    };

    let mut devices = Devices::default();
    for item in &running {
        devices
            .hosted
            .insert(item.plugin.device, item.linked.latency);
    }
    let mut routing = routing::routing(&job.project, &Cue::default(), &devices, job.rate);
    // An export always takes the master's stereo bus, independent of the
    // physical output pair selected for listening on this machine.
    if let Some(master) = routing
        .nodes
        .iter_mut()
        .find(|n| n.id == job.project.master.id.0)
    {
        master.output = Destination::Hardware(0);
    }
    if let Some(stem) = job.stem {
        route_stem(&job.project, &mut routing, stem)?;
    }

    let (mut clips, clip_source) = clip_player();
    let (mut midi, midi_source) = midi_player();
    let (mut mixer_control, mixer_source) = mixer(lumen_engine::Layered(clip_source, midi_source));
    let mut statuses = Vec::with_capacity(running.len());
    let mut instruments = HashMap::new();
    let mut outputs = HashMap::new();
    for Running { plugin, linked } in running {
        let guard = Guard::new(Box::new(linked.link), linked.latency);
        statuses.push((plugin.name.clone(), guard.status()));
        if plugin.instrument {
            let Some(track) = job.project.tracks.iter().find(|track| {
                track
                    .devices
                    .iter()
                    .any(|device| device.id == plugin.device)
            }) else {
                return Err(format!(
                    "{} is not attached to a project track",
                    plugin.name
                ));
            };
            instruments.insert(
                track.id,
                InstrumentSpec {
                    kind: HOSTED_KIND,
                    preset: 0,
                    instrument: Box::new(HostedInstrument::new(guard)),
                },
            );
            // Export all declared auxiliary outputs the host made available.
            outputs.insert(plugin.device, linked.aux.outputs);
        } else if mixer_control.attach_host(plugin.device.0, guard).is_err() {
            return Err("Too many plugins to export at once".into());
        }
    }
    clips
        .set(job.clips.clone())
        .map_err(|_| "The engine could not take the export clips".to_owned())?;
    let mut cache = Cache::default();
    let mut hosts = ProjectInstruments {
        instruments,
        outputs,
    };
    let mut tracks = midi_play::engine_midi_with(
        &job.project,
        &Inputs::default(),
        &mut PortTable::default(),
        &mut cache,
        &mut hosts,
    );
    midi_play::with_lanes(&mut tracks, &job.project, &Default::default(), job.rate);
    midi.set(tracks)
        .map_err(|_| "The engine could not take the export MIDI".to_owned())?;
    if !mixer_control
        .set(&routing)
        .map_err(|e| format!("The project routing cannot be rendered: {e}"))?
    {
        return Err("The engine could not take the export routing".into());
    }
    Ok(Some(Prepared {
        source: Box::new(mixer_source),
        manager,
        statuses,
        timeline,
        rate: job.rate,
    }))
}

/// Renders to a private scratch float WAV, publishing it only once complete.
/// `Ok(None)` means canceled; no partial output or existing destination is
/// removed. Progress is normalized over the requested span plus tail.
pub fn render_job(
    job: Job,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<Rendered>, String> {
    if job.frames == 0 {
        return Err("The export range is empty".into());
    }
    if job.file.exists() {
        return Err(format!("{} already exists", job.file.display()));
    }
    let Some(mut prepared) = prepare(&job, cancel)? else {
        return Ok(None);
    };
    let parent = job
        .file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Could not make {}: {e}", parent.display()))?;
    let mut writer = Writer::create_long(
        &job.file,
        Spec {
            sample_rate: job.rate,
            channels: 2,
            sample: Sample::F32,
        },
    )
    .map_err(|e| format!("Could not create export scratch file: {e}"))?;
    let mut late = None;
    let mut write_failed = None;
    let write_stopped = Cell::new(false);
    let ended = render_source(
        prepared.source.as_mut(),
        SourceRender {
            timeline: &prepared.timeline,
            sample_rate: prepared.rate,
            from: job.from,
            frames: job.frames,
            most_tail: if job.tail {
                u64::from(MAX_TAIL_SECONDS) * u64::from(job.rate)
            } else {
                0
            },
            block: lumen_engine::MOST_BLOCK,
            wait: OFFLINE_WAIT,
        },
        cancel,
        &mut || {
            if write_stopped.get() {
                return true;
            }
            late = prepared
                .statuses
                .iter()
                .find(|(_, status)| status.failed() || status.late_blocks() > 0)
                .map(|(name, _)| name.clone());
            late.is_some()
        },
        progress,
        &mut |left, right| {
            // A filesystem failure is retained and reported after rendering.
            if write_failed.is_none()
                && let Err(error) = writer.write(&[left, right])
            {
                write_failed = Some(format!("Could not write export scratch: {error}"));
                write_stopped.set(true);
            }
        },
    );
    prepared.manager.shutdown(Duration::from_secs(2));
    if let Some(why) = write_failed {
        return Err(why);
    }
    if ended == lumen_engine::Ended::Cancelled {
        return Ok(None);
    }
    if let Some(why) = late {
        return Err(why);
    }
    if ended == lumen_engine::Ended::Failed {
        return Err("A plugin stopped answering; the export was discarded".into());
    }
    // The destination belongs to this task's private Stage. Normal rename
    // works on removable filesystems too; hard-link support is unnecessary.
    if job.file.exists() {
        return Err(format!("{} already exists", job.file.display()));
    }
    let frames = writer
        .finish()
        .map_err(|e| format!("Could not finish export scratch: {e}"))?;
    Ok(Some(Rendered {
        file: job.file,
        frames,
        tail_frames: frames.saturating_sub(job.frames),
        rate: job.rate,
    }))
}

/// Routes a single channel-strip stem while retaining the full upstream audio
/// and sidechain graph. Direct outputs and sends into the selected bus remain;
/// unrelated buses are silenced. Sidechain-only nodes are processed but not
/// included as audible output. The selected bus is tapped after its own mixer.
fn route_stem(_project: &Project, routing: &mut Routing, stem: Id) -> Result<(), String> {
    let selected = routing
        .nodes
        .iter()
        .position(|node| node.id == stem.0)
        .ok_or_else(|| "The requested export stem is not in the project".to_owned())?;
    let mut signal = BTreeSet::from([stem.0]);
    let mut sidechain_only = BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &routing.nodes {
            let feeds_signal = matches!(node.output, Destination::Node(to) if signal.contains(&to))
                || node.sends.iter().any(|send| signal.contains(&send.to));
            if feeds_signal && signal.insert(node.id) {
                changed = true;
            }
        }
        let needed: BTreeSet<u64> = signal.union(&sidechain_only).copied().collect();
        for node in &routing.nodes {
            let feeds_needed = matches!(node.output, Destination::Node(to) if needed.contains(&to))
                || node.sends.iter().any(|send| needed.contains(&send.to));
            if feeds_needed && !signal.contains(&node.id) && sidechain_only.insert(node.id) {
                changed = true;
            }
        }
        for node in routing
            .nodes
            .iter()
            .filter(|node| needed.contains(&node.id))
        {
            for sidechain in &node.sidechains {
                if sidechain_only.insert(sidechain.from) && !signal.contains(&sidechain.from) {
                    changed = true;
                }
            }
        }
    }
    // Keep only dependencies reachable through the selected signal graph.
    // Sidechain sources and the nodes feeding them remain available for their
    // requested tap, while no path from that auxiliary graph reaches hardware.
    for (index, node) in routing.nodes.iter_mut().enumerate() {
        if index == selected {
            node.output = Destination::Hardware(0);
            node.sends_audible = false;
            continue;
        }
        let needed = signal.contains(&node.id) || sidechain_only.contains(&node.id);
        if !needed {
            node.audible = false;
            node.sends_audible = false;
            node.output = Destination::None;
            continue;
        }
        node.sends
            .retain(|send| signal.contains(&send.to) || sidechain_only.contains(&send.to));
        node.sends_audible &= !node.sends.is_empty();
        if let Destination::Node(to) = node.output {
            if !signal.contains(&to) && !sidechain_only.contains(&to) {
                node.output = Destination::None;
            }
        } else {
            node.output = Destination::None;
        }
    }
    routing.cue = None;
    routing.measure = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_engine::{Clip, ClipAudio, ClipEnvelope, ClipShape};
    use lumen_project::model::{Color, Level, Send, Track, TrackKind};
    use lumen_project::testing::TempDir;

    const RATE: u32 = 48_000;
    const FRAMES: u64 = RATE as u64 * 2;

    fn track(id: u64, name: &str, kind: TrackKind) -> Track {
        Track::new(Id(id), name, kind, Color(0x5E8C6A))
    }

    fn source_clip(id: u64, track: u64, value: f32) -> Clip {
        Clip {
            id,
            track,
            start: Ticks::ZERO,
            offset: 0,
            length: FRAMES as i64,
            gain: 1.0,
            audio: Arc::new(ClipAudio::Memory {
                sample_rate: RATE,
                channels: vec![vec![value; FRAMES as usize]; 2],
            }),
            shape: ClipShape::default(),
            envelope: ClipEnvelope::default(),
        }
    }

    fn project(tracks: Vec<Track>) -> Project {
        let mut project = crate::empty_project();
        project.sample_rate = RATE;
        project.tracks = tracks.into_iter().map(Arc::new).collect();
        project
    }

    fn render_sample(project: Project, clips: Vec<Clip>, stem: Option<Id>, name: &str) -> f32 {
        let dir = TempDir::new("export-render");
        let file = dir.path().join(name);
        let job = Job {
            project,
            clips,
            plugins: Vec::new(),
            rate: RATE,
            from: 0,
            frames: FRAMES,
            tail: false,
            file: file.clone(),
            stem,
        };
        let cancel = AtomicBool::new(false);
        let mut progress = |_| {};
        let rendered = render_job(job, &cancel, &mut progress)
            .expect("synthetic project renders")
            .expect("not cancelled");
        assert_eq!(rendered.frames, FRAMES);
        let decoded = lumen_media::decode(&file, &cancel, &mut |_| {}).expect("float WAV decodes");
        decoded.channel(0)[RATE as usize / 2]
    }

    #[test]
    fn master_group_and_track_stems_keep_their_signal_paths() {
        let group = track(10, "Group", TrackKind::Group);
        let mut first = track(11, "First", TrackKind::Audio);
        first.parent = Some(group.id);
        let mut second = track(12, "Second", TrackKind::Audio);
        second.parent = Some(group.id);
        let clips = vec![source_clip(1, 11, 0.2), source_clip(2, 12, 0.3)];
        let p = project(vec![group.clone(), first.clone(), second.clone()]);

        let master = render_sample(p.clone(), clips.clone(), None, "master.wav");
        let group_stem = render_sample(p.clone(), clips.clone(), Some(group.id), "group.wav");
        let first_stem = render_sample(p, clips, Some(first.id), "first.wav");
        assert!((master - 0.5).abs() < 0.002, "master sum: {master}");
        assert!((group_stem - 0.5).abs() < 0.002, "group sum: {group_stem}");
        assert!((first_stem - 0.2).abs() < 0.002, "track stem: {first_stem}");
    }

    #[test]
    fn return_stem_contains_the_incoming_send_but_not_the_dry_track() {
        let mut source = track(21, "Source", TrackKind::Audio);
        let ret = track(22, "Return", TrackKind::Return);
        source.sends.push(Send {
            to: ret.id,
            level: Level(0.0),
            pre_fader: false,
        });
        let p = project(vec![source.clone(), ret.clone()]);
        let clips = vec![source_clip(1, source.id.0, 0.25)];
        let master = render_sample(p.clone(), clips.clone(), None, "send-master.wav");
        let return_stem = render_sample(p, clips, Some(ret.id), "return.wav");
        assert!((master - 0.5).abs() < 0.003, "dry plus return: {master}");
        assert!(
            (return_stem - 0.25).abs() < 0.002,
            "return send: {return_stem}"
        );
        source.mute = true;
        let muted = render_sample(
            project(vec![source.clone(), ret.clone()]),
            vec![source_clip(1, source.id.0, 0.25)],
            Some(ret.id),
            "muted-return.wav",
        );
        assert_eq!(
            muted, 0.0,
            "a muted source must not leak into a return stem"
        );
    }

    #[test]
    fn existing_destination_is_preserved() {
        let dir = TempDir::new("export-existing");
        let file = dir.path().join("existing.wav");
        std::fs::write(&file, b"keep me").expect("fixture");
        let job = Job {
            project: project(vec![track(1, "Audio", TrackKind::Audio)]),
            clips: vec![source_clip(1, 1, 0.1)],
            plugins: Vec::new(),
            rate: RATE,
            from: 0,
            frames: FRAMES,
            tail: false,
            file: file.clone(),
            stem: None,
        };
        let mut progress = |_| {};
        assert!(render_job(job, &AtomicBool::new(false), &mut progress).is_err());
        assert_eq!(std::fs::read(file).expect("preserved"), b"keep me");
    }
}
