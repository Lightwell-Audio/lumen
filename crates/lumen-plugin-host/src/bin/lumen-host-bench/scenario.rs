//! One scenario: start hosts, attach them, run paced blocks as an audio
//! callback would, then leave them idle and see what they cost.

use std::path::Path;
use std::time::{Duration, Instant};

use lumen_plugin_host::client::{ExitKind, HostProcess};
use lumen_plugin_host::protocol::{Event, EventKind, ParamQueue};
use lumen_plugin_host::shm::{BlockLink, Layout, Outcome, Poll, Wake};

use crate::options::Fault;
use crate::platform::{Contention, Pacer, cpu_time};

const SLOW: Duration = Duration::from_secs(20);
/// The environment variable the test plugin reads its faults from.
const PLUGIN_FAULTS: &str = "LUMEN_TEST_PLUGIN_FAULT";
const PATTERNS: usize = 64;
/// How many silent blocks in a row make a host that is still running "hung".
const MISS_LIMIT: u32 = 3;

pub struct Scenario<'a> {
    pub host_exe: &'a Path,
    pub plugin: &'a Path,
    pub hosts: usize,
    pub frames: u32,
    pub rate: f64,
    pub wake: Wake,
    pub load_us: u64,
    pub contention: usize,
    pub seconds: f64,
    pub warmup: usize,
    pub idle: Duration,
    pub cold: Duration,
    pub deadline: f64,
    pub fault: Option<Fault>,
}

#[derive(Default)]
pub struct Report {
    /// Per block, nanoseconds: submit to the last answer, copies included.
    pub block_ns: Vec<u32>,
    /// The first blocks after a cold start, apart.
    pub cold_ns: Vec<u32>,
    pub overruns: usize,
    pub misses: Vec<u32>,
    pub busy: Vec<u32>,
    pub stale: Vec<u32>,
    pub failed: Vec<u32>,
    pub divergent: u32,
    pub heap_ops: u64,
    /// Percent of one core, per host, while idle.
    pub idle_cpu: Vec<f64>,
    pub detected_after: Option<usize>,
    pub verdict: Option<String>,
    pub restart_ms: Option<f64>,
    pub mmcss: bool,
    pub notes: Vec<String>,
}

struct Running {
    process: HostProcess,
    link: BlockLink,
}

fn start(s: &Scenario<'_>, index: usize) -> Result<Running, String> {
    let mut env: Vec<(&str, String)> = Vec::new();
    let mut faults = Vec::new();
    if s.load_us > 0 {
        faults.push(format!("process_slow={}", s.load_us));
    }
    if index == 0
        && let Some(fault) = s.fault
    {
        // Calls count from activation, so the warm-up is counted too.
        let at = s.warmup + 200;
        faults.push(match fault {
            Fault::Crash => format!("process_crash@{at}"),
            Fault::Hang => format!("process_hang@{at}"),
        });
    }
    if !faults.is_empty() {
        env.push((PLUGIN_FAULTS, faults.join(",")));
    }
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut process =
        HostProcess::spawn(s.host_exe, &env_refs).map_err(|e| format!("start the host: {e}"))?;
    process
        .load(s.plugin, None, SLOW)
        .map_err(|e| format!("load the plugin: {e}"))?;
    process
        .configure(s.rate, s.frames, 0, 2, SLOW)
        .map_err(|e| format!("configure: {e}"))?;
    let link = process
        .attach(Layout::new(s.frames, 0, 2), s.wake, SLOW)
        .map_err(|e| format!("attach: {e}"))?;
    Ok(Running { process, link })
}

fn patterns() -> Vec<Vec<Event>> {
    (0..PATTERNS)
        .map(|n| {
            let pitch = 48 + (n % 24) as i16;
            vec![
                Event {
                    offset: 0,
                    kind: EventKind::NoteOn {
                        channel: 0,
                        pitch,
                        velocity: 0.8,
                    },
                },
                Event {
                    offset: 8,
                    kind: EventKind::NoteOff {
                        channel: 0,
                        pitch,
                        velocity: 0.0,
                    },
                },
            ]
        })
        .collect()
}

