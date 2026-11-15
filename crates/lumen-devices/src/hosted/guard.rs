use super::*;

pub struct Guard {
    host: Box<dyn Hosted>,
    sent: bool,
    frames: usize,
    missed: u64,
    quiet: u64,
    asked: bool,
    ready: bool,
    status: Arc<Status>,
}

impl Guard {
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

    pub fn sounding(&self, rate: u32) -> bool {
        let rate = u64::from(rate);
        let tail = u64::from(self.tail()).min(rate * u64::from(TAIL_MOST_SECONDS));
        let window = (rate * u64::from(QUIET_SECONDS)).max(tail);
        !self.failed() && self.quiet < window
    }

    pub fn wake(&mut self) {
        self.quiet = 0;
    }

    pub fn hush(&mut self) {
        self.quiet = u64::MAX;
    }

    pub fn flush(&mut self) {
        if !self.failed() {
            self.host.flush();
        }
    }

    pub fn age(&mut self, frames: usize) {
        if !std::mem::take(&mut self.asked) {
            self.quiet = self.quiet.saturating_add(frames as u64);
        }
    }

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

    pub fn status(&self) -> Arc<Status> {
        self.status.clone()
    }

    pub fn failed(&self) -> bool {
        self.status.failed()
    }

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

    pub fn forget_block(&mut self) {
        self.ready = false;
    }

    pub fn begin(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> bool {
        self.start(frames, input, events, params, true)
    }

    pub fn begin_again(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> bool {
        self.start(frames, input, events, params, false)
    }

    fn start(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
        count: bool,
    ) -> bool {
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
        if params.cut() > 0 && count {
            self.status
                .params_cut
                .fetch_add(params.cut(), Ordering::Relaxed);
        }
        match self.host.submit(frames, input, events, params) {
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

    pub fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if !(self.ready && self.host.output(pair, left, right)) {
            return false;
        }
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        }
        true
    }

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
