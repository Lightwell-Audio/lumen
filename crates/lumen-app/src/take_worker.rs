//! Writing recorded audio to disk (M9 slice R2, ADR-0102): the worker thread
//! that empties the engine's capture ring into one file per recording track.
//!
//! The audio thread only copies input into the ring (`lumen_engine::capture`);
//! everything slow happens here: creating the files, writing, refreshing their
//! headers once a second so a crash loses at most that second, and keeping the
//! small summary the arrangement draws while a take grows. [`Takes`] is the
//! logic, a plain struct fed chunks and tested without threads; [`TakeWorker`]
//! runs it on a thread.
//!
//! # Where a frame lands
//!
//! A chunk says which timeline sample was *heard* at its first frame. The
//! player's own sound took the output latency to reach the ear and the played
//! sound takes the input latency to come back, so the frame belongs
//! [`Plan::shift`] samples earlier: its placed position is `at - shift + i`.
//!
//! - A file begins at the first frame whose placed position is inside the
//!   window (the punch range, or the whole timeline from sample 0) and is then
//!   written without a break, whatever the timeline does: a loop wrap carries
//!   on in the same file. With Loop on, the file frame and placed position of
//!   each wrap are kept ([`Wrap`]), so the file can be cut into passes
//!   (`loop_passes.rs`, M9 slice R3).
//! - A gap (a chunk the ring had to drop, seen as a jump in `device`) is
//!   filled with silence so the rest stays in time.
//! - The first frame past the punch-out ends that track's take.
//! - A write that fails ends that track's take with a message, keeping what
//!   was written.
//!
//! Each file has a sidecar (`<file>.take`) from its first frame until it is
//! finished. A file found with its sidecar is a take a crash interrupted.

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

/// How often the worker looks at the ring.
const POLL: Duration = Duration::from_millis(5);
/// How often a file's header is brought up to date.
const REFRESH: Duration = Duration::from_secs(1);
/// How far back the timeline must jump, in samples, to be a loop wrap rather
/// than rounding.
const WRAP_SLACK: i64 = 64;
/// The extension of a take's sidecar, after `.wav`.
const SIDECAR: &str = "take";

/// One track's file.
#[derive(Debug, Clone)]
pub(crate) struct Job {
    /// The track, by its id in the project.
    pub track: u64,
    pub path: PathBuf,
    /// One for a mono input, two for a pair.
    pub channels: u16,
    /// What the arrangement draws while the take grows.
    pub live: Arc<Mutex<Live>>,
    /// The autosave session that recorded it, so recovery gives the take back
    /// to that session's project and no other.
    pub session: String,
}

/// What to record.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub jobs: Vec<Job>,
    pub rate: u32,
    /// Samples a take moves earlier: input plus output latency plus the
    /// manual offset (negative moves it later).
    pub shift: i64,
    /// Placed samples `[in, out)` that are kept (the punch range), or all.
    pub window: Option<(i64, i64)>,
    /// Loop is on: a jump back in the timeline is a wrap, a new pass.
    pub looped: bool,
}

/// Where a loop pass begins in a take's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Wrap {
    /// The file frame of the pass's first frame.
    pub frame: u64,
    /// The placed timeline sample of that frame.
    pub at: i64,
}

/// The summary of a take as it grows.
#[derive(Debug, Default)]
pub(crate) struct Live {
    /// The placed sample of the file's first frame; `None` before it begins.
    pub start: Option<i64>,
    /// Frames of the pass under way: all of them without Loop.
    pub frames: u64,
    /// Frames written in all, across the loop's passes.
    pub written: u64,
    /// The waveform of what has been written, in the finished waveform's
    /// own buckets (`lumen_media::GrowingPeaks`).
    pub peaks: lumen_media::GrowingPeaks,
}

