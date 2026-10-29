//! The automated soak (M9 slice R6, S8): 64 streamed stereo tracks played in
//! real time from a disk whose reads take a varying time and now and then
//! stall, with a loop going round and locates to markers, to the loop's
//! start and to places nothing marks, each held until the disk has read
//! there, as the transport holds them (L A). No late read is allowed. The
//! wait is not cut off at 150 ms, so that a CI machine under load tests the
//! read-ahead rather than its own disk; holds longer than that are reported.
//! Blocks whose time passed while the machine was busy are missed, as a
//! device misses them, and reported apart: they are dropouts, not late reads.
//!
//! `LUMEN_SOAK_SECONDS` runs it longer than CI's default.

use std::sync::atomic::AtomicUsize;

use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipShape, Command, Destination, NodeSpec, Region, Routing, Timeline,
    clip_player, engine, mixer,
};

use super::plan::Scene;
use super::*;
use crate::transport::hold::{Gate, Held, LONGEST};

const RATE: u32 = 48_000;
const BLOCK: usize = 128;
const TRACKS: u64 = 64;
/// A minute of audio per file: 704 pages.
const FILE_SECONDS: u64 = 60;
/// A quarter at 120 BPM.
const QUARTER: i64 = 24_000;

/// Pages read after a time that varies from 50 to 250 µs (a fast disk under
/// load), and every 1,000th read stalls for 40 ms (the disk busy elsewhere).
struct Uneven {
    reads: Arc<AtomicUsize>,
}

impl Pages for Uneven {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let n = self.reads.fetch_add(1, Ordering::Relaxed);
        if n % 1_000 == 999 {
            std::thread::sleep(Duration::from_millis(40));
        } else {
            // Spun: a sleep this short lasts a millisecond or more on some
            // systems.
            let wait = Duration::from_micros(50 + (n as u64 * 7_919) % 200);
            let started = Instant::now();
            while started.elapsed() < wait {
                std::hint::spin_loop();
            }
        }
        let from = page * PAGE;
        let frames = (audio.frames() as usize).saturating_sub(from).min(PAGE);
        left[..frames].fill(0.01);
        right[..frames].fill(-0.01);
        Ok(frames)
    }

    fn keep(&mut self, _: &[StreamAudio]) {}
}

/// What the soak does, in seconds after it starts.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// To the start (a point the disk keeps).
    Start,
    /// To the marker (a point).
    Marker,
    /// To the loop's start (a point), and round the loop from there.
    Loop,
    /// Somewhere nothing marks: the disk is cued there first.
    Elsewhere(i64),
}

impl Step {
    fn at(self) -> Ticks {
        match self {
            Step::Start => Ticks::quarters(0),
            Step::Marker => Ticks::quarters(80),
            Step::Loop => Ticks::quarters(16),
            Step::Elsewhere(quarter) => Ticks::quarters(quarter),
        }
    }
}

/// The steps of one 8 s round, the `round`th: each place is played for 2 s.
fn round(round: i64) -> [Step; 4] {
    [
        Step::Start,
        Step::Marker,
        Step::Elsewhere(50 + 6 * (round % 8)),
        Step::Loop,
    ]
}

