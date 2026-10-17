//! One plugin device's host process and the thread that owns it (M6 slice 2c).
//!
//! The thread starts the process, loads the plugin, restores its state,
//! configures it and opens the audio link, with a deadline on every step; then
//! it waits for requests (a fresh link for a new engine, quit) and, between
//! them, looks at the process every 10 ms and sets the flag the audio thread
//! reads, so the callback never makes a system call to ask whether a host has
//! ended. A host that hangs blocks only this thread.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use lumen_plugin_host::client::{ExitKind, HostError, HostProcess};
use lumen_plugin_host::protocol::MAX_FRAMES;
use lumen_plugin_host::shm::{Layout, Wake};

use crate::link::{Counts, Link};

/// How the audio loop and the host wait for each other. The setting
/// ADR-0069's runs support (inside the budget to 100 µs of plugin work per
/// track); one constant, to re-measure in MT-VST-2.
pub const WAKE: Wake = Wake::EVENT;

/// The step a failure happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    Load,
    State,
    Configure,
    Attach,
    /// The process ended while the project played.
    Running,
}

/// Why a device has no working host.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub step: Step,
    pub error: HostError,
    /// Where the plugin died, when the host could say ("in Name.dll+0x1f2a,
    /// reading address 0x0"): it tells a plugin's fault from the host's.
    pub detail: Option<String>,
}

impl Step {
    /// What the step is, for "crashed while ...".
    fn doing(self) -> &'static str {
        match self {
            Step::Start => "starting",
            Step::Load => "loading",
            Step::State => "restoring its saved state",
            Step::Configure => "setting up",
            Step::Attach => "opening its audio link",
            Step::Running => "running",
        }
    }
}

impl Failure {
    /// Whether the plugin took its host down (a crash) rather than refusing.
    pub fn crashed(&self) -> bool {
        matches!(
            self.error,
            HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
        )
    }

    /// Whether it stopped answering.
    pub fn hung(&self) -> bool {
        matches!(self.error, HostError::Timeout)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.step {
            Step::Start => "start the plugin host",
            Step::Load => "load the plugin",
            Step::State => "restore the plugin's saved state",
            Step::Configure => "set the plugin up",
            Step::Attach => "open the audio link to the plugin",
            Step::Running => "keep running",
        };
        match &self.error {
            // The cause first and short: it has to fit on a card.
            HostError::Ended(ExitKind::Crashed(how)) => {
                let cause = ExitKind::Crashed(how.clone()).to_string();
                let cause = cause.strip_prefix("it crashed: ").unwrap_or(&cause);
                match &self.detail {
                    Some(detail) => {
                        write!(f, "Crashed while {}: {cause}, {detail}.", self.step.doing())
                    }
                    None => write!(f, "Crashed while {}: {cause}.", self.step.doing()),
                }
            }
            HostError::Timeout => write!(f, "The plugin did not answer while trying to {what}."),
            HostError::Ended(how) => {
                write!(f, "The plugin host ended while trying to {what}: {how}.")
            }
            HostError::Protocol(why) => write!(f, "The plugin host broke the protocol: {why}."),
            HostError::Failed { message, .. } => {
                write!(f, "The plugin could not {what}: {message}")
            }
        }
    }
}

/// How long each step may take before the host is given up on.
#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    pub load: Duration,
    pub state: Duration,
    pub configure: Duration,
    pub attach: Duration,
    /// To quit when asked, before the host is killed.
    pub quit: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            load: Duration::from_secs(20),
            state: Duration::from_secs(10),
            configure: Duration::from_secs(5),
            attach: Duration::from_secs(2),
            quit: Duration::from_secs(1),
        }
    }
}

/// What to start for one device.
#[derive(Debug, Clone)]
pub struct Start {
    /// The `lumen-plugin-host` program.
    pub host: PathBuf,
    /// The plugin's module, and its class in it.
    pub module: PathBuf,
    pub class: [u8; 16],
    /// An instrument takes no audio in; an effect takes a stereo pair.
    pub instrument: bool,
    /// The saved component state, restored at load; empty for none.
    pub state: Vec<u8>,
    pub rate: f64,
    /// Added to the host's environment (the test plugin's faults).
    pub env: Vec<(String, String)>,
    pub deadlines: Deadlines,
    /// How the two sides wait for each other: [`WAKE`] unless a test asks
    /// for another.
    pub wake: Wake,
}