impl Live {
    /// Starts the summary again at placed sample `start`: a new loop pass.
    fn restart(&mut self, start: i64) {
        *self = Live {
            start: Some(start),
            written: self.written,
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

/// What a track's take came to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackResult {
    pub track: u64,
    pub path: PathBuf,
    pub rate: u32,
    pub channels: u16,
    /// The placed sample of the file's first frame (`None`: nothing was kept).
    pub start: Option<i64>,
    /// Frames in the file.
    pub frames: u64,
    /// Frames the ring dropped, filled with silence.
    pub dropped: u64,
    /// Why the take ended early, if it did.
    pub error: Option<String>,
    /// Where each later loop pass begins in the file.
    pub wraps: Vec<Wrap>,
}

/// The file beside a take that says it is unfinished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Sidecar {
    #[serde(default)]
    pub session: String,
    pub track: u64,
    pub start: i64,
    pub rate: u32,
    pub channels: u16,
    /// The loop wraps so far, as of the last refresh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wraps: Vec<Wrap>,
}

pub(crate) fn sidecar_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".");
    name.push(SIDECAR);
    PathBuf::from(name)
}

/// A take a session left unfinished: its file and what its sidecar says.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Unfinished {
    pub file: PathBuf,
    pub sidecar: Sidecar,
}

/// The unfinished takes in `dir` that `session` was recording, by file name.
/// A sidecar that cannot be read is left alone.
pub(crate) fn unfinished(dir: &Path, session: &str) -> Vec<Unfinished> {
    let suffix = format!(".wav.{SIDECAR}");
    let mut found: Vec<Unfinished> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let stem = name.strip_suffix(&suffix)?;
            let file = path.with_file_name(format!("{stem}.wav"));
            let sidecar: Sidecar = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            (sidecar.session == session && file.is_file()).then_some(Unfinished { file, sidecar })
        })
        .collect();
    found.sort_by(|a, b| a.file.cmp(&b.file));
    found
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    /// No frame inside the window yet.
    Waiting,
    Writing,
    /// Past the punch-out, or failed: nothing more is written.
    Done,
}

struct Lane {
    job: Job,
    state: State,
    writer: Option<TakeWriter>,
    /// The device frame the next chunk should begin on.
    next: Option<u64>,
    start: Option<i64>,
    dropped: u64,
    error: Option<String>,
    refreshed: Instant,
    rate: u32,
    /// Frames written to the file, silence for gaps included.
    written: u64,
    /// The placed sample the next frame should have if the timeline ran on.
    expected: Option<i64>,
    wraps: Vec<Wrap>,
    /// The sidecar holds `wraps` as they are now.
    sidecar_current: bool,
}

/// The takes of one recording.
pub(crate) struct Takes {
    plan: Plan,
    lanes: Vec<Lane>,
}

