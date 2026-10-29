//! The automated soak (M9 slice R6, S8): 64 streamed stereo tracks played in
//! real time from a disk whose reads take a varying time and now and then
//! stall, with a loop going round and locates to markers, to the loop's
//! start and to places nothing marks, each held until the disk has read
//! there, as the transport holds them (L A). No late read is allowed. The
//! wait uses the production 150 ms release rule, so cold-start verification
//! cannot hide behind a longer hold than the application allows.
//! Callbacks whose deadlines pass before the next render are counted apart
//! from late reads; render duration and sleep overshoot are reported too.
//!
//! `LUMEN_SOAK_SECONDS` runs it longer than CI's default.

use std::sync::Mutex;
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
    stalls: Arc<Mutex<Vec<InjectedStall>>>,
    dropped_stalls: Arc<AtomicUsize>,
    diagnostic_epoch: Instant,
}

#[derive(Clone, Copy, Debug)]
struct InjectedStall {
    read: usize,
    started: Duration,
    ended: Duration,
    elapsed: Duration,
}

#[derive(Debug)]
struct HeldDiagnostic {
    step_index: usize,
    sample: i64,
    started: Duration,
    released: Duration,
    waited: Duration,
    ready_at_release: bool,
    cue_adopted: bool,
    deadline_triggered: bool,
    at_or_after_deadline_at_send: bool,
    missing_required: usize,
    first_missing: Option<(usize, usize)>,
    readiness_scan_elapsed: Duration,
    worker: WorkerSnapshot,
    read_count: usize,
}

#[derive(Debug)]
struct PendingHeldDiagnostic {
    step_index: usize,
    sample: i64,
    since: Instant,
    deadline_triggered: bool,
    ready_at_release: bool,
    cue_adopted: bool,
    missing_required: usize,
    first_missing: Option<(usize, usize)>,
    readiness_scan_elapsed: Duration,
    worker: WorkerSnapshot,
    read_count: usize,
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
            let started = Instant::now();
            std::thread::sleep(Duration::from_millis(40));
            let ended = Instant::now();
            let mut stalls = self.stalls.lock().expect("soak stall diagnostics lock");
            if stalls.len() < stalls.capacity() {
                stalls.push(InjectedStall {
                    read: n,
                    started: started.duration_since(self.diagnostic_epoch),
                    ended: ended.duration_since(self.diagnostic_epoch),
                    elapsed: ended.duration_since(started),
                });
            } else {
                self.dropped_stalls.fetch_add(1, Ordering::Relaxed);
            }
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
    let thread = std::thread::Builder::new()
        .name("lumen-soak-audio".into())
        .spawn(run_soak)
        .expect("spawn the soak render thread");
    thread.join().expect("the soak render thread completed");
}

