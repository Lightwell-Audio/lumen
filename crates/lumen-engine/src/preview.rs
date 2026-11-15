use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

pub const BLOCK_FRAMES: usize = 2_048;
pub const RING_BLOCKS: usize = 32;
pub const RAMP_SECONDS: f32 = 0.003;
const SKIP_PER_CALL: usize = 8;

#[derive(Debug)]
pub struct PreviewBlock {
    generation: u32,
    frames: usize,
    stereo: bool,
    last: bool,
    samples: Box<[f32]>,
}

impl PreviewBlock {
    fn new() -> Self {
        Self {
            generation: 0,
            frames: 0,
            stereo: false,
            last: false,
            samples: vec![0.0; 2 * BLOCK_FRAMES].into_boxed_slice(),
        }
    }

    pub fn fill(
        &mut self,
        generation: u32,
        left: &[f32],
        right: Option<&[f32]>,
        last: bool,
    ) -> usize {
        let frames = left
            .len()
            .min(right.map_or(usize::MAX, <[f32]>::len))
            .min(BLOCK_FRAMES);
        let (l, r) = self.samples.split_at_mut(BLOCK_FRAMES);
        l[..frames].copy_from_slice(&left[..frames]);
        if let Some(right) = right {
            r[..frames].copy_from_slice(&right[..frames]);
        }
        self.generation = generation;
        self.frames = frames;
        self.stereo = right.is_some();
        self.last = last;
        frames
    }

    fn frame(&self, at: usize) -> (f32, f32) {
        let left = self.samples[at];
        let right = if self.stereo {
            self.samples[BLOCK_FRAMES + at]
        } else {
            left
        };
        (left, right)
    }
}

fn pack(generation: u32, state: u8) -> u64 {
    (u64::from(generation) << 32) | u64::from(state)
}

fn unpack(word: u64) -> (u32, u8) {
    ((word >> 32) as u32, (word & 0xff) as u8)
}

const SILENT: u8 = 0;
const SOUNDING: u8 = 1;
const ENDED: u8 = 2;

#[derive(Debug, Default)]
struct Shared {
    asked: AtomicU32,
    first_channel: AtomicU32,
    level: AtomicU32,
    rate: AtomicU32,
    heard: AtomicU64,
    heard_frames: AtomicU64,
    stale: AtomicU64,
    underruns: AtomicU64,
}