impl Takes {
    pub(crate) fn new(plan: Plan) -> Self {
        let lanes = plan
            .jobs
            .iter()
            .map(|job| Lane {
                job: job.clone(),
                state: State::Waiting,
                writer: None,
                next: None,
                start: None,
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

    /// Takes in a chunk: its samples are the left channel, then the right for
    /// a pair.
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
        // A gap the ring made: silence keeps the rest in time.
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
            // The timeline ran on while the chunks were lost.
            lane.expected = lane.expected.map(|e| e + gap as i64);
            if let Ok(mut live) = lane.job.live.lock() {
                live.add(&[&vec![0.0; gap as usize]]);
            }
        }
        lane.next = Some(chunk.device + frames as u64);

        let placed = chunk.at - shift;
        // A jump back in the timeline while Loop is on is a wrap: a new pass
        // begins at this chunk's first frame.
        if self.plan.looped
            && lane.state == State::Writing
            && lane.expected.is_some_and(|e| placed + WRAP_SLACK < e)
        {
            lane.wraps.push(Wrap {
                frame: lane.written,
                at: placed,
            });
            lane.sidecar_current = false;
            // What is drawn while recording is the pass under way, from the
            // loop's start, as it will be placed.
            if let Ok(mut live) = lane.job.live.lock() {
                live.restart(placed.max(0));
            }
        }
        let (lo, hi) = window.unwrap_or((0, i64::MAX));
        // Frames before the window are skipped only until the take begins.
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
        if lane.state == State::Waiting && lane.open(rate, placed + from as i64).is_err() {
            return;
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
            // The first frame past the punch-out ends the take.
            lane.finish_early();
        }
    }

    /// Brings the headers up to date once a second.
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

    /// Ends every file. Tracks that kept nothing leave no file.
    pub(crate) fn finish(self) -> Vec<TrackResult> {
        let rate = self.plan.rate;
        self.lanes
            .into_iter()
            .map(|mut lane| {
                let frames = lane.close();
                TrackResult {
                    track: lane.job.track,
                    path: lane.job.path.clone(),
                    rate,
                    channels: lane.job.channels,
                    start: lane.start.filter(|_| frames > 0),
                    frames,
                    dropped: lane.dropped,
                    error: lane.error.take(),
                    wraps: std::mem::take(&mut lane.wraps),
                }
            })
            .collect()
    }
}

impl Lane {
    /// Creates the file and its sidecar for a take beginning at `start`.
    fn open(&mut self, rate: u32, start: i64) -> Result<(), ()> {
        let made = (|| -> Result<TakeWriter, String> {
            if let Some(dir) = self.job.path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            self.start = Some(start);
            self.rate = rate;
            self.write_sidecar_now().map_err(|e| e.to_string())?;
            TakeWriter::create(&self.job.path, rate, self.job.channels).map_err(|e| e.to_string())
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

    /// The sidecar as the take stands.
    fn sidecar(&self) -> Option<Sidecar> {
        Some(Sidecar {
            session: self.job.session.clone(),
            track: self.job.track,
            start: self.start?,
            rate: self.rate,
            channels: self.job.channels,
            wraps: self.wraps.clone(),
        })
    }

    fn write_sidecar_now(&mut self) -> std::io::Result<()> {
        let Some(side) = self.sidecar() else {
            return Ok(());
        };
        let json = serde_json::to_vec(&side).map_err(std::io::Error::other)?;
        fs::write(sidecar_path(&self.job.path), json)?;
        self.sidecar_current = true;
        Ok(())
    }

    /// Brings the sidecar up to date if a wrap has been noted since it was
    /// written. A failure is not the take's: the file is what matters.
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

    /// Finishes the file; returns the frames it holds. The sidecar goes with
    /// the clean end.
    fn close(&mut self) -> u64 {
        let Some(writer) = self.writer.take() else {
            return 0;
        };
        match writer.finish() {
            Ok(frames) => {
                let _ = fs::remove_file(sidecar_path(&self.job.path));
                frames
            }
            Err(error) => {
                self.error.get_or_insert(error.to_string());
                lumen_media::take::repair(&self.job.path).map_or(0, |r| r.frames)
            }
        }
    }
}

enum Message {
    Begin(Plan),
    Finish,
}

/// What the app can see of the worker.
#[derive(Debug, Default)]
pub(crate) struct Gauge {
    /// Frames the ring dropped since the take began, all takes: input the
    /// worker was too slow to take.
    pub dropped: AtomicU64,
}

/// The worker thread. Dropping it stops the thread.
pub(crate) struct TakeWorker {
    tx: mpsc::Sender<Message>,
    done: mpsc::Receiver<Vec<TrackResult>>,
    gauge: Arc<Gauge>,
}

impl TakeWorker {
    /// Starts the thread, which owns the ring's reading end.
    pub(crate) fn spawn(reader: CaptureReader) -> Self {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let gauge = Arc::new(Gauge::default());
        let seen = Arc::clone(&gauge);
        let spawned = thread::Builder::new()
            .name("take-writer".to_owned())
            .spawn(move || run(reader, &rx, &done_tx, &seen));
        // A thread that cannot start leaves the channel closed: begin() and
        // finish() then do nothing and nothing is recorded.
        drop(spawned);
        Self { tx, done, gauge }
    }

    /// Starts writing. Anything the ring held from before is thrown away.
    pub(crate) fn begin(&self, plan: Plan) {
        let _ = self.tx.send(Message::Begin(plan));
    }

    /// Ends the takes once the ring is empty; [`Self::poll`] returns them.
    pub(crate) fn finish(&self) {
        let _ = self.tx.send(Message::Finish);
    }

    /// The finished takes, once, after [`Self::finish`].
    pub(crate) fn poll(&self) -> Option<Vec<TrackResult>> {
        self.done.try_recv().ok()
    }

    /// Frames of input the ring has dropped since the take began.
    pub(crate) fn dropped(&self) -> u64 {
        self.gauge.dropped.load(Ordering::Relaxed)
    }
}

fn run(
    mut reader: CaptureReader,
    rx: &mpsc::Receiver<Message>,
    done: &mpsc::Sender<Vec<TrackResult>>,
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
                // What is in the ring now is not this take's.
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
            let _ = done.send(takes.finish());
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
            session: session.to_owned(),
            track,
            start: 480,
            rate: 48_000,
            channels: 1,
            wraps: Vec::new(),
        };
        for (name, session, track) in [("a", "s1", 1), ("b", "s2", 2), ("c", "s1", 3)] {
            fs::write(dir.join(format!("{name}.wav")), b"x").unwrap();
            fs::write(
                sidecar_path(&dir.join(format!("{name}.wav"))),
                serde_json::to_vec(&side(session, track)).unwrap(),
            )
            .unwrap();
        }
        // A sidecar whose file is gone, and one that is not ours.
        fs::write(
            sidecar_path(&dir.join("gone.wav")),
            serde_json::to_vec(&side("s1", 9)).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("junk.wav.take"), b"not json").unwrap();
        let found = unfinished(&dir, "s1");
        let names: Vec<_> = found
            .iter()
            .map(|u| u.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.wav", "c.wav"]);
        assert_eq!(found[1].sidecar, side("s1", 3));
        assert!(unfinished(&dir, "nobody").is_empty());
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
            jobs,
            rate: 48_000,
            shift,
            window,
            looped: false,
        }
    }

    /// A chunk of `frames` mono frames whose sample is its device frame.
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
        assert!(!sidecar_path(&j.path).exists(), "finished: no sidecar");
        assert_eq!(results.len(), 1);
        // Heard at 500, shifted 100 earlier: placed at 400.
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
        // Heard from 0, shifted 100: the first 100 frames would land before 0.
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
        // Chunks of 100 frames from 0 to 600, placed where they are heard.
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
        // Heard 0..200, then the loop wraps back to 0 for 100 more frames.
        let (a, da) = mono(1, 0, 0, 200);
        let (b, db) = mono(1, 200, 0, 100);
        takes.feed(a, &da);
        takes.feed(b, &db);
        let r = takes.finish().remove(0);
        // The first 10 frames land before 0 and are cut; nothing else is.
        assert_eq!((r.start, r.frames), (Some(0), 290));
        let audio = read(&j.path);
        assert_eq!((audio[189], audio[190], audio[289]), (199.0, 200.0, 299.0));
        fs::remove_dir_all(dir).unwrap();
    }

    fn looping(jobs: Vec<Job>, shift: i64) -> Plan {
        Plan {
            looped: true,
            ..plan(jobs, shift, None)
        }
    }

    #[test]
    fn each_jump_back_with_loop_on_notes_where_the_new_pass_begins() {
        let dir = folder("wraps");
        let j = job(&dir, 1, 1);
        let mut takes = Takes::new(looping(vec![j.clone()], 10));
        // A loop from 1_000 to 1_200, recorded from its start: two passes
        // and a half, in blocks of 100.
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
        // 100..200 never arrived: the timeline ran on to 1_200.
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
        // 100..300 never arrived.
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
        // A track that is not recording is ignored.
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
        // Left from before the take: thrown away.
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
                break r;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((results[0].frames, results[0].start), (1_000, Some(0)));
        assert_eq!(read(&j.path)[999], 999.0);
        drop(worker);
        fs::remove_dir_all(dir).unwrap();
    }
}
