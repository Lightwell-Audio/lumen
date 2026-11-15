use std::fmt;
use std::ptr;
use std::sync::atomic::Ordering;

use vst3::Steinberg::Vst::BusDirections_::{kInput, kOutput};
use vst3::Steinberg::Vst::Event_::EventTypes_::{kNoteOffEvent, kNoteOnEvent};
use vst3::Steinberg::Vst::MediaTypes_::{kAudio, kEvent};
use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_::{
    kPlaying, kTempoValid, kTimeSigValid,
};
use vst3::Steinberg::Vst::ProcessModes_::kRealtime;
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusInfo, Event as VstEvent, Event__type0,
    IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentHandler, IComponentTrait,
    IConnectionPoint, IConnectionPointTrait, IEditController, IEditControllerTrait, IEventList,
    IMidiMapping, IMidiMappingTrait, IParameterChanges, IUnitInfo, IUnitInfoTrait, NoteOffEvent,
    NoteOnEvent, ParameterInfo, ProcessContext, ProcessData, ProcessSetup, ProgramListInfo,
    SpeakerArr, SpeakerArrangement, UnitInfo as VstUnitInfo,
};
use vst3::Steinberg::{
    IPlugView, IPluginBaseTrait, IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, TUID,
    kResultOk,
};
use vst3::{ComPtr, ComWrapper, Interface};

