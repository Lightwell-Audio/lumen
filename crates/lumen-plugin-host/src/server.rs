//! The host process's side of the protocol: read a request, act on the
//! plugin, answer it. One request in flight at a time; every request gets
//! exactly one response.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::blocks::{Blocks, Shared, lock};
use crate::editor::{Editors, Unavailable};
use crate::instance::{Format, Instance, InstanceError};
use crate::module::Module;
use crate::protocol::{FailureKind, FrameError, Request, Response, read_request, write_response};
use crate::shm::HostLink;

/// How long a request about the editor window may keep the main thread
/// before the host answers that it is not responding.
pub const EDITOR_WAIT: Duration = Duration::from_millis(600);

/// Why the loop ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// Asked to quit.
    Quit,
    /// Lumen closed the stream (it exited or dropped this host).
    Closed,
    /// A message that could not be trusted; the stream is abandoned.
    Malformed,
}

struct State {
    // The module is kept so it outlives the instance.
    module: Option<Module>,
    /// The plugin, shared with the audio thread once one is attached.
    shared: Shared,
    blocks: Option<Blocks>,
    /// The plugin's editor window, if the platform has any.
    editors: Box<dyn Editors>,
}

fn failed(kind: FailureKind, message: impl Into<String>) -> Response {
    Response::Failed {
        kind,
        message: message.into(),
    }
}

fn from_instance(e: InstanceError) -> Response {
    match e {
        InstanceError::NoClass => failed(FailureKind::Load, e.to_string()),
        InstanceError::Plugin(_) => failed(FailureKind::Plugin, e.to_string()),
        InstanceError::Sequence(_) => failed(FailureKind::Sequence, e.to_string()),
    }
}

impl State {
    fn new(editors: Box<dyn Editors>) -> Self {
        Self {
            module: None,
            shared: Shared::default(),
            blocks: None,
            editors,
        }
    }

    /// Ends the audio thread, if there is one. Waits for the block it is on.
    fn detach(&mut self) {
        if let Some(blocks) = self.blocks.take() {
            blocks.stop();
        }
    }

    fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Load { path, class } => {
                let mut guard = lock(&self.shared);
                if guard.instance.is_some() {
                    return failed(FailureKind::Sequence, "a plugin is already loaded");
                }
                let module = match Module::load(Path::new(&path)) {
                    Ok(m) => m,
                    Err(e) => return failed(FailureKind::Load, e.to_string()),
                };
                match Instance::create(&module, class) {
                    Ok(instance) => {
                        let info = instance.class.clone();
                        guard.instance = Some(instance);
                        self.module = Some(module);
                        Response::Loaded(info)
                    }
                    Err(e) => from_instance(e),
                }
            }
            Request::Configure {
                generation,
                sample_rate,
                max_frames,
                inputs,
                outputs,
            } => {
                let mut guard = lock(&self.shared);
                let Some(instance) = &mut guard.instance else {
                    return failed(FailureKind::Sequence, "no plugin is loaded");
                };
                let format = Format {
                    sample_rate,
                    max_frames,
                    inputs,
                    outputs,
                };
                match instance.configure(format) {
                    Ok(()) => {
                        let response = Response::Configured {
                            generation,
                            latency: instance.latency(),
                            tail: instance.tail(),
                        };
                        guard.generation = Some(generation);
                        response
                    }
                    Err(e) => {
                        // A failed configuration leaves nothing to process.
                        guard.generation = None;
                        from_instance(e)
                    }
                }
            }
            Request::Process {
                generation,
                seq,
                frames,
                input,
                events,
                params,
            } => {
                let mut guard = lock(&self.shared);
                let current = guard.generation;
                let Some(instance) = &mut guard.instance else {
                    return failed(FailureKind::Sequence, "no plugin is loaded");
                };
                match current {
                    None => return failed(FailureKind::Sequence, "not configured"),
                    Some(current) if current != generation => {
                        return failed(
                            FailureKind::StaleConfiguration,
                            format!("block is for generation {generation}, not {current}"),
                        );
                    }
                    Some(_) => {}
                }
                match instance.process(frames, &input, &events, &params) {
                    Ok(done) => Response::Processed {
                        generation,
                        seq,
                        status: done.status,
                        latency: done.latency,
                        restart: done.restart,
                        repaired: done.repaired,
                        output: done.output,
                    },
                    Err(e) => from_instance(e),
                }
            }
            Request::GetState => match &lock(&self.shared).instance {
                Some(instance) => match instance.state() {
                    Ok(bytes) => Response::State(bytes),
                    Err(e) => from_instance(e),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::SetState(bytes) => match &lock(&self.shared).instance {
                Some(instance) => match instance.set_state(&bytes) {
                    Ok(()) => Response::Done,
                    Err(e) => from_instance(e),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::Parameters => match &lock(&self.shared).instance {
                Some(instance) => {
                    let (total, list) = instance.parameters();
                    Response::Parameters { total, list }
                }
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::Units => match &lock(&self.shared).instance {
                Some(instance) => Response::Units(instance.units()),
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::ParamText { id, value } => match &lock(&self.shared).instance {
                Some(instance) => match instance.param_text(id, value) {
                    Some(text) => Response::Text(text),
                    None => failed(FailureKind::Plugin, "the plugin has no text for the value"),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::ParamValue { id, text } => match &lock(&self.shared).instance {
                Some(instance) => match instance.param_value(id, &text) {
                    Some(value) => Response::Value(value),
                    None => failed(
                        FailureKind::Plugin,
                        "the plugin did not understand the text",
                    ),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::SetParam { id, value } => match &mut lock(&self.shared).instance {
                Some(instance) => {
                    instance.set_param(id, value);
                    Response::Done
                }
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::TakeEdits => match &mut lock(&self.shared).instance {
                Some(instance) => {
                    let taken = instance.take_edits();
                    Response::Edits {
                        list: taken.list,
                        dropped: taken.dropped,
                        ignored: taken.ignored,
                    }
                }
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::Attach { base, layout, wake } => {
                // A second attach replaces the first, and the old audio
                // thread is finished before the new one starts.
                self.detach();
                match HostLink::open(&base, layout, wake) {
                    Ok((link, stopper)) => {
                        match Blocks::start(link, stopper, Arc::clone(&self.shared)) {
                            Ok(blocks) => {
                                self.blocks = Some(blocks);
                                Response::Attached
                            }
                            Err(e) => failed(FailureKind::Transport, e.to_string()),
                        }
                    }
                    Err(e) => failed(FailureKind::Transport, e.to_string()),
                }
            }
            Request::OpenEditor { bounds } => {
                // The view is made under the lock (it asks the controller);
                // the window is not, so the audio thread never waits for it.
                let (title, view) = match &lock(&self.shared).instance {
                    Some(instance) => (instance.class.name.clone(), instance.create_editor_view()),
                    None => return failed(FailureKind::Sequence, "no plugin is loaded"),
                };
                match self.editors.open(&title, view, bounds) {
                    Ok(status) => Response::Editor(status),
                    Err(why) => failed(FailureKind::NoEditor, why),
                }
            }
            Request::CloseEditor => Response::Editor(self.editors.close()),
            Request::EditorStatus => Response::Editor(self.editors.status()),
            Request::Quit => Response::Done,
        }
    }

    /// What follows the last request: the audio thread ends first, then the
    /// editor, then the plugin, then the module it came from.
    fn finish(&mut self) {
        self.detach();
        self.editors.close();
        lock(&self.shared).instance = None;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        // However the loop ended, the audio thread stops before the plugin
        // it is using can go.
        self.detach();
    }
}

/// Serves requests until the stream closes, a quit arrives or a message
/// cannot be trusted.
pub fn serve<R: Read, W: Write>(input: &mut R, output: &mut W) -> io::Result<Exit> {
    let mut state = State::new(Box::new(Unavailable));
    loop {
        let request = match read_request(input) {
            Ok(request) => request,
            Err(FrameError::Closed) => return Ok(Exit::Closed),
            Err(FrameError::Io(e)) => return Err(e),
            Err(FrameError::Wire(e)) => {
                write_response(output, &failed(FailureKind::Protocol, e.to_string()))?;
                return Ok(Exit::Malformed);
            }
        };
        let quit = request == Request::Quit;
        let response = state.handle(request);
        write_response(output, &response)?;
        if quit {
            state.finish();
            return Ok(Exit::Quit);
        }
    }
}

/// Wakes the main thread after a request is queued for it.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The loop the host's main thread runs while a reader thread takes requests
/// off the stream. A window system's main thread pumps messages in it; others
/// wait to be woken ([`Plain`]).
pub trait Pump {
    /// What the reader calls after queueing a request.
    fn waker(&self) -> Waker;
    /// Runs on the calling thread: calls `drain` once at the start and again
    /// whenever the waker is called, until it returns false.
    fn run(&mut self, drain: &mut dyn FnMut() -> bool);
}

/// A main thread that does nothing but wait for work.
pub struct Plain {
    wake: Sender<()>,
    woken: Receiver<()>,
}

impl Default for Plain {
    fn default() -> Self {
        let (wake, woken) = mpsc::channel();
        Self { wake, woken }
    }
}

impl Pump for Plain {
    fn waker(&self) -> Waker {
        let wake = Mutex::new(self.wake.clone());
        Arc::new(move || {
            let _ = lock_plain(&wake).send(());
        })
    }

    fn run(&mut self, drain: &mut dyn FnMut() -> bool) {
        while drain() {
            if self.woken.recv().is_err() {
                return;
            }
        }
    }
}

fn lock_plain<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A request for the main thread, and where its answer goes.
struct Job {
    request: Request,
    reply: Sender<Response>,
}

enum Queued {
    Job(Job),
    /// The reader is done: the stream closed, a quit was answered or a
    /// message could not be trusted.
    End,
}

/// How long the reader waits for the main thread's answer to `request`:
/// only a request about the editor window has a limit. A plugin that stops
/// answering in its own window code stops the main thread, and the host says
/// so rather than going quiet; every other request has the deadline its
/// caller gave it.
fn wait_for(request: &Request) -> Option<Duration> {
    matches!(
        request,
        Request::OpenEditor { .. } | Request::CloseEditor | Request::EditorStatus
    )
    .then_some(EDITOR_WAIT)
}

fn unresponsive() -> Response {
    failed(
        FailureKind::Unresponsive,
        "the plugin's window is not responding",
    )
}

/// What the reader thread does: takes requests off `input`, gives each to
/// the main thread and writes its answer to `output`. A request the main
/// thread is too slow to answer is answered "not responding"; until the
/// main thread finishes it, further requests are answered the same way, and
/// its late answer is dropped, so every request still gets exactly one
/// response.
fn read_requests<R: Read, W: Write>(
    mut input: R,
    mut output: W,
    jobs: &Sender<Queued>,
    wake: &Waker,
    stuck: &Mutex<bool>,
) -> io::Result<Exit> {
    // The answer owed by a request given up on.
    let mut owed: Option<Receiver<Response>> = None;
    let end = |exit: io::Result<Exit>| {
        let _ = jobs.send(Queued::End);
        wake();
        // A main thread stuck in a plugin's window code cannot be waited for.
        if *lock_plain(stuck) {
            crate::process::end_now(0);
        }
        exit
    };
    loop {
        let request = match read_request(&mut input) {
            Ok(request) => request,
            Err(FrameError::Closed) => return end(Ok(Exit::Closed)),
            Err(FrameError::Io(e)) => return end(Err(e)),
            Err(FrameError::Wire(e)) => {
                let sent =
                    write_response(&mut output, &failed(FailureKind::Protocol, e.to_string()));
                return end(sent.map(|()| Exit::Malformed));
            }
        };
        if let Some(late) = &owed {
            match late.try_recv() {
                Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {
                    owed = None;
                    *lock_plain(stuck) = false;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    let quit = request == Request::Quit;
                    if quit {
                        // Nobody to end the plugin cleanly: the process goes.
                        let _ = write_response(&mut output, &Response::Done);
                        crate::process::end_now(0);
                    }
                    write_response(&mut output, &unresponsive())?;
                    continue;
                }
            }
        }
        let quit = request == Request::Quit;
        let limit = wait_for(&request);
        let (reply, answer) = mpsc::channel();
        if jobs.send(Queued::Job(Job { request, reply })).is_err() {
            return Ok(Exit::Closed);
        }
        wake();
        let response = match limit {
            None => match answer.recv() {
                Ok(response) => response,
                Err(_) => return Ok(Exit::Closed),
            },
            Some(limit) => match answer.recv_timeout(limit) {
                Ok(response) => response,
                Err(RecvTimeoutError::Timeout) => {
                    *lock_plain(stuck) = true;
                    owed = Some(answer);
                    unresponsive()
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(Exit::Closed),
            },
        };
        write_response(&mut output, &response)?;
        if quit {
            return end(Ok(Exit::Quit));
        }
    }
}

/// Serves requests as [`serve`] does, but with the requests read on a thread
/// of their own while the calling thread runs `pump` and acts on them: the
/// calling thread is the one that made the plugin, and the one that can show
/// its window.
pub fn serve_pumped<R, W>(
    input: R,
    output: W,
    pump: &mut dyn Pump,
    editors: Box<dyn Editors>,
) -> io::Result<Exit>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let (jobs, queue) = mpsc::channel::<Queued>();
    let wake = pump.waker();
    let stuck = Arc::new(Mutex::new(false));
    let reader = {
        let (stuck, wake) = (Arc::clone(&stuck), Arc::clone(&wake));
        std::thread::Builder::new()
            .name("host requests".into())
            .spawn(move || read_requests(input, output, &jobs, &wake, &stuck))?
    };
    let mut state = State::new(editors);
    let mut drain = || {
        while let Ok(queued) = queue.try_recv() {
            let Queued::Job(job) = queued else {
                return false;
            };
            let response = state.handle(job.request);
            let _ = job.reply.send(response);
        }
        true
    };
    pump.run(&mut drain);
    state.finish();
    reader
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the request reader panicked")))
}