impl Start {
    /// A plugin with no saved state, [`Deadlines::default`] and [`WAKE`].
    pub fn new(
        host: PathBuf,
        module: PathBuf,
        class: [u8; 16],
        instrument: bool,
        rate: f64,
    ) -> Self {
        Self {
            host,
            module,
            class,
            instrument,
            state: Vec::new(),
            rate,
            env: Vec::new(),
            deadlines: Deadlines::default(),
            wake: WAKE,
        }
    }
}

/// What the thread tells the app.
#[derive(Debug)]
pub enum Happened {
    /// The host is running and has a link for the engine to play it through.
    Linked(Box<Linked>),
    /// The device has no host and will not get one without a reload.
    Failed(Failure),
}

pub struct Linked {
    pub link: Link,
    pub name: String,
    pub vendor: String,
    /// What the plugin reports, in frames at the rate it was configured at.
    pub latency: u32,
    pub tail: u32,
    pub pid: u32,
    /// What the host says about itself: how long its first blocks took, and
    /// where it died if it does. For the card when the plugin is given up on.
    pub log: Arc<lumen_plugin_host::client::HostLog>,
}

impl fmt::Debug for Linked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Linked")
            .field("name", &self.name)
            .field("latency", &self.latency)
            .field("tail", &self.tail)
            .finish_non_exhaustive()
    }
}

enum Command {
    /// A fresh link, at `rate`, for a new engine.
    Relink(f64),
    Quit,
}

/// A permit to load: only a few plugins load at once, as the scanner's.
pub(crate) struct Gate {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Gate {
    pub(crate) fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            free: Mutex::new(permits),
            freed: Condvar::new(),
        })
    }

    fn take(&self) {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
    }

    fn give(&self) {
        *self.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.freed.notify_one();
    }
}

/// Which device, and which start of it, a message is from: the manager drops
/// what a replaced or removed device's old thread still says.
pub type Tag = (u64, u64);

/// The handle to a device's thread. Dropping it asks the host to quit and
/// does not wait.
pub struct Process {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Process {
    /// Starts the thread; its messages go to `out` tagged with `tag`.
    pub(crate) fn start(
        start: Start,
        gate: Arc<Gate>,
        tag: Tag,
        out: Sender<(Tag, Happened)>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let (commands, requests) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("plugin-host".into())
            .spawn(move || {
                Run {
                    start,
                    gate,
                    tag,
                    out,
                    wake,
                    requests,
                    ended: Arc::new(AtomicBool::new(false)),
                    counts: Arc::new(Counts::default()),
                }
                .run();
            })?;
        Ok(Self {
            commands,
            thread: Some(thread),
        })
    }

    /// Asks for a fresh link at `rate`, for a new engine.
    pub fn relink(&self, rate: f64) {
        let _ = self.commands.send(Command::Relink(rate));
    }

