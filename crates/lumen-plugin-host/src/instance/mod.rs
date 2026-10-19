//! One plugin instance inside a host process: a component and its
//! controller, driven through the VST3 interfaces.
//!
//! Nothing here is called from Lumen's own audio thread. It runs in the host
//! process, which owns the plugin's threads and, when it dies, takes the
//! plugin with it.

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
    Aux, ClassId, ClassInfo, Edit, EditKind, Event, EventKind, MAX_CHANNELS, MAX_FRAMES,
    MAX_PARAMETERS, MAX_PROGRAM_LISTS, MAX_PROGRAMS, MAX_PROGRAMS_ALL, MAX_STATE, MAX_UNITS,
    ParamInfo, ParamQueue, ProgramList, Taken, UnitInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    /// The module has no class like the one asked for.
    NoClass,
    /// The plugin refused or failed a call.
    Plugin(String),
    /// The call does not fit the instance's state.
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

/// The processing format. To [`Instance::configure`] it gives the channels of
/// the main buses; what [`Instance::format`] returns holds the channels a
/// block carries each way (the main buses and every auxiliary bus turned on).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    pub max_frames: u32,
    pub inputs: u8,
    pub outputs: u8,
}

/// A bus that is turned on: which of the plugin's buses it is, how many
/// channels it has and where they start in a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    bus: usize,
    channels: u8,
    first: usize,
}

/// What one block produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Processed {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
    pub output: Vec<Vec<f32>>,
}

/// What a block's processing reported, apart from its audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
}

/// A stretch of a block: where it starts and how long it is, in frames.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    len: usize,
}

/// Channel buffers for [`Instance::process_into`].
pub struct Buffers {
    pub input: Vec<Vec<f32>>,
    pub output: Vec<Vec<f32>>,
}

pub struct Instance {
    pub class: ClassInfo,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    /// The controller is its own object, so it is initialised and ended
    /// separately from the component.
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
    /// Values to give the processor at the start of a block: edits made in
    /// the plugin's window and values set from Lumen.
    now: Vec<(u32, f64)>,
    /// The plugin's controller mapping (CC, bend, pressure to parameters).
    mapping: Option<ComPtr<IMidiMapping>>,
    /// What the mapping said for each channel and controller number: unknown
    /// (not asked yet), none, or a parameter id. Asked lazily, once.
    midi_map: Vec<i64>,
    /// This block's controller events as `(parameter id, offset in the
    /// block, normalized value)`.
    mapped: Vec<(u32, u32, f64)>,
    /// The plugin's program-change parameter and its step count, if it has
    /// one: a program change (MIDI) sets it to `program / steps`.
    program_param: Option<(u32, i32)>,
    /// Controller and program events no parameter took, since the last
    /// `take_edits`.
    ignored: u32,
    /// The processor's reported values after a block, before they are noted.
    reported: Vec<(u32, f64)>,
    /// Values the processor has heard that the controller has not: it is told
    /// from the control thread ([`Instance::take_edits`]), never from the
    /// audio thread.
    for_controller: Vec<(u32, f64)>,
    context: ProcessContext,
    /// How many audio buses the plugin declares each way. The process data
    /// carries an entry for every one, those that are off included.
    in_buses: usize,
    out_buses: usize,
    /// The buses turned on, in block order (set by `configure`).
    in_slots: Vec<Slot>,
    out_slots: Vec<Slot>,
    notes: NoteIds,
    /// Silence for the buses that are off to point at (see `empty_bus`).
    spare: Vec<f32>,
    format: Option<Format>,
    processing: bool,
    latency: u32,
    // Kept alive for the component's life.
    _host: ComPtr<vst3::Steinberg::Vst::IHostApplication>,
}

/// The most audio buses a plugin may declare each way. Multi-output
/// instruments declare a handful to a few dozen.
const MAX_BUSES: usize = 32;