#[test]
fn sixty_four_streamed_tracks_play_with_loops_and_locates_and_none_is_late() {
    let seconds: f64 = std::env::var("LUMEN_SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16.0);
    let pool = PagePool::new(crate::media::streaming::POOL_PAGES);
    let frames = FILE_SECONDS * u64::from(RATE);
    let clips: Vec<Clip> = (0..TRACKS)
        .map(|track| {
            let audio = StreamAudio::new(&pool, track + 1, RATE, 2, frames);
            Clip {
                envelope: Default::default(),
                id: track + 1,
                track,
                start: Ticks::quarters(0),
                offset: 0,
                length: frames as i64,
                gain: 1.0,
                audio: Arc::new(ClipAudio::Stream(audio)),
                shape: ClipShape::default(),
            }
        })
        .collect();

    let (mut clips_handle, source) = clip_player();
    clips_handle.set(clips.clone()).unwrap();
    let (mut routing, source) = mixer(source);
    assert!(
        routing
            .set(&Routing {
                nodes: (0..TRACKS)
                    .map(|t| NodeSpec::unity(t, Destination::Hardware(0)))
                    .collect(),
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (mut handle, mut engine) = engine(Timeline::default(), source, RATE);
    // A loop of one second, from 8 s.
    let looped = (16 * QUARTER, 18 * QUARTER);
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::quarters(16),
            Ticks::quarters(18),
            true,
        )))
        .unwrap();

    let reads = Arc::new(AtomicUsize::new(0));
    let pages = Box::new(Uneven {
        reads: Arc::clone(&reads),
    });
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    let scene = Arc::new(Scene::new(
        &clips,
        &Timeline::default(),
        RATE,
        Some(looped),
        vec![0, looped.0, 80 * QUARTER],
    ));
    disk.set_scene(Arc::clone(&scene));
    let gate = Gate {
        scene,
        cuer: disk.cuer(),
    };

    let mut data = vec![0.0; 2 * BLOCK];
    let block = Duration::from_secs_f64(BLOCK as f64 / f64::from(RATE));
    let started = Instant::now();
    let mut played = 0u32;
    let mut held: Option<Held> = None;
    let mut next = 0usize;
    let steps: Vec<Step> = (0..)
        .flat_map(round)
        .take((seconds / 2.0) as usize)
        .collect();
    let mut first = true;
    let mut longest_hold = Duration::ZERO;
    let mut overdue = 0usize;
    // Each late read: when, how long after the last start was sent, and in
    // which step, so a failure says where the read-ahead fell short.
    let mut sent = Instant::now();
    let mut step = None;
    let mut late = Vec::new();
    let mut counted = 0;
    let mut missed = 0u32;
    while started.elapsed().as_secs_f64() < seconds {
        // The next step every 2 s, held for the disk.
        if held.is_none()
            && next < steps.len()
            && started.elapsed().as_secs_f64() >= 2.0 * next as f64
        {
            let at = steps[next].at();
            let sample = gate.sample_at(at);
            let command = if first {
                handle.send(Command::Locate(at)).unwrap();
                Command::Play
            } else {
                Command::Locate(at)
            };
            first = false;
            next += 1;
            step = Some(steps[next - 1]);
            gate.cuer.cue(Some(sample));
            if gate.ready(sample) {
                handle.send(command).unwrap();
                gate.cuer.follow_after(handle.sent_command());
                sent = Instant::now();
            } else {
                held = Some(Held {
                    command,
                    sample,
                    since: Instant::now(),
                });
            }
        }
        if let Some(waiting) = &held
            && gate.ready(waiting.sample)
        {
            let waiting = held.take().expect("checked above");
            let waited = waiting.since.elapsed();
            longest_hold = longest_hold.max(waited);
            overdue += usize::from(waited > LONGEST);
            handle.send(waiting.command).unwrap();
            gate.cuer.follow_after(handle.sent_command());
            sent = Instant::now();
        }
        engine.process(RATE, &mut data, 2, BLOCK);
        played += 1;
        let now = pool.underruns();
        if now > counted {
            late.push((now - counted, started.elapsed(), sent.elapsed(), step));
            counted = now;
        }
        // In real time: wait for the block's time to come. A block whose
        // time has passed is missed, as a device's callback is when the
        // machine is busy: a dropout, which the app counts apart from late
        // reads. Processing the missed blocks one after another instead
        // would play faster than real time, which no device does.
        match (block * played).checked_sub(started.elapsed()) {
            Some(wait) => std::thread::sleep(wait),
            None => {
                let due = (started.elapsed().as_secs_f64() / block.as_secs_f64()) as u32;
                missed += due.saturating_sub(played);
                played = due.max(played);
            }
        }
    }
    let heard = data.iter().any(|v| *v != 0.0);
    let underruns = pool.underruns();
    let failed = disk.stats().failed();
    drop(disk);
    eprintln!(
        "soak: {seconds} s, {next} steps, {} pages read, longest hold {longest_hold:?} \
         ({overdue} past 150 ms), {underruns} late, {missed} blocks missed (the machine was busy)",
        reads.load(Ordering::Relaxed),
    );
    for (n, at, after, step) in &late {
        eprintln!("soak: {n} late at {at:?}, {after:?} after {step:?} was sent");
    }
    let _ = (&clips_handle, &routing);
    assert!(heard, "the tracks were heard");
    assert_eq!(failed, 0);
    assert_eq!(underruns, 0, "no read came too late");
}
