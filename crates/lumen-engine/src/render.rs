//! Rendering clips without a device (M7b slice E3).
//!
//! A stretch of the timeline is played through the same clip player the
//! callback runs, block after block, as fast as the machine allows, and handed
//! to a sink: consolidating clips into a new file, later bounce. Because it is
//! the live player, a render sounds like playback: gain, polarity, reverse,
//! fades, crossfades and clip envelopes are all applied as they are there.
//!
//! What is not applied is anything above the clips: a track's fader, pan,
//! sends and devices. Every clip is summed onto one bus, whatever track it is
//! on. Not real-time code: it allocates and takes as long as it takes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lumen_devices::hosted::QUIET_LEVEL;

use crate::clips::{Clip, clip_player};
use crate::mixer::{BUS_CHANNELS, Buses, CHUNK, TrackSource};
use crate::source::{Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

/// The most frames a render handles at a time.
pub const MOST_BLOCK: usize = CHUNK;

/// The bus every clip is summed on.
const BUS_ID: u64 = 0;

/// A stretch of clips to render.
pub struct ClipRender {
    pub clips: Vec<Clip>,
    pub timeline: Timeline,
    pub sample_rate: u32,
    /// Timeline sample of the first frame rendered.
    pub from: i64,
    /// Frames to render.
    pub frames: u64,
    /// The first clip's start and the last clip's end of each track play
    /// without the clip player's 2 ms edge fade (see
    /// [`ClipsHandle::bare_ends`](crate::ClipsHandle::bare_ends)). The edges
    /// between clips keep theirs.
    pub bare_ends: bool,
    /// Frames rendered at a time, from 1 to 256 (anything else is clamped).
    pub block: usize,
}

/// How a render ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    Done,
    /// `cancel` was set; the sink holds the frames given so far.
    Cancelled,
}

/// Renders `job`, giving the sink each block as a left and a right slice of
/// the same length (a mono clip is on both sides). `progress` receives 0.0
/// to 1.0 after each block; `cancel` is looked at before each one.
pub fn render_clips(
    job: ClipRender,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
    sink: &mut dyn FnMut(&[f32], &[f32]),
) -> Finished {
    let ClipRender {
        mut clips,
        timeline,
        sample_rate,
        from,
        frames,
        bare_ends,
        block,
    } = job;
    for clip in &mut clips {
        clip.track = BUS_ID;
    }
    let (mut handle, mut player) = clip_player();
    handle.bare_ends(bare_ends);
    if handle.set(clips).is_err() {
        unreachable!("a new clip queue has room for one list");
    }
    let by_id = [(BUS_ID, 0)];
    let mut data = vec![0.0; CHUNK * BUS_CHANNELS];
    let block = block.clamp(1, MOST_BLOCK) as u64;
    let mut done = 0;
    while done < frames {
        if cancel.load(Ordering::Relaxed) {
            return Finished::Cancelled;
        }
        let n = block.min(frames - done) as usize;
        data.fill(0.0);
        let cx = RenderContext {
            timeline: &timeline,
            sample_rate,
            start: from + done as i64,
            tail: false,
            stopping: false,
            mode: Mode::Play,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: None,
        };
        player.render(&cx, &mut Buses::over(&by_id, &mut data, n));
        sink(&data[..n], &data[CHUNK..CHUNK + n]);
        done += n as u64;
        progress(done as f32 / frames as f32);
    }
    Finished::Done
}

/// A [`Source`] (the mixer, with its tracks, devices and plugin hosts) to
/// render without a device (M7b slice E4): bounce, later freeze.
pub struct SourceRender<'a> {
    pub timeline: &'a Timeline,
    pub sample_rate: u32,
    /// Timeline sample of the first frame handed to the sink.
    pub from: i64,
    /// Frames of the span itself.
    pub frames: u64,
    /// The most frames of tail to add after the span, while the source still
    /// rings (0 for none). Trailing frames below [`QUIET_LEVEL`] are left off.
    pub most_tail: u64,
    /// Frames rendered at a time, from 1 to 256 (anything else is clamped).
    pub block: usize,
    /// How long a plugin host is waited for, per block (see
    /// [`OFFLINE_WAIT`](crate::OFFLINE_WAIT)).
    pub wait: Duration,
}