fn run_soak() {
    #[cfg(target_os = "macos")]
    let before_sleep = sleep_probe();
    let qos = lumen_rt::thread_qos::request(lumen_rt::thread_qos::Class::UserInteractive);
    #[cfg(target_os = "macos")]
    eprintln!("lumen-soak render thread QoS: {qos:?}");
    #[cfg(target_os = "macos")]
    assert!(
        qos.established(),
        "the macOS soak render thread must establish UserInteractive QoS: {qos:?}"
    );
    #[cfg(target_os = "macos")]
    eprintln!(
        "lumen-soak fixed sleep probe (32 blocks, total/max overshoot): before {before_sleep:?}, after {:?}",
        sleep_probe()
    );
    #[cfg(not(target_os = "macos"))]
    let _ = qos;

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
    let diagnostic_epoch = Instant::now();
    // Enough for the 120-second run; the atomic counter below makes any
    // truncation explicit if a longer soak exceeds the fixed buffer.
    let stalls = Arc::new(Mutex::new(Vec::with_capacity(4_096)));
    let dropped_stalls = Arc::new(AtomicUsize::new(0));
    let pages = Box::new(Uneven {
        reads: Arc::clone(&reads),
        stalls: Arc::clone(&stalls),
        dropped_stalls: Arc::clone(&dropped_stalls),
        diagnostic_epoch,
    });
    let scene = Arc::new(Scene::new(
        &clips,
        &Timeline::default(),
        RATE,
        Some(looped),
        vec![0, looped.0, 80 * QUARTER],
    ));
    let steps: Vec<Step> = (0..)
        .flat_map(round)
        .take((seconds / 2.0) as usize)
        .collect();
    // Cache each step's exact 50 ms readiness prefix before starting the
    // timed loop. Deadline diagnostics only scan these resident bits.
    let required_by_step: Vec<Vec<plan::Want>> = steps
        .iter()
        .map(|step| {
            let sample = scene.sample_at(step.at());
            let mut wants = Vec::with_capacity(TRACKS as usize * 8);
            let required = scene.plan(
                plan::Position {
                    sample,
                    cue: Some(sample),
                    playing: true,
                    ..Default::default()
                },
                plan::Horizon {
                    near: i64::from(RATE) / 20,
                    far: i64::from(RATE) / 20,
                },
                &mut wants,
            );
            wants.truncate(required);
            wants
        })
        .collect();

    // Diagnostic preparation must not give the live worker extra warmup
    // before the timed cold start.
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.set_scene(Arc::clone(&scene));
    let gate = Gate {
        scene,
        cuer: disk.cuer(),
    };
    let transport = handle.transport_reader();

    let mut data = vec![0.0; 2 * BLOCK];
    let block = Duration::from_secs_f64(BLOCK as f64 / f64::from(RATE));
    let mut held_diagnostics = Vec::with_capacity(steps.len());
    let started = Instant::now();
    let mut played = 0u32;
    let mut held: Option<Held> = None;
    let mut held_step_index: Option<usize> = None;
    let mut next = 0usize;
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
    let mut rendered = 0u32;
    let mut render_total = Duration::ZERO;
    let mut render_max = Duration::ZERO;
    let mut render_over_budget = 0u32;
    let mut sleep_overshoot_total = Duration::ZERO;
    let mut sleep_overshoot_max = Duration::ZERO;
    let mut deadline_releases = 0usize;
    while started.elapsed().as_secs_f64() < seconds {
        // The next step every 2 s, held for the disk.
        if held.is_none()
            && next < steps.len()
            && started.elapsed().as_secs_f64() >= 2.0 * next as f64
        {
            let step_index = next;
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
                let since = Instant::now();
                held_step_index = Some(step_index);
                held = Some(Held {
                    command,
                    sample,
                    since,
                });
            }
        }
        let (release_due, deadline_triggered) = if let Some(waiting) = &held {
            let now = Instant::now();
            (
                waiting.due(Some(&gate), now),
                now.duration_since(waiting.since) >= LONGEST,
            )
        } else {
            (false, false)
        };
        let mut pending_held_diagnostic = None;
        let mut held_send_at = None;
        if release_due {
            let waiting = held.take().expect("checked above");
            let step_index = held_step_index
                .take()
                .expect("a held start has a step index");
            let cue_adopted = gate.cuer.adopted();
            let (missing_required, first_missing, readiness_scan_elapsed) = if deadline_triggered {
                let scan_started = Instant::now();
                let mut first_missing = None;
                let missing = required_by_step[step_index]
                    .iter()
                    .filter(|want| {
                        let resident = gate.scene.streams()[want.stream].resident(want.page);
                        if !resident && first_missing.is_none() {
                            first_missing = Some((want.stream, want.page));
                        }
                        !resident
                    })
                    .count();
                (missing, first_missing, scan_started.elapsed())
            } else {
                (0, None, Duration::ZERO)
            };
            // A ready-based release was triggered by the gate's successful
            // 50 ms check above. At a deadline, derive readiness from the
            // cached page scan and adoption state without replanning.
            let ready_at_release = if deadline_triggered {
                cue_adopted && missing_required == 0
            } else {
                true
            };
            pending_held_diagnostic = Some(PendingHeldDiagnostic {
                step_index,
                sample: waiting.sample,
                since: waiting.since,
                deadline_triggered,
                ready_at_release,
                cue_adopted,
                missing_required,
                first_missing,
                readiness_scan_elapsed,
                worker: disk.stats().worker_snapshot(),
                read_count: reads.load(Ordering::Relaxed),
            });
            deadline_releases += usize::from(deadline_triggered);
            held_send_at = Some(Instant::now());
            handle.send(waiting.command).unwrap();
            gate.cuer.follow_after(handle.sent_command());
            sent = Instant::now();
        }
        let render_started = Instant::now();
        engine.process(RATE, &mut data, 2, BLOCK);
        let render_elapsed = render_started.elapsed();
        if let (Some(pending), Some(released)) =
            (pending_held_diagnostic.take(), held_send_at.take())
        {
            let waited = released.duration_since(pending.since);
            let at_or_after_deadline_at_send = waited >= LONGEST;
            longest_hold = longest_hold.max(waited);
            overdue += usize::from(waited > LONGEST);
            held_diagnostics.push(HeldDiagnostic {
                step_index: pending.step_index,
                sample: pending.sample,
                started: pending.since.duration_since(diagnostic_epoch),
                released: released.duration_since(diagnostic_epoch),
                waited,
                ready_at_release: pending.ready_at_release,
                cue_adopted: pending.cue_adopted,
                deadline_triggered: pending.deadline_triggered,
                at_or_after_deadline_at_send,
                missing_required: pending.missing_required,
                first_missing: pending.first_missing,
                readiness_scan_elapsed: pending.readiness_scan_elapsed,
                worker: pending.worker,
                read_count: pending.read_count,
            });
        }
        rendered += 1;
        render_total += render_elapsed;
        render_max = render_max.max(render_elapsed);
        render_over_budget += u32::from(render_elapsed > block);
        played += 1;
        let now = pool.underruns();
        if now > counted {
            let snapshot = transport.read();
            let mut wants = Vec::new();
            gate.scene.plan(
                plan::Position {
                    sample: snapshot.sample,
                    playing: snapshot.playing,
                    ..Default::default()
                },
                plan::Horizon {
                    near: i64::from(RATE) / 25,
                    far: i64::from(RATE) / 25,
                },
                &mut wants,
            );
            let missing: Vec<_> = wants
                .into_iter()
                .filter_map(|want| {
                    let audio = &gate.scene.streams()[want.stream];
                    (!audio.resident(want.page)).then_some((
                        audio.id(),
                        want.page,
                        want.page * PAGE,
                    ))
                })
                .collect();
            late.push((
                now - counted,
                started.elapsed(),
                sent.elapsed(),
                step,
                snapshot,
                disk.stats().worker_snapshot(),
                gate.cuer.adopted(),
                missing,
            ));
            counted = now;
        }
        // In real time: wait for the block's time to come. A block whose
        // time has passed is missed, as a device's callback is when the
        // machine is busy: a dropout, which the app counts apart from late
        // reads. Processing the missed blocks one after another instead
        // would play faster than real time, which no device does.
        match (block * played).checked_sub(started.elapsed()) {
            Some(wait) => {
                let sleep_started = Instant::now();
                std::thread::sleep(wait);
                let overshoot = sleep_started.elapsed().saturating_sub(wait);
                sleep_overshoot_total += overshoot;
                sleep_overshoot_max = sleep_overshoot_max.max(overshoot);
            }
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
    let stalls = stalls.lock().expect("soak stall diagnostics lock");
    eprintln!(
        "soak: {seconds} s, {next} steps, {} pages read, longest hold {longest_hold:?} \
         ({overdue} past 150 ms), {underruns} late, {missed} callback deadlines missed",
        reads.load(Ordering::Relaxed),
    );
    eprintln!(
        "soak: {rendered} callbacks rendered; render total {render_total:?}, max {render_max:?}, \
         {render_over_budget} over block budget; sleep overshoot total {sleep_overshoot_total:?}, \
         max {sleep_overshoot_max:?}; {deadline_releases} releases at the production hold deadline"
    );
    for (n, at, after, step, transport, worker, adopted, missing) in &late {
        eprintln!("soak: {n} late at {at:?}, {after:?} after {step:?} was sent");
        eprintln!(
            "soak: transport {transport:?}; cue adopted={adopted}; worker {worker:?}; missing (stream, page, source frame) {missing:?}"
        );
    }
    for (index, held) in held_diagnostics.iter().enumerate() {
        eprintln!(
            "soak: held release #{index}: step {} {:?}, sample {}, start {:?}, sent {:?}, waited {:?}, 50ms ready={}, cue adopted={}, deadline_triggered={}, at_or_after_150ms_at_send={}, missing required 50ms pages={}, first missing (stream index, page)={:?}, readiness scan {:?}, reads={}, worker {:?}",
            held.step_index,
            steps[held.step_index],
            held.sample,
            held.started,
            held.released,
            held.waited,
            held.ready_at_release,
            held.cue_adopted,
            held.deadline_triggered,
            held.at_or_after_deadline_at_send,
            held.missing_required,
            held.first_missing,
            held.readiness_scan_elapsed,
            held.read_count,
            held.worker,
        );
    }
    for stall in stalls.iter() {
        let overlaps: Vec<_> = held_diagnostics
            .iter()
            .enumerate()
            .filter_map(|(index, held)| {
                (stall.started <= held.released && held.started <= stall.ended).then_some(index)
            })
            .collect();
        eprintln!(
            "soak: injected 40ms read stall #{}: start {:?}, end {:?}, elapsed {:?}, overlapped held releases {:?}",
            stall.read, stall.started, stall.ended, stall.elapsed, overlaps
        );
    }
    let dropped_stalls = dropped_stalls.load(Ordering::Relaxed);
    if dropped_stalls > 0 {
        eprintln!(
            "soak: injected stall diagnostics truncated: {dropped_stalls} events not retained"
        );
    }
    let _ = (&clips_handle, &routing);
    assert!(heard, "the tracks were heard");
    assert_eq!(failed, 0);
    assert_eq!(underruns, 0, "no read came too late");
}

/// Observes the same sleep primitive before and after the macOS QoS request.
/// Fixed sample counts only; this probe never waits for a favorable result
/// and does not change the soak's timing assertions or 120-second interval.
#[cfg(target_os = "macos")]
fn sleep_probe() -> (Duration, Duration) {
    let period = Duration::from_secs_f64(BLOCK as f64 / f64::from(RATE));
    let (mut total, mut longest) = (Duration::ZERO, Duration::ZERO);
    for _ in 0..32 {
        let started = Instant::now();
        std::thread::sleep(period);
        let overshoot = started.elapsed().saturating_sub(period);
        total += overshoot;
        longest = longest.max(overshoot);
    }
    (total, longest)
}
