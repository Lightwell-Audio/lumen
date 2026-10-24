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

use crate::clips::{Clip, clip_player};
use crate::mixer::{BUS_CHANNELS, Buses, CHUNK, TrackSource};
use crate::source::{Mode, RenderContext};
use crate::timeline::Timeline;

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
    let block = block.clamp(1, CHUNK) as u64;
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
