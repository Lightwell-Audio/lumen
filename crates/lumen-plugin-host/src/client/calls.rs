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
            if self.owed > 0 {
                self.owed -= 1;
                self.stale += 1;
                continue;
            }
            match (&response, wanted) {
                (
                    Response::Processed {
                        generation, seq, ..
                    },
                    Some((g, s)),
                ) if *generation == g && *seq == s => {
                    return Ok(response);
                }
                (Response::Processed { .. }, _) => self.stale += 1,
                (Response::Failed { kind, message }, _) => {
                    return Err(HostError::Failed {
                        kind: *kind,
                        message: message.clone(),
                    });
                }
                (_, None) => return Ok(response),
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

    pub fn buses(&mut self, within: Duration) -> Result<(Vec<BusInfo>, Vec<BusInfo>), HostError> {
        match self.call(&Request::Buses, within)? {
            Response::Buses { inputs, outputs } => Ok((inputs, outputs)),
            _ => Err(HostError::Protocol("expected Buses".into())),
        }
    }

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

    pub fn units(&mut self, within: Duration) -> Result<Vec<UnitInfo>, HostError> {
        match self.call(&Request::Units, within)? {
            Response::Units(list) => Ok(list),
            _ => Err(HostError::Protocol("expected Units".into())),
        }
    }

    pub fn programs(&mut self, within: Duration) -> Result<Vec<ProgramList>, HostError> {
        match self.call(&Request::Programs, within)? {
            Response::Programs(lists) => Ok(lists),
            _ => Err(HostError::Protocol("expected Programs".into())),
        }
    }

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

    pub fn open_editor(
        &mut self,
        bounds: Option<EditorBounds>,
        within: Duration,
    ) -> Result<EditorStatus, HostError> {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(self.child.id())
        };
        self.editor_call(&Request::OpenEditor { bounds }, within)
    }

    pub fn close_editor(&mut self, within: Duration) -> Result<EditorStatus, HostError> {
        self.editor_call(&Request::CloseEditor, within)
    }

    pub fn show_editor(
        &mut self,
        visible: bool,
        within: Duration,
    ) -> Result<EditorStatus, HostError> {
        self.editor_call(&Request::ShowEditor(visible), within)
    }

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

    pub fn capture_keys(&mut self, keys: Vec<u8>, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::CaptureKeys(keys), within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    pub fn set_param(&mut self, id: u32, value: f64, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::SetParam { id, value }, within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

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
