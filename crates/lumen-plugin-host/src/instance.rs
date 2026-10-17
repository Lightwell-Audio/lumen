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
    IParameterChanges, NoteOffEvent, NoteOnEvent, ParameterInfo, ProcessContext, ProcessData,
    ProcessSetup, SpeakerArr, SpeakerArrangement,
};
use vst3::Steinberg::{
    IPlugView, IPluginBaseTrait, IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, TUID,
    kResultOk,
};
use vst3::{ComPtr, ComWrapper, Interface};

use crate::com::{self, Changes, EventList, Handler, MemoryStream, NoOutputChanges};
use crate::module::{AUDIO_CLASS, Module, to_tuid};
use crate::protocol::{
    ClassId, ClassInfo, Event, EventKind, MAX_CHANNELS, MAX_FRAMES, MAX_PARAMETERS, MAX_STATE,
    ParamInfo, ParamQueue,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    pub max_frames: u32,
    pub inputs: u8,
    pub outputs: u8,
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
    no_changes_out: ComPtr<IParameterChanges>,
    context: ProcessContext,
    /// How many audio buses the plugin declares each way. The process data
    /// carries an entry for every one, those that are off included.
    in_buses: usize,
    out_buses: usize,
    notes: NoteIds,
    /// Silence for the buses that are off to point at (see `empty_bus`).
    spare: Vec<f32>,
    format: Option<Format>,
    processing: bool,
    latency: u32,
    // Kept alive for the component's life.
    _host: ComPtr<vst3::Steinberg::Vst::IHostApplication>,
}

/// Hands out the ids that tie a note-off to its note-on: samplers key their
/// voices by note id, and a host that gives every event -1 leaves them
/// guessing which voice to release.
pub(crate) struct NoteIds {
    /// The id of the note sounding on each channel and pitch, or -1.
    held: Vec<i32>,
    next: i32,
}

impl NoteIds {
    const CHANNELS: usize = 16;
    const PITCHES: usize = 128;

    pub(crate) fn new() -> Self {
        Self {
            held: vec![-1; Self::CHANNELS * Self::PITCHES],
            next: 0,
        }
    }

    fn slot(channel: i16, pitch: i16) -> Option<usize> {
        let (c, p) = (usize::try_from(channel).ok()?, usize::try_from(pitch).ok()?);
        (c < Self::CHANNELS && p < Self::PITCHES).then_some(c * Self::PITCHES + p)
    }

