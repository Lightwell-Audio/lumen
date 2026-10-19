//! The guard around one host: a block handed over, a block collected against
//! a deadline, and what the engine and the interface may read about it.

use super::*;

/// A host and the rules for using it.
pub struct Guard {
    host: Box<dyn Hosted>,
    /// A block was submitted and not yet collected.
    sent: bool,
    /// The frames of the block in flight.
    frames: usize,
    /// Frames of audio late in a row.
    missed: u64,
    /// Frames since the host's output was last above [`QUIET_LEVEL`]; it
    /// starts quiet.
    quiet: u64,
    /// A block was asked for since the last [`Guard::age`].
    asked: bool,
    /// The last block collected was the host's (see [`Guard::output`]).
    ready: bool,
    status: Arc<Status>,
}

impl Guard {
    /// A host whose plugin last reported `latency` frames. Allocates (the
    /// [`Status`]): call it on the interface side.
    pub fn new(host: Box<dyn Hosted>, latency: u32) -> Self {
        Self {
            host,
            sent: false,
            frames: 0,
            missed: 0,
            quiet: u64::MAX,
            asked: false,
            ready: false,
            status: Status::new(latency),
        }
    }

    /// Whether the host's output may still be heard at `rate`: it was above
    /// [`QUIET_LEVEL`] within the last [`QUIET_SECONDS`], or within its
    /// reported tail when that is longer (at most ten seconds). A failed host
    /// is not.
    pub fn sounding(&self, rate: u32) -> bool {
        let rate = u64::from(rate);
        let tail = u64::from(self.tail()).min(rate * u64::from(TAIL_MOST_SECONDS));
        let window = (rate * u64::from(QUIET_SECONDS)).max(tail);
        !self.failed() && self.quiet < window
    }

    /// Something was said that may make the plugin sound (a note, a pedal):
    /// it counts as sounding from now, before its output says so.
    pub fn wake(&mut self) {
        self.quiet = 0;
    }

    /// The plugin was told to stop every sound: it counts as quiet at once.
    pub fn hush(&mut self) {
        self.quiet = u64::MAX;
    }

    /// The plugin is asked to forget what it holds before its next block: a
    /// reverb's tail must not carry to a new place in the song. It keeps
    /// being asked for blocks and is heard quiet as soon as its output is.
    pub fn flush(&mut self) {
        if !self.failed() {
            self.host.flush();
        }
    }

    /// `frames` went by: a host that was not asked for a block since the
    /// last call has been quiet for them (its node did not run).
    pub fn age(&mut self, frames: usize) {
        if !std::mem::take(&mut self.asked) {
            self.quiet = self.quiet.saturating_add(frames as u64);
        }
    }

    /// Follows how loud the block's output was.
    fn heard(&mut self, left: &[f32], right: &[f32]) {
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(left.len() as u64);
        }
    }

    fn quiet_for(&mut self, frames: usize) {
        self.quiet = self.quiet.saturating_add(frames as u64);
    }

    /// What the interface may read about the host.
    pub fn status(&self) -> Arc<Status> {
        self.status.clone()
    }

    pub fn failed(&self) -> bool {
        self.status.failed()
    }

    /// The interface bypassed the device.
    pub fn bypassed(&self) -> bool {
        self.status.bypassed()
    }

    pub fn latency(&self) -> u32 {
        self.status.latency()
    }

    pub fn tail(&self) -> u32 {
        self.status.tail()
    }

    pub fn late_blocks(&self) -> u32 {
        self.status.late_blocks()
    }

    pub fn take_news(&mut self) -> Option<u32> {
        self.status.take_news()
    }

    pub fn take_restart(&mut self) -> bool {
        self.status.take_restart()
    }

    /// Lets go of the last block's outputs: [`Guard::output`] answers nothing
    /// until the next block is finished.
    pub fn forget_block(&mut self) {
        self.ready = false;
    }

    /// Hands the host a block. Returns whether it took it; when it did not,
    /// [`Guard::finish`] answers [`Outcome::Silent`] at once.
    pub fn begin(&mut self, frames: usize, input: &[&[f32]], events: &[Event]) -> bool {
        self.start(frames, input, events, true)
    }

    /// As [`Guard::begin`], for a block the host already refused once in
    /// this callback (it was tried early, and is tried again before it is
    /// given up on): a second refusal is the same missed block, not another.
    pub fn begin_again(&mut self, frames: usize, input: &[&[f32]], events: &[Event]) -> bool {
        self.start(frames, input, events, false)
    }

    fn start(&mut self, frames: usize, input: &[&[f32]], events: &[Event], count: bool) -> bool {
        self.sent = false;
        self.frames = frames;
        self.asked = true;
        if self.failed() {
            return false;
        }
        if self.host.ended() {
            self.fail();
            return false;
        }
        match self.host.submit(frames, input, events) {
            Ok(()) => {
                self.sent = true;
                true
            }
            Err(Refused::Busy) => {
                if count {
                    self.miss();
                }
                false
            }
            Err(Refused::Gone) => {
                self.fail();
                false
            }
        }
    }

    /// Bus pair `pair` (0 is the main bus, then the auxiliary outputs that
    /// are on) of the block [`Guard::finish`] last answered, into `left` and
    /// `right`: false when that block was not the host's or there is no such
    /// pair. Finite samples only. Never blocks.
    pub fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if !(self.ready && self.host.output(pair, left, right)) {
            return false;
        }
        // An instrument that plays only on its other outputs is still
        // sounding: they count as much as the main one.
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        }
        true
    }

    /// Takes the block's answer, waiting until `deadline` at most.
    pub fn finish(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Outcome {
        self.ready = false;
        if !std::mem::take(&mut self.sent) {
            self.quiet_for(left.len());
            return Outcome::Silent;
        }
        let outcome = self.collect(deadline, left, right);
        self.ready = outcome == Outcome::Ready;
        match outcome {
            Outcome::Ready => self.heard(left, right),
            Outcome::Silent => self.quiet_for(left.len()),
        }
        outcome
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Outcome {
        let asked = Instant::now();
        let reply = self.host.collect(deadline, left, right);
        let waited = asked.elapsed();
        match reply {
            Reply::Ready {
                latency,
                tail,
                restart,
                work_ns,
            } => {
                self.status.costs.note(Some(work_ns), waited);
                self.missed = 0;
                if self.status.latency.swap(latency, Ordering::Relaxed) != latency {
                    self.status.news.store(true, Ordering::Release);
                }
                self.status.tail.store(tail, Ordering::Relaxed);
                if restart {
                    self.status.restart.store(true, Ordering::Release);
                }
                Outcome::Ready
            }
            Reply::Late => {
                self.status.costs.note(None, waited);
                self.miss();
                if self.host.ended() {
                    self.fail();
                }
                Outcome::Silent
            }
            Reply::Gone => {
                self.fail();
                Outcome::Silent
            }
        }
    }

    fn fail(&mut self) {
        self.status.failed.store(true, Ordering::Release);
    }

    fn miss(&mut self) {
        self.missed += self.frames as u64;
        self.status.late.fetch_add(1, Ordering::Relaxed);
        if self.missed >= FAILED_AFTER_FRAMES {
            self.fail();
        }
    }
}
