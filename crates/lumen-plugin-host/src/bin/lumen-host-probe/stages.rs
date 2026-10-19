//! The stages, run in order against one class in a host process of its own.
//! A stage that the host does not survive ends the run: the rest are
//! skipped, and the report says where it ended.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lumen_plugin_host::client::{ExitKind, HostError, HostLog, HostProcess};
use lumen_plugin_host::protocol::{Aux, BusInfo, Event, EventKind, FailureKind};
use lumen_plugin_host::scan::report::ScanClass;
use lumen_plugin_host::shm::{Layout, Poll, Wake};

use crate::report::{Found, Kind, Stage, Verdict};

/// What the stages are run with.
pub struct Setup {
    pub host: PathBuf,
    pub rate: f64,
    pub frames: u32,
    /// How long any one call may take before the host is ended.
    pub deadline: Duration,
    /// Blocks rendered by the process and link stages.
    pub blocks: u32,
    /// Open the plugin's window.
    pub editor: bool,
}

/// The pitch an instrument is played at.
const PITCH: i16 = 69;
/// Blocks of silence before the note or the signal.
const LEAD: u32 = 4;
/// The input given to an effect, in amplitude.
const SIGNAL: f32 = 0.25;
/// A block that takes longer than this share of its period is worth saying.
const BUDGET_SHARE: f64 = 0.75;
/// Under this a plugin's output is silent.
const SILENT: f32 = 1e-6;

type Outcome = Result<(Verdict, String), HostError>;

struct Run {
    stages: Vec<Stage>,
    /// Why the stages after this one do not run.
    stopped: Option<String>,
    log: Option<std::sync::Arc<HostLog>>,
}

impl Run {
    /// Runs `f` as the stage `name`, unless an earlier one ended the run.
    /// `ends` makes a failure end it too (a plugin that did not load has
    /// nothing to configure).
    fn stage(&mut self, name: &'static str, ends: bool, f: impl FnOnce() -> Outcome) {
        if let Some(why) = &self.stopped {
            self.skip(name, why.clone());
            return;
        }
        let start = Instant::now();
        let (verdict, mut detail) = f().unwrap_or_else(classify);
        if verdict == Verdict::Crashed
            && let Some(at) = self.log.as_ref().and_then(|l| l.crash())
        {
            detail = format!("{detail}; {at}");
        }
        if matches!(verdict, Verdict::Crashed | Verdict::TimedOut)
            || (ends && verdict == Verdict::Fail)
        {
            self.stopped = Some(format!("{name} {}", verdict.word()));
        }
        self.stages.push(Stage {
            name,
            verdict,
            detail,
            ms: ms_since(start),
        });
    }

    fn skip(&mut self, name: &'static str, why: String) {
        self.stages.push(Stage {
            name,
            verdict: Verdict::Skipped,
            detail: why,
            ms: 0,
        });
    }
}

fn ms_since(start: Instant) -> u32 {
    u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX)
}

/// A host error as a verdict: how the call failed says which.
fn classify(error: HostError) -> (Verdict, String) {
    match error {
        HostError::Timeout => (
            Verdict::TimedOut,
            "no answer by the deadline; the host was ended".into(),
        ),
        HostError::Ended(how) => (Verdict::Crashed, format!("the host ended: {how}")),
        other => (Verdict::Fail, other.to_string()),
    }
}