    /// A new id for a note starting on `channel` and `pitch`.
    pub(crate) fn on(&mut self, channel: i16, pitch: i16) -> i32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(0);
        if let Some(slot) = Self::slot(channel, pitch) {
            self.held[slot] = id;
        }
        id
    }

    /// The id the note ending on `channel` and `pitch` began with, or -1
    /// when it is not known.
    pub(crate) fn off(&mut self, channel: i16, pitch: i16) -> i32 {
        Self::slot(channel, pitch).map_or(-1, |slot| std::mem::replace(&mut self.held[slot], -1))
    }

    /// Forgets every note (the plugin was stopped).
    pub(crate) fn clear(&mut self) {
        self.held.fill(-1);
    }
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
    /// Creates the first audio class of `module` (or the one named).
    pub fn create(module: &Module, class: Option<ClassId>) -> Result<Self, InstanceError> {
        let info = module
            .classes()
            .into_iter()
            .find(|c| match class {
                Some(id) => c.id == id,
                None => c.category == AUDIO_CLASS,
            })
            .ok_or(InstanceError::NoClass)?;
        if info.category != AUDIO_CLASS {
            return Err(InstanceError::NoClass);
        }

        // The host context goes to the factory first, for the plugins that
        // keep it (the VST3 hosting notes ask for it; most ignore it).
        let host = com::host_application();
        if let Some(factory) = module.factory.cast::<IPluginFactory3>() {
            // SAFETY: the context outlives the factory's use of it (the
            // instance owns it, and the module is never unloaded).
            unsafe { factory.setHostContext(host.as_ptr().cast()) };
        }

        let create =
            |cid: &ClassId, iid: &[u8; 16]| -> Result<*mut std::ffi::c_void, InstanceError> {
                let mut object = ptr::null_mut();
                let cid: TUID = to_tuid(cid);
                // SAFETY: valid class and interface ids; `object` receives an
                // owned reference on success.
                let result = unsafe {
                    module
                        .factory
                        .createInstance(cid.as_ptr(), iid.as_ptr().cast(), &mut object)
                };
                if result == kResultOk && !object.is_null() {
                    Ok(object)
                } else {
                    Err(plugin(format!("createInstance failed ({result})")))
                }
            };

        let object = create(&info.id, &IComponent::IID)?;
        // SAFETY: an owned IComponent reference from createInstance.
        let component = unsafe { ComPtr::<IComponent>::from_raw(object.cast()) }
            .ok_or_else(|| plugin("no component"))?;
        // SAFETY: the host context outlives the component (both are owned
        // by the instance).
        let result = unsafe { component.initialize(host.as_ptr().cast()) };
        if result != kResultOk {
            return Err(plugin(format!("initialize failed ({result})")));
        }
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or_else(|| plugin("the component has no audio processor"))?;

        // The controller: the component itself (a single-component plugin)
        // or a class of its own.
        let mut separate_controller = false;
        let mut controller = component.cast::<IEditController>();
        if controller.is_none() {
            let mut cid: TUID = [0; 16];
            // SAFETY: a valid out pointer.
            if unsafe { component.getControllerClassId(&mut cid) } == kResultOk {
                let id: ClassId = cid.map(|c| c as u8);
                if let Ok(object) = create(&id, &IEditController::IID) {
                    // SAFETY: an owned IEditController reference.
                    controller = unsafe { ComPtr::<IEditController>::from_raw(object.cast()) };
                    if let Some(c) = &controller {
                        // SAFETY: as for the component.
                        let r = unsafe { c.initialize(host.as_ptr().cast()) };
                        if r == kResultOk {
                            separate_controller = true;
                        } else {
                            controller = None;
                        }
                    }
                }
            }
        }

        let handler = ComWrapper::new(Handler::default());
        if let Some(c) = &controller {
            if let Some(h) = handler.to_com_ptr::<IComponentHandler>() {
                // SAFETY: the handler outlives the controller (owned by the instance).
                unsafe { c.setComponentHandler(h.as_ptr()) };
            }
            if separate_controller
                && let (Some(a), Some(b)) = (
                    component.cast::<IConnectionPoint>(),
                    c.cast::<IConnectionPoint>(),
                )
            {
                // SAFETY: both are live connection points.
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
        }

        let events_in = ComWrapper::new(EventList::default());
        let events_in_ptr = events_in.to_com_ptr::<IEventList>().expect("listed");
        let events_out = ComWrapper::new(EventList::default());
        let events_out_ptr = events_out.to_com_ptr::<IEventList>().expect("listed");
        let changes = ComWrapper::new(Changes::new());
        let changes_ptr = changes.to_com_ptr::<IParameterChanges>().expect("listed");
        let no_changes_out = ComWrapper::new(NoOutputChanges::new())
            .to_com_ptr::<IParameterChanges>()
            .expect("listed");

        Ok(Self {
            class: info,
            component,
            processor,
            controller,
            separate_controller,
            handler,
            events_in,
            events_in_ptr,
            events_out,
            events_out_ptr,
            changes,
            changes_ptr,
            no_changes_out,
            // SAFETY: a plain C struct; zero is a valid "nothing known" context.
            context: unsafe { std::mem::zeroed() },
            in_buses: 0,
            out_buses: 0,
            notes: NoteIds::new(),
            spare: Vec::new(),
            format: None,
            processing: false,
            latency: 0,
            _host: host,
        })
    }

    pub fn format(&self) -> Option<Format> {
        self.format
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// The plugin's editor view, if its controller offers one. Its window is
    /// made, used and destroyed on one thread (the host's window thread):
    /// the view must not be shared with the audio or control threads.
    pub fn create_editor_view(&self) -> Option<ComPtr<IPlugView>> {
        let controller = self.controller.as_ref()?;
        // SAFETY: a plain call on a live controller; a non-null result is a
        // reference the caller owns.
        let raw = unsafe { controller.createView(c"editor".as_ptr()) };
        // SAFETY: as above.
        unsafe { ComPtr::from_raw(raw) }
    }

    pub fn tail(&self) -> u32 {
        // SAFETY: a plain call on a live processor.
        unsafe { self.processor.getTailSamples() }.min(u32::MAX - 1)
    }

    fn stop(&mut self) {
        self.notes.clear();
        if self.processing {
            // SAFETY: shutdown calls on live objects.
            unsafe {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.processing = false;
        }
    }

    /// Sets the format and starts processing. Safe to call again with a new
    /// format: the plugin is stopped first.
    pub fn configure(&mut self, format: Format) -> Result<(), InstanceError> {
        self.stop();
        let ins = usize::from(format.inputs);
        let outs = usize::from(format.outputs);
        if ins > MAX_CHANNELS || outs > MAX_CHANNELS {
            return Err(plugin("too many channels"));
        }
        // One arrangement per bus, as the plugin declares them: the main bus
        // as asked, the others as they are (they stay off). A count that does
        // not match what the plugin declares is refused by plugins that
        // check, so the host never gives a shorter list.
        // SAFETY: plain calls on live objects.
        let (in_buses, out_buses) = unsafe {
            (
                self.component
                    .getBusCount(kAudio as i32, kInput as i32)
                    .max(0) as usize,
                self.component
                    .getBusCount(kAudio as i32, kOutput as i32)
                    .max(0) as usize,
            )
        };
        if in_buses > MAX_BUSES || out_buses > MAX_BUSES {
            return Err(plugin(format!(
                "the plugin declares {} audio buses; at most {MAX_BUSES} are supported",
                in_buses.max(out_buses)
            )));
        }
        let layout = |direction: i32,
                      buses: usize,
                      wanted: usize|
         -> Result<Vec<SpeakerArrangement>, InstanceError> {
            (0..buses)
                .map(|bus| {
                    if bus == 0 && wanted > 0 {
                        return speakers(wanted as u8);
                    }
                    let mut current: SpeakerArrangement = SpeakerArr::kStereo;
                    // SAFETY: a valid out pointer; a refusal leaves stereo.
                    unsafe {
                        self.processor
                            .getBusArrangement(direction, bus as i32, &mut current)
                    };
                    Ok(current)
                })
                .collect()
        };
        let mut input_layout = layout(kInput as i32, in_buses, ins)?;
        let mut output_layout = layout(kOutput as i32, out_buses, outs)?;
        // SAFETY: valid pointers to vectors that outlive the call, with the
        // counts they hold.
        unsafe {
            self.processor.setBusArrangements(
                if in_buses > 0 {
                    input_layout.as_mut_ptr()
                } else {
                    ptr::null_mut()
                },
                in_buses as i32,
                if out_buses > 0 {
                    output_layout.as_mut_ptr()
                } else {
                    ptr::null_mut()
                },
                out_buses as i32,
            );
        }
        self.in_buses = in_buses;
        self.out_buses = out_buses;
        // The plugin may refuse a layout; what it really has must match
        // what was asked, or blocks would be mis-sized.
        for (direction, wanted) in [(kInput as i32, ins), (kOutput as i32, outs)] {
            // SAFETY: plain calls with valid out pointers.
            let have = unsafe {
                if self.component.getBusCount(kAudio as i32, direction) > 0 {
                    let mut info: BusInfo = std::mem::zeroed();
                    if self
                        .component
                        .getBusInfo(kAudio as i32, direction, 0, &mut info)
                        == kResultOk
                    {
                        usize::try_from(info.channelCount).unwrap_or(usize::MAX)
                    } else {
                        usize::MAX
                    }
                } else {
                    0
                }
            };
            // A plugin with a main bus can run it unused, but not with a
            // different count than the host fills.
            if wanted > 0 && have != wanted {
                return Err(plugin(format!(
                    "the plugin's main bus has {have} channels, not {wanted}"
                )));
            }
        }
        // SAFETY: valid pointers to locals for each call.
        unsafe {
            let mut setup = ProcessSetup {
                processMode: kRealtime as i32,
                symbolicSampleSize: kSample32 as i32,
                maxSamplesPerBlock: format.max_frames as i32,
                sampleRate: format.sample_rate,
            };
            let result = self.processor.setupProcessing(&mut setup);
            if result != kResultOk {
                return Err(plugin(format!("setupProcessing failed ({result})")));
            }
            for (media, direction, active) in [
                (kAudio as i32, kInput as i32, ins > 0),
                (kAudio as i32, kOutput as i32, outs > 0),
                (kEvent as i32, kInput as i32, true),
                (kEvent as i32, kOutput as i32, true),
            ] {
                if self.component.getBusCount(media, direction) > 0 {
                    self.component
                        .activateBus(media, direction, 0, u8::from(active));
                }
            }
            let result = self.component.setActive(1);
            if result != kResultOk {
                return Err(plugin(format!("setActive failed ({result})")));
            }
            let result = self.processor.setProcessing(1);
            // Plugins may answer "not implemented" here and still work.
            if result != kResultOk && result != vst3::Steinberg::kNotImplemented {
                self.component.setActive(0);
                return Err(plugin(format!("setProcessing failed ({result})")));
            }
            self.latency = self.processor.getLatencySamples();
        }
        self.context.sampleRate = format.sample_rate;
        self.context.tempo = 120.0;
        // A time signature even where nothing is known: a plugin that
        // divides by the denominator without looking at the flag must not
        // meet zero.
        self.context.timeSigNumerator = 4;
        self.context.timeSigDenominator = 4;
        self.context.state = (kPlaying | kTempoValid | kTimeSigValid) as u32;
        self.context.projectTimeSamples = 0;
        self.handler.restart.store(0, Ordering::Release);
        self.format = Some(format);
        self.processing = true;
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
        let (ins, outs) = (buffers.input.len(), buffers.output.len());

        let notes = &mut self.notes;
        self.events_in.set(
            events
                .iter()
                .filter(|e| (start..end).contains(&(e.offset as usize)))
                .map(|e| {
                    let mut event = vst_event(e, notes);
                    event.sampleOffset -= start as i32;
                    event
                }),
        );
        self.events_out.set([]);
        self.changes.set_span(params, start as u32, n as u32);

        // An entry for every bus the plugin declares; only the main bus of
        // each direction carries audio, the rest are off (no channels).
        // The silence the off buses point at: as long as a block can be.
        self.spare.resize(format.max_frames as usize, 0.0);
        let mut spare_ptrs = [self.spare.as_mut_ptr(); MAX_CHANNELS];
        let mut in_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_BUSES];
        let mut out_buses = [empty_bus(spare_ptrs.as_mut_ptr()); MAX_BUSES];
        if ins > 0 {
            in_buses[0] = AudioBusBuffers {
                numChannels: ins as i32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: in_ptrs.as_mut_ptr(),
                },
            };
        }
        if outs > 0 {
            out_buses[0] = AudioBusBuffers {
                numChannels: outs as i32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: out_ptrs.as_mut_ptr(),
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
            outputParameterChanges: self.no_changes_out.as_ptr(),
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

    /// The component's saved state.
    pub fn state(&self) -> Result<Vec<u8>, InstanceError> {
        let (stream, ptr) = MemoryStream::open(Vec::new(), MAX_STATE);
        // SAFETY: a live stream object.
        let result = unsafe { self.component.getState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!("getState failed ({result})")));
        }
        Ok(stream.bytes())
    }

    /// Restores the component, then tells the controller what it holds.
    pub fn set_state(&self, state: &[u8]) -> Result<(), InstanceError> {
        let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
        // SAFETY: a live stream object.
        let result = unsafe { self.component.setState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!("setState failed ({result})")));
        }
        if let Some(controller) = &self.controller {
            let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
            // SAFETY: a live stream object. A controller may decline.
            unsafe { controller.setComponentState(ptr.as_ptr()) };
        }
        Ok(())
    }

    /// The controller's parameters, capped; and the plugin's own count.
    pub fn parameters(&self) -> (u32, Vec<ParamInfo>) {
        let Some(controller) = &self.controller else {
            return (0, Vec::new());
        };
        // SAFETY: plain calls on a live controller with valid out pointers.
        unsafe {
            let total = controller.getParameterCount().max(0) as usize;
            let list = (0..total.min(MAX_PARAMETERS) as i32)
                .filter_map(|i| {
                    let mut info: ParameterInfo = std::mem::zeroed();
                    (controller.getParameterInfo(i, &mut info) == kResultOk).then(|| ParamInfo {
                        id: info.id,
                        title: wide(&info.title),
                        units: wide(&info.units),
                        steps: info.stepCount,
                        default: info.defaultNormalizedValue,
                    })
                })
                .collect();
            (total as u32, list)
        }
    }
}

