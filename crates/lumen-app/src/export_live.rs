//! Device-callback driven export. The prepared, immutable render source is
//! installed by a worker; the audio callback renders it alongside normal
//! playback and substitutes its stereo output for the device's main pair.
//! File access, queue draining and WAV encoding stay on the worker.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use lumen_devices::hosted::QUIET_LEVEL;
use lumen_devices::hosted::Status;
use lumen_engine::{BUS_CHANNELS, Jump, MOST_BLOCK, Mode, Output, RenderContext, Source, Timeline};
use lumen_media::wav::{Sample, Spec, Writer};

const AUDIO_QUEUE_FRAMES: usize = 1 << 15;
const DEVICE_STALL_LIMIT: Duration = Duration::from_secs(5);
const LEAD_IN_MS: u64 = 100;
const ERROR_NONE: u8 = 0;
const ERROR_QUEUE: u8 = 1;
const ERROR_RATE: u8 = 2;
const ERROR_HOST: u8 = 3;
const ERROR_DISCONTINUITY: u8 = 4;

/// A callback to return the in-flight render source to the worker.
#[derive(Clone)]
pub struct Handle {
    worker: Arc<Mutex<Worker>>,
    error: Arc<AtomicU8>,
    heartbeat: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
}

struct Worker {
    pending: rtrb::Producer<Session>,
    retired: rtrb::Consumer<Session>,
    audio: rtrb::Consumer<[f32; 2]>,
}

/// The device callback side. Keep this in the callback closure and call
/// [`Tap::process`] after the normal engine callback.
pub struct Tap {
    pending: rtrb::Consumer<Session>,
    retired: rtrb::Producer<Session>,
    audio: rtrb::Producer<[f32; 2]>,
    active: Option<Session>,
    error: Arc<AtomicU8>,
    heartbeat: Arc<AtomicU64>,
}

/// One completed device-driven export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub file: PathBuf,
    pub frames: u64,
}

/// Everything needed to render a stable export snapshot. Build this on a
/// worker (typically from `Prepared`) before calling [`Handle::capture`].
pub struct Request {
    pub source: Box<dyn Source>,
    pub timeline: Timeline,
    pub rate: u32,
    pub from: i64,
    pub frames: u64,
    pub most_tail: u64,
    pub file: PathBuf,
    /// Plugin statuses belonging to the prepared source. A failed host ends
    /// capture with an error rather than silently exporting bypassed audio.
    pub statuses: Vec<(String, Arc<Status>)>,
    /// Keeps plugin processes and their state alive if a callback is delayed.
    /// It crosses back to the worker with the source before being dropped.
    pub keepalive: Box<dyn Send>,
}

struct Session {
    source: Box<dyn Source>,
    timeline: Timeline,
    rate: u32,
    cursor: i64,
    skip: u64,
    span: u64,
    most_tail: u64,
    rendered: u64,
    captured: u64,
    tail_frames: u64,
    tail_started: bool,
    complete: bool,
    cancelled: Arc<AtomicBool>,
    statuses: Vec<(String, Arc<Status>)>,
    status_late: Vec<u32>,
    _keepalive: Box<dyn Send>,
    buffer: Box<[f32]>,
    expected_position: Option<u64>,
}

/// Creates the lock-free worker/callback bridge. The returned callback side
/// contains no render object until a worker installs one.
pub fn channel() -> (Handle, Tap) {
    let (pending_tx, pending_rx) = rtrb::RingBuffer::new(1);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(1);
    let (audio_tx, audio_rx) = rtrb::RingBuffer::new(AUDIO_QUEUE_FRAMES);
    let error = Arc::new(AtomicU8::new(ERROR_NONE));
    let heartbeat = Arc::new(AtomicU64::new(0));
    let active = Arc::new(AtomicBool::new(false));
    (
        Handle {
            worker: Arc::new(Mutex::new(Worker {
                pending: pending_tx,
                retired: retired_rx,
                audio: audio_rx,
            })),
            error: Arc::clone(&error),
            heartbeat: Arc::clone(&heartbeat),
            active: Arc::clone(&active),
        },
        Tap {
            pending: pending_rx,
            retired: retired_tx,
            audio: audio_tx,
            active: None,
            error,
            heartbeat,
        },
    )
}