pub fn run(s: &Scenario<'_>) -> Result<Report, String> {
    let mut running = (0..s.hosts)
        .map(|i| start(s, i))
        .collect::<Result<Vec<_>, _>>()?;
    let generation = running[0].process.generation();
    let _busy = Contention::start(s.contention);

    let period = Duration::from_secs_f64(f64::from(s.frames) / s.rate);
    let deadline_after = period.mul_f64(s.deadline);
    let measured = (s.seconds / period.as_secs_f64()) as usize;
    let total = s.warmup + measured;
    let n = s.hosts;
    let frames = s.frames as usize;
    let events = patterns();
    let params: Vec<ParamQueue> = Vec::new();

    let mut report = Report {
        block_ns: Vec::with_capacity(measured),
        cold_ns: Vec::with_capacity(500),
        misses: vec![0; n],
        busy: vec![0; n],
        stale: vec![0; n],
        failed: vec![0; n],
        ..Report::default()
    };
    let mut outputs = vec![vec![0.0_f32; frames]; n];
    let mut reference = vec![0.0_f32; frames];
    let mut submitted = vec![false; n];
    let mut dead = vec![false; n];
    let mut consecutive = vec![0_u32; n];
    let pacer = Pacer::new();
    report.mmcss = lumen_plugin_host::shm::audio_thread_priority();

    if !s.cold.is_zero() {
        std::thread::sleep(s.cold);
    }
    let heap_before = lumen_rt::total_violations();
    let mut next = Instant::now() + period;
    for block in 0..total {
        pacer.sleep_until(next);
        next += period;
        let recording = block >= s.warmup;
        let pattern = &events[block % PATTERNS];

        lumen_rt::rt_section(|| {
            let started = Instant::now();
            let deadline = started + deadline_after;
            for (i, r) in running.iter_mut().enumerate() {
                submitted[i] = false;
                if dead[i] {
                    continue;
                }
                match r.link.submit(generation, s.frames, &[], pattern, &params) {
                    Ok(_) => submitted[i] = true,
                    Err(_) => {
                        // Still on an earlier block: late, and the same
                        // evidence of a stuck host as a missed deadline.
                        if recording {
                            report.busy[i] += 1;
                        }
                        late(&mut report, &mut consecutive, &mut dead, r, i, block, s);
                    }
                }
            }
            let mut have_reference = false;
            for (i, r) in running.iter_mut().enumerate() {
                if !submitted[i] {
                    continue;
                }
                match r.link.wait(deadline) {
                    Poll::Done(answer) if answer.outcome == Outcome::Done => {
                        consecutive[i] = 0;
                        r.link.copy_output(0, &mut outputs[i]);
                        if !have_reference {
                            reference.copy_from_slice(&outputs[i]);
                            have_reference = true;
                        } else if recording && outputs[i] != reference {
                            report.divergent += 1;
                        }
                    }
                    Poll::Done(_) => {
                        if recording {
                            report.failed[i] += 1;
                        }
                    }
                    Poll::Pending | Poll::Idle => {
                        if recording {
                            report.misses[i] += 1;
                        }
                        late(&mut report, &mut consecutive, &mut dead, r, i, block, s);
                    }
                }
            }
            let elapsed = started.elapsed().as_nanos() as u32;
            if recording {
                report.block_ns.push(elapsed);
                if Duration::from_nanos(u64::from(elapsed)) > period {
                    report.overruns += 1;
                }
            }
            if !s.cold.is_zero() && recording && report.cold_ns.len() < 500 {
                report.cold_ns.push(elapsed);
            }
        });
    }
    report.heap_ops = lumen_rt::total_violations() - heap_before;
    for (i, r) in running.iter_mut().enumerate() {
        report.stale[i] = r.link.stale();
    }
    if report.detected_after.is_some() {
        // The process takes a moment to be reaped after a crash; ask now,
        // outside the loop, so waiting costs the other hosts nothing.
        report.verdict = Some(
            match running[0].process.wait_ended(Duration::from_millis(500)) {
                Some(how) => format!("the host ended: {how:?}"),
                None => "the host is still running after its missed deadlines: it would be killed"
                    .into(),
            },
        );
    }

    // Idle: attached, no blocks. What the hosts cost doing nothing.
    if !s.idle.is_zero() {
        let before: Vec<Option<Duration>> =
            running.iter().map(|r| cpu_time(r.process.id())).collect();
        let at = Instant::now();
        std::thread::sleep(s.idle);
        let wall = at.elapsed().as_secs_f64();
        for (r, b) in running.iter().zip(before) {
            if let (Some(b), Some(a)) = (b, cpu_time(r.process.id())) {
                report
                    .idle_cpu
                    .push((a.saturating_sub(b)).as_secs_f64() / wall * 100.0);
            }
        }
        if report.idle_cpu.is_empty() {
            report
                .notes
                .push("idle CPU is not measured on this system".into());
        }
    }

    // How long a replacement takes, for the fault scenarios: the same steps
    // from nothing to a host answering blocks.
    if s.fault.is_some() {
        let began = Instant::now();
        let mut fresh = Scenario {
            fault: None,
            ..clone_for_restart(s)
        };
        fresh.hosts = 1;
        let mut replacement = start(&fresh, 1)?;
        replacement
            .link
            .submit(generation, s.frames, &[], &events[0], &params)
            .map_err(|e| e.to_string())?;
        let ok = matches!(
            replacement.link.wait(Instant::now() + SLOW),
            Poll::Done(a) if a.outcome == Outcome::Done
        );
        if ok {
            report.restart_ms = Some(began.elapsed().as_secs_f64() * 1e3);
        }
        replacement.process.quit(SLOW);
    }

    let mut ends = Vec::new();
    for r in &mut running {
        let how = r.process.quit(Duration::from_secs(2));
        if how != ExitKind::Clean {
            ends.push(format!("{how:?}"));
        }
    }
    if s.fault.is_none() && !ends.is_empty() {
        report
            .notes
            .push(format!("hosts did not end cleanly: {ends:?}"));
    }
    Ok(report)
}

/// A host did not answer a block in time. After a few in a row, or once the
/// process is gone, it is out of the loop. The process is looked at only
/// after a miss, never on the fast path.
fn late(
    report: &mut Report,
    consecutive: &mut [u32],
    dead: &mut [bool],
    r: &mut Running,
    i: usize,
    block: usize,
    s: &Scenario<'_>,
) {
    consecutive[i] += 1;
    if dead[i] || (consecutive[i] < MISS_LIMIT && !r.process.has_ended()) {
        return;
    }
    dead[i] = true;
    if report.detected_after.is_none() && i == 0 && s.fault.is_some() {
        report.detected_after = Some(block + 1 - (s.warmup + 200));
    }
}

fn clone_for_restart<'a>(s: &Scenario<'a>) -> Scenario<'a> {
    Scenario {
        host_exe: s.host_exe,
        plugin: s.plugin,
        hosts: s.hosts,
        frames: s.frames,
        rate: s.rate,
        wake: s.wake,
        load_us: s.load_us,
        contention: 0,
        seconds: s.seconds,
        warmup: s.warmup,
        idle: Duration::ZERO,
        cold: Duration::ZERO,
        deadline: s.deadline,
        fault: None,
    }
}
