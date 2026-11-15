use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use lumen_engine::{CaptureChunk, CaptureReader};
use lumen_media::take::TakeWriter;
use serde::{Deserialize, Serialize};

const POLL: Duration = Duration::from_millis(5);
const REFRESH: Duration = Duration::from_secs(1);
const WRAP_SLACK: i64 = 64;
const SIDECAR: &str = "take";
pub(crate) const VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct Job {
    pub track: u64,
    pub path: PathBuf,
    pub channels: u16,
    pub live: Arc<Mutex<Live>>,
    pub session: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub id: u64,
    pub jobs: Vec<Job>,
    pub rate: u32,
    pub shift: i64,
    pub window: Option<(i64, i64)>,
    pub looped: Option<(i64, i64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Wrap {
    pub frame: u64,
    pub at: i64,
}

#[derive(Debug, Default)]
pub(crate) struct Live {
    pub start: Option<i64>,
    pub frames: u64,
    pub written: u64,
    pub previous: Option<(u64, lumen_media::GrowingPeaks)>,
    pub peaks: lumen_media::GrowingPeaks,
}

impl Live {
    fn restart(&mut self, start: i64, whole: bool) {
        let ended = std::mem::take(&mut self.peaks);
        *self = Live {
            start: Some(start),
            written: self.written,
            previous: whole.then_some((self.frames, ended)),
            ..Live::default()
        };
    }

    fn add(&mut self, channels: &[&[f32]]) {
        if let Some(left) = channels.first() {
            self.peaks.add(left, channels.get(1).copied());
        }
        let n = channels.first().map_or(0, |c| c.len()) as u64;
        self.frames += n;
        self.written += n;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackResult {
    pub track: u64,
    pub path: PathBuf,
    pub rate: u32,
    pub channels: u16,
    pub start: Option<i64>,
    pub frames: u64,
    pub size_bytes: u64,
    pub dropped: u64,
    pub error: Option<String>,
    pub wraps: Vec<Wrap>,
    pub silence: u64,
    pub looped: Option<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Sidecar {
    #[serde(default = "legacy_version")]
    pub version: u32,
    #[serde(default)]
    pub session: String,
    pub track: u64,
    pub start: i64,
    pub rate: u32,
    pub channels: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wraps: Vec<Wrap>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub silence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looped: Option<(i64, i64)>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn legacy_version() -> u32 {
    VERSION
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SidecarRead {
    Known(Sidecar),
    Newer {
        version: u32,
        session: Option<String>,
    },
    Unreadable,
}

pub(crate) fn read_sidecar(file: &Path) -> SidecarRead {
    #[derive(Deserialize)]
    struct Header {
        #[serde(default)]
        version: u32,
        #[serde(default)]
        session: Option<String>,
    }
    let Ok(bytes) = fs::read(file) else {
        return SidecarRead::Unreadable;
    };
    let Ok(header) = serde_json::from_slice::<Header>(&bytes) else {
        return SidecarRead::Unreadable;
    };
    let version = if header.version == 0 {
        VERSION
    } else {
        header.version
    };
    if version > VERSION {
        return SidecarRead::Newer {
            version,
            session: header.session,
        };
    }
    match serde_json::from_slice(&bytes) {
        Ok(sidecar) => SidecarRead::Known(sidecar),
        Err(_) => SidecarRead::Unreadable,
    }
}

pub(crate) fn free_file(preferred: &Path, taken: &[PathBuf]) -> PathBuf {
    let free = |p: &Path| !p.exists() && !sidecar_path(p).exists() && !taken.iter().any(|t| t == p);
    if free(preferred) {
        return preferred.to_owned();
    }
    let dir = preferred.parent().unwrap_or(Path::new("."));
    let stem = preferred
        .file_stem()
        .map_or_else(|| "Audio".to_owned(), |s| s.to_string_lossy().into_owned());
    let base = match stem.rsplit_once(' ') {
        Some((base, n)) if !base.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => base,
        _ => stem.as_str(),
    };
    (1..)
        .map(|n| dir.join(format!("{base} {n}.wav")))
        .find(|p| free(p))
        .expect("an unused take name exists")
}

pub(crate) fn sidecar_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".");
    name.push(SIDECAR);
    PathBuf::from(name)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Unfinished {
    pub file: PathBuf,
    pub sidecar: Sidecar,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct SessionTakes {
    pub takes: Vec<Unfinished>,
    pub newer: usize,
}

pub(crate) fn unfinished(dir: &Path, session: &str) -> SessionTakes {
    let suffix = format!(".wav.{SIDECAR}");
    let mut found = SessionTakes::default();
    for path in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = path.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(&suffix) else {
            continue;
        };
        let file = path.with_file_name(format!("{stem}.wav"));
        match read_sidecar(&path) {
            SidecarRead::Known(sidecar) if sidecar.session == session && file.is_file() => {
                found.takes.push(Unfinished { file, sidecar });
            }
            SidecarRead::Newer {
                session: Some(owner),
                ..
            } if owner == session => found.newer += 1,
            _ => {}
        }
    }
    found.takes.sort_by(|a, b| a.file.cmp(&b.file));
    found
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Waiting,
    Writing,
    Done,
}

struct Lane {
    job: Job,
    path: PathBuf,
    state: State,
    writer: Option<TakeWriter>,
    next: Option<u64>,
    start: Option<i64>,
    silence: u64,
    looped: Option<(i64, i64)>,
    dropped: u64,
    error: Option<String>,
    refreshed: Instant,
    rate: u32,
    written: u64,
    expected: Option<i64>,
    wraps: Vec<Wrap>,
    sidecar_current: bool,
}

pub(crate) struct Takes {
    plan: Plan,
    lanes: Vec<Lane>,
}

impl Takes {
    pub(crate) fn new(plan: Plan) -> Self {
        let mut taken: Vec<PathBuf> = Vec::new();
        let lanes = plan
            .jobs
            .iter()
            .map(|job| {
                let path = free_file(&job.path, &taken);
                taken.push(path.clone());
                (job, path)
            })
            .map(|(job, path)| Lane {
                job: job.clone(),
                path,
                state: State::Waiting,
                writer: None,
                next: None,
                start: None,
                silence: 0,
                looped: plan.looped,
                dropped: 0,
                error: None,
                refreshed: Instant::now(),
                rate: plan.rate,
                written: 0,
                expected: None,
                wraps: Vec::new(),
                sidecar_current: true,
            })
            .collect();
        Self { plan, lanes }
    }

    pub(crate) fn feed(&mut self, chunk: CaptureChunk, data: &[f32]) {
        let Some(index) = self.lanes.iter().position(|l| l.job.track == chunk.track) else {
            return;
        };
        let (rate, shift, window) = (self.plan.rate, self.plan.shift, self.plan.window);
        let lane = &mut self.lanes[index];
        if lane.state == State::Done {
            return;
        }
        let frames = chunk.frames as usize;
        if u16::from(chunk.channels) != lane.job.channels
            || data.len() != frames * usize::from(chunk.channels)
        {
            lane.fail("The input changed while recording".to_owned());
            return;
        }
        if let Some(next) = lane.next
            && chunk.device > next
            && lane.state == State::Writing
        {
            let gap = chunk.device - next;
            lane.dropped += gap;
            let silence = lane.writer.as_mut().map(|w| w.write_silence(gap as usize));
            if let Some(Err(error)) = silence {
                lane.fail(error.to_string());
                return;
            }
            lane.written += gap;
            lane.expected = lane.expected.map(|e| e + gap as i64);
            if let Ok(mut live) = lane.job.live.lock() {
                live.add(&[&vec![0.0; gap as usize]]);
            }
        }
        lane.next = Some(chunk.device + frames as u64);

        let placed = chunk.at - shift;
        if self.plan.looped.is_some()
            && lane.state == State::Writing
            && lane.expected.is_some_and(|e| placed + WRAP_SLACK < e)
        {
            lane.wraps.push(Wrap {
                frame: lane.written,
                at: placed,
            });
            lane.sidecar_current = false;
            let whole = lane.wraps.len() > 1
                || lane.start.is_some_and(|s| {
                    (s + lane.silence as i64 - placed).unsigned_abs() <= u64::from(rate / 50)
                });
            if let Ok(mut live) = lane.job.live.lock() {
                live.restart(placed.max(0), whole);
            }
        }
        let (lo, hi) = window.unwrap_or((0, i64::MAX));
        let from = if lane.state == State::Waiting {
            (lo - placed).clamp(0, frames as i64) as usize
        } else {
            0
        };
        let to = (hi.saturating_sub(placed)).clamp(0, frames as i64) as usize;
        if to <= from {
            if placed >= hi {
                lane.finish_early();
            }
            return;
        }
        if lane.state == State::Waiting {
            let at = placed + from as i64;
            let silence = self.plan.looped.map_or(0, |(start, _)| {
                let pad = at - (start - shift).max(lo);
                if pad > i64::from(rate / 50) {
                    pad as u64
                } else {
                    0
                }
            });
            if lane.open(rate, at, silence).is_err() {
                return;
            }
        }
        let left = &data[from..to];
        let right = (chunk.channels == 2).then(|| &data[frames + from..frames + to]);
        let result = match (lane.writer.as_mut(), right) {
            (Some(w), Some(r)) => w.write(&[left, r]),
            (Some(w), None) => w.write(&[left]),
            (None, _) => return,
        };
        if let Err(error) = result {
            lane.fail(error.to_string());
            return;
        }
        lane.written += left.len() as u64;
        lane.expected = Some(placed + to as i64);
        if let Ok(mut live) = lane.job.live.lock() {
            match right {
                Some(r) => live.add(&[left, r]),
                None => live.add(&[left]),
            }
        }
        if to < frames {
            lane.finish_early();
        }
    }

    pub(crate) fn tick(&mut self) {
        for lane in &mut self.lanes {
            if lane.refreshed.elapsed() >= REFRESH {
                lane.refreshed = Instant::now();
                if let Some(Err(error)) = lane.writer.as_mut().map(TakeWriter::refresh) {
                    lane.fail(error.to_string());
                }
                lane.write_sidecar();
            }
        }
    }

    pub(crate) fn finish(self) -> Vec<TrackResult> {
        let rate = self.plan.rate;
        self.lanes
            .into_iter()
            .map(|mut lane| {
                let frames = lane.close();
                TrackResult {
                    track: lane.job.track,
                    path: lane.path.clone(),
                    rate,
                    channels: lane.job.channels,
                    start: lane.start.filter(|_| frames > 0),
                    frames,
                    size_bytes: fs::metadata(&lane.path).map_or(0, |m| m.len()),
                    dropped: lane.dropped,
                    error: lane.error.take(),
                    wraps: std::mem::take(&mut lane.wraps),
                    silence: lane.silence,
                    looped: self.plan.looped,
                }
            })
            .collect()
    }
}

impl Lane {
    fn open(&mut self, rate: u32, start: i64, silence: u64) -> Result<(), ()> {
        let made = (|| -> Result<TakeWriter, String> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            self.start = Some(start - silence as i64);
            self.silence = silence;
            self.rate = rate;
            self.write_sidecar_now().map_err(|e| e.to_string())?;
            let mut writer = TakeWriter::create(&self.path, rate, self.job.channels)
                .map_err(|e| e.to_string())?;
            writer
                .write_silence(silence as usize)
                .map_err(|e| e.to_string())?;
            self.written = silence;
            Ok(writer)
        })();
        match made {
            Ok(writer) => {
                self.writer = Some(writer);
                self.state = State::Writing;
                self.refreshed = Instant::now();
                if let Ok(mut live) = self.job.live.lock() {
                    live.start = Some(start);
                }
                Ok(())
            }
            Err(error) => {
                self.fail(format!("Could not start the recording file: {error}"));
                Err(())
            }
        }
    }

    fn sidecar(&self) -> Option<Sidecar> {
        Some(Sidecar {
            version: VERSION,
            session: self.job.session.clone(),
            track: self.job.track,
            start: self.start?,
            rate: self.rate,
            channels: self.job.channels,
            wraps: self.wraps.clone(),
            silence: self.silence,
            looped: self.looped,
        })
    }

    fn write_sidecar_now(&mut self) -> std::io::Result<()> {
        let Some(side) = self.sidecar() else {
            return Ok(());
        };
        let json = serde_json::to_vec(&side).map_err(std::io::Error::other)?;
        write_whole(&sidecar_path(&self.path), &json)?;
        self.sidecar_current = true;
        Ok(())
    }

    fn write_sidecar(&mut self) {
        if !self.sidecar_current && self.writer.is_some() {
            let _ = self.write_sidecar_now();
        }
    }

    fn fail(&mut self, error: String) {
        self.error.get_or_insert(error);
        self.state = State::Done;
    }

    fn finish_early(&mut self) {
        self.state = State::Done;
    }

    fn close(&mut self) -> u64 {
        let Some(writer) = self.writer.take() else {
            return 0;
        };
        match writer.finish() {
            Ok(frames) => frames,
            Err(error) => {
                self.error.get_or_insert(error.to_string());
                lumen_media::take::repair(&self.path).map_or(0, |r| r.frames)
            }
        }
    }
}

pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[derive(Debug)]
pub(crate) struct Finished {
    pub id: u64,
    pub results: Vec<TrackResult>,
}

#[derive(Debug, Default)]
pub(crate) struct Found {
    pub takes: Vec<TrackResult>,
    pub unreadable: usize,
    pub newer: usize,
}

pub(crate) fn recover(dir: &Path, session: &str) -> Found {
    let mut found = Found::default();
    let leftover = unfinished(dir, session);
    found.newer = leftover.newer;
    for lost in leftover.takes {
        match lumen_media::take::repair(&lost.file) {
            Ok(r) if r.frames > 0 => found.takes.push(TrackResult {
                track: lost.sidecar.track,
                path: lost.file.clone(),
                rate: r.sample_rate,
                channels: r.channels,
                start: Some(lost.sidecar.start),
                frames: r.frames,
                size_bytes: fs::metadata(&lost.file).map_or(0, |m| m.len()),
                dropped: 0,
                error: None,
                wraps: lost.sidecar.wraps.clone(),
                silence: lost.sidecar.silence,
                looped: lost.sidecar.looped,
            }),
            Ok(_) => {
                let _ = fs::remove_file(&lost.file);
                let _ = fs::remove_file(sidecar_path(&lost.file));
            }
            Err(_) => found.unreadable += 1,
        }
    }
    found
}

enum Message {
    Begin(Plan),
    Finish,
}

#[derive(Debug, Default)]
pub(crate) struct Gauge {
    pub dropped: AtomicU64,
}

pub(crate) struct TakeWorker {
    tx: mpsc::Sender<Message>,
    done: mpsc::Receiver<Finished>,
    gauge: Arc<Gauge>,
}

impl TakeWorker {
    pub(crate) fn spawn(reader: CaptureReader) -> Self {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let gauge = Arc::new(Gauge::default());
        let seen = Arc::clone(&gauge);
        let spawned = thread::Builder::new()
            .name("take-writer".to_owned())
            .spawn(move || run(reader, &rx, &done_tx, &seen));
        drop(spawned);
        Self { tx, done, gauge }
    }

    pub(crate) fn begin(&self, plan: Plan) {
        let _ = self.tx.send(Message::Begin(plan));
    }

    pub(crate) fn finish(&self) {
        let _ = self.tx.send(Message::Finish);
    }

    pub(crate) fn poll(&self) -> Option<Finished> {
        self.done.try_recv().ok()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.gauge.dropped.load(Ordering::Relaxed)
    }
}

fn run(
    mut reader: CaptureReader,
    rx: &mpsc::Receiver<Message>,
    done: &mpsc::Sender<Finished>,
    gauge: &Gauge,
) {
    let mut takes: Option<Takes> = None;
    let mut samples = Vec::new();
    let mut baseline = 0;
    loop {
        let message = rx.recv_timeout(POLL);
        match &message {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(Message::Begin(_)) => {
                while reader.next(&mut samples).is_some() {}
                baseline = reader.dropped_frames();
            }
            _ => {}
        }
        if let Ok(Message::Begin(plan)) = message {
            takes = Some(Takes::new(plan));
            continue;
        }
        while let Some(chunk) = reader.next(&mut samples) {
            if let Some(takes) = &mut takes {
                takes.feed(chunk, &samples);
            }
        }
        if let Some(takes) = &mut takes {
            takes.tick();
        }
        gauge.dropped.store(
            reader.dropped_frames().saturating_sub(baseline),
            Ordering::Relaxed,
        );
        if let Ok(Message::Finish) = message
            && let Some(takes) = takes.take()
        {
            let id = takes.plan.id;
            let _ = done.send(Finished {
                id,
                results: takes.finish(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-takes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unfinished_takes_are_found_by_session_with_their_sidecars() {
        let dir = folder("unfinished");
        let side = |session: &str, track: u64| Sidecar {
            version: VERSION,
            session: session.to_owned(),
            track,
            start: 480,
            rate: 48_000,
            channels: 1,
            wraps: Vec::new(),
            silence: 0,
            looped: None,
        };
        for (name, session, track) in [("a", "s1", 1), ("b", "s2", 2), ("c", "s1", 3)] {
            fs::write(dir.join(format!("{name}.wav")), b"x").unwrap();
            fs::write(
                sidecar_path(&dir.join(format!("{name}.wav"))),
                serde_json::to_vec(&side(session, track)).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            sidecar_path(&dir.join("gone.wav")),
            serde_json::to_vec(&side("s1", 9)).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("junk.wav.take"), b"not json").unwrap();
        let found = unfinished(&dir, "s1");
        let names: Vec<_> = found
            .takes
            .iter()
            .map(|u| u.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.wav", "c.wav"]);
        assert_eq!(found.takes[1].sidecar, side("s1", 3));
        assert!(unfinished(&dir, "nobody").takes.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn frozen_take() -> Sidecar {
        Sidecar {
            version: VERSION,
            session: "1762310400000-4242".into(),
            track: 3,
            start: 480,
            rate: 48_000,
            channels: 2,
            wraps: vec![Wrap {
                frame: 9_600,
                at: 480,
            }],
            silence: 240,
            looped: Some((480, 9_600)),
        }
    }

    #[test]
    fn a_legacy_sidecar_without_a_version_still_says_what_its_take_is() {
        let read = read_sidecar(&fixture("take-sidecar-legacy.json"));
        let SidecarRead::Known(sidecar) = read else {
            panic!("a file written before versioning must load: {read:?}")
        };
        assert_eq!(
            sidecar,
            frozen_take(),
            "a file without a version is the version 1 shape"
        );
    }

    #[test]
    fn a_version_1_sidecar_says_what_its_take_is() {
        let read = read_sidecar(&fixture("take-sidecar-v1.json"));
        let SidecarRead::Known(sidecar) = read else {
            panic!("a version 1 sidecar must load: {read:?}")
        };
        assert_eq!(sidecar, frozen_take());
    }

    #[test]
    fn recovery_finds_both_sidecar_shapes_for_their_session() {
        for name in ["take-sidecar-legacy.json", "take-sidecar-v1.json"] {
            let dir = folder("fixture-read");
            let file = dir.join("Vox 1.wav");
            fs::write(&file, b"x").unwrap();
            fs::copy(fixture(name), sidecar_path(&file)).unwrap();
            let found = unfinished(&dir, "1762310400000-4242");
            assert_eq!(found.takes.len(), 1, "{name}");
            assert_eq!(found.takes[0].sidecar, frozen_take(), "{name}");
            assert_eq!(found.newer, 0, "{name}");
            assert_eq!(unfinished(&dir, "someone-else").takes.len(), 0, "{name}");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn a_sidecar_a_newer_build_wrote_is_kept_and_never_read() {
        let dir = folder("newer-version");
        let file = dir.join("Vox 1.wav");
        fs::write(&file, b"x").unwrap();
        let side = r#"{"version":2,"session":"s","track":1,"start":0,"rate":48000,"channels":1}"#;
        fs::write(sidecar_path(&file), side).unwrap();
        let found = unfinished(&dir, "s");
        assert!(found.takes.is_empty(), "a newer shape is not read");
        assert_eq!(found.newer, 1, "and is counted, to be said");
        assert_eq!(unfinished(&dir, "other").newer, 0, "not another session's");
        let found = recover(&dir, "s");
        assert!(found.takes.is_empty());
        assert_eq!(found.newer, 1);
        assert!(file.exists(), "the take is kept");
        assert_eq!(
            fs::read_to_string(sidecar_path(&file)).unwrap(),
            side,
            "the sidecar is kept as it was"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sidecar_written_now_declares_its_version() {
        let dir = folder("versioned");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, None));
        let (chunk, data) = mono(1, 0, 0, 64);
        takes.feed(chunk, &data);
        let text = fs::read_to_string(sidecar_path(&j.path)).unwrap();
        assert!(text.starts_with(r#"{"version":1,"#), "{text}");
        let SidecarRead::Known(side) = read_sidecar(&sidecar_path(&j.path)) else {
            panic!("what the worker wrote must read back")
        };
        assert_eq!((side.version, side.track, side.start), (VERSION, 1, 0));
        takes.finish();
        fs::remove_dir_all(dir).unwrap();
    }

    fn job(dir: &Path, track: u64, channels: u16) -> Job {
        Job {
            track,
            path: dir.join(format!("t{track}.wav")),
            channels,
            live: Arc::default(),
            session: String::new(),
        }
    }

    fn plan(jobs: Vec<Job>, shift: i64, window: Option<(i64, i64)>) -> Plan {
        Plan {
            id: 1,
            jobs,
            rate: 48_000,
            shift,
            window,
            looped: None,
        }
    }

    fn mono(track: u64, device: u64, at: i64, frames: u32) -> (CaptureChunk, Vec<f32>) {
        let chunk = CaptureChunk {
            track,
            device,
            at,
            frames,
            channels: 1,
        };
        (
            chunk,
            (0..frames)
                .map(|i| (device + u64::from(i)) as f32)
                .collect(),
        )
    }

    fn read(path: &Path) -> Vec<f32> {
        let audio = lumen_media::load(
            path,
            48_000,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        audio.channel(0).to_vec()
    }

    #[test]
    fn a_take_is_written_from_the_first_frame_placed_on_the_timeline() {
        let dir = folder("basic");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 100, None));
        for k in 0..4_u64 {
            let (chunk, data) = mono(1, 1_000 + k * 64, 500 + k as i64 * 64, 64);
            takes.feed(chunk, &data);
        }
        assert!(sidecar_path(&j.path).exists(), "unfinished while it runs");
        let results = takes.finish();
        assert!(
            sidecar_path(&j.path).exists(),
            "finished: the sidecar stays until a save holds the take"
        );
        assert_eq!(results[0].size_bytes, fs::metadata(&j.path).unwrap().len());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].start, Some(400));
        assert_eq!(results[0].frames, 256);
        assert_eq!(results[0].error, None);
        let audio = read(&j.path);
        assert_eq!(audio[0], 1_000.0);
        assert_eq!(audio[255], 1_255.0);
        assert_eq!(j.live.lock().unwrap().start, Some(400));
        assert_eq!(j.live.lock().unwrap().frames, 256);
        assert_eq!(j.live.lock().unwrap().peaks.frames(), 256);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn frames_before_the_start_of_the_project_are_cut_off_the_front() {
        let dir = folder("front");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 100, None));
        let (chunk, data) = mono(1, 0, 0, 256);
        takes.feed(chunk, &data);
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.frames), (Some(0), 156));
        assert_eq!(read(&j.path)[0], 100.0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_later_shift_moves_the_take_later() {
        let dir = folder("later");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j], -50, None));
        let (chunk, data) = mono(1, 0, 1_000, 64);
        takes.feed(chunk, &data);
        assert_eq!(takes.finish()[0].start, Some(1_050));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_punch_range_is_all_a_file_holds() {
        let dir = folder("punch");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, Some((150, 400))));
        for k in 0..6_u64 {
            let (chunk, data) = mono(1, k * 100, (k * 100) as i64, 100);
            takes.feed(chunk, &data);
        }
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.frames), (Some(150), 250));
        let audio = read(&j.path);
        assert_eq!((audio[0], audio[249]), (150.0, 399.0));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_punch_range_never_reached_leaves_no_file() {
        let dir = folder("never");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, Some((5_000, 6_000))));
        let (chunk, data) = mono(1, 0, 0, 100);
        takes.feed(chunk, &data);
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.frames), (None, 0));
        assert!(!j.path.exists() && !sidecar_path(&j.path).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_loop_wrap_goes_on_in_the_same_file() {
        let dir = folder("loop");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 10, None));
        let (a, da) = mono(1, 0, 0, 200);
        let (b, db) = mono(1, 200, 0, 100);
        takes.feed(a, &da);
        takes.feed(b, &db);
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.frames), (Some(0), 290));
        let audio = read(&j.path);
        assert_eq!((audio[189], audio[190], audio[289]), (199.0, 200.0, 299.0));
        fs::remove_dir_all(dir).unwrap();
    }

    fn looping(jobs: Vec<Job>, shift: i64) -> Plan {
        Plan {
            looped: Some((1_000, 1_200)),
            ..plan(jobs, shift, None)
        }
    }

    #[test]
    fn a_take_begun_inside_the_loop_begins_with_silence_back_to_its_start() {
        let dir = folder("lineup");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        for (k, at) in [3_000, 1_000, 1_100].into_iter().enumerate() {
            let (chunk, data) = mono(1, k as u64 * 100, at, 100);
            takes.feed(chunk, &data);
        }
        assert_eq!(j.live.lock().unwrap().previous.as_ref().map(|p| p.0), None);
        takes.lanes[0].write_sidecar_now().unwrap();
        let side: Sidecar =
            serde_json::from_slice(&fs::read(sidecar_path(&j.path)).unwrap()).unwrap();
        assert_eq!(
            (side.start, side.silence),
            (990, 2_000),
            "recovery lines it up too"
        );
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.silence, r.frames), (Some(990), 2_000, 2_300));
        assert_eq!(
            r.wraps,
            [Wrap {
                frame: 2_100,
                at: 990
            }]
        );
        let audio = read(&j.path);
        assert!(audio[..2_000].iter().all(|&a| a == 0.0));
        assert_eq!((audio[2_001], audio[2_100]), (1.0, 100.0));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_take_begun_at_or_before_the_loop_has_no_silence() {
        let dir = folder("nolineup");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        let (chunk, data) = mono(1, 0, 400, 100);
        takes.feed(chunk, &data);
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.silence), (Some(390), 0));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_jump_back_with_loop_on_notes_where_the_new_pass_begins() {
        let dir = folder("wraps");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        for (k, at) in [1_000, 1_100, 1_000, 1_100, 1_000].into_iter().enumerate() {
            let (chunk, data) = mono(1, k as u64 * 100, at, 100);
            takes.feed(chunk, &data);
        }
        let r = takes.finish().remove(0);
        assert_eq!((r.start, r.frames), (Some(990), 500));
        assert_eq!(
            r.wraps,
            [
                Wrap {
                    frame: 200,
                    at: 990
                },
                Wrap {
                    frame: 400,
                    at: 990
                }
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_is_drawn_while_looping_is_the_pass_under_way() {
        let dir = folder("livewraps");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        for (k, at) in [1_000, 1_100, 1_000].into_iter().enumerate() {
            let (chunk, data) = mono(1, k as u64 * 100, at, 100);
            takes.feed(chunk, &data);
        }
        let live = j.live.lock().unwrap();
        assert_eq!(live.start, Some(990), "from the loop's start");
        assert_eq!(live.frames, 100, "the new pass so far");
        assert!(live.peaks.frames() > 0 && live.peaks.frames() <= 100);
        drop(live);
        takes.finish();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_whole_pass_before_the_one_under_way_is_kept_to_draw_behind_it() {
        let dir = folder("livebefore");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        for (k, at) in [1_000, 1_100, 1_000].into_iter().enumerate() {
            let (chunk, data) = mono(1, k as u64 * 100, at, 100);
            takes.feed(chunk, &data);
        }
        let live = j.live.lock().unwrap();
        let (frames, peaks) = live.previous.as_ref().expect("the first pass");
        assert_eq!(*frames, 200);
        assert!(peaks.frames() > 0);
        drop(live);
        takes.finish();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_pass_that_began_inside_the_loop_is_not_drawn_behind_the_next() {
        let dir = folder("livepartial");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        for (k, at) in [6_000, 1_000, 1_100].into_iter().enumerate() {
            let (chunk, data) = mono(1, k as u64 * 100, at, 100);
            takes.feed(chunk, &data);
        }
        assert!(j.live.lock().unwrap().previous.is_none());
        takes.finish();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_loop_a_jump_back_is_no_pass() {
        let dir = folder("nowraps");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j], 0, None));
        let (a, da) = mono(1, 0, 1_000, 100);
        let (b, db) = mono(1, 100, 1_000, 100);
        takes.feed(a, &da);
        takes.feed(b, &db);
        assert!(takes.finish()[0].wraps.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dropped_chunk_is_not_taken_for_a_wrap_and_the_sidecar_follows_the_wraps() {
        let dir = folder("sidecar");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 0));
        let (a, da) = mono(1, 0, 1_000, 100);
        let (b, db) = mono(1, 200, 1_200, 100);
        let (c, dc) = mono(1, 300, 1_000, 100);
        takes.feed(a, &da);
        takes.feed(b, &db);
        takes.feed(c, &dc);
        takes.lanes[0].write_sidecar();
        let side: Sidecar =
            serde_json::from_slice(&fs::read(sidecar_path(&j.path)).unwrap()).unwrap();
        assert_eq!(
            side.wraps,
            [Wrap {
                frame: 300,
                at: 1_000
            }]
        );
        assert_eq!(takes.finish()[0].wraps.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dropped_chunk_is_filled_with_silence_so_the_rest_stays_in_time() {
        let dir = folder("gap");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, None));
        let (a, da) = mono(1, 0, 0, 100);
        let (b, db) = mono(1, 300, 300, 100);
        takes.feed(a, &da);
        takes.feed(b, &db);
        let r = takes.finish().remove(0);
        assert_eq!((r.frames, r.dropped), (400, 200));
        let audio = read(&j.path);
        assert_eq!(
            (audio[99], audio[100], audio[299], audio[300]),
            (99.0, 0.0, 0.0, 300.0)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn two_tracks_get_two_files_and_a_pair_keeps_both_channels() {
        let dir = folder("two");
        let (a, b) = (job(&dir, 1, 1), job(&dir, 2, 2));
        let mut takes = Takes::new(plan(vec![a.clone(), b.clone()], 0, None));
        let (chunk, data) = mono(1, 0, 0, 8);
        takes.feed(chunk, &data);
        let pair = CaptureChunk {
            track: 2,
            device: 0,
            at: 0,
            frames: 4,
            channels: 2,
        };
        takes.feed(pair, &[1.0, 2.0, 3.0, 4.0, -1.0, -2.0, -3.0, -4.0]);
        let (chunk, data) = mono(9, 0, 0, 8);
        takes.feed(chunk, &data);
        let results = takes.finish();
        assert_eq!(results.iter().map(|r| r.frames).collect::<Vec<_>>(), [8, 4]);
        let audio = lumen_media::load(
            &b.path,
            48_000,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(audio.channel(0), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(audio.channel(1), [-1.0, -2.0, -3.0, -4.0]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_input_that_changes_shape_ends_that_take_with_a_message() {
        let dir = folder("shape");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, None));
        let (chunk, data) = mono(1, 0, 0, 64);
        takes.feed(chunk, &data);
        let pair = CaptureChunk {
            channels: 2,
            ..mono(1, 64, 64, 64).0
        };
        takes.feed(pair, &[0.0; 128]);
        let (chunk, data) = mono(1, 128, 128, 64);
        takes.feed(chunk, &data);
        let r = takes.finish().remove(0);
        assert_eq!(r.frames, 64, "what was written is kept");
        assert!(r.error.unwrap().contains("changed"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_worker_thread_drains_a_ring_into_files_and_reports() {
        let dir = folder("thread");
        let (reader, mut sink) = lumen_engine::capture();
        let worker = TakeWorker::spawn(reader);
        let j = job(&dir, 1, 1);
        sink.push(1, 0, 0, &[9.0; 10], None);
        std::thread::sleep(Duration::from_millis(30));
        worker.begin(plan(vec![j.clone()], 0, None));
        std::thread::sleep(Duration::from_millis(30));
        for k in 0..10_u64 {
            let data: Vec<f32> = (0..100).map(|i| (k * 100 + i) as f32).collect();
            assert!(sink.push(1, 1_000 + k * 100, (k * 100) as i64, &data, None));
        }
        worker.finish();
        let results = loop {
            if let Some(r) = worker.poll() {
                assert_eq!(r.id, 1, "the answer names its recording");
                break r.results;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((results[0].frames, results[0].start), (1_000, Some(0)));
        assert_eq!(read(&j.path)[999], 999.0);
        drop(worker);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_take_name_in_use_moves_on_to_the_next_number() {
        let dir = folder("names");
        assert_eq!(
            free_file(&dir.join("Vox 1.wav"), &[]),
            dir.join("Vox 1.wav")
        );
        fs::write(dir.join("Vox 1.wav"), b"x").unwrap();
        fs::write(sidecar_path(&dir.join("Vox 2.wav")), b"x").unwrap();
        let now = [dir.join("Vox 3.wav")];
        assert_eq!(
            free_file(&dir.join("Vox 1.wav"), &now),
            dir.join("Vox 4.wav")
        );
        let a = Job {
            path: dir.join("Vox 1.wav"),
            ..job(&dir, 1, 1)
        };
        let b = Job {
            track: 2,
            ..a.clone()
        };
        let takes = Takes::new(plan(vec![a, b], 0, None));
        assert_eq!(takes.lanes[0].path, dir.join("Vox 3.wav"));
        assert_eq!(takes.lanes[1].path, dir.join("Vox 4.wav"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sidecar_is_replaced_whole_and_a_temporary_one_is_not_a_take() {
        let dir = folder("whole");
        let file = dir.join("Vox 1.wav");
        fs::write(&file, b"x").unwrap();
        let side = sidecar_path(&file);
        write_whole(
            &side,
            br#"{"session":"s","track":1,"start":0,"rate":48000,"channels":1}"#,
        )
        .unwrap();
        let mut temporary = side.as_os_str().to_owned();
        temporary.push(".tmp");
        fs::write(PathBuf::from(temporary), br#"{"session":"s","tr"#).unwrap();
        let found = unfinished(&dir, "s");
        assert_eq!(found.takes.len(), 1);
        assert_eq!(found.takes[0].file, file);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_repairs_takes_and_removes_the_empty_ones() {
        let dir = folder("recover");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(plan(vec![j.clone()], 0, None));
        let (chunk, data) = mono(1, 1_000, 0, 480);
        takes.feed(chunk, &data);
        takes.finish();
        let empty = dir.join("empty.wav");
        lumen_media::take::TakeWriter::create(&empty, 48_000, 1)
            .unwrap()
            .finish()
            .unwrap();
        write_whole(
            &sidecar_path(&empty),
            br#"{"session":"","track":2,"start":0,"rate":48000,"channels":1}"#,
        )
        .unwrap();
        let found = recover(&dir, "");
        assert_eq!(found.unreadable, 0);
        assert_eq!(found.takes.len(), 1);
        assert_eq!(found.takes[0].track, 1);
        assert_eq!(found.takes[0].frames, 480);
        assert!(sidecar_path(&j.path).exists(), "kept until a save holds it");
        assert!(!empty.exists() && !sidecar_path(&empty).exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
