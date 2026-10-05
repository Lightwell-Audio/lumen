//! Measured scenarios: transport latency, fan-out, crash and hang survival.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::ipc::{
    CHANNELS, CMD_CRASH, CMD_HANG, Host, Job, Timer, pro_audio_priority, wait_either,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Crash,
    Hang,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub hosts: usize,
    pub spin: bool,
    pub frames: usize,
    pub rate: f64,
    pub seconds: f64,
    /// Share of the period the parent waits for hosts before using silence.
    pub deadline_fraction: f64,
    /// Inject into host 0 at this block.
    pub fault: Option<(Fault, usize)>,
}

/// Consecutive missed deadlines before a live host is declared hung.
const MISS_LIMIT: u32 = 3;

struct Supervisor {
    requests: rtrb::Producer<Box<Host>>,
    replacements: rtrb::Consumer<Box<Host>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<String>>>,
}

/// Restarts failed hosts off the audio thread. Receives dead hosts, kills
/// and frees them, spawns replacements.
fn supervisor(job: Arc<Job>, spin: bool) -> Supervisor {
    let (requests, mut incoming) = rtrb::RingBuffer::<Box<Host>>::new(16);
    let (mut outgoing, replacements) = rtrb::RingBuffer::<Box<Host>>::new(16);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let mut log = Vec::new();
        while !stop_flag.load(Ordering::Acquire) {
            while let Ok(mut old) = incoming.pop() {
                let verdict = match old.exit_code() {
                    Some(code) => format!("crashed (exit code 0x{:08X})", code as u32),
                    None => "hung (still running; killed)".to_string(),
                };
                let (id, generation) = (old.id, old.generation);
                old.kill();
                drop(old);
                let started = Instant::now();
                match Host::spawn(id, generation + 1, spin, &job) {
                    Ok(host) => {
                        log.push(format!(
                            "host {id}: {verdict}; replacement ready in {:.1} ms",
                            started.elapsed().as_secs_f64() * 1e3
                        ));
                        let _ = outgoing.push(Box::new(host));
                    }
                    Err(e) => log.push(format!("host {id}: {verdict}; restart failed: {e}")),
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        log
    });
    Supervisor {
        requests,
        replacements,
        stop,
        thread: Some(thread),
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub blocks: usize,
    pub round_trip_ns: Vec<u32>,
    pub iteration_ns: Vec<u32>,
    pub overruns: usize,
    pub deadline_misses: Vec<usize>,
    pub silent_blocks: Vec<usize>,
    pub mismatches: Vec<usize>,
    pub fault_detected_after_blocks: Option<usize>,
    pub recovered_after_blocks: Option<usize>,
    pub supervisor_log: Vec<String>,
    pub heap_ops: u64,
}

pub fn run(config: &Config) -> std::io::Result<Report> {
    let job = Arc::new(Job::new()?);
    let mut hosts: Vec<Option<Box<Host>>> = (0..config.hosts)
        .map(|id| Host::spawn(id, 0, config.spin, &job).map(|h| Some(Box::new(h))))
        .collect::<Result<_, _>>()?;
    let mut sup = supervisor(Arc::clone(&job), config.spin);

    let period = Duration::from_secs_f64(config.frames as f64 / config.rate);
    let deadline = period.mul_f64(config.deadline_fraction);
    let blocks = (config.seconds / period.as_secs_f64()) as usize;
    let samples = config.frames * CHANNELS;
    let mut report = Report {
        blocks,
        round_trip_ns: Vec::with_capacity(blocks),
        iteration_ns: Vec::with_capacity(blocks),
        deadline_misses: vec![0; config.hosts],
        silent_blocks: vec![0; config.hosts],
        mismatches: vec![0; config.hosts],
        ..Report::default()
    };
    let mut consecutive_misses = vec![0_u32; config.hosts];
    let mut input = vec![0.0_f32; samples];
    let mut output = vec![0.0_f32; samples];
    let pacing = Timer::new()?;
    let deadline_timer = Timer::new()?;
    let mmcss = pro_audio_priority();
    let heap_before = lumen_rt::total_violations();

    let mut next = Instant::now() + period;
    for block in 0..blocks {
        let now = Instant::now();
        if next > now {
            pacing.arm(next - now);
            pacing.wait();
        }
        next += period;
        let seq = block as u64 + 1;

        lumen_rt::rt_section(|| {
            let started = Instant::now();
            while let Ok(host) = sup.replacements.pop() {
                let id = host.id;
                hosts[id] = Some(host);
                consecutive_misses[id] = 0;
                if report.recovered_after_blocks.is_none()
                    && let Some((_, at)) = config.fault
                {
                    report.recovered_after_blocks = Some(block - at);
                }
            }
            if let Some((fault, at)) = config.fault
                && block == at
                && let Some(host) = &hosts[0]
            {
                host.inject(if fault == Fault::Crash {
                    CMD_CRASH
                } else {
                    CMD_HANG
                });
            }

            for (i, sample) in input.iter_mut().enumerate() {
                *sample = ((block * 31 + i) % 997) as f32 / 997.0 - 0.5;
            }
            for host in hosts.iter().flatten() {
                host.submit(seq, &input, config.frames, !config.spin);
            }
            deadline_timer.arm(deadline);

            for (id, slot) in hosts.iter_mut().enumerate() {
                let Some(host) = slot else {
                    report.silent_blocks[id] += 1;
                    continue;
                };
                let done = if config.spin {
                    loop {
                        if host.is_done(seq) {
                            break true;
                        }
                        if started.elapsed() >= deadline {
                            break host.is_done(seq);
                        }
                        std::hint::spin_loop();
                    }
                } else {
                    loop {
                        if host.is_done(seq) {
                            break true;
                        }
                        // A stale wake-up from a late earlier block just loops.
                        if !wait_either(host.done_handle(), deadline_timer.handle()) {
                            break host.is_done(seq);
                        }
                    }
                };
                if done {
                    consecutive_misses[id] = 0;
                    host.read_output(seq, &mut output);
                    let correct = input
                        .iter()
                        .zip(&output)
                        .all(|(x, y)| (x * 0.5).to_bits() == y.to_bits());
                    if !correct {
                        report.mismatches[id] += 1;
                    }
                } else {
                    // Silence for this host this block; the audio loop moves on.
                    report.deadline_misses[id] += 1;
                    report.silent_blocks[id] += 1;
                    consecutive_misses[id] += 1;
                    let exited = host.exit_code().is_some();
                    if exited || consecutive_misses[id] >= MISS_LIMIT {
                        if report.fault_detected_after_blocks.is_none()
                            && let Some((_, at)) = config.fault
                        {
                            report.fault_detected_after_blocks = Some(block - at);
                        }
                        if let Some(dead) = slot.take()
                            && let Err(rtrb::PushError::Full(dead)) = sup.requests.push(dead)
                        {
                            *slot = Some(dead); // retry next block
                        }
                    }
                }
            }
            let elapsed = started.elapsed();
            report.round_trip_ns.push(elapsed.as_nanos() as u32);
            report.iteration_ns.push(elapsed.as_nanos() as u32);
            if elapsed > period {
                report.overruns += 1;
            }
        });
    }
    report.heap_ops = lumen_rt::total_violations() - heap_before;
    if !mmcss {
        report
            .supervisor_log
            .push("warning: MMCSS Pro Audio unavailable".into());
    }

    sup.stop.store(true, Ordering::Release);
    if let Some(thread) = sup.thread.take() {
        report
            .supervisor_log
            .extend(thread.join().unwrap_or_default());
    }
    drop(hosts);
    Ok(report)
}

pub fn print(title: &str, config: &Config, report: &Report) {
    let period_us = config.frames as f64 / config.rate * 1e6;
    println!("\n== {title}");
    println!(
        "   {} host(s), {} mode, {} frames @ {} Hz (period {:.0} µs, deadline {:.0} µs), {} blocks",
        config.hosts,
        if config.spin { "spin" } else { "event" },
        config.frames,
        config.rate,
        period_us,
        period_us * config.deadline_fraction,
        report.blocks
    );
    let mut sorted = report.round_trip_ns.clone();
    sorted.sort_unstable();
    if !sorted.is_empty() {
        let at = |p: f64| f64::from(sorted[((sorted.len() - 1) as f64 * p) as usize]) / 1e3;
        println!(
            "   round trip (submit -> all done): p50 {:.1} / p99 {:.1} / p99.9 {:.1} / max {:.1} µs",
            at(0.5),
            at(0.99),
            at(0.999),
            at(1.0)
        );
    }
    println!(
        "   deadline misses per host: {:?}; silent blocks: {:?}; output mismatches: {:?}; loop overruns: {}",
        report.deadline_misses, report.silent_blocks, report.mismatches, report.overruns
    );
    if let Some(after) = report.fault_detected_after_blocks {
        println!(
            "   fault detected {after} block(s) after injection ({:.1} ms)",
            after as f64 * period_us / 1e3
        );
    }
    if let Some(after) = report.recovered_after_blocks {
        println!(
            "   host back in service {after} blocks after injection ({:.1} ms)",
            after as f64 * period_us / 1e3
        );
    }
    for line in &report.supervisor_log {
        println!("   supervisor: {line}");
    }
    println!(
        "   real-time heap operations in the audio loop: {}",
        report.heap_ops
    );
}
