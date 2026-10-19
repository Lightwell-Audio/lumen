//! A soak: real host processes running the test plugins are played like the
//! engine plays them, while one or two of them at a time are made to fail
//! (crash, hang, run too slowly, write non-finite samples, be ended from
//! outside, fail to load) and are then brought back. Throughout:
//!
//! - the hosts that are not failing never miss a block because of the ones
//!   that are, and never fail;
//! - a failed host is silent at once and is not started again by itself;
//! - the audio thread's work (`begin` and `finish`) never allocates, even
//!   while a host is failing;
//! - no host process outlives the one that replaced it, and the test process
//!   holds no more memory or handles at the end than after its first pass.
//!
//! `cargo test -p lumen-hosting --test soak` makes one pass through every
//! fault. The long run is `LUMEN_SOAK_SECONDS=3600 cargo test --release -p
//! lumen-hosting --test soak -- --ignored --nocapture`; `LUMEN_SOAK_SEED`
//! repeats a run that found something (the seed is printed).

mod support;

use std::time::{Duration, Instant};

use lumen_core::midi::Event;
use lumen_devices::{Guard, Outcome};
use lumen_hosting::{Happened, Manager, Start};
use support::*;

lumen_rt::install_guarded_allocator!();

const FRAMES: usize = lumen_devices::MOST_FRAMES;
/// The engine's share of a block's period.
const BUDGET: Duration = Duration::from_micros(4_000);
/// Blocks between looks at the world, outside the audio section.
const CHUNK: u32 = 20;
/// Blocks a failing host is played for after it has failed.
const AFTER_FAILURE: u32 = 40;
/// Blocks every survivor is played for before a round ends.
const SURVIVE_BLOCKS: u32 = 200;
/// Most blocks a fault may take to show itself.
const MOST_BLOCKS: u32 = 3000;
/// How long a quiet world is watched for a host starting by itself.
const WATCH: Duration = Duration::from_millis(300);
/// Late blocks a healthy host may have in a hundred (a busy machine is not
/// the hosts' fault).
const LATE_PERCENT: u32 = 5;
/// Blocks a replacement host plays before its late share is counted.
const WARM_UP_BLOCKS: u32 = 30;
const SEATS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    /// The host must be given up on.
    Fails,
    /// It must go on playing, with finite samples.
    Survives,
    /// Either; nothing may go wrong around it.
    Either,
    /// It must not load.
    NoLoad,
}

#[derive(Clone, Debug)]
struct Fault {
    label: String,
    env: String,
    expect: Expect,
    /// End the host from outside after this many blocks.
    kill_after: Option<u32>,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn catalog(rng: &mut Rng) -> Vec<Fault> {
    let at = |rng: &mut Rng| rng.below(60);
    let mut list = Vec::new();
    let mut add = |label: &str, env: String, expect, kill_after| {
        list.push(Fault {
            label: label.into(),
            env,
            expect,
            kill_after,
        })
    };
    add(
        "crash",
        format!("process_crash@{}", at(rng)),
        Expect::Fails,
        None,
    );
    add(
        "hang",
        format!("process_hang@{}", at(rng)),
        Expect::Fails,
        None,
    );
    add("too slow", "process_slow=20000".into(), Expect::Fails, None);
    add(
        "not finite",
        format!("process_nan@{}", at(rng)),
        Expect::Survives,
        None,
    );
    add(
        "errors",
        format!("process_error@{}", at(rng)),
        Expect::Either,
        None,
    );
    add("ended from outside", String::new(), Expect::Fails, Some(30));
    add(
        "factory crash",
        "factory_crash".into(),
        Expect::NoLoad,
        None,
    );
    add("no factory", "factory_null".into(), Expect::NoLoad, None);
    add("factory hang", "factory_hang".into(), Expect::NoLoad, None);
    add("absurd buses", "bad_buses".into(), Expect::Either, None);
    add(
        "absurd parameters",
        "bad_params".into(),
        Expect::Either,
        None,
    );
    list
}

fn start(instrument_: bool, fault: &str) -> Start {
    let mut start = if instrument_ {
        instrument(fault)
    } else {
        effect(fault)
    };
    start.frames = FRAMES as u32;
    // A load that hangs is given up on soon.
    start.deadlines.load = Duration::from_secs(3);
    start.deadlines.configure = Duration::from_secs(3);
    start
}

struct Seat {
    device: u64,
    instrument: bool,
    guard: Option<Guard>,
    pid: u32,
}

#[derive(Default, Clone, Copy)]
struct Tally {
    blocks: u32,
    ready: u32,
    not_finite: u32,
}

struct World {
    manager: Manager,
    seats: Vec<Seat>,
    left: Vec<f32>,
    right: Vec<f32>,
    sine: Vec<f32>,
    patterns: Vec<Vec<Event>>,
    /// Every host process started.
    pids: Vec<u32>,
    violations: u64,
    blocks: u64,
    rounds: u32,
    rng: Rng,
}

impl World {
    fn new(seed: u64) -> Self {
        assert!(lumen_rt::guard_is_active(), "the guarded allocator is off");
        let mut manager = Manager::new(|| {});
        for device in 1..=SEATS as u64 {
            manager
                .want(device, start(device <= 3, ""))
                .expect("start a host");
        }
        let devices: Vec<u64> = (1..=SEATS as u64).collect();
        let linked = linked_all(&mut manager, &devices);
        let mut pids = Vec::new();
        let seats = linked
            .into_iter()
            .zip(&devices)
            .map(|(l, &device)| {
                pids.push(l.pid);
                Seat {
                    device,
                    instrument: device <= 3,
                    pid: l.pid,
                    guard: Some(Guard::new(Box::new(l.link), l.latency)),
                }
            })
            .collect();
        let patterns = (0..8)
            .map(|phase| {
                let key = 48 + phase as u8 * 3;
                match phase {
                    0 => vec![note_on(0, key), note_on(5, key + 7)],
                    5 => vec![note_off(100, 48), note_off(120, 55), note_off(200, 63)],
                    _ => Vec::new(),
                }
            })
            .collect();
        Self {
            manager,
            seats,
            left: vec![0.0; FRAMES],
            right: vec![0.0; FRAMES],
            sine: (0..FRAMES).map(|i| (i as f32 * 0.05).sin() * 0.5).collect(),
            patterns,
            pids,
            violations: 0,
            blocks: 0,
            rounds: 0,
            rng: Rng(seed | 1),
        }
    }

