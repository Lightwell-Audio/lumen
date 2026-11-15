use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lumen_devices::hosted::QUIET_LEVEL;

use crate::clips::{Clip, clip_player_for_render};
use crate::mixer::{BUS_CHANNELS, Buses, CHUNK, TrackSource};
use crate::source::{Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

pub const MOST_BLOCK: usize = CHUNK;

const BUS_ID: u64 = 0;

pub struct ClipRender {
    pub clips: Vec<Clip>,
    pub timeline: Timeline,
    pub sample_rate: u32,
    pub from: i64,
    pub frames: u64,
    pub bare_ends: bool,
    pub block: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    Done,
    Cancelled,
}

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
    let (mut handle, mut player) = clip_player_for_render();
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

pub struct SourceRender<'a> {
    pub timeline: &'a Timeline,
    pub sample_rate: u32,
    pub from: i64,
    pub frames: u64,
    pub most_tail: u64,
    pub block: usize,
    pub wait: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Done,
    Cancelled,
    Failed,
}

fn lead_in(sample_rate: u32, block: u64) -> u64 {
    let block = block.max(1);
    let lead = u64::from(sample_rate / 10).max(1);
    block * lead.div_ceil(block)
}

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
    let block = block.clamp(1, MOST_BLOCK) as u64;
    let lead = lead_in(sample_rate, block);
    let begin = from - lead as i64;
    source.locate(timeline, sample_rate, begin, Jump::Play);
    let skip = lead + u64::from(source.latency());
    let total = skip + frames;
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use lumen_core::time::Ticks;

    use crate::clips::{ClipAudio, ClipShape, clip_player};
    use crate::source::Mode;
    use crate::stream::{OfflineLoader, PAGE, PagePool, StreamAudio};

    const RATE: u32 = 48_000;

    fn offline_stream(pool: &Arc<PagePool>, loads: &Arc<AtomicUsize>) -> StreamAudio {
        let counted = Arc::clone(loads);
        let loader = OfflineLoader::new(
            pool,
            Box::new(move |_, _, left, right| {
                counted.fetch_add(1, Ordering::Relaxed);
                left[..PAGE].fill(0.5);
                right[..PAGE].fill(-0.5);
                Some(PAGE)
            }),
        );
        let live = StreamAudio::new(&PagePool::new(1), 7, RATE, 2, PAGE as u64);
        loader.stream(&live)
    }

    fn clip(audio: ClipAudio) -> Clip {
        Clip {
            id: 1,
            track: BUS_ID,
            start: Ticks::ZERO,
            offset: 0,
            length: PAGE as i64,
            gain: 1.0,
            audio: Arc::new(audio),
            shape: ClipShape::default(),
            envelope: Default::default(),
        }
    }

    #[test]
    fn only_an_offline_render_loads_the_pages_it_reads() {
        let loads = Arc::new(AtomicUsize::new(0));
        let stream = offline_stream(&PagePool::new(4), &loads);
        let mut heard = Vec::new();
        let ended = render_clips(
            ClipRender {
                clips: vec![clip(ClipAudio::Stream(stream))],
                timeline: Timeline::default(),
                sample_rate: RATE,
                from: 0,
                frames: PAGE as u64,
                bare_ends: false,
                block: 256,
            },
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |left, right| {
                heard.extend(left.iter().zip(right).map(|(l, r)| (*l, *r)));
            },
        );
        assert_eq!(ended, Finished::Done);
        assert!(loads.load(Ordering::Relaxed) > 0, "the render loads");
        assert!(heard.iter().any(|(l, _)| *l != 0.0), "and hears the file");

        let loads = Arc::new(AtomicUsize::new(0));
        let stream = offline_stream(&PagePool::new(4), &loads);
        let (mut handle, mut player) = clip_player();
        assert!(
            handle.set(vec![clip(ClipAudio::Stream(stream))]).is_ok(),
            "room for one list"
        );
        let timeline = Timeline::default();
        let context = RenderContext {
            timeline: &timeline,
            sample_rate: RATE,
            start: 0,
            tail: false,
            stopping: false,
            mode: Mode::Play,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: None,
        };
        let mut data = vec![0.0; CHUNK * BUS_CHANNELS];
        let by_id = [(BUS_ID, 0)];
        player.render(&context, &mut Buses::over(&by_id, &mut data, 256));
        assert_eq!(
            loads.load(Ordering::Relaxed),
            0,
            "the audio thread's player loads nothing"
        );
        assert!(
            data.iter().all(|&x| x == 0.0),
            "and what it cannot hear is silent"
        );
    }
}
