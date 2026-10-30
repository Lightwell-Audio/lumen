//! Measuring the round trip through a cable (M9 slice R2b, ADR-0110).
//!
//! A cable joins one of the device's outputs to one of its inputs. The audio
//! side plays [`CLICKS`] short clicks on the output, with everything else
//! Lumen plays silenced, and keeps what the input hears meanwhile. Off the
//! audio thread, [`find_round_trip`] finds each click in what came back.
//!
//! Output and input are counted in the same callback frames, so the delay
//! found is what recording must shift a take by: the frames between a sample
//! leaving the callback and the same sound arriving in it. That is what
//! recording's placement takes from the driver's reported latencies
//! (ADR-0102), so the difference between the two is the recording offset.
//!
//! The click is a burst of [`CLICK_FRAMES`] pseudo-random steps at −12 dBFS:
//! it correlates to one sharp peak, also after the converters' filters, and
//! sounds like a tick.

/// How many clicks are played. Every one must be found at the same delay.
pub const CLICKS: usize = 5;
/// Frames in one click.
pub const CLICK_FRAMES: usize = 256;
/// −12 dBFS.
const LEVEL: f32 = 0.25;
/// Seconds between the clicks' starts.
pub const SPACING_SECONDS: f64 = 0.3;
/// Silence before the first click.
const LEAD_SECONDS: f64 = 0.1;
/// The longest round trip looked for. Shorter than the spacing, so a click
/// is never taken for the one before or after it.
pub const MAX_ROUND_TRIP_SECONDS: f64 = 0.25;
/// How far apart, in frames, the clicks' delays may be and still agree.
pub const AGREE_FRAMES: u32 = 2;
/// Below this (−60 dBFS), nothing came back.
const SILENT: f32 = 0.001;
/// What counts as clipped.
const CLIPPED: f32 = 0.99;
/// How far a correlation peak must stand above the rest of its window.
const CLEAR: f32 = 8.0;
/// Lags this close to the peak are its own ringing, not the rest.
const NEAR_PEAK: usize = 2 * CLICK_FRAMES;
/// What [`Measured::returns`] keeps of each click: lags before the one
/// found, and in all.
pub const RETURN_BEFORE: usize = 32;
pub const RETURN_FRAMES: usize = 288;

/// Where the clicks go in a measurement at one rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    lead: usize,
    spacing: usize,
    max_lag: usize,
    /// Frames kept from the input.
    pub frames: usize,
}

impl Plan {
    pub fn new(rate: u32) -> Self {
        let at = |seconds: f64| (seconds * f64::from(rate.max(1))).round() as usize;
        let (lead, spacing, max_lag) = (
            at(LEAD_SECONDS),
            at(SPACING_SECONDS),
            at(MAX_ROUND_TRIP_SECONDS),
        );
        Self {
            lead,
            spacing,
            max_lag,
            frames: lead + (CLICKS - 1) * spacing + CLICK_FRAMES + max_lag,
        }
    }

    /// The frame click `k` starts on.
    pub fn click_at(&self, k: usize) -> usize {
        self.lead + k * self.spacing
    }
}

/// The click: steps of ±[`LEVEL`] from an 8-bit maximal-length sequence,
/// the same every time.
pub fn click() -> [f32; CLICK_FRAMES] {
    let mut state: u8 = 0x5A;
    std::array::from_fn(|_| {
        // x^8 + x^6 + x^5 + x^4 + 1.
        let bit = (state ^ (state >> 2) ^ (state >> 3) ^ (state >> 4)) & 1;
        state = (state >> 1) | (bit << 7);
        if bit == 1 { LEVEL } else { -LEVEL }
    })
}

/// How a measurement ended on the audio side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Running,
    /// Every frame was kept.
    Done,
    /// The device changed rate part way.
    RateChanged,
    /// The device has no such output or input (any more).
    NoChannel,
}

/// One measurement: what to play where, and what came back. Made off the
/// audio thread, with its buffer, and handed back when it ends.
#[derive(Debug)]
pub struct Job {
    /// Tells this measurement from an older one still on its way back.
    id: u64,
    /// The first output channel, in the callback's order.
    output: usize,
    /// Both channels of a pair, or one.
    pair: bool,
    /// The input channel, in the callback's order.
    input: usize,
    rate: u32,
    plan: Plan,
    click: [f32; CLICK_FRAMES],
    captured: Box<[f32]>,
    kept: usize,
    ended: Ended,
}