    /// Asks the host to quit and waits for the thread.
    pub fn join(mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // The thread quits the host on its own and ends; nothing waits for
        // it here.
        let _ = self.commands.send(Command::Quit);
    }
}

struct Run {
    start: Start,
    gate: Arc<Gate>,
    tag: Tag,
    out: Sender<(Tag, Happened)>,
    wake: Arc<dyn Fn() + Send + Sync>,
    requests: Receiver<Command>,
    ended: Arc<AtomicBool>,
    counts: Arc<Counts>,
}

/// Where a crashed host said it died. The host writes the line as it goes
/// down and Lumen reads it from the pipe a moment later, so look for it for
/// a short while.
fn crash_detail(host: &HostProcess, error: &HostError) -> Option<String> {
    if !matches!(
        error,
        HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
    ) {
        return None;
    }
    let log = host.log();
    for _ in 0..20 {
        if let Some(line) = log.crash() {
            return Some(line);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

struct Up {
    host: HostProcess,
    name: String,
    vendor: String,
    generation: u32,
}

impl Run {
    fn say(&self, what: Happened) {
        let _ = self.out.send((self.tag, what));
        (self.wake)();
    }

    fn run(self) {
        self.gate.take();
        let loaded = self.load();
        self.gate.give();
        let mut up = match loaded {
            Ok((up, linked)) => {
                self.say(Happened::Linked(Box::new(linked)));
                up
            }
            Err(failure) => {
                self.say(Happened::Failed(failure));
                return;
            }
        };
        let d = self.start.deadlines;
        loop {
            match self.requests.recv_timeout(Duration::from_millis(10)) {
                Ok(Command::Relink(rate)) => match self.relink(&mut up, rate) {
                    Ok(linked) => self.say(Happened::Linked(Box::new(linked))),
                    Err(failure) => {
                        self.ended.store(true, Ordering::Release);
                        up.host.kill();
                        self.say(Happened::Failed(failure));
                        return;
                    }
                },
                Ok(Command::Quit) | Err(RecvTimeoutError::Disconnected) => {
                    up.host.quit(d.quit);
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if up.host.has_ended() {
                self.ended.store(true, Ordering::Release);
                let how = up
                    .host
                    .ended()
                    .unwrap_or(lumen_plugin_host::client::ExitKind::Killed);
                let error = HostError::Ended(how);
                let detail = crash_detail(&up.host, &error);
                self.say(Happened::Failed(Failure {
                    step: Step::Running,
                    error,
                    detail,
                }));
                return;
            }
        }
    }

    fn fail(&self, host: &mut HostProcess, step: Step, error: HostError) -> Failure {
        self.ended.store(true, Ordering::Release);
        host.kill();
        let detail = crash_detail(host, &error);
        Failure {
            step,
            error,
            detail,
        }
    }

    fn load(&self) -> Result<(Up, Linked), Failure> {
        let s = &self.start;
        let env: Vec<(&str, &str)> = s
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut host = HostProcess::spawn(&s.host, &env).map_err(|e| Failure {
            step: Step::Start,
            error: HostError::Failed {
                kind: lumen_plugin_host::protocol::FailureKind::Load,
                message: e.to_string(),
            },
            detail: None,
        })?;
        let class = host
            .load(&s.module, Some(s.class), s.deadlines.load)
            .map_err(|e| self.fail(&mut host, Step::Load, e))?;
        if !s.state.is_empty() {
            host.set_state(s.state.clone(), s.deadlines.state)
                .map_err(|e| self.fail(&mut host, Step::State, e))?;
        }
        let mut up = Up {
            host,
            name: class.name,
            vendor: class.vendor,
            generation: 0,
        };
        let linked = self.configure_and_link(&mut up, s.rate)?;
        Ok((up, linked))
    }

    /// Configures at `rate` (a new generation) and opens a link.
    fn configure_and_link(&self, up: &mut Up, rate: f64) -> Result<Linked, Failure> {
        let s = &self.start;
        let inputs = if s.instrument { 0 } else { 2 };
        let (latency, tail) = up
            .host
            .configure(rate, MAX_FRAMES as u32, inputs, 2, s.deadlines.configure)
            .map_err(|e| self.fail(&mut up.host, Step::Configure, e))?;
        up.generation = up.host.generation();
        let block = up
            .host
            .attach(
                Layout::new(MAX_FRAMES as u32, inputs, 2),
                s.wake,
                s.deadlines.attach,
            )
            .map_err(|e| self.fail(&mut up.host, Step::Attach, e))?;
        Ok(Linked {
            link: Link::new(
                block,
                up.generation,
                tail,
                self.ended.clone(),
                self.counts.clone(),
            ),
            name: up.name.clone(),
            vendor: up.vendor.clone(),
            latency,
            tail,
            pid: up.host.id(),
            log: up.host.log(),
        })
    }

    fn relink(&self, up: &mut Up, rate: f64) -> Result<Linked, Failure> {
        self.configure_and_link(up, rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_is_worded_short_with_its_cause_first() {
        let failure = Failure {
            step: Step::Load,
            error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
            detail: None,
        };
        assert_eq!(
            failure.to_string(),
            "Crashed while loading: access violation, exception 0xc0000005."
        );
        let located = Failure {
            step: Step::Running,
            error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
            detail: Some("in Plug.dll+0x1f2a, reading address 0x0".into()),
        };
        assert_eq!(
            located.to_string(),
            "Crashed while running: access violation, exception 0xc0000005, in Plug.dll+0x1f2a, reading address 0x0."
        );
        let timeout = Failure {
            step: Step::Configure,
            error: HostError::Timeout,
            detail: None,
        };
        assert!(timeout.to_string().contains("did not answer"));
    }

    use std::sync::atomic::AtomicUsize;

    #[test]
    fn the_gate_lets_only_its_permits_through_at_once() {
        let gate = Gate::new(2);
        let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (gate, inside, most) = (gate.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    gate.take();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    inside.fetch_sub(1, Ordering::SeqCst);
                    gate.give();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn failures_say_what_was_tried_and_how_it_went() {
        let hung = Failure {
            step: Step::Load,
            error: HostError::Timeout,
            detail: None,
        };
        assert!(hung.hung() && !hung.crashed());
        assert!(hung.to_string().contains("did not answer"));
        assert!(hung.to_string().contains("load the plugin"));
    }
}