    /// Plays `count` blocks the way the engine does: every host is handed
    /// its block, then each is collected until the shared deadline. The
    /// whole of it is the audio thread's work.
    fn play(&mut self, count: u32) -> Vec<Tally> {
        let mut tally = vec![Tally::default(); self.seats.len()];
        let first = self.blocks as usize;
        let stereo: [&[f32]; 2] = [&self.sine, &self.sine];
        let (seats, left, right, patterns) = (
            &mut self.seats,
            &mut self.left,
            &mut self.right,
            &self.patterns,
        );
        let ((), violations) = lumen_rt::count_rt_violations(|| {
            for block in 0..count as usize {
                let events: &[Event] = &patterns[(first + block) % 8];
                for seat in seats.iter_mut() {
                    if let Some(guard) = seat.guard.as_mut() {
                        if seat.instrument {
                            guard.begin(FRAMES, &[], events);
                        } else {
                            guard.begin(FRAMES, &stereo, &[]);
                        }
                    }
                }
                let deadline = Instant::now() + BUDGET;
                for (seat, tally) in seats.iter_mut().zip(tally.iter_mut()) {
                    let Some(guard) = seat.guard.as_mut() else {
                        continue;
                    };
                    tally.blocks += 1;
                    if guard.finish(deadline, left, right) == Outcome::Ready {
                        tally.ready += 1;
                    }
                    if left.iter().chain(right.iter()).any(|s| !s.is_finite()) {
                        tally.not_finite += 1;
                    }
                }
            }
        });
        self.violations += violations;
        self.blocks += u64::from(count);
        tally
    }

    /// The next thing each of `devices` does: a link (`Ok`) or a failure.
    /// Anything said about another device is a host started by itself.
    fn answers(
        &mut self,
        devices: &[u64],
    ) -> Vec<(u64, Result<Box<lumen_hosting::Linked>, String>)> {
        let mut found = Vec::new();
        let end = Instant::now() + Duration::from_secs(60);
        while found.len() < devices.len() {
            for event in self.manager.poll() {
                assert!(
                    devices.contains(&event.device),
                    "device {} was started again by itself",
                    event.device
                );
                match event.happened {
                    Happened::Linked(l) => found.push((event.device, Ok(l))),
                    Happened::Failed(f) => found.push((event.device, Err(f.to_string()))),
                    _ => {}
                }
            }
            assert!(Instant::now() < end, "devices {devices:?} said nothing");
            std::thread::sleep(Duration::from_millis(5));
        }
        found
    }