/// How a [`render_source`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Done,
    /// `cancel` was set.
    Cancelled,
    /// `failed` answered yes: a plugin host did not answer a block, so what
    /// follows would not be what playback plays.
    Failed,
}

/// The pre-roll: the source plays this long before `from` and the sink never
/// sees it, so the fades a source makes when playback starts (a track coming
/// in from silence, a clip first heard partway) are over before the span.
fn lead_in(sample_rate: u32) -> u64 {
    u64::from(sample_rate / 10)
}

/// Plays `source` from `job.from` as fast as it goes, giving the sink the
/// audio as the left and right slices of each block. The source's own delay
/// compensation is followed: the frames its latency adds at the start are not
/// given. Nothing of the engine's is applied (play fade-in, metronome).
///
/// `failed` is asked after each block; `cancel` before it. `progress`
/// receives 0.0 to 1.0 over the span. The sink gets the span, then the tail.
pub fn render_source<S: Source + ?Sized>(
    source: &mut S,
    job: SourceRender<'_>,
    cancel: &AtomicBool,
    failed: &mut dyn FnMut() -> bool,
    progress: &mut dyn FnMut(f32),
    sink: &mut dyn FnMut(&[f32], &[f32]),
) -> Ended {
    let SourceRender {
        timeline,
        sample_rate,
        from,
        frames,
        most_tail,
        block,
        wait,
    } = job;
    let lead = lead_in(sample_rate);
    let begin = from - lead as i64;
    source.locate(timeline, sample_rate, begin, Jump::Play);
    // Read after the locate, which takes up a waiting routing.
    let skip = lead + u64::from(source.latency());
    let total = skip + frames;
    let block = block.clamp(1, MOST_BLOCK) as u64;
    let mut data = vec![0.0; BUS_CHANNELS * MOST_BLOCK];
    let mut cursor = begin;
    let mut done = 0;
    let mut ended = Ended::Done;
    let context = |start: i64, mode: Mode, source: &S| RenderContext {
        timeline,
        sample_rate,
        start,
        tail: false,
        stopping: false,
        mode,
        stream: 0,
        device: 0,
        recording: false,
        looping: None,
        deadline: source.hosted().then(|| Instant::now() + wait),
    };
    while done < total {
        if cancel.load(Ordering::Relaxed) {
            ended = Ended::Cancelled;
            break;
        }
        let n = block.min(total - done) as usize;
        let part = &mut data[..BUS_CHANNELS * n];
        part.fill(0.0);
        let cx = context(cursor, Mode::Play, source);
        source.render(&cx, &mut Output::new(part, n, 0, n, BUS_CHANNELS));
        // Only the frames past the lead-in and the latency are the span.
        let first = skip.saturating_sub(done).min(n as u64) as usize;
        if first < n {
            sink(&part[first..n], &part[n + first..2 * n]);
        }
        cursor += n as i64;
        done += n as u64;
        progress(done.saturating_sub(skip) as f32 / frames.max(1) as f32);
        if failed() {
            ended = Ended::Failed;
            break;
        }
    }
    source.stopped();
    if ended != Ended::Done || most_tail == 0 {
        return ended;
    }
    // The tail: what rings on after the span, until it is quiet. Held back, so
    // the quiet end can be left off.
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while source.ringing() && (left.len() as u64) < most_tail {
        if cancel.load(Ordering::Relaxed) {
            return Ended::Cancelled;
        }
        let n = block.min(most_tail - left.len() as u64) as usize;
        let part = &mut data[..BUS_CHANNELS * n];
        part.fill(0.0);
        let cx = context(cursor, Mode::Ring, source);
        source.render(&cx, &mut Output::new(part, n, 0, n, BUS_CHANNELS));
        left.extend_from_slice(&part[..n]);
        right.extend_from_slice(&part[n..2 * n]);
        cursor += n as i64;
        if failed() {
            return Ended::Failed;
        }
    }
    let kept = (0..left.len())
        .rev()
        .find(|&i| left[i].abs() > QUIET_LEVEL || right[i].abs() > QUIET_LEVEL)
        .map_or(0, |i| i + 1);
    for (l, r) in left[..kept]
        .chunks(MOST_BLOCK)
        .zip(right[..kept].chunks(MOST_BLOCK))
    {
        sink(l, r);
    }
    Ended::Done
}