impl Job {
    /// Allocates: call it off the audio thread.
    pub fn new(id: u64, rate: u32, output: usize, pair: bool, input: usize) -> Box<Self> {
        let plan = Plan::new(rate);
        Box::new(Self {
            id,
            output,
            pair,
            input,
            rate,
            plan,
            click: click(),
            captured: vec![0.0; plan.frames].into_boxed_slice(),
            kept: 0,
            ended: Ended::Running,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn ended(&self) -> Ended {
        self.ended
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// What the input heard, from the measurement's first frame.
    pub fn captured(&self) -> &[f32] {
        &self.captured
    }

    /// Plays and keeps one block. Real-time safe.
    fn process(&mut self, block: Block<'_, '_>) {
        let Block {
            rate,
            inputs,
            input_channels,
            outputs,
            output_channels,
            frames,
        } = block;
        if rate != self.rate {
            self.ended = Ended::RateChanged;
            return;
        }
        let last_output = self.output + usize::from(self.pair);
        if self.input >= input_channels
            || last_output >= output_channels
            || inputs.len() < input_channels * frames
            || outputs.len() < output_channels * frames
        {
            self.ended = Ended::NoChannel;
            return;
        }
        // Only the clicks are heard while measuring.
        outputs[..output_channels * frames].fill(0.0);
        let n = frames.min(self.captured.len() - self.kept);
        let from = self.kept;
        self.captured[from..from + n].copy_from_slice(&inputs[self.input * frames..][..n]);
        for k in 0..CLICKS {
            let start = self.plan.click_at(k);
            let (a, b) = (start.max(from), (start + CLICK_FRAMES).min(from + n));
            for at in a..b {
                for channel in self.output..=last_output {
                    outputs[channel * frames + at - from] = self.click[at - start];
                }
            }
        }
        self.kept += n;
        if self.kept == self.captured.len() {
            self.ended = Ended::Done;
        }
    }
}

/// One callback's buffers, as the device hands them over: channel after
/// channel.
#[derive(Debug)]
pub struct Block<'i, 'o> {
    pub rate: u32,
    pub inputs: &'i [f32],
    pub input_channels: usize,
    pub outputs: &'o mut [f32],
    pub output_channels: usize,
    pub frames: usize,
}

/// UI side: starts measurements and takes them back. May allocate.
#[derive(Debug)]
pub struct LoopbackHandle {
    jobs: rtrb::Producer<Box<Job>>,
    ended: rtrb::Consumer<Box<Job>>,
}

impl LoopbackHandle {
    /// Queues a measurement; it starts at the next block. Returns it if one
    /// is already waiting.
    pub fn start(&mut self, job: Box<Job>) -> Result<(), Box<Job>> {
        self.jobs.push(job).map_err(|rtrb::PushError::Full(j)| j)
    }

    /// A measurement that ended, if one did.
    pub fn ended(&mut self) -> Option<Box<Job>> {
        self.ended.pop().ok()
    }
}

/// Audio side. Runs after the engine in the device callback.
#[derive(Debug)]
pub struct LoopbackPlayer {
    jobs: rtrb::Consumer<Box<Job>>,
    ended: rtrb::Producer<Box<Job>>,
    job: Option<Box<Job>>,
}

impl LoopbackPlayer {
    /// Plays the clicks over what the engine rendered, and keeps the input.
    /// Real-time safe: the job's buffer was made with it, and a job that
    /// ends goes back to the UI to be freed there.
    pub fn process(&mut self, block: Block<'_, '_>) {
        if self.job.is_none() {
            self.job = self.jobs.pop().ok();
        }
        let Some(job) = self.job.as_mut() else {
            return;
        };
        if job.ended == Ended::Running {
            job.process(block);
        }
        if job.ended != Ended::Running
            && let Some(job) = self.job.take()
            && let Err(rtrb::PushError::Full(job)) = self.ended.push(job)
        {
            // Tried again next block.
            self.job = Some(job);
        }
    }
}

/// Makes the pair. Allocates: call it off the audio thread.
pub fn loopback() -> (LoopbackHandle, LoopbackPlayer) {
    let (jobs_tx, jobs_rx) = rtrb::RingBuffer::new(1);
    let (ended_tx, ended_rx) = rtrb::RingBuffer::new(2);
    (
        LoopbackHandle {
            jobs: jobs_tx,
            ended: ended_rx,
        },
        LoopbackPlayer {
            jobs: jobs_rx,
            ended: ended_tx,
            job: None,
        },
    )
}

/// A round trip that every click agreed on.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// Frames from leaving the callback to arriving back in it.
    pub frames: u32,
    /// The input reached full scale somewhere.
    pub clipped: bool,
    /// The clicks came back upside down (a polarity flip on the way).
    pub inverted: bool,
    /// Each click's correlation with what came back, from [`RETURN_BEFORE`]
    /// lags before the delay found, [`RETURN_FRAMES`] long, scaled so a
    /// return at full level peaks at 1: for drawing. A clean return is one
    /// sharp line at the delay; the converters' filters ring around it.
    pub returns: Vec<Vec<f32>>,
}

/// Why a measurement gave no number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The input stayed silent: no cable, the wrong input, or its gain is
    /// down.
    Silent,
    /// Something came back, but no click stood out of the noise.
    Unclear,
    /// The clicks came back at different delays, this many frames apart.
    Disagree { spread: u32 },
    /// The device changed rate part way.
    RateChanged,
    /// The output or the input went away.
    NoChannel,
}

/// Finds the round trip in a measurement that ended.
pub fn analyse(job: &Job) -> Result<Measured, Failure> {
    match job.ended {
        Ended::Done => find_round_trip(job.captured(), job.rate),
        Ended::RateChanged => Err(Failure::RateChanged),
        Ended::NoChannel | Ended::Running => Err(Failure::NoChannel),
    }
}

/// Finds the clicks of a measurement at `rate` in what the input heard
/// (from the measurement's first frame).
pub fn find_round_trip(captured: &[f32], rate: u32) -> Result<Measured, Failure> {
    let plan = Plan::new(rate);
    let loudest = captured.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if loudest < SILENT || !loudest.is_finite() {
        return Err(Failure::Silent);
    }
    let click = click();
    let mut lags = [0_usize; CLICKS];
    let mut negative = 0;
    for (k, lag_found) in lags.iter_mut().enumerate() {
        let start = plan.click_at(k);
        let correlation: Vec<f32> = (0..=plan.max_lag)
            .map_while(|lag| {
                let window = captured.get(start + lag..start + lag + CLICK_FRAMES)?;
                Some(window.iter().zip(&click).map(|(x, c)| x * c).sum())
            })
            .collect();
        let (peak, value) = correlation
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, v)| (i, *v))
            .ok_or(Failure::Unclear)?;
        let (sum, count) = correlation
            .iter()
            .enumerate()
            .filter(|(i, _)| i.abs_diff(peak) >= NEAR_PEAK)
            .fold((0.0_f64, 0_usize), |(s, n), (_, v)| {
                (s + f64::from(*v) * f64::from(*v), n + 1)
            });
        let rest = if count == 0 {
            0.0
        } else {
            (sum / count as f64).sqrt() as f32
        };
        if value.abs() < CLEAR * rest || value == 0.0 {
            return Err(Failure::Unclear);
        }
        if value < 0.0 {
            negative += 1;
        }
        *lag_found = peak;
    }
    let mut sorted = lags;
    sorted.sort_unstable();
    let spread = (sorted[CLICKS - 1] - sorted[0]) as u32;
    if spread > AGREE_FRAMES {
        return Err(Failure::Disagree { spread });
    }
    let frames = sorted[CLICKS / 2] as u32;
    let energy: f32 = click.iter().map(|c| c * c).sum();
    let first = (frames as usize).saturating_sub(RETURN_BEFORE);
    let returns = (0..CLICKS)
        .map(|k| {
            let start = plan.click_at(k);
            (first..first + RETURN_FRAMES)
                .map(|lag| {
                    captured
                        .get(start + lag..start + lag + CLICK_FRAMES)
                        .map_or(0.0, |w| {
                            w.iter().zip(&click).map(|(x, c)| x * c).sum::<f32>() / energy
                        })
                })
                .collect()
        })
        .collect();
    Ok(Measured {
        frames,
        clipped: loudest >= CLIPPED,
        inverted: negative * 2 > CLICKS,
        returns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// What the input hears through a cable `delay` frames long with `gain`,
    /// plus noise at `noise` (a fixed pseudo-random sequence).
    fn through_cable(delay: usize, gain: f32, noise: f32) -> Vec<f32> {
        let plan = Plan::new(RATE);
        let click = click();
        let mut seed = 1_u32;
        let mut captured: Vec<f32> = (0..plan.frames)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
            })
            .map(|r| r * noise)
            .collect();
        for k in 0..CLICKS {
            for (i, c) in click.iter().enumerate() {
                if let Some(s) = captured.get_mut(plan.click_at(k) + delay + i) {
                    *s += c * gain;
                }
            }
        }
        captured
    }