fn wide(buf: &[u16]) -> String {
    let units: Vec<u16> = buf.iter().copied().take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&units)
}

fn vst_event(event: &Event, ids: &mut NoteIds) -> VstEvent {
    let (kind, body) = match event.kind {
        EventKind::NoteOn {
            channel,
            pitch,
            velocity,
        } => (
            kNoteOnEvent,
            Event__type0 {
                noteOn: NoteOnEvent {
                    channel,
                    pitch,
                    tuning: 0.0,
                    velocity,
                    length: 0,
                    noteId: ids.on(channel, pitch),
                },
            },
        ),
        EventKind::NoteOff {
            channel,
            pitch,
            velocity,
        } => (
            kNoteOffEvent,
            Event__type0 {
                noteOff: NoteOffEvent {
                    channel,
                    pitch,
                    velocity,
                    noteId: ids.off(channel, pitch),
                    tuning: 0.0,
                },
            },
        ),
    };
    VstEvent {
        busIndex: 0,
        sampleOffset: event.offset as i32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: kind as u16,
        __field0: body,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_off_carries_the_id_its_note_on_was_given() {
        let mut ids = NoteIds::new();
        let a = ids.on(0, 60);
        let b = ids.on(0, 64);
        assert_ne!(a, b);
        assert_eq!(ids.off(0, 64), b);
        assert_eq!(ids.off(0, 60), a);
        // Ended, or never started: not known.
        assert_eq!(ids.off(0, 60), -1);
        assert_eq!(ids.off(3, 10), -1);
        // Out of range is allowed, and has no id to find again.
        let wild = ids.on(40, 300);
        assert!(wild >= 0);
        assert_eq!(ids.off(40, 300), -1);
        ids.on(1, 1);
        ids.clear();
        assert_eq!(ids.off(1, 1), -1);
    }
}