fn describe(buses: &[BusInfo]) -> String {
    if buses.is_empty() {
        return "none".into();
    }
    buses
        .iter()
        .map(|b| {
            format!(
                "{} {}ch{}{}",
                b.name,
                b.channels,
                if b.main { "" } else { " aux" },
                if b.default_active { "" } else { " off" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn db(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

/// The `share` quantile of `values` (sorted in place), in the same unit.
fn quantile(values: &mut [u32], share: f64) -> u32 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let at = ((values.len() - 1) as f64 * share).round() as usize;
    values[at]
}

fn timing(micros: &mut [u32]) -> String {
    format!(
        "median {} µs, p99 {} µs, max {} µs",
        quantile(micros, 0.5),
        quantile(micros, 0.99),
        quantile(micros, 1.0)
    )
}

/// What every block is given: an instrument a note, an effect a signal, for
/// the first half of the blocks and nothing after.
struct Score {
    kind: Kind,
    frames: u32,
    blocks: u32,
    inputs: usize,
    phase: f64,
    step: f64,
}

impl Score {
    fn new(setup: &Setup, kind: Kind, inputs: u8) -> Self {
        Self {
            kind,
            frames: setup.frames,
            blocks: setup.blocks.max(LEAD * 4),
            inputs: usize::from(inputs),
            phase: 0.0,
            step: std::f64::consts::TAU * 440.0 / setup.rate,
        }
    }

    fn sounding(&self, block: u32) -> bool {
        (LEAD..self.blocks / 2).contains(&block)
    }

    fn events(&self, block: u32) -> Vec<Event> {
        if self.kind != Kind::Instrument {
            return Vec::new();
        }
        let kind = if block == LEAD {
            EventKind::NoteOn {
                channel: 0,
                pitch: PITCH,
                velocity: 0.8,
            }
        } else if block == self.blocks / 2 {
            EventKind::NoteOff {
                channel: 0,
                pitch: PITCH,
                velocity: 0.0,
            }
        } else {
            return Vec::new();
        };
        vec![Event { offset: 0, kind }]
    }

    /// The input for `block`, one slice per channel.
    fn input(&mut self, block: u32) -> Vec<Vec<f32>> {
        let frames = self.frames as usize;
        let live = self.kind == Kind::Effect && self.sounding(block);
        let mut channel = vec![0.0f32; frames];
        if live {
            for sample in &mut channel {
                *sample = SIGNAL * self.phase.sin() as f32;
                self.phase += self.step;
            }
        }
        vec![channel; self.inputs]
    }
}

/// Runs the stages after the scan against `class` of `module`.
pub fn probe_class(setup: &Setup, module: &Path, class: &ScanClass, kind: Kind) -> Vec<Stage> {
    let mut run = Run {
        stages: Vec::new(),
        stopped: None,
        log: None,
    };
    let within = setup.deadline;
    let mut host = match HostProcess::spawn(&setup.host, &[]) {
        Ok(host) => host,
        Err(e) => {
            run.stage("load", true, || {
                Ok((Verdict::Fail, format!("the host could not start: {e}")))
            });
            for &name in &Found::STAGES[2..] {
                run.skip(name, "the host did not start".into());
            }
            return run.stages;
        }
    };
    run.log = Some(host.log());

    run.stage("load", true, || {
        let info = host.load(module, Some(class.id), within)?;
        Ok((
            Verdict::Pass,
            format!("{} | {}", info.category, info.subcategories),
        ))
    });

    let mut buses: Option<(Vec<BusInfo>, Vec<BusInfo>)> = None;
    run.stage("buses", false, || {
        let (inputs, outputs) = host.buses(within)?;
        let text = format!("in: {}; out: {}", describe(&inputs), describe(&outputs));
        buses = Some((inputs, outputs));
        Ok((Verdict::Pass, text))
    });

    // The main bus of each direction; a plugin without buses is given the
    // usual stereo pair.
    let main = |list: &[BusInfo], fallback: u8| {
        list.iter()
            .find(|b| b.main)
            .map_or(fallback, |b| b.channels)
    };
    let (want_in, want_out) = match &buses {
        Some((i, o)) => (main(i, 0), main(o, 2)),
        None => (if kind == Kind::Instrument { 0 } else { 2 }, 2),
    };
    let mut layout = None;
    run.stage("configure", true, || {
        let done = host.configure_buses(
            setup.rate,
            setup.frames,
            (want_in, want_out),
            Aux::default(),
            within,
        )?;
        layout = Some(done);
        Ok((
            Verdict::Pass,
            format!(
                "{} Hz, {} frames: latency {} samples ({:.1} ms), tail {}, a block carries {} in and {} out",
                setup.rate,
                setup.frames,
                done.latency,
                f64::from(done.latency) * 1000.0 / setup.rate,
                done.tail,
                done.inputs,
                done.outputs
            ),
        ))
    });
    let (inputs, outputs) = layout.map_or((want_in, want_out), |c| (c.inputs, c.outputs));

    run.stage("process", false, || {
        process(&mut host, setup, kind, inputs, outputs)
    });
    run.stage("link", false, || {
        link(&mut host, setup, kind, inputs, outputs)
    });
    run.stage("state", false, || {
        let first = host.state(within)?;
        host.set_state(first.clone(), within)?;
        let second = host.state(within)?;
        if first == second {
            Ok((
                Verdict::Pass,
                format!("{} bytes, read back identical", first.len()),
            ))
        } else {
            Ok((
                Verdict::Note,
                format!(
                    "{} bytes saved, {} after restoring them: the plugin writes something that changes",
                    first.len(),
                    second.len()
                ),
            ))
        }
    });
    run.stage("parameters", false, || {
        parameters(&mut host, setup, inputs, within)
    });
    run.stage("editor", false, || editor(&mut host, setup));
    run.stage("shutdown", false, || {
        Ok(match host.quit(within) {
            ExitKind::Clean => (Verdict::Pass, "it quit when asked".into()),
            ExitKind::Killed => (
                Verdict::TimedOut,
                "it did not quit when asked and was ended".into(),
            ),
            ExitKind::Crashed(how) => (Verdict::Crashed, format!("it crashed on quitting: {how}")),
            ExitKind::Failed(code) => (
                Verdict::Fail,
                format!("it quit with the failure code {code}"),
            ),
        })
    });
    run.stages
}

/// Blocks over the pipe: finite output, a status of zero, and how loud.
fn process(host: &mut HostProcess, setup: &Setup, kind: Kind, inputs: u8, outputs: u8) -> Outcome {
    let mut score = Score::new(setup, kind, inputs);
    let mut micros = Vec::new();
    let (mut peak, mut loud_blocks, mut repaired) = (0.0f32, 0u32, 0u32);
    let mut restarts = 0u32;
    for block in 0..score.blocks {
        let input = score.input(block);
        let events = score.events(block);
        let began = Instant::now();
        let got = host.process(setup.frames, input, events, Vec::new(), setup.deadline)?;
        micros.push(u32::try_from(began.elapsed().as_micros()).unwrap_or(u32::MAX));
        if got.status != 0 {
            return Ok((
                Verdict::Fail,
                format!("block {block} returned status {}", got.status),
            ));
        }
        if got.output.len() != usize::from(outputs) {
            return Ok((
                Verdict::Fail,
                format!(
                    "block {block} had {} output channels, not {outputs}",
                    got.output.len()
                ),
            ));
        }
        repaired += got.repaired;
        restarts += u32::from(got.restart != 0);
        let level = got.output.iter().flatten().fold(0.0f32, |m, &s| {
            if s.is_finite() { m.max(s.abs()) } else { m }
        });
        peak = peak.max(level);
        loud_blocks += u32::from(level > SILENT);
    }
    let note = timing(&mut micros);
    if repaired > 0 {
        return Ok((
            Verdict::Fail,
            format!("{repaired} samples were not finite and were replaced with silence"),
        ));
    }
    let mut text = format!(
        "peak {:.1} dBFS over {loud_blocks} of {} blocks; {note} (pipe path)",
        db(peak),
        score.blocks
    );
    if restarts > 0 {
        text.push_str(&format!(
            "; the plugin asked to be set up again {restarts}x"
        ));
    }
    if peak <= SILENT {
        return Ok((
            Verdict::Note,
            format!(
                "silent: no sound from the {}; {note} (pipe path)",
                if kind == Kind::Instrument {
                    "note (a patch may need loading)"
                } else {
                    "signal"
                }
            ),
        ));
    }
    Ok((Verdict::Pass, text))
}

/// The same blocks over the shared-memory link, which is what Lumen plays
/// through, timed against the period of a block.
fn link(host: &mut HostProcess, setup: &Setup, kind: Kind, inputs: u8, outputs: u8) -> Outcome {
    let mut link = host.attach(
        Layout::new(setup.frames, inputs, outputs),
        Wake::EVENT,
        setup.deadline,
    )?;
    let generation = host.generation();
    let period = Duration::from_secs_f64(f64::from(setup.frames) / setup.rate);
    let budget = period.mul_f64(BUDGET_SHARE);
    let mut score = Score::new(setup, kind, inputs);
    let (mut round, mut work) = (Vec::new(), Vec::new());
    let (mut over_budget, mut over_period) = (0u32, 0u32);
    let mut scratch = vec![0.0f32; setup.frames as usize];
    for block in 0..score.blocks {
        let input = score.input(block);
        let slices: Vec<&[f32]> = input.iter().map(Vec::as_slice).collect();
        let events = score.events(block);
        let began = Instant::now();
        link.submit(generation, setup.frames, &slices, &events, &[])
            .map_err(|e| HostError::Protocol(format!("the link refused a block: {e:?}")))?;
        let answer = match link.wait(began + setup.deadline) {
            Poll::Done(answer) => answer,
            _ => return Err(HostError::Timeout),
        };
        let took = began.elapsed();
        round.push(u32::try_from(took.as_micros()).unwrap_or(u32::MAX));
        work.push(answer.work_ns / 1000);
        over_budget += u32::from(took > budget);
        over_period += u32::from(took > period);
        if answer.outcome != lumen_plugin_host::shm::Outcome::Done || answer.status != 0 {
            return Ok((
                Verdict::Fail,
                format!(
                    "block {block}: {:?}, status {}",
                    answer.outcome, answer.status
                ),
            ));
        }
        let mut replaced = 0;
        for channel in 0..usize::from(outputs) {
            replaced += link.copy_output(channel, &mut scratch);
        }
        if replaced > 0 {
            return Ok((
                Verdict::Fail,
                format!("block {block}: {replaced} samples were not finite"),
            ));
        }
    }
    let text = format!(
        "{} blocks of {} frames, period {} µs: round trip {}; the plugin's own work {}; \
         {over_budget} over {:.0}% of the period, {over_period} over the period",
        score.blocks,
        setup.frames,
        period.as_micros(),
        timing(&mut round),
        timing(&mut work),
        BUDGET_SHARE * 100.0
    );
    let verdict = if over_budget == 0 {
        Verdict::Pass
    } else {
        Verdict::Note
    };
    Ok((verdict, text))
}

/// The plugin's parameters: listed, one set and read back, and a block
/// rendered after.
fn parameters(host: &mut HostProcess, setup: &Setup, inputs: u8, within: Duration) -> Outcome {
    let (total, list) = host.parameters(within)?;
    let programs = host.programs(within).map(|p| p.len()).unwrap_or(0);
    let Some(target) = list.iter().find(|p| !p.read_only() && p.can_automate()) else {
        return Ok((
            Verdict::Pass,
            format!(
                "{total} parameters, {} listed, none to set; {programs} program lists",
                list.len()
            ),
        ));
    };
    let value = if target.value < 0.5 { 0.75 } else { 0.25 };
    host.set_param(target.id, value, within)?;
    let silence = vec![vec![0.0f32; setup.frames as usize]; usize::from(inputs)];
    let block = host.process(setup.frames, silence, Vec::new(), Vec::new(), within)?;
    let (_, after) = host.parameters(within)?;
    let read = after.iter().find(|p| p.id == target.id).map(|p| p.value);
    let head = format!(
        "{total} parameters, {} listed; {programs} program lists; set `{}` to {value}",
        list.len(),
        target.title
    );
    Ok(match read {
        _ if block.status != 0 => (
            Verdict::Fail,
            format!("{head}; the next block returned status {}", block.status),
        ),
        Some(v) if (v - value).abs() < 1e-3 => (Verdict::Pass, format!("{head}, and it read back")),
        Some(v) => (
            Verdict::Note,
            format!("{head}, and it read back as {v:.4}: the plugin changed it"),
        ),
        None => (
            Verdict::Fail,
            format!("{head}, and the parameter was gone afterwards"),
        ),
    })
}

/// The editor window: opened, left alone for a moment, and closed.
fn editor(host: &mut HostProcess, setup: &Setup) -> Outcome {
    if !setup.editor {
        return Ok((Verdict::Skipped, "not asked for (--editor)".into()));
    }
    let within = setup.deadline;
    let opened = match host.open_editor(None, within) {
        Ok(status) => status,
        Err(HostError::Failed {
            kind: FailureKind::NoEditor,
            message,
        }) => return Ok((Verdict::Skipped, message)),
        Err(e) => return Err(e),
    };
    std::thread::sleep(Duration::from_millis(300));
    let still = host.editor_status(within)?;
    host.close_editor(within)?;
    let size = opened
        .bounds
        .map_or_else(String::new, |b| format!(" {}x{}", b.width, b.height));
    if still.open {
        Ok((Verdict::Pass, format!("opened{size}, closed")))
    } else {
        Ok((
            Verdict::Note,
            format!("opened{size}, and the window was gone 300 ms later"),
        ))
    }
}