impl Handle {
    /// Runs a callback-driven export on the calling worker thread. It waits
    /// for actual device callbacks, drains their preallocated stereo queue,
    /// trims quiet tail frames and atomically writes a 32-bit float WAV.
    /// Returns `Ok(None)` when cancelled. A device rate change, failed host,
    /// queue overflow, or five seconds without callbacks returns an error.
    pub fn capture(
        &self,
        request: Request,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32),
    ) -> Result<Option<Rendered>, String> {
        if request.rate == 0 {
            return Err("Export sample rate must be greater than zero".into());
        }
        if request.frames == 0 {
            return Err("Export range is empty".into());
        }
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Worker {
            pending,
            retired,
            audio,
        } = &mut *worker;
        // Reclaim an abandoned session after the device resumes. The source
        // and its plugin manager are freed here, never in process().
        while let Ok(session) = retired.pop() {
            drop(session);
            self.active.store(false, Ordering::Release);
        }
        if self.active.load(Ordering::Acquire) {
            return Err("A real-time export is already active".into());
        }
        while audio.pop().is_ok() {}
        self.error.store(ERROR_NONE, Ordering::Release);
        let writer = Writer::create(
            &request.file,
            Spec {
                sample_rate: request.rate,
                channels: 2,
                sample: Sample::F32,
            },
        )
        .map_err(|e| format!("Could not create {}: {e}", request.file.display()))?;
        let lead = u64::from(request.rate).saturating_mul(LEAD_IN_MS) / 1000;
        let begin = request.from.saturating_sub(lead as i64);
        let cancelled = Arc::new(AtomicBool::new(false));
        let status_late = request
            .statuses
            .iter()
            .map(|(_, status)| status.late_blocks())
            .collect();
        let session = Session {
            source: request.source,
            timeline: request.timeline,
            rate: request.rate,
            cursor: begin,
            skip: lead,
            span: request.frames,
            most_tail: request.most_tail,
            rendered: 0,
            captured: 0,
            tail_frames: 0,
            tail_started: false,
            complete: false,
            cancelled: Arc::clone(&cancelled),
            statuses: request.statuses,
            status_late,
            _keepalive: request.keepalive,
            buffer: vec![0.0; BUS_CHANNELS * MOST_BLOCK].into_boxed_slice(),
            expected_position: None,
        };
        self.active.store(true, Ordering::Release);
        pending.push(session).map_err(|_| {
            self.active.store(false, Ordering::Release);
            "The audio callback is not ready for an export".to_owned()
        })?;

        let mut writer = writer;
        let mut tail = Vec::new();
        let mut written = 0_u64;
        let mut observed_callbacks = self.heartbeat.load(Ordering::Acquire);
        let mut last_callback = Instant::now();
        let mut finished = None;
        let mut failure = None;
        let mut chunk = [[0.0_f32; 2]; 2048];
        loop {
            let callbacks = self.heartbeat.load(Ordering::Acquire);
            if callbacks != observed_callbacks {
                observed_callbacks = callbacks;
                last_callback = Instant::now();
            }
            if cancel.load(Ordering::Relaxed) {
                cancelled.store(true, Ordering::Release);
                // A lost device may never call back to retire the source.
                // Its keepalive travels with it, so returning is safe.
                return Ok(None);
            }
            let status_error = self.error.swap(ERROR_NONE, Ordering::AcqRel);
            if status_error != ERROR_NONE {
                cancelled.store(true, Ordering::Release);
                failure = Some(match status_error {
                    ERROR_QUEUE => "Real-time export audio queue overflowed",
                    ERROR_RATE => "Audio device sample rate changed during export",
                    ERROR_HOST => "A plugin failed during real-time export",
                    ERROR_DISCONTINUITY => "Audio device skipped frames during export",
                    _ => "Real-time export failed",
                });
            }
            let available = audio.slots().min(chunk.len());
            if available > 0 {
                let mut read = 0;
                while read < available {
                    match audio.pop() {
                        Ok(sample) => {
                            chunk[read] = sample;
                            read += 1;
                        }
                        Err(_) => break,
                    }
                }
                let span_left = request.frames.saturating_sub(written);
                let main = read.min(span_left as usize);
                if main > 0 {
                    let frames = main;
                    let mut left = [0.0_f32; 2048];
                    let mut right = [0.0_f32; 2048];
                    for i in 0..frames {
                        left[i] = chunk[i][0];
                        right[i] = chunk[i][1];
                    }
                    if let Err(error) = writer.write(&[&left[..frames], &right[..frames]]) {
                        cancelled.store(true, Ordering::Release);
                        return Err(format!(
                            "Could not write {}: {error}",
                            request.file.display()
                        ));
                    }
                    written += frames as u64;
                    progress((written.min(request.frames)) as f32 / request.frames as f32);
                }
                if main < read {
                    tail.extend_from_slice(&chunk[main..read]);
                }
            }
            if let Ok(retired) = retired.pop() {
                finished = Some(retired.complete);
                // Drop the source and all plugin-owned render state here,
                // after ownership has crossed back to the worker.
                drop(retired);
                self.active.store(false, Ordering::Release);
            }
            if finished.is_some() && audio.is_empty() {
                // A callback may have published its error after this loop's
                // first read, immediately before retiring the source.
                let final_error = self.error.swap(ERROR_NONE, Ordering::AcqRel);
                if final_error != ERROR_NONE {
                    failure = Some(match final_error {
                        ERROR_QUEUE => "Real-time export audio queue overflowed",
                        ERROR_RATE => "Audio device sample rate changed during export",
                        ERROR_HOST => "A plugin failed during real-time export",
                        ERROR_DISCONTINUITY => "Audio device skipped frames during export",
                        _ => "Real-time export failed",
                    });
                }
                if let Some(reason) = failure {
                    return Err(reason.into());
                }
                if cancelled.load(Ordering::Acquire) || cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                if written != request.frames {
                    return Err(format!(
                        "Real-time export captured {written} of {} frames",
                        request.frames
                    ));
                }
                let tail_frames = tail.len();
                let kept = (0..tail_frames)
                    .rev()
                    .find(|&i| tail[i][0].abs() > QUIET_LEVEL || tail[i][1].abs() > QUIET_LEVEL)
                    .map_or(0, |i| i + 1);
                if kept > 0 {
                    let mut left = [0.0_f32; 2048];
                    let mut right = [0.0_f32; 2048];
                    for start in (0..kept).step_by(left.len()) {
                        let n = (kept - start).min(left.len());
                        for i in 0..n {
                            left[i] = tail[start + i][0];
                            right[i] = tail[start + i][1];
                        }
                        writer.write(&[&left[..n], &right[..n]]).map_err(|e| {
                            format!("Could not write {}: {e}", request.file.display())
                        })?;
                    }
                }
                let frames = writer
                    .finish()
                    .map_err(|e| format!("Could not finish {}: {e}", request.file.display()))?;
                progress(1.0);
                return Ok(Some(Rendered {
                    file: request.file,
                    frames,
                }));
            }
            if last_callback.elapsed() >= DEVICE_STALL_LIMIT {
                cancelled.store(true, Ordering::Release);
                return Err("Audio device stopped calling back during export".into());
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Tap {
    /// Renders export audio for this actual device block after the normal
    /// engine pass. Only output channels 0 and 1 are replaced; other outputs
    /// remain the engine's usual output. This function is real-time safe.
    pub fn process(&mut self, block: &mut lumen_audio::Block<'_>) {
        self.heartbeat.fetch_add(1, Ordering::Relaxed);
        if self.active.is_none() {
            self.active = self.pending.pop().ok();
        }
        let Some(session) = self.active.as_mut() else {
            return;
        };
        let stereo = block.output_channels.min(2) * block.frames;
        block.outputs[..stereo].fill(0.0);
        if let (Some(expected), Some(actual)) = (session.expected_position, block.position)
            && expected != actual
        {
            self.error.store(ERROR_DISCONTINUITY, Ordering::Release);
            session.complete = true;
        }
        session.expected_position = block
            .position
            .map(|p| p.saturating_add(block.frames as u64));
        if block.sample_rate != session.rate {
            self.error.store(ERROR_RATE, Ordering::Release);
            session.complete = true;
        }
        if session.statuses.iter().any(|(_, status)| status.failed()) {
            self.error.store(ERROR_HOST, Ordering::Release);
            session.complete = true;
        }
        if session
            .statuses
            .iter()
            .zip(&session.status_late)
            .any(|((_, status), previous)| status.late_blocks() > *previous)
        {
            self.error.store(ERROR_HOST, Ordering::Release);
            session.complete = true;
        }
        if session.cancelled.load(Ordering::Acquire) {
            session.complete = true;
        }
        if !session.complete {
            if session.rendered == 0 {
                session
                    .source
                    .locate(&session.timeline, session.rate, session.cursor, Jump::Play);
                session.skip = session
                    .skip
                    .saturating_add(u64::from(session.source.latency()));
            }
            let frames = block.frames;
            let deadline =
                Instant::now() + Duration::from_secs_f64(frames as f64 / session.rate as f64);
            let mut done = 0;
            while done < frames && !session.complete {
                let span_end = session.skip.saturating_add(session.span);
                if !session.tail_started && session.rendered >= span_end {
                    session.source.stopped();
                    session.tail_started = true;
                    if session.most_tail == 0 || !session.source.ringing() {
                        session.complete = true;
                        break;
                    }
                }
                let mut n = (frames - done).min(MOST_BLOCK);
                if session.rendered < session.skip {
                    n = n.min((session.skip - session.rendered) as usize);
                } else if session.rendered < span_end {
                    n = n.min((span_end - session.rendered) as usize);
                } else if session.tail_started {
                    n = n.min((session.most_tail - session.tail_frames) as usize);
                }
                if n == 0 {
                    session.complete = true;
                    break;
                }
                let start = session.cursor;
                let mode = if session.tail_started {
                    Mode::Ring
                } else {
                    Mode::Play
                };
                let cx = RenderContext {
                    timeline: &session.timeline,
                    sample_rate: session.rate,
                    start,
                    tail: false,
                    stopping: false,
                    mode,
                    stream: 0,
                    device: 0,
                    recording: false,
                    looping: None,
                    deadline: session.source.hosted().then_some(deadline),
                };
                let samples = &mut session.buffer[..BUS_CHANNELS * n];
                samples.fill(0.0);
                if let Some(mut output) = Output::planar(samples, n, BUS_CHANNELS) {
                    session.source.render(&cx, &mut output);
                }
                if (session.rendered >= session.skip || session.tail_started)
                    && self.audio.slots() < n
                {
                    self.error.store(ERROR_QUEUE, Ordering::Release);
                    session.complete = true;
                    break;
                }
                for i in 0..n {
                    let frame = session.rendered + i as u64;
                    let l = samples[i];
                    let r = samples[n + i];
                    let device_i = done + i;
                    if block.output_channels > 0 {
                        block.outputs[device_i] = l;
                    }
                    if block.output_channels > 1 {
                        block.outputs[block.frames + device_i] = r;
                    }
                    if frame >= session.skip && frame < span_end {
                        if self.audio.push([l, r]).is_err() {
                            self.error.store(ERROR_QUEUE, Ordering::Release);
                            session.complete = true;
                            break;
                        }
                        session.captured += 1;
                    } else if session.tail_started
                        && frame >= span_end
                        && session.tail_frames < session.most_tail
                    {
                        if self.audio.push([l, r]).is_err() {
                            self.error.store(ERROR_QUEUE, Ordering::Release);
                            session.complete = true;
                            break;
                        }
                        session.tail_frames += 1;
                    }
                }
                session.rendered += n as u64;
                session.cursor += n as i64;
                done += n;
                if session.tail_started
                    && (session.tail_frames >= session.most_tail || !session.source.ringing())
                {
                    session.complete = true;
                }
            }
        }
        if session
            .statuses
            .iter()
            .zip(&session.status_late)
            .any(|((_, status), before)| status.failed() || status.late_blocks() > *before)
        {
            self.error.store(ERROR_HOST, Ordering::Release);
            session.complete = true;
        }
        if session.complete
            && !self.retired.is_full()
            && let Some(session) = self.active.take()
        {
            // Ownership moves to the worker; no render object is dropped
            // or freed in the callback.
            if let Err(error) = self.retired.push(session) {
                match error {
                    rtrb::PushError::Full(session) => self.active = Some(session),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