use crate::com::{self, Changes, EventList, Handler, MOST_PENDING, MemoryStream, OutputChanges};
use crate::module::{AUDIO_CLASS, Module, to_tuid};
use crate::protocol::{
    Aux, ClassId, ClassInfo, Edit, EditKind, Event, EventKind, MAX_BUSES, MAX_CHANNELS, MAX_FRAMES,
    MAX_PARAMETERS, MAX_PROGRAM_LISTS, MAX_PROGRAMS, MAX_PROGRAMS_ALL, MAX_STATE, MAX_UNITS,
    ParamInfo, ParamQueue, ProgramList, Taken, UnitInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    NoClass,
    Plugin(String),
    Sequence(&'static str),
}

impl fmt::Display for InstanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstanceError::NoClass => write!(f, "the module has no such audio class"),
            InstanceError::Plugin(what) => write!(f, "{what}"),
            InstanceError::Sequence(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for InstanceError {}

fn plugin(what: impl Into<String>) -> InstanceError {
    InstanceError::Plugin(what.into())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    pub max_frames: u32,
    pub inputs: u8,
    pub outputs: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    bus: usize,
    channels: u8,
    first: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Processed {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
    pub output: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    len: usize,
}

pub struct Buffers {
    pub input: Vec<Vec<f32>>,
    pub output: Vec<Vec<f32>>,
}

pub struct Instance {
    pub class: ClassInfo,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    handler: ComWrapper<Handler>,
    events_in: ComWrapper<EventList>,
    events_in_ptr: ComPtr<IEventList>,
    events_out: ComWrapper<EventList>,
    events_out_ptr: ComPtr<IEventList>,
    changes: ComWrapper<Changes>,
    changes_ptr: ComPtr<IParameterChanges>,
    outputs: ComWrapper<OutputChanges>,
    outputs_ptr: ComPtr<IParameterChanges>,
    now: Vec<(u32, f64)>,
    mapping: Option<ComPtr<IMidiMapping>>,
    midi_map: Vec<i64>,
    mapped: Vec<(u32, u32, f64)>,
    program_param: Option<(u32, i32)>,
    ignored: u32,
    reported: Vec<(u32, f64)>,
    for_controller: Vec<(u32, f64)>,
    context: ProcessContext,
    in_buses: usize,
    out_buses: usize,
    in_slots: Vec<Slot>,
    out_slots: Vec<Slot>,
    notes: NoteIds,
    spare: Vec<f32>,
    format: Option<Format>,
    processing: bool,
    latency: u32,
    _host: ComPtr<vst3::Steinberg::Vst::IHostApplication>,
}

const MAX_DECLARED_BUSES: usize = 128;

fn empty_bus(buffers: *mut *mut f32) -> AudioBusBuffers {
    AudioBusBuffers {
        numChannels: 0,
        silenceFlags: 0,
        __field0: AudioBusBuffers__type0 {
            channelBuffers32: buffers,
        },
    }
}

fn speakers(channels: u8) -> Result<SpeakerArrangement, InstanceError> {
    match channels {
        1 => Ok(SpeakerArr::kMono),
        2 => Ok(SpeakerArr::kStereo),
        n => Err(plugin(format!("{n} channels is not a supported main bus"))),
    }
}

impl Instance {
    pub fn format(&self) -> Option<Format> {
        self.format
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn tail(&self) -> u32 {
        unsafe { self.processor.getTailSamples() }.min(u32::MAX - 1)
    }

    pub fn flush(&mut self) -> Result<(), InstanceError> {
        if !self.processing {
            return Ok(());
        }
        let result = unsafe {
            self.processor.setProcessing(0);
            self.processor.setProcessing(1)
        };
        if result != kResultOk && result != vst3::Steinberg::kNotImplemented {
            return Err(plugin(format!("setProcessing failed ({result})")));
        }
        Ok(())
    }

    pub fn process(
        &mut self,
        frames: u32,
        input: &[Vec<f32>],
        events: &[Event],
        params: &[ParamQueue],
    ) -> Result<Processed, InstanceError> {
        let format = self
            .format
            .filter(|_| self.processing)
            .ok_or(InstanceError::Sequence("not configured"))?;
        if frames == 0 || frames > format.max_frames {
            return Err(plugin("block longer than the configured maximum"));
        }
        if input.len() != usize::from(format.inputs) {
            return Err(plugin("input channels do not match the format"));
        }
        let n = frames as usize;
        let mut buffers = Buffers {
            input: input.to_vec(),
            output: vec![vec![0.0; n]; usize::from(format.outputs)],
        };
        let done = self.process_into(frames, &mut buffers, events, params)?;
        Ok(Processed {
            status: done.status,
            latency: done.latency,
            restart: done.restart,
            repaired: done.repaired,
            output: buffers.output,
        })
    }

    pub fn process_into(
        &mut self,
        frames: u32,
        buffers: &mut Buffers,
        events: &[Event],
        params: &[ParamQueue],
    ) -> Result<Done, InstanceError> {
        let format = self
            .format
            .filter(|_| self.processing)
            .ok_or(InstanceError::Sequence("not configured"))?;
        if frames == 0 || frames as usize > MAX_FRAMES {
            return Err(plugin("block longer than the protocol allows"));
        }
        let n = frames as usize;
        if buffers.input.len() != usize::from(format.inputs)
            || buffers.output.len() != usize::from(format.outputs)
            || buffers.input.iter().any(|c| c.len() < n)
            || buffers.output.iter().any(|c| c.len() < n)
        {
            return Err(plugin("buffers do not match the format"));
        }
        let most = (format.max_frames as usize).max(1);
        self.now.clear();
        self.handler.drain_to_processor(&mut self.now);
        for (id, _) in &self.now {
            self.for_controller.retain(|(i, _)| i != id);
        }
        self.note_for_controller(params);
        let mut done = Done {
            status: 0,
            latency: self.latency,
            restart: 0,
            repaired: 0,
        };
        let mut start = 0;
        while start < n {
            let span = Span {
                start,
                len: most.min(n - start),
            };
            let piece = self.process_span(format, span, buffers, events, params);
            if done.status == 0 {
                done.status = piece.status;
            }
            done.latency = piece.latency;
            done.restart |= piece.restart;
            done.repaired += piece.repaired;
            start += span.len;
        }
        Ok(done)
    }

    #[allow(
        clippy::unnecessary_cast,
        reason = "the bindings' enum constants are `i32` on Windows and `u32` elsewhere, so the cast one platform needs is a no-op on the other"
    )]
    fn process_span(
        &mut self,
        format: Format,
        span: Span,
        buffers: &mut Buffers,
        events: &[Event],
        params: &[ParamQueue],
    ) -> Done {
        let Span { start, len: n } = span;
        let end = start + n;
        let mut in_ptrs = [ptr::null_mut::<f32>(); MAX_CHANNELS];
        let mut out_ptrs = [ptr::null_mut::<f32>(); MAX_CHANNELS];
        for (slot, channel) in in_ptrs.iter_mut().zip(&mut buffers.input) {
            *slot = channel[start..].as_mut_ptr();
        }
        for (slot, channel) in out_ptrs.iter_mut().zip(&mut buffers.output) {
            channel[start..end].fill(0.0);
            *slot = channel[start..].as_mut_ptr();
        }

        self.mapped.clear();
        for e in events
            .iter()
            .filter(|e| (start..end).contains(&(e.offset as usize)))
        {
            if let EventKind::Program { program, .. } = e.kind {
                match self.program_param {
                    Some((id, steps)) if steps > 0 => {
                        let value =
                            f64::from(i32::from(program).clamp(0, steps)) / f64::from(steps);
                        self.mapped.push((id, e.offset, value));
                    }
                    _ => self.ignored = self.ignored.saturating_add(1),
                }
            } else if let Some((channel, number, value)) = controller_value(&e.kind) {
                match self.mapped_param(channel, number) {
                    Some(id) => self.mapped.push((id, e.offset, value)),
                    None => self.ignored = self.ignored.saturating_add(1),
                }
            }
        }
        self.ignored = self
            .ignored
            .saturating_add(keep_most(&mut self.mapped, MOST_PENDING));
        for i in 0..self.mapped.len() {
            let (id, _, value) = self.mapped[i];
            self.note(id, value);
        }
        let notes = &mut self.notes;
        self.events_in.set(
            events
                .iter()
                .filter(|e| (start..end).contains(&(e.offset as usize)))
                .filter_map(|e| {
                    let mut event = vst_event(e, notes)?;
                    event.sampleOffset -= start as i32;
                    Some(event)
                }),
        );
        self.events_out.set([]);
        let now: &[(u32, f64)] = if start == 0 { &self.now } else { &[] };
        self.changes
            .set_span(params, now, &self.mapped, start as u32, n as u32);

        self.spare.resize(format.max_frames as usize, 0.0);
        let mut spare_ptrs = [self.spare.as_mut_ptr(); MAX_CHANNELS];
        let mut in_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_DECLARED_BUSES];
        let mut out_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_DECLARED_BUSES];
        for slot in &self.in_slots {
            let list = unsafe { in_ptrs.as_mut_ptr().add(slot.first) };
            in_buses[slot.bus] = AudioBusBuffers {
                numChannels: i32::from(slot.channels),
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: list,
                },
            };
        }
        for slot in &self.out_slots {
            let list = unsafe { out_ptrs.as_mut_ptr().add(slot.first) };
            out_buses[slot.bus] = AudioBusBuffers {
                numChannels: i32::from(slot.channels),
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: list,
                },
            };
        }
        let mut data = ProcessData {
            processMode: kRealtime as i32,
            symbolicSampleSize: kSample32 as i32,
            numSamples: n as i32,
            numInputs: self.in_buses as i32,
            numOutputs: self.out_buses as i32,
            inputs: if self.in_buses == 0 {
                ptr::null_mut()
            } else {
                in_buses.as_mut_ptr()
            },
            outputs: if self.out_buses == 0 {
                ptr::null_mut()
            } else {
                out_buses.as_mut_ptr()
            },
            inputParameterChanges: self.changes_ptr.as_ptr(),
            outputParameterChanges: self.outputs_ptr.as_ptr(),
            inputEvents: self.events_in_ptr.as_ptr(),
            outputEvents: self.events_out_ptr.as_ptr(),
            processContext: &mut self.context,
        };
        let status = unsafe { self.processor.process(&mut data) };
        self.context.projectTimeSamples += n as i64;

        let mut repaired = 0;
        for channel in &mut buffers.output {
            for s in &mut channel[start..end] {
                if !s.is_finite() {
                    *s = 0.0;
                    repaired += 1;
                }
            }
        }
        self.collect_outputs();
        let restart = self.handler.restart.swap(0, Ordering::AcqRel);
        self.latency = unsafe { self.processor.getLatencySamples() };
        Done {
            status,
            latency: self.latency,
            restart,
            repaired,
        }
    }

    pub fn controls(&self) -> Controls {
        Controls {
            component: self.component.clone(),
            controller: self.controller.clone(),
            handler: self.handler.clone(),
        }
    }

    pub fn create_editor_view(&self) -> Option<ComPtr<IPlugView>> {
        self.controls().create_editor_view()
    }

    pub fn state(&self) -> Result<Vec<u8>, InstanceError> {
        self.controls().state()
    }

    pub fn set_state(&self, state: &[u8]) -> Result<(), InstanceError> {
        self.controls().set_state(state)
    }

    pub fn parameters(&self) -> (u32, Vec<ParamInfo>) {
        self.controls().parameters()
    }

    pub fn units(&self) -> Vec<UnitInfo> {
        self.controls().units()
    }

    pub fn param_text(&self, id: u32, value: f64) -> Option<String> {
        self.controls().param_text(id, value)
    }

    pub fn param_value(&self, id: u32, text: &str) -> Option<f64> {
        self.controls().param_value(id, text)
    }

    pub fn set_param(&self, id: u32, value: f64) {
        self.controls().set_param(id, value);
    }

    pub fn take_edits(&mut self) -> Taken {
        let heard = self.take_heard();
        self.controls().take_edits(heard)
    }

    pub fn take_heard(&mut self) -> Heard {
        let mut values = self.for_controller.to_vec();
        self.for_controller.clear();
        values.retain(|(id, _)| !self.handler.is_queued(*id));
        Heard {
            values,
            ignored: std::mem::take(&mut self.ignored),
        }
    }

    fn mapped_param(&mut self, channel: i16, number: i16) -> Option<u32> {
        let (channel, number) = (
            usize::try_from(channel).ok()?,
            usize::try_from(number).ok()?,
        );
        if channel >= MAP_CHANNELS || number >= MAP_NUMBERS {
            return None;
        }
        let slot = channel * MAP_NUMBERS + number;
        if self.midi_map[slot] == MAP_UNKNOWN {
            let mut id: u32 = 0;
            let found = self.mapping.as_ref().is_some_and(|m| {
                let result = unsafe {
                    m.getMidiControllerAssignment(0, channel as i16, number as i16, &mut id)
                };
                result == kResultOk
            });
            self.midi_map[slot] = if found { i64::from(id) } else { MAP_NONE };
        }
        u32::try_from(self.midi_map[slot]).ok()
    }

    fn note_for_controller(&mut self, params: &[ParamQueue]) {
        for queue in params {
            if let Some(&(_, value)) = queue.points.last() {
                self.note(queue.id, value);
            }
        }
    }

    fn note(&mut self, id: u32, value: f64) {
        match self.for_controller.iter().position(|(i, _)| *i == id) {
            Some(at) => self.for_controller[at].1 = value,
            None if self.for_controller.len() < MOST_PENDING => {
                self.for_controller.push((id, value));
            }
            None => {}
        }
    }

    fn collect_outputs(&mut self) {
        self.reported.clear();
        self.outputs.take(&mut self.reported);
        for i in 0..self.reported.len() {
            let (id, value) = self.reported[i];
            self.handler.report(Edit {
                id,
                value,
                kind: EditKind::Output,
            });
            self.note(id, value);
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            if self.separate_controller
                && let Some(controller) = &self.controller
                && let (Some(component_point), Some(controller_point)) = (
                    self.component
                        .cast::<vst3::Steinberg::Vst::IConnectionPoint>(),
                    controller.cast::<vst3::Steinberg::Vst::IConnectionPoint>(),
                )
            {
                component_point.disconnect(controller_point.as_ptr());
                controller_point.disconnect(component_point.as_ptr());
            }

            self.component.terminate();
            if self.separate_controller
                && let Some(controller) = &self.controller
            {
                controller.terminate();
            }
        }
    }
}

mod controls;
mod events;
mod setup;

#[cfg(test)]
mod tests;

fn keep_most<T>(mapped: &mut Vec<T>, most: usize) -> u32 {
    let over = mapped.len().saturating_sub(most);
    mapped.truncate(most);
    over as u32
}

pub use controls::{Controls, Heard};
use controls::{program_parameter, wide};
pub(crate) use events::NoteIds;
use events::{MAP_CHANNELS, MAP_NONE, MAP_NUMBERS, MAP_UNKNOWN, controller_value, vst_event};