    /// Watches for `WATCH` that no host starts or is given up on by itself.
    fn watch_quiet(&mut self, context: &str) {
        let end = Instant::now() + WATCH;
        while Instant::now() < end {
            for event in self.manager.poll() {
                assert!(
                    !matches!(event.happened, Happened::Linked(_)),
                    "{context}: device {} was started again by itself",
                    event.device
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// One pass: `victims` fail as their faults say, the rest play on, then
    /// every victim is replaced by a good host.
    fn round(&mut self, victims: &[(usize, Fault)]) {
        let label: Vec<String> = victims
            .iter()
            .map(|(i, f)| format!("seat {i}: {}", f.label))
            .collect();
        let label = label.join(", ");
        let began = Instant::now();
        self.rounds += 1;

        // The faulty hosts replace the good ones.
        let mut replaced = Vec::new();
        for (i, fault) in victims {
            let seat = &mut self.seats[*i];
            replaced.push(seat.pid);
            seat.guard = None;
            let instrument_ = seat.instrument;
            self.manager
                .want(seat.device, start(instrument_, &fault.env))
                .expect("start a host");
        }
        let devices: Vec<u64> = victims.iter().map(|(i, _)| self.seats[*i].device).collect();
        let mut loaded = vec![false; victims.len()];
        for (device, answer) in self.answers(&devices) {
            let at = victims
                .iter()
                .position(|(i, _)| self.seats[*i].device == device)
                .expect("a victim");
            let (i, fault) = &victims[at];
            match answer {
                Ok(l) => {
                    assert_ne!(fault.expect, Expect::NoLoad, "{label}: it loaded");
                    self.pids.push(l.pid);
                    self.seats[*i].pid = l.pid;
                    self.seats[*i].guard = Some(Guard::new(Box::new(l.link), l.latency));
                    loaded[at] = true;
                }
                Err(_) => {
                    assert!(
                        matches!(fault.expect, Expect::NoLoad | Expect::Either),
                        "{label}: it did not load"
                    );
                }
            }
        }

        // Play on until each loaded fault has shown itself.
        let mut totals = vec![Tally::default(); self.seats.len()];
        let mut killed = vec![false; victims.len()];
        let mut failed_at: Vec<Option<u32>> = vec![None; victims.len()];
        let mut played = 0u32;
        let mut after = 0u32;
        loop {
            for (at, (i, fault)) in victims.iter().enumerate() {
                if let (Some(when), false, true) = (fault.kill_after, killed[at], loaded[at])
                    && played >= when
                {
                    kill_from_outside(self.seats[*i].pid);
                    killed[at] = true;
                }
            }
            let tally = self.play(CHUNK);
            played += CHUNK;
            for (total, now) in totals.iter_mut().zip(&tally) {
                total.blocks += now.blocks;
                total.ready += now.ready;
                total.not_finite += now.not_finite;
            }
            let mut all_done = true;
            for (at, (i, fault)) in victims.iter().enumerate() {
                if !loaded[at] {
                    continue;
                }
                let guard = self.seats[*i].guard.as_ref().expect("a guard");
                match fault.expect {
                    Expect::Fails => {
                        if guard.failed() {
                            // It may have failed part way through this chunk;
                            // from the next chunk on it is silent at once.
                            let already = failed_at[at].is_some();
                            failed_at[at].get_or_insert(played);
                            if already {
                                assert_eq!(tally[*i].ready, 0, "{label}: not silent after failing");
                            }
                        } else {
                            all_done = false;
                        }
                    }
                    Expect::Survives | Expect::Either => all_done &= played >= SURVIVE_BLOCKS,
                    Expect::NoLoad => unreachable!("it loaded"),
                }
            }
            if all_done {
                after += CHUNK;
                if after >= AFTER_FAILURE {
                    break;
                }
            }
            assert!(played < MOST_BLOCKS, "{label}: a fault never took effect");
        }

        // The survivors: playing, finite, and not late because of the rest.
        for (i, seat) in self.seats.iter().enumerate() {
            let victim = victims.iter().find(|(v, _)| *v == i);
            let guard = seat.guard.as_ref();
            if victim.is_none() {
                let tally = totals[i];
                assert!(
                    !guard.expect("a guard").failed(),
                    "{label}: seat {i} was given up on (late {}, tally {}/{} ready, played {played})",
                    guard.expect("a guard").late_blocks(),
                    tally.ready,
                    tally.blocks
                );
                assert!(
                    tally.ready * 100 >= tally.blocks * (100 - LATE_PERCENT),
                    "{label}: seat {i} missed {} of {} blocks",
                    tally.blocks - tally.ready,
                    tally.blocks
                );
            }
            assert_eq!(totals[i].not_finite, 0, "{label}: seat {i} was not finite");
        }
        for (at, (i, fault)) in victims.iter().enumerate() {
            if fault.expect == Expect::Survives && loaded[at] {
                let guard = self.seats[*i].guard.as_ref().expect("a guard");
                assert!(!guard.failed(), "{label}: it was given up on");
            }
        }
        // Nothing is started by itself behind the failure.
        self.watch_quiet(&label);

        // Bring the victims back with good hosts.
        let faulty: Vec<u32> = victims.iter().map(|(i, _)| self.seats[*i].pid).collect();
        for (i, _) in victims {
            let seat = &mut self.seats[*i];
            seat.guard = None;
            let instrument_ = seat.instrument;
            self.manager
                .want(seat.device, start(instrument_, ""))
                .expect("start a host");
        }
        for (device, answer) in self.answers(&devices) {
            let l = answer.unwrap_or_else(|e| panic!("{label}: the good host failed: {e}"));
            let seat = self
                .seats
                .iter_mut()
                .find(|s| s.device == device)
                .expect("a seat");
            self.pids.push(l.pid);
            seat.pid = l.pid;
            seat.guard = Some(Guard::new(Box::new(l.link), l.latency));
        }
        // A host just started pays for its first blocks (pages loaded, caches
        // cold) while the others play on: on a runner with few cores that
        // made a healthy seat late more than the share allows. Those blocks
        // are played, and checked for failure below, but not counted.
        self.play(WARM_UP_BLOCKS);
        let tally = self.play(60);
        for (i, seat) in self.seats.iter().enumerate() {
            assert!(
                !seat.guard.as_ref().expect("a guard").failed(),
                "{label}: seat {i}"
            );
            assert!(
                tally[i].ready * 100 >= tally[i].blocks * (100 - LATE_PERCENT),
                "{label}: seat {i} did not play after the replacement"
            );
        }

        // No host outlives the one that replaced it.
        for pid in replaced.into_iter().chain(faulty) {
            assert!(
                gone_within(pid, Duration::from_secs(10)),
                "{label}: host {pid} is still running"
            );
        }
        let to_fail = failed_at
            .iter()
            .flatten()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join("/");
        println!(
            "round {:>3} [{label}] {:.1} s, blocks to failure {}",
            self.rounds,
            began.elapsed().as_secs_f64(),
            if to_fail.is_empty() { "-" } else { &to_fail }
        );
    }

    /// The end: every host process ever started has gone, the audio thread
    /// never allocated.
    fn finish(mut self, baseline: Option<(u64, u64)>) {
        self.manager.shutdown(Duration::from_secs(10));
        drop(self.seats);
        for pid in &self.pids {
            assert!(
                gone_within(*pid, Duration::from_secs(10)),
                "host {pid} outlived the session: {}",
                describe(*pid)
            );
        }
        assert_eq!(
            self.violations, 0,
            "the audio thread allocated {} times",
            self.violations
        );
        let now = resources();
        println!(
            "{} rounds, {} blocks, {} host processes, none left; memory and handles {baseline:?} -> {now:?}",
            self.rounds,
            self.blocks,
            self.pids.len()
        );
        if let (Some((memory, handles)), Some((memory_now, handles_now))) = (baseline, now) {
            assert!(
                memory_now < memory + 100 * 1024,
                "memory grew from {memory} KB to {memory_now} KB"
            );
            assert!(
                handles_now < handles + 30,
                "handles grew from {handles} to {handles_now}"
            );
        }
    }
}

fn seed() -> u64 {
    std::env::var("LUMEN_SOAK_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
        })
}

#[test]
fn every_fault_in_turn_leaves_the_other_hosts_playing_and_nothing_behind() {
    // A fixed seed: this test is a regression test, not a search.
    let mut world = World::new(0x5eed);
    let mut rng = Rng(0x5eed);
    let faults = catalog(&mut rng);
    let mut baseline = None;
    for (n, fault) in faults.iter().enumerate() {
        world.round(&[(n % SEATS, fault.clone())]);
        if n == 0 {
            // After the first pass the one-off allocations are done.
            baseline = resources();
        }
    }
    // Two at a time, an instrument and an effect among them.
    for pair in [(0, 3, 0, 1), (1, 4, 5, 6), (2, 5, 1, 7)] {
        let (a, b, x, y) = pair;
        world.round(&[
            (a, faults[x].clone()),
            (b, faults[y % faults.len()].clone()),
        ]);
    }
    world.finish(baseline);
}

#[test]
#[ignore = "a long run: set LUMEN_SOAK_SECONDS and run with --ignored"]
fn a_long_soak_of_mixed_faults() {
    let seconds: u64 = std::env::var("LUMEN_SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let seed = seed();
    println!("seed {seed} (LUMEN_SOAK_SEED={seed} repeats this run)");
    let mut world = World::new(seed);
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut baseline = None;
    while Instant::now() < end {
        let mut rng = Rng(world.rng.next() | 1);
        let faults = catalog(&mut rng);
        let a = world.rng.below(SEATS as u64) as usize;
        let mut victims = vec![(a, faults[rng.below(faults.len() as u64) as usize].clone())];
        if rng.below(3) == 0 {
            let b = (a + 1 + rng.below(SEATS as u64 - 1) as usize) % SEATS;
            victims.push((b, faults[rng.below(faults.len() as u64) as usize].clone()));
        }
        world.round(&victims);
        if baseline.is_none() && world.rounds >= faults.len() as u32 {
            baseline = resources();
        }
    }
    world.finish(baseline);
}
