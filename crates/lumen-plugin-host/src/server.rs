//! The host process's side of the protocol: read a request, act on the
//! plugin, answer it. One request in flight at a time; every request gets
//! exactly one response.

use std::io::{self, Read, Write};
use std::path::Path;

use crate::instance::{Format, Instance, InstanceError};
use crate::module::Module;
use crate::protocol::{FailureKind, FrameError, Request, Response, read_request, write_response};

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
    instance: Option<Instance>,
    generation: Option<u32>,
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
    fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Load { path, class } => {
                if self.instance.is_some() {
                    return failed(FailureKind::Sequence, "a plugin is already loaded");
                }
                let module = match Module::load(Path::new(&path)) {
                    Ok(m) => m,
                    Err(e) => return failed(FailureKind::Load, e.to_string()),
                };
                match Instance::create(&module, class) {
                    Ok(instance) => {
                        let info = instance.class.clone();
                        self.instance = Some(instance);
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
                let Some(instance) = &mut self.instance else {
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
                        self.generation = Some(generation);
                        Response::Configured {
                            generation,
                            latency: instance.latency(),
                            tail: instance.tail(),
                        }
                    }
                    Err(e) => {
                        // A failed configuration leaves nothing to process.
                        self.generation = None;
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
                let Some(instance) = &mut self.instance else {
                    return failed(FailureKind::Sequence, "no plugin is loaded");
                };
                match self.generation {
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
            Request::GetState => match &self.instance {
                Some(instance) => match instance.state() {
                    Ok(bytes) => Response::State(bytes),
                    Err(e) => from_instance(e),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::SetState(bytes) => match &self.instance {
                Some(instance) => match instance.set_state(&bytes) {
                    Ok(()) => Response::Done,
                    Err(e) => from_instance(e),
                },
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::Parameters => match &self.instance {
                Some(instance) => {
                    let (total, list) = instance.parameters();
                    Response::Parameters { total, list }
                }
                None => failed(FailureKind::Sequence, "no plugin is loaded"),
            },
            Request::Quit => Response::Done,
        }
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
            // The plugin goes before the module it came from.
            state.instance = None;
            return Ok(Exit::Quit);
        }
    }
}
