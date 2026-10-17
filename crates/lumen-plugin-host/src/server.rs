//! The host process's side of the protocol: read a request, act on the
//! plugin, answer it. One request in flight at a time; every request gets
//! exactly one response.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::blocks::{Blocks, Shared, lock};
use crate::instance::{Format, Instance, InstanceError};
use crate::module::Module;
use crate::protocol::{FailureKind, FrameError, Request, Response, read_request, write_response};
use crate::shm::HostLink;

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

#[derive(Default)]
struct State {
    // The module is kept so it outlives the instance.
    module: Option<Module>,
    /// The plugin, shared with the audio thread once one is attached.
    shared: Shared,
    blocks: Option<Blocks>,
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
            Request::Quit => Response::Done,
        }
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
    let mut state = State::default();
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
            // The audio thread ends first, then the plugin, then the module
            // it came from.
            state.detach();
            lock(&state.shared).instance = None;
            return Ok(Exit::Quit);
        }
    }
}