pub fn preview() -> (PreviewHandle, PreviewFeed, PreviewPlayer) {
    let shared = Arc::new(Shared {
        level: AtomicU32::new(1.0_f32.to_bits()),
        ..Shared::default()
    });
    let (full_in, full_out) = RingBuffer::new(RING_BLOCKS);
    let (mut empty_in, empty_out) = RingBuffer::new(RING_BLOCKS);
    for _ in 0..RING_BLOCKS {
        empty_in
            .push(PreviewBlock::new())
            .expect("the empty ring holds every block");
    }
    (
        PreviewHandle {
            shared: Arc::clone(&shared),
        },
        PreviewFeed {
            shared: Arc::clone(&shared),
            full: full_in,
            empty: empty_out,
        },
        PreviewPlayer {
            shared,
            full: full_out,
            empty: empty_in,
            current: None,
            offset: 0,
            playing: 0,
            next: None,
            dead: 0,
            gain: 0.0,
            level: 1.0,
            hold: [0.0; 2],
            hold_gain: 0.0,
            rate: 0,
            played: 0,
            ended: false,
            starved: false,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    pub generation: u32,
    pub frames: u64,
    pub sounding: bool,
    pub ended: bool,
}

#[derive(Debug, Clone)]
pub struct PreviewHandle {
    shared: Arc<Shared>,
}

impl PreviewHandle {
    pub fn start(&self) -> u32 {
        self.bump()
    }

    pub fn stop(&self) {
        self.bump();
    }

    fn bump(&self) -> u32 {
        let next = self
            .shared
            .asked
            .load(Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        self.shared.asked.store(next, Ordering::Release);
        next
    }

    pub fn asked(&self) -> u32 {
        self.shared.asked.load(Ordering::Acquire)
    }

    pub fn set_level(&self, gain: f32) {
        self.shared
            .level
            .store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_first_channel(&self, channel: usize) {
        self.shared
            .first_channel
            .store(u32::try_from(channel).unwrap_or(0), Ordering::Relaxed);
    }

    pub fn rate(&self) -> u32 {
        self.shared.rate.load(Ordering::Acquire)
    }

    pub fn heard(&self) -> Heard {
        let word = self.shared.heard.load(Ordering::Acquire);
        let (generation, state) = unpack(word);
        Heard {
            generation,
            frames: self.shared.heard_frames.load(Ordering::Acquire),
            sounding: state == SOUNDING,
            ended: state == ENDED,
        }
    }

    pub fn counts(&self) -> (u64, u64) {
        (
            self.shared.stale.load(Ordering::Relaxed),
            self.shared.underruns.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug)]
pub struct PreviewFeed {
    shared: Arc<Shared>,
    full: Producer<PreviewBlock>,
    empty: Consumer<PreviewBlock>,
}

impl PreviewFeed {
    pub fn asked(&self) -> u32 {
        self.shared.asked.load(Ordering::Acquire)
    }

    pub fn rate(&self) -> u32 {
        self.shared.rate.load(Ordering::Acquire)
    }

    pub fn take(&mut self) -> Option<PreviewBlock> {
        self.empty.pop().ok()
    }

    pub fn send(&mut self, block: PreviewBlock) {
        let sent = self.full.push(block);
        debug_assert!(
            sent.is_ok(),
            "a sent block needs the room the empty ring guarantees"
        );
    }
}

#[derive(Debug)]
pub struct PreviewPlayer {
    shared: Arc<Shared>,
    full: Consumer<PreviewBlock>,
    empty: Producer<PreviewBlock>,
    current: Option<PreviewBlock>,
    offset: usize,
    playing: u32,
    next: Option<u32>,
    dead: u32,
    gain: f32,
    level: f32,
    hold: [f32; 2],
    hold_gain: f32,
    rate: u32,
    played: u64,
    ended: bool,
    starved: bool,
}

impl PreviewPlayer {
    pub fn process(
        &mut self,
        rate: u32,
        outputs: &mut [f32],
        output_channels: usize,
        frames: usize,
    ) {
        if rate == 0 || output_channels == 0 || outputs.len() < output_channels * frames {
            return;
        }
        let asked = self.shared.asked.load(Ordering::Acquire);
        if rate != self.rate {
            self.rate = rate;
            self.shared.rate.store(rate, Ordering::Release);
            self.dead = asked;
            self.recycle_current();
            self.switch_to(asked);
        }
        if asked != self.playing {
            if self.gain > 0.0 && self.current.is_some() {
                self.next = Some(asked);
            } else if self.next.is_none() {
                self.switch_to(asked);
            }
        }
        if let Some(next) = &mut self.next {
            *next = asked;
        }
        let first = self.shared.first_channel.load(Ordering::Relaxed) as usize;
        let (left_at, right_at) = if first + 1 < output_channels {
            (first, first + 1)
        } else if output_channels >= 2 {
            (0, 1)
        } else {
            (0, 0)
        };
        let target = f32::from_bits(self.shared.level.load(Ordering::Relaxed));
        let step = 1.0 / (RAMP_SECONDS * rate as f32);
        let mut skipped = 0;
        for i in 0..frames {
            self.level += (target - self.level).clamp(-step, step);
            if let Some(next) = self.next {
                self.gain -= step;
                if self.gain <= 0.0 {
                    self.gain = 0.0;
                    self.recycle_current();
                    self.switch_to(next);
                }
            }
            if self.current.is_none() && self.wanted() {
                self.fetch(&mut skipped);
            }
            let (left, right) = if let Some(block) = &self.current {
                let (left, right) = block.frame(self.offset);
                if self.next.is_none() {
                    self.gain = (self.gain + step).min(1.0);
                }
                self.offset += 1;
                self.played += 1;
                self.starved = false;
                self.hold = [left * self.gain, right * self.gain];
                self.hold_gain = 1.0;
                if self.offset >= block.frames {
                    if block.last {
                        self.ended = true;
                    }
                    self.recycle_current();
                }
                (left * self.gain, right * self.gain)
            } else {
                if self.wanted() && self.played > 0 && !self.starved {
                    self.starved = true;
                    self.shared.underruns.fetch_add(1, Ordering::Relaxed);
                }
                self.gain = 0.0;
                self.hold_gain = (self.hold_gain - step).max(0.0);
                if self.hold_gain == 0.0 {
                    self.hold = [0.0; 2];
                }
                (self.hold[0] * self.hold_gain, self.hold[1] * self.hold_gain)
            };
            if left != 0.0 || right != 0.0 {
                if left_at == right_at {
                    outputs[left_at * frames + i] += 0.5 * (left + right) * self.level;
                } else {
                    outputs[left_at * frames + i] += left * self.level;
                    outputs[right_at * frames + i] += right * self.level;
                }
            }
        }
        let state = if self.current.is_some() && self.gain > 0.0 {
            SOUNDING
        } else if self.ended || (self.playing != 0 && self.playing == self.dead) {
            ENDED
        } else {
            SILENT
        };
        self.shared
            .heard_frames
            .store(self.played, Ordering::Release);
        self.shared
            .heard
            .store(pack(self.playing, state), Ordering::Release);
    }

    fn wanted(&self) -> bool {
        self.playing != 0 && self.playing != self.dead && !self.ended && self.next.is_none()
    }

    fn switch_to(&mut self, generation: u32) {
        self.playing = generation;
        self.next = None;
        self.gain = 0.0;
        self.played = 0;
        self.ended = false;
        self.starved = false;
    }

    fn fetch(&mut self, skipped: &mut usize) {
        while *skipped < SKIP_PER_CALL {
            let Ok(block) = self.full.pop() else {
                return;
            };
            if block.generation == self.playing && block.frames > 0 {
                self.current = Some(block);
                self.offset = 0;
                return;
            }
            if block.generation != self.playing {
                *skipped += 1;
                self.shared.stale.fetch_add(1, Ordering::Relaxed);
            }
            self.give_back(block);
        }
    }

    fn recycle_current(&mut self) {
        if let Some(block) = self.current.take() {
            self.give_back(block);
        }
        self.offset = 0;
    }

    fn give_back(&mut self, block: PreviewBlock) {
        let given = self.empty.push(block);
        debug_assert!(
            given.is_ok(),
            "a block going back needs the room every block's place guarantees"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn feed(feed: &mut PreviewFeed, generation: u32, value: f32, blocks: usize, last: bool) {
        let samples = vec![value; BLOCK_FRAMES];
        for n in 0..blocks {
            let mut block = feed.take().expect("a free block");
            block.fill(generation, &samples, None, last && n + 1 == blocks);
            feed.send(block);
        }
    }

    fn run(player: &mut PreviewPlayer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; 2 * frames];
        player.process(RATE, &mut out, 2, frames);
        out[..frames].to_vec()
    }

    #[test]
    fn what_is_heard_never_pairs_one_generation_with_another_s_end() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reads = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let reader = {
            let (handle, done, reads) = (handle.clone(), Arc::clone(&done), Arc::clone(&reads));
            std::thread::spawn(move || {
                let mut count = 0_u64;
                while !done.load(Ordering::Relaxed) {
                    let heard = handle.heard();
                    assert!(
                        !(heard.ended && heard.generation % 2 == 0),
                        "{heard:?} read as ended"
                    );
                    count += 1;
                    reads.store(count, Ordering::Release);
                }
                count
            })
        };
        while reads.load(Ordering::Acquire) == 0 {
            std::thread::yield_now();
        }
        for _ in 0..1_000 {
            let g = handle.start();
            let ends = g % 2 == 1;
            feed(&mut f, g, 0.5, 1, ends);
            let frames = if ends { BLOCK_FRAMES + 256 } else { 256 };
            run(&mut player, frames);
            assert_eq!(handle.heard().ended, ends, "generation {g}");
            run(&mut player, 256);
        }
        done.store(true, Ordering::Relaxed);
        assert!(reader.join().expect("the reader saw no mixed state") > 0);
    }

    #[test]
    fn a_preview_fades_in_and_plays_at_its_level() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let g = handle.start();
        feed(&mut f, g, 0.5, 2, false);
        let out = run(&mut player, 1_024);
        assert!(out[0] < 0.01, "starts from silence: {}", out[0]);
        assert!((out[1_000] - 0.5).abs() < 1e-4, "{}", out[1_000]);
        assert!(handle.heard().sounding);
        assert_eq!(handle.heard().generation, g);
    }

    #[test]
    fn nothing_of_an_older_generation_plays_after_a_change() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let a = handle.start();
        feed(&mut f, a, 0.25, 4, false);
        run(&mut player, 512);
        let b = handle.start();
        feed(&mut f, b, -0.75, 2, false);
        let c = handle.start();
        feed(&mut f, c, 0.5, 4, false);
        let out = run(&mut player, 4_096);
        let fade = (RAMP_SECONDS * RATE as f32).ceil() as usize + 1;
        assert!(out[..fade].iter().all(|s| (0.0..=0.25 + 1e-6).contains(s)));
        assert!(
            out.iter().all(|s| *s >= 0.0),
            "B was never heard: {:?}",
            out.iter().find(|s| **s < 0.0)
        );
        assert!((out[4_000] - 0.5).abs() < 1e-4);
        assert!(handle.counts().0 >= 2, "B's blocks were skipped");
    }

    #[test]
    fn stop_fades_to_silence_without_a_step() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let g = handle.start();
        feed(&mut f, g, 0.5, 4, false);
        run(&mut player, 1_024);
        handle.stop();
        let out = run(&mut player, 1_024);
        for pair in out.windows(2) {
            assert!((pair[0] - pair[1]).abs() < 0.01, "a step: {pair:?}");
        }
        assert!(out[1_000].abs() < 1e-6);
        assert!(!handle.heard().sounding);
    }

    #[test]
    fn an_underrun_fades_out_and_is_counted() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let g = handle.start();
        feed(&mut f, g, 0.5, 1, false);
        let out = run(&mut player, 4_096);
        let edge = BLOCK_FRAMES;
        assert!((out[edge - 1] - 0.5).abs() < 1e-4);
        assert!(out[edge] < 0.5 && out[edge] > 0.4, "{}", out[edge]);
        assert_eq!(out[edge + 200], 0.0);
        assert_eq!(handle.counts().1, 1);
    }

    #[test]
    fn the_end_of_the_file_ends_the_preview() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let g = handle.start();
        feed(&mut f, g, 0.5, 1, true);
        run(&mut player, 4_096);
        let heard = handle.heard();
        assert!(heard.ended && !heard.sounding);
        assert_eq!(heard.frames, BLOCK_FRAMES as u64);
        assert_eq!(handle.counts().1, 0, "an end is not an underrun");
    }

    #[test]
    fn a_rate_change_ends_the_preview() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        let g = handle.start();
        feed(&mut f, g, 0.5, 4, false);
        run(&mut player, 512);
        let mut out = vec![0.0; 2 * 512];
        player.process(44_100, &mut out, 2, 512);
        assert!(out[..512].windows(2).all(|w| w[1] <= w[0]));
        assert_eq!(out[511], 0.0);
        assert_eq!(handle.rate(), 44_100);
        let heard = handle.heard();
        assert_eq!(heard.generation, g);
        assert!(heard.ended && !heard.sounding);
        let g = handle.start();
        feed(&mut f, g, 0.5, 2, false);
        let mut out = vec![0.0; 2 * 1_024];
        player.process(44_100, &mut out, 2, 1_024);
        assert!((out[1_000] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn it_plays_on_the_cue_pair_when_asked_and_on_main_when_there_is_none() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        handle.set_first_channel(2);
        let g = handle.start();
        feed(&mut f, g, 0.5, 2, false);
        let frames = 512;
        let mut out = vec![0.0; 4 * frames];
        player.process(RATE, &mut out, 4, frames);
        assert!(
            out[..2 * frames].iter().all(|s| *s == 0.0),
            "main is silent"
        );
        assert!((out[3 * frames - 1] - 0.5).abs() < 1e-4);
        let mut out = vec![0.0; 2 * frames];
        player.process(RATE, &mut out, 2, frames);
        assert!((out[frames - 1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn every_block_comes_back() {
        let (handle, mut f, mut player) = preview();
        run(&mut player, 64);
        for _ in 0..50 {
            let g = handle.start();
            while let Some(mut block) = f.take() {
                block.fill(g, &[0.1; 16], None, false);
                f.send(block);
                if f.full.slots() == 0 {
                    break;
                }
            }
            run(&mut player, 256);
        }
        handle.stop();
        for _ in 0..200 {
            run(&mut player, 256);
        }
        let mut free = 0;
        while f.take().is_some() {
            free += 1;
        }
        assert_eq!(free + usize::from(player.current.is_some()), RING_BLOCKS);
    }
}
