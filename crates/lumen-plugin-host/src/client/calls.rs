//! The requests Lumen makes of a host, a wrapper each.

use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use crate::protocol::{
    Aux, BusInfo, ClassId, ClassInfo, EditorBounds, EditorStatus, Event, FailureKind, FrameError,
    ParamInfo, ParamQueue, ProgramList, Request, Response, Taken, UnitInfo, write_request,
};
use crate::shm::sys::{link_base, remove_names};
use crate::shm::{BlockLink, Layout, Wake};

use super::{Block, Configured, ExitKind, HostError, HostProcess};

impl HostProcess {
    fn send(&mut self, request: &Request) -> Result<(), HostError> {
        let mut frame = Vec::new();
        write_request(&mut frame, request).map_err(|e| HostError::Protocol(e.to_string()))?;
        match &self.writer {
            Some(writer) if writer.send(frame).is_ok() => Ok(()),
            _ => Err(self.lost()),
        }
    }

    /// The next response that is not stale, within `deadline`.
    fn receive(
        &mut self,
        deadline: Instant,
        wanted: Option<(u32, u64)>,
    ) -> Result<Response, HostError> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let item = match self.incoming.recv_timeout(left) {
                Ok(item) => item,
                Err(RecvTimeoutError::Timeout) => return Err(HostError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(self.lost()),
            };
            let response = match item {
                Ok(response) => response,
                Err(FrameError::Closed | FrameError::Io(_)) => return Err(self.lost()),
                Err(FrameError::Wire(e)) => return Err(HostError::Protocol(e.to_string())),
            };
            // The answer to a request given up on, whatever it says.
            if self.owed > 0 {
                self.owed -= 1;
                self.stale += 1;
                continue;
            }
            match (&response, wanted) {
                // The answer to the block being waited for.
                (
                    Response::Processed {
                        generation, seq, ..
                    },
                    Some((g, s)),
                ) if *generation == g && *seq == s => {
                    return Ok(response);
                }
                // A block answered after its deadline, or from before a
                // reconfiguration: not the one waited for.
                (Response::Processed { .. }, _) => self.stale += 1,
                (Response::Failed { kind, message }, _) => {
                    return Err(HostError::Failed {
                        kind: *kind,
                        message: message.clone(),
                    });
                }
                (_, None) => return Ok(response),
                // A control response while waiting for a block: out of step.
                (_, Some(_)) => {
                    return Err(HostError::Protocol("unexpected response".into()));
                }
            }
        }
    }

    fn call(&mut self, request: &Request, within: Duration) -> Result<Response, HostError> {
        let deadline = Instant::now() + within;
        self.send(request)?;
        self.answer(deadline, None)
    }

    /// The answer to the request just sent; one given up on is owed.
    fn answer(
        &mut self,
        deadline: Instant,
        wanted: Option<(u32, u64)>,
    ) -> Result<Response, HostError> {
        let answer = self.receive(deadline, wanted);
        if matches!(answer, Err(HostError::Timeout)) {
            self.owed += 1;
        }
        answer
    }

    pub fn load(
        &mut self,
        path: &Path,
        class: Option<ClassId>,
        within: Duration,
    ) -> Result<ClassInfo, HostError> {
        let request = Request::Load {
            path: path.to_string_lossy().into_owned(),
            class,
        };
        match self.call(&request, within)? {
            Response::Loaded(info) => Ok(info),
            _ => Err(HostError::Protocol("expected Loaded".into())),
        }
    }

    /// Sets the format and starts a new generation: blocks and answers of
    /// the previous one are stale from here on. Returns `(latency, tail)`.
    pub fn configure(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
        inputs: u8,
        outputs: u8,
        within: Duration,
    ) -> Result<(u32, u32), HostError> {
        let done = self.configure_buses(
            sample_rate,
            max_frames,
            (inputs, outputs),
            Aux::default(),
            within,
        )?;
        Ok((done.latency, done.tail))
    }

    /// As [`Self::configure`], with the auxiliary buses to turn on as well.
    /// The answer says how many channels a block then carries each way.
    pub fn configure_buses(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
        (inputs, outputs): (u8, u8),
        aux: Aux,
        within: Duration,
    ) -> Result<Configured, HostError> {
        self.generation += 1;
        let request = Request::Configure {
            generation: self.generation,
            sample_rate,
            max_frames,
            inputs,
            outputs,
            aux_inputs: aux.inputs,
            aux_outputs: aux.outputs,
        };
        match self.call(&request, within)? {
            Response::Configured {
                generation,
                latency,
                tail,
                inputs,
                outputs,
            } if generation == self.generation => Ok(Configured {
                latency,
                tail,
                inputs,
                outputs,
            }),
            _ => Err(HostError::Protocol("expected Configured".into())),
        }
    }

    /// The audio buses the plugin declares, inputs then outputs.
    pub fn buses(&mut self, within: Duration) -> Result<(Vec<BusInfo>, Vec<BusInfo>), HostError> {
        match self.call(&Request::Buses, within)? {
            Response::Buses { inputs, outputs } => Ok((inputs, outputs)),
            _ => Err(HostError::Protocol("expected Buses".into())),
        }
    }

    /// Renders one block, or reports why it could not. On `Timeout` the
    /// block is given up on; if the host answers later, the answer is
    /// dropped as stale by the next call.
    pub fn process(
        &mut self,
        frames: u32,
        input: Vec<Vec<f32>>,
        events: Vec<Event>,
        params: Vec<ParamQueue>,
        within: Duration,
    ) -> Result<Block, HostError> {
        let deadline = Instant::now() + within;
        let seq = self.next_seq;
        self.next_seq += 1;
        let generation = self.generation;
        self.send(&Request::Process {
            generation,
            seq,
            frames,
            input,
            events,
            params,
        })?;
        match self.answer(deadline, Some((generation, seq)))? {
            Response::Processed {
                status,
                latency,
                restart,
                repaired,
                output,
                ..
            } => Ok(Block {
                status,
                latency,
                restart,
                repaired,
                output,
            }),
            _ => Err(HostError::Protocol("expected Processed".into())),
        }
    }

    /// Opens the shared-memory audio path to this host and waits until the
    /// host's block loop is running. The returned link is for the audio
    /// thread; this handle stays with the control side. Attaching again
    /// replaces the earlier link, which then answers nothing.
    pub fn attach(
        &mut self,
        layout: Layout,
        wake: Wake,
        within: Duration,
    ) -> Result<BlockLink, HostError> {
        let transport = |e: std::io::Error| HostError::Failed {
            kind: FailureKind::Transport,
            message: e.to_string(),
        };
        let deadline = Instant::now() + within;
        let base = link_base();
        let (link, names) = BlockLink::create(&base, layout, wake).map_err(transport)?;
        let attached = self.call(&Request::Attach { base, layout, wake }, within);
        // The host has opened the objects, or has failed to; either way the
        // names have done their work.
        remove_names(&names);
        match attached? {
            Response::Attached => {}
            _ => return Err(HostError::Protocol("expected Attached".into())),
        }
        while !link.host_ready() {
            if Instant::now() >= deadline {
                return Err(HostError::Timeout);
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        Ok(link)
    }

    pub fn state(&mut self, within: Duration) -> Result<Vec<u8>, HostError> {
        match self.call(&Request::GetState, within)? {
            Response::State(bytes) => Ok(bytes),
            _ => Err(HostError::Protocol("expected State".into())),
        }
    }

    pub fn set_state(&mut self, state: Vec<u8>, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::SetState(state), within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    pub fn parameters(&mut self, within: Duration) -> Result<(u32, Vec<ParamInfo>), HostError> {
        match self.call(&Request::Parameters, within)? {
            Response::Parameters { total, list } => Ok((total, list)),
            _ => Err(HostError::Protocol("expected Parameters".into())),
        }
    }

    /// The groups parameters belong to.
    pub fn units(&mut self, within: Duration) -> Result<Vec<UnitInfo>, HostError> {
        match self.call(&Request::Units, within)? {
            Response::Units(list) => Ok(list),
            _ => Err(HostError::Protocol("expected Units".into())),
        }
    }

    /// The programs the plugin offers, with the parameter that selects each
    /// list's program.
    pub fn programs(&mut self, within: Duration) -> Result<Vec<ProgramList>, HostError> {
        match self.call(&Request::Programs, within)? {
            Response::Programs(lists) => Ok(lists),
            _ => Err(HostError::Protocol("expected Programs".into())),
        }
    }

    /// The edit controller's own state (apart from the component's).
    pub fn controller_state(&mut self, within: Duration) -> Result<Vec<u8>, HostError> {
        match self.call(&Request::GetControllerState, within)? {
            Response::State(bytes) => Ok(bytes),
            _ => Err(HostError::Protocol("expected State".into())),
        }
    }

    pub fn set_controller_state(
        &mut self,
        state: Vec<u8>,
        within: Duration,
    ) -> Result<(), HostError> {
        match self.call(&Request::SetControllerState(state), within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    /// The plugin's own text for a normalized value.
    pub fn param_text(
        &mut self,
        id: u32,
        value: f64,
        within: Duration,
    ) -> Result<String, HostError> {
        match self.call(&Request::ParamText { id, value }, within)? {
            Response::Text(text) => Ok(text),
            _ => Err(HostError::Protocol("expected Text".into())),
        }
    }

    /// The normalized value the plugin reads from the user's text; an error
    /// when it does not understand it.
    pub fn param_value(&mut self, id: u32, text: &str, within: Duration) -> Result<f64, HostError> {
        let request = Request::ParamValue {
            id,
            text: text.to_owned(),
        };
        match self.call(&request, within)? {
            Response::Value(value) => Ok(value),
            _ => Err(HostError::Protocol("expected Value".into())),
        }
    }

    /// Opens the plugin's editor window, where `bounds` say or where the
    /// system puts it. Fails with `NoEditor` when the plugin or the platform
    /// has none, and with `Unresponsive` when the host's window code does not
    /// answer in time.
    pub fn open_editor(
        &mut self,
        bounds: Option<EditorBounds>,
        within: Duration,
    ) -> Result<EditorStatus, HostError> {
        // The window should come to the front: Lumen has the foreground now
        // (the user just asked), and lets the host take it.
        #[cfg(windows)]
        // SAFETY: a plain call naming the host's process.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(self.child.id())
        };
        self.editor_call(&Request::OpenEditor { bounds }, within)
    }

    /// Closes the editor window, if there is one.
    pub fn close_editor(&mut self, within: Duration) -> Result<EditorStatus, HostError> {
        self.editor_call(&Request::CloseEditor, within)
    }

    /// Hides the editor window without closing it, or shows it again.
    pub fn show_editor(
        &mut self,
        visible: bool,
        within: Duration,
    ) -> Result<EditorStatus, HostError> {
        self.editor_call(&Request::ShowEditor(visible), within)
    }

    /// Whether the editor window is open, and where.
    pub fn editor_status(&mut self, within: Duration) -> Result<EditorStatus, HostError> {
        self.editor_call(&Request::EditorStatus, within)
    }

    fn editor_call(
        &mut self,
        request: &Request,
        within: Duration,
    ) -> Result<EditorStatus, HostError> {
        match self.call(request, within)? {
            Response::Editor(status) => Ok(status),
            _ => Err(HostError::Protocol("expected Editor".into())),
        }
    }

    /// Takes `keys` (virtual-key codes) from the plugin's window, or gives
    /// every key back when empty; the keys arrive through [`Self::on_notice`].
    pub fn capture_keys(&mut self, keys: Vec<u8>, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::CaptureKeys(keys), within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    /// Sets a parameter; the processor hears it in its next block.
    pub fn set_param(&mut self, id: u32, value: f64, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::SetParam { id, value }, within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    /// The edits made in the plugin since the last call, oldest first, and
    /// how many were dropped for want of room.
    pub fn take_edits(&mut self, within: Duration) -> Result<Taken, HostError> {
        match self.call(&Request::TakeEdits, within)? {
            Response::Edits {
                list,
                dropped,
                ignored,
            } => Ok(Taken {
                list,
                dropped,
                ignored,
            }),
            _ => Err(HostError::Protocol("expected Edits".into())),
        }
    }

    /// Asks the host to quit and waits for it; kills it if it does not.
    pub fn quit(&mut self, within: Duration) -> ExitKind {
        let _ = self.call(&Request::Quit, within);
        self.writer = None;
        match self.wait_ended(within) {
            Some(how) => how,
            None => {
                self.kill();
                ExitKind::Killed
            }
        }
    }
}