    #[test]
    fn the_click_is_balanced_and_peaks_alone() {
        let click = click();
        let sum: f32 = click.iter().sum();
        assert!(sum.abs() <= 2.0 * LEVEL, "about as many ups as downs");
        let energy: f32 = click.iter().map(|c| c * c).sum();
        for shift in 1..CLICK_FRAMES {
            let side: f32 = click[shift..].iter().zip(&click).map(|(a, b)| a * b).sum();
            assert!(
                side.abs() < energy * 0.2,
                "shift {shift}: {side} of {energy}"
            );
        }
    }

    #[test]
    fn a_clean_cable_gives_its_delay_to_the_frame() {
        for delay in [0, 1, 129, 470, 2_048, 11_000] {
            let found = find_round_trip(&through_cable(delay, 0.5, 0.0), RATE).unwrap();
            assert_eq!(found.frames, delay as u32);
            assert!(!found.clipped && !found.inverted);
            assert_eq!(found.returns.len(), CLICKS);
            assert!(found.returns.iter().all(|r| r.len() == RETURN_FRAMES));
            // One sharp line at the delay, at the cable's gain.
            let at = (delay).min(RETURN_BEFORE);
            for back in &found.returns {
                assert!((back[at] - 0.5).abs() < 1e-3, "{delay}: {}", back[at]);
                let side = back
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != at)
                    .fold(0.0_f32, |m, (_, v)| m.max(v.abs()));
                assert!(side < 0.05, "{delay}: {side}");
            }
        }
    }

    #[test]
    fn noise_and_a_quiet_input_still_find_it() {
        let found = find_round_trip(&through_cable(733, 0.02, 0.01), RATE).unwrap();
        assert_eq!(found.frames, 733);
    }

    #[test]
    fn a_polarity_flip_is_found_and_said() {
        let found = find_round_trip(&through_cable(500, -0.5, 0.0), RATE).unwrap();
        assert_eq!(found.frames, 500);
        assert!(found.inverted);
    }

    #[test]
    fn a_loud_input_says_it_clipped() {
        let mut captured = through_cable(500, 0.5, 0.0);
        captured[10] = 1.0;
        assert!(find_round_trip(&captured, RATE).unwrap().clipped);
    }

    #[test]
    fn silence_and_noise_are_refused() {
        let plan = Plan::new(RATE);
        assert_eq!(
            find_round_trip(&vec![0.0; plan.frames], RATE),
            Err(Failure::Silent)
        );
        assert_eq!(
            find_round_trip(&through_cable(500, 0.0, 0.1), RATE),
            Err(Failure::Unclear)
        );
    }

    #[test]
    fn clicks_that_disagree_are_refused() {
        let plan = Plan::new(RATE);
        let click = click();
        let mut captured = vec![0.0; plan.frames];
        for k in 0..CLICKS {
            // Each click later than the one before: a drifting clock.
            let delay = 400 + k * 3;
            for (i, c) in click.iter().enumerate() {
                captured[plan.click_at(k) + delay + i] += c * 0.5;
            }
        }
        assert_eq!(
            find_round_trip(&captured, RATE),
            Err(Failure::Disagree { spread: 12 })
        );
    }

    /// Runs `job` through the player as a device would, the cable joining
    /// output `out` to input `inp` `delay` frames later; returns what the
    /// first block played.
    fn run(job: Box<Job>, delay: usize, block: usize) -> (Box<Job>, Vec<f32>) {
        let (mut handle, mut player) = loopback();
        handle.start(job).unwrap();
        let (outs, ins) = (4, 2);
        let mut line = vec![0.0_f32; 1 << 20];
        let mut first = Vec::new();
        for n in 0.. {
            let at = n * block;
            let mut inputs = vec![0.0_f32; ins * block];
            for i in 0..block {
                inputs[block + i] = (at + i).checked_sub(delay).map_or(0.0, |p| line[p]);
            }
            // The engine played something; the measurement silences it.
            let mut outputs = vec![0.3_f32; outs * block];
            player.process(Block {
                rate: RATE,
                inputs: &inputs,
                input_channels: ins,
                outputs: &mut outputs,
                output_channels: outs,
                frames: block,
            });
            line[at..at + block].copy_from_slice(&outputs[2 * block..3 * block]);
            if n == 0 {
                first = outputs;
            }
            if let Some(job) = handle.ended() {
                return (job, first);
            }
        }
        unreachable!()
    }

    #[test]
    fn the_player_measures_a_cable_through_blocks() {
        let (job, first) = run(Job::new(1, RATE, 2, true, 1), 517, 128);
        assert_eq!(job.ended(), Ended::Done);
        assert!(first.iter().all(|s| *s == 0.0), "the rest is silenced");
        assert_eq!(analyse(&job).unwrap().frames, 517);
    }

    #[test]
    fn a_channel_the_device_lacks_ends_it_at_once() {
        let (job, _) = run(Job::new(1, RATE, 0, true, 5), 300, 64);
        assert_eq!(job.ended(), Ended::NoChannel);
        assert_eq!(analyse(&job), Err(Failure::NoChannel));
    }

    #[test]
    fn a_rate_change_ends_it() {
        let (mut handle, mut player) = loopback();
        handle.start(Job::new(1, 44_100, 0, false, 0)).unwrap();
        let mut outputs = vec![0.0; 128];
        player.process(Block {
            rate: RATE,
            inputs: &[0.0; 128],
            input_channels: 1,
            outputs: &mut outputs,
            output_channels: 1,
            frames: 128,
        });
        let job = handle.ended().unwrap();
        assert_eq!(analyse(&job), Err(Failure::RateChanged));
    }
}
