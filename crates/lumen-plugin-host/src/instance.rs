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
use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_::{kPlaying, kTempoValid};
use vst3::Steinberg::Vst::ProcessModes_::kRealtime;
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusInfo, Event as VstEvent, Event__type0,
    IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentHandler, IComponentTrait,
    IConnectionPoint, IConnectionPointTrait, IEditController, IEditControllerTrait, IEventList,
    IParameterChanges, NoteOffEvent, NoteOnEvent, ParameterInfo, ProcessContext, ProcessData,
    ProcessSetup, SpeakerArr, SpeakerArrangement,
};
use vst3::Steinberg::{IPluginBaseTrait, IPluginFactoryTrait, TUID, kResultOk};
use vst3::{ComPtr, ComWrapper, Interface};

use crate::com::{self, Changes, EventList, Handler, MemoryStream, NoOutputChanges};
use crate::module::{AUDIO_CLASS, Module, to_tuid};
use crate::protocol::{
    ClassId, ClassInfo, Event, EventKind, MAX_CHANNELS, MAX_PARAMETERS, MAX_STATE, ParamInfo,
    ParamQueue,
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
    format: Option<Format>,
    processing: bool,
    latency: u32,
    // Kept alive for the component's life.
    _host: ComPtr<vst3::Steinberg::Vst::IHostApplication>,
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
        let host = com::host_application();
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
        let no_changes_out = ComWrapper::new(NoOutputChanges)
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

    pub fn tail(&self) -> u32 {
        // SAFETY: a plain call on a live processor.
        unsafe { self.processor.getTailSamples() }.min(u32::MAX - 1)
    }

    fn stop(&mut self) {
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
        let mut input_layout: [SpeakerArrangement; 1] = [if format.inputs > 0 {
            speakers(format.inputs)?
        } else {
            SpeakerArr::kEmpty
        }];
        let mut output_layout: [SpeakerArrangement; 1] = [if format.outputs > 0 {
            speakers(format.outputs)?
        } else {
            SpeakerArr::kEmpty
        }];
        // SAFETY: valid pointers to locals for the duration of each call.
        unsafe {
            self.processor.setBusArrangements(
                if ins > 0 {
                    input_layout.as_mut_ptr()
                } else {
                    ptr::null_mut()
                },
                i32::from(ins > 0),
                if outs > 0 {
                    output_layout.as_mut_ptr()
                } else {
                    ptr::null_mut()
                },
                i32::from(outs > 0),
            );
        }
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
        self.context.state = (kPlaying | kTempoValid) as u32;
        self.context.projectTimeSamples = 0;
        self.handler.restart.store(0, Ordering::Release);
        self.format = Some(format);
        self.processing = true;
        Ok(())
    }

    /// Renders one block. `input` has one slice per input channel; the
    /// result's output has one per output channel, `frames` long.
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
        let mut ins: Vec<Vec<f32>> = input.to_vec();
        let mut outs: Vec<Vec<f32>> = vec![vec![0.0; n]; usize::from(format.outputs)];
        let mut in_ptrs: Vec<*mut f32> = ins.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let mut out_ptrs: Vec<*mut f32> = outs.iter_mut().map(|c| c.as_mut_ptr()).collect();

        self.events_in.set(events.iter().map(vst_event));
        self.events_out.set([]);
        self.changes.set(params);

        let mut in_bus = AudioBusBuffers {
            numChannels: in_ptrs.len() as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: in_ptrs.as_mut_ptr(),
            },
        };
        let mut out_bus = AudioBusBuffers {
            numChannels: out_ptrs.len() as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: out_ptrs.as_mut_ptr(),
            },
        };
        let mut data = ProcessData {
            processMode: kRealtime as i32,
            symbolicSampleSize: kSample32 as i32,
            numSamples: frames as i32,
            numInputs: i32::from(!in_ptrs.is_empty()),
            numOutputs: i32::from(!out_ptrs.is_empty()),
            inputs: if in_ptrs.is_empty() {
                ptr::null_mut()
            } else {
                &mut in_bus
            },
            outputs: if out_ptrs.is_empty() {
                ptr::null_mut()
            } else {
                &mut out_bus
            },
            inputParameterChanges: self.changes_ptr.as_ptr(),
            outputParameterChanges: self.no_changes_out.as_ptr(),
            inputEvents: self.events_in_ptr.as_ptr(),
            outputEvents: self.events_out_ptr.as_ptr(),
            processContext: &mut self.context,
        };
        // SAFETY: every pointer in `data` refers to live locals or objects
        // owned by the instance, and outlives the call.
        let status = unsafe { self.processor.process(&mut data) };
        self.context.projectTimeSamples += i64::from(frames);

        // A plugin's output is untrusted: anything not finite becomes
        // silence before it can reach a mixer.
        let mut repaired = 0;
        for channel in &mut outs {
            for s in channel.iter_mut() {
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
        Ok(Processed {
            status,
            latency: self.latency,
            restart,
            repaired,
            output: outs,
        })
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

fn vst_event(event: &Event) -> VstEvent {
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
                    noteId: -1,
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
                    noteId: -1,
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