/// A bus entry with no channels: what an off bus is given. Its buffer list is
/// never null: a plugin that walks the list of a bus it has off, without
/// looking at the count, would otherwise read through a null pointer, as one
/// widely used synthesizer does. The list points at silence.
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
        // SAFETY: a plain call on a live processor.
        unsafe { self.processor.getTailSamples() }.min(u32::MAX - 1)
    }

    /// Makes the plugin forget what it holds (a reverb's tail, a filter's
    /// memory): processing is stopped and started again, which is where the
    /// standard says a plugin clears its state. Runs on the audio path, like
    /// [`Instance::process_into`]; a plugin that cannot start again has
    /// failed.
    pub fn flush(&mut self) -> Result<(), InstanceError> {
        if !self.processing {
            return Ok(());
        }
        // SAFETY: calls on live objects, from the thread that processes.
        let result = unsafe {
            self.processor.setProcessing(0);
            self.processor.setProcessing(1)
        };
        if result != kResultOk && result != vst3::Steinberg::kNotImplemented {
            return Err(plugin(format!("setProcessing failed ({result})")));
        }
        Ok(())
    }

    /// Renders one block. `input` has one slice per input channel; the
    /// result's output has one per output channel, `frames` long. This
    /// allocates; the audio path uses [`Instance::process_into`].
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

    /// Renders one block in buffers the caller owns, allocating nothing of
    /// its own once the events and parameter lists have grown to the size of
    /// the busiest block. `buffers` holds one `Vec` per channel, each at
    /// least `frames` long; the output is zeroed first and, afterwards,
    /// anything not finite in it has been replaced by silence.
    ///
    /// A block longer than the plugin was configured for (a driver that
    /// delivered more than its stated buffer) is given to the plugin in
    /// pieces it accepts, each with the events and parameter points that
    /// fall in it: a plugin is never handed more than it was promised.
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

    /// One call of the plugin's `process` over `span` of the buffers, checked
    /// by [`Self::process_into`].
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
        // The pointers live on the stack: a main bus has at most
        // `MAX_CHANNELS` channels, checked when the format was set.
        let mut in_ptrs = [ptr::null_mut::<f32>(); MAX_CHANNELS];
        let mut out_ptrs = [ptr::null_mut::<f32>(); MAX_CHANNELS];
        for (slot, channel) in in_ptrs.iter_mut().zip(&mut buffers.input) {
            *slot = channel[start..].as_mut_ptr();
        }
        for (slot, channel) in out_ptrs.iter_mut().zip(&mut buffers.output) {
            channel[start..end].fill(0.0);
            *slot = channel[start..].as_mut_ptr();
        }

        // Controller, bend and pressure become parameter points through the
        // plugin's own mapping, and program changes through its program
        // parameter.
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
        // Past the room a span has for them, controller events are dropped,
        // and counted with those no parameter took.
        self.ignored = self
            .ignored
            .saturating_add(keep_most(&mut self.mapped, MOST_PENDING));
        // The controller hears what the processor is given through the
        // mapping too (a plugin's window follows a controller, and a program
        // change is carried out by the controller: it sets the parameters the
        // program holds).
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
        // The edits waiting go in the first piece of a block.
        let now: &[(u32, f64)] = if start == 0 { &self.now } else { &[] };
        self.changes
            .set_span(params, now, &self.mapped, start as u32, n as u32);

        // An entry for every bus the plugin declares; only the main bus of
        // each direction carries audio, the rest are off (no channels).
        // The silence the off buses point at: as long as a block can be.
        self.spare.resize(format.max_frames as usize, 0.0);
        let mut spare_ptrs = [self.spare.as_mut_ptr(); MAX_CHANNELS];
        let mut in_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_BUSES];
        let mut out_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_BUSES];
        // A slot's channels lie inside the pointer arrays: `configure` checked
        // the totals against `MAX_CHANNELS`, and the arrays hold that many.
        for slot in &self.in_slots {
            // SAFETY: `slot.first` is inside the array, as above.
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
            // SAFETY: `slot.first` is inside the array, as above.
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
        // SAFETY: every pointer in `data` refers to live locals, buffers the
        // caller lent for this call, or objects owned by the instance, and
        // outlives the call.
        let status = unsafe { self.processor.process(&mut data) };
        self.context.projectTimeSamples += n as i64;

        // A plugin's output is untrusted: anything not finite becomes
        // silence before it can reach a mixer.
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
        // A plugin's latency may change with a parameter, so it is read
        // after every block; `restart` tells the caller the plugin also
        // asked to be restarted (a graph rebuild is the caller's to do).
        // SAFETY: a plain call on a live processor.
        self.latency = unsafe { self.processor.getLatencySamples() };
        Done {
            status,
            latency: self.latency,
            restart,
            repaired,
        }
    }

    /// The controller's side of the plugin, for calls made without the lock
    /// the audio thread takes (see [`Controls`]).
    pub fn controls(&self) -> Controls {
        Controls {
            component: self.component.clone(),
            controller: self.controller.clone(),
            handler: self.handler.clone(),
        }
    }

    /// Gives the processor the values Lumen set since its last block. A value
    /// set while the plugin is not processing reaches the processor only in a
    /// block, so a state read taken before one would miss it. Runs one block of
    /// silence on the calling thread when values are waiting, and nothing
    /// otherwise. Call with the instance lock held, as for any control call.
    pub fn flush_parameters(&mut self) -> Result<(), InstanceError> {
        let Some(format) = self.format.filter(|_| self.processing) else {
            return Ok(());
        };
        if !self.handler.has_pending() {
            return Ok(());
        }
        let silence = vec![vec![0.0_f32; 1]; usize::from(format.inputs)];
        self.process(1, &silence, &[], &[]).map(|_| ())
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

    /// The edits made since the last call and how many were dropped, the
    /// controller told first what the processor heard (see
    /// [`Controls::take_edits`]).
    pub fn take_edits(&mut self) -> Taken {
        let heard = self.take_heard();
        self.controls().take_edits(heard)
    }

    /// What the processor heard that the controller has not, and the
    /// controller events no parameter took, since the last call: the part of
    /// taking edits that needs the instance (and so its lock).
    pub fn take_heard(&mut self) -> Heard {
        // Copied and cleared, not taken: the list keeps its room, and the
        // audio thread that fills it never allocates.
        let values = self.for_controller.to_vec();
        self.for_controller.clear();
        Heard {
            values,
            ignored: std::mem::take(&mut self.ignored),
        }
    }

    /// The parameter a controller number on a channel is mapped to.
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
                // SAFETY: a plain call on a live interface with a valid out pointer.
                let result = unsafe {
                    m.getMidiControllerAssignment(0, channel as i16, number as i16, &mut id)
                };
                result == kResultOk
            });
            self.midi_map[slot] = if found { i64::from(id) } else { MAP_NONE };
        }
        u32::try_from(self.midi_map[slot]).ok()
    }

    /// Notes the last value of each parameter in `params` for the controller.
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

    /// After a block: what the processor reported becomes edits for Lumen
    /// and a value for the controller.
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
        // SAFETY: the end of the plugin's life, in the order the SDK lists.
        unsafe {
            if self.separate_controller
                && let Some(c) = &self.controller
            {
                c.terminate();
            }
            self.component.terminate();
        }
    }
}

mod controls;
mod events;
mod setup;

#[cfg(test)]
mod tests;

/// Shortens `mapped` to `most` entries; how many it dropped.
fn keep_most<T>(mapped: &mut Vec<T>, most: usize) -> u32 {
    let over = mapped.len().saturating_sub(most);
    mapped.truncate(most);
    over as u32
}

pub use controls::{Controls, Heard};
use controls::{program_parameter, wide};
pub(crate) use events::NoteIds;
use events::{MAP_CHANNELS, MAP_NONE, MAP_NUMBERS, MAP_UNKNOWN, controller_value, vst_event};
