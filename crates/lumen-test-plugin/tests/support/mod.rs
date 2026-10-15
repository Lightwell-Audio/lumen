//! A small VST3 host, just enough to drive the test plugins through their
//! COM interfaces the way a real host does.
#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use lumen_test_plugin::faults::Faults;
use lumen_test_plugin::params::Kind;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, Event, Event_::EventTypes_::kNoteOffEvent,
    Event_::EventTypes_::kNoteOnEvent, Event__type0, IAudioProcessor, IAudioProcessorTrait,
    IComponent, IComponentHandler, IComponentHandlerTrait, IComponentTrait, IEditController,
    IEventList, IEventListTrait, IParamValueQueue, IParamValueQueueTrait, IParameterChanges,
    IParameterChangesTrait, NoteOffEvent, NoteOnEvent, ParamID, ParamValue, ProcessData,
    ProcessSetup, SymbolicSampleSizes_::kSample32,
};
use vst3::Steinberg::{
    IBStream, IBStreamTrait, IPluginBaseTrait, IPluginFactoryTrait, int32, int64, kInvalidArgument,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper, Interface};

pub struct Notes(pub Vec<Event>);

impl Class for Notes {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for Notes {
    unsafe fn getEventCount(&self) -> int32 {
        self.0.len() as int32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.0.get(index as usize) {
            // SAFETY: the plugin passes a valid out pointer.
            Some(event) => unsafe { *e = *event },
            None => return kInvalidArgument,
        }
        kResultOk
    }
    unsafe fn addEvent(&self, _e: *mut Event) -> tresult {
        kInvalidArgument
    }
}

pub fn note_on(at: i32, pitch: i16, velocity: f32) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: at,
        ppqPosition: 0.0,
        flags: 0,
        r#type: kNoteOnEvent as u16,
        __field0: Event__type0 {
            noteOn: NoteOnEvent {
                channel: 0,
                pitch,
                tuning: 0.0,
                velocity,
                length: 0,
                noteId: -1,
            },
        },
    }
}

pub fn note_off(at: i32, pitch: i16) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: at,
        ppqPosition: 0.0,
        flags: 0,
        r#type: kNoteOffEvent as u16,
        __field0: Event__type0 {
            noteOff: NoteOffEvent {
                channel: 0,
                pitch,
                velocity: 0.0,
                noteId: -1,
                tuning: 0.0,
            },
        },
    }
}

pub struct Queue {
    id: ParamID,
    points: Vec<(i32, f64)>,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }
    unsafe fn getPointCount(&self) -> int32 {
        self.points.len() as int32
    }
    unsafe fn getPoint(&self, index: int32, at: *mut int32, value: *mut ParamValue) -> tresult {
        match self.points.get(index as usize) {
            Some(&(a, v)) => {
                // SAFETY: valid out pointers from the plugin.
                unsafe {
                    *at = a;
                    *value = v;
                }
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addPoint(&self, _at: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kInvalidArgument
    }
}

pub struct Changes {
    queues: Vec<ComPtr<IParamValueQueue>>,
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.queues.len() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        self.queues
            .get(index as usize)
            .map_or(std::ptr::null_mut(), |q| q.as_ptr())
    }
    unsafe fn addParameterData(
        &self,
        _id: *const ParamID,
        _index: *mut int32,
    ) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

/// `(parameter id, [(sample offset, normalized value)])` queues.
pub fn changes(list: &[(ParamID, &[(i32, f64)])]) -> ComPtr<IParameterChanges> {
    let queues = list
        .iter()
        .map(|&(id, points)| {
            ComWrapper::new(Queue {
                id,
                points: points.to_vec(),
            })
            .to_com_ptr::<IParamValueQueue>()
            .expect("listed")
        })
        .collect();
    ComWrapper::new(Changes { queues })
        .to_com_ptr::<IParameterChanges>()
        .expect("listed")
}

/// The bytes of a stream and the read position, shared with the test.
pub type Shared = Arc<Mutex<(Vec<u8>, usize)>>;

pub struct Stream(Shared);

impl Stream {
    pub fn open(data: Vec<u8>) -> (ComPtr<IBStream>, Shared) {
        let shared: Shared = Arc::new(Mutex::new((data, 0)));
        let stream = ComWrapper::new(Self(shared.clone()))
            .to_com_ptr::<IBStream>()
            .expect("listed");
        (stream, shared)
    }
}

impl Class for Stream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, num: int32, got: *mut int32) -> tresult {
        let mut g = self.0.lock().unwrap();
        let (data, pos) = &mut *g;
        let n = (num.max(0) as usize).min(data.len() - *pos);
        // SAFETY: the plugin's buffer holds `num` bytes and n <= num.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer.cast::<u8>(), n) };
        *pos += n;
        // SAFETY: valid out pointer.
        unsafe { *got = n as int32 };
        kResultOk
    }
    unsafe fn write(&self, buffer: *mut c_void, num: int32, put: *mut int32) -> tresult {
        let mut g = self.0.lock().unwrap();
        let (data, pos) = &mut *g;
        let n = num.max(0) as usize;
        data.resize((*pos + n).max(data.len()), 0);
        // SAFETY: the plugin's buffer holds `num` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer.cast::<u8>(), data.as_mut_ptr().add(*pos), n)
        };
        *pos += n;
        // SAFETY: valid out pointer.
        unsafe { *put = n as int32 };
        kResultOk
    }
    unsafe fn seek(&self, _offset: int64, _mode: int32, _result: *mut int64) -> tresult {
        kInvalidArgument
    }
    unsafe fn tell(&self, _pos: *mut int64) -> tresult {
        kInvalidArgument
    }
}

/// What a host has been told by the plugin's controller.
#[derive(Default)]
pub struct Told {
    pub restarts: Mutex<Vec<i32>>,
}

impl Class for Told {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Told {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restarts.lock().unwrap().push(flags);
        kResultOk
    }
}

/// One loaded plugin: its component and controller.
pub struct Plugin {
    pub component: ComPtr<IComponent>,
    pub processor: ComPtr<IAudioProcessor>,
    pub controller: ComPtr<IEditController>,
    pub kind: Kind,
}

impl Plugin {
    pub fn new(kind: Kind, faults: &str) -> Self {
        let faults = Faults::parse(faults).expect("valid faults");
        let component = lumen_test_plugin::component(kind, faults.clone());
        let controller = lumen_test_plugin::controller(kind, faults);
        Self {
            processor: component.to_com_ptr::<IAudioProcessor>().expect("listed"),
            component: component.to_com_ptr::<IComponent>().expect("listed"),
            controller: controller.to_com_ptr::<IEditController>().expect("listed"),
            kind,
        }
    }

    /// Creates the plugin through the module's factory, so the environment's
    /// faults apply, as they do for a host process.
    pub fn from_factory(kind: Kind) -> Self {
        let factory = lumen_test_plugin::factory();
        let (component_id, controller_id) = match kind {
            Kind::Instrument => (
                lumen_test_plugin::INSTRUMENT,
                lumen_test_plugin::INSTRUMENT_CONTROLLER,
            ),
            Kind::Effect => (
                lumen_test_plugin::EFFECT,
                lumen_test_plugin::EFFECT_CONTROLLER,
            ),
        };
        let make = |cid: vst3::Steinberg::TUID, iid: &[u8; 16]| {
            let mut object = std::ptr::null_mut();
            // SAFETY: valid ids; `object` receives an owned reference.
            let result =
                unsafe { factory.createInstance(cid.as_ptr(), iid.as_ptr().cast(), &mut object) };
            assert_eq!(result, kResultOk);
            object
        };
        let component = make(component_id, &IComponent::IID);
        let controller = make(controller_id, &IEditController::IID);
        // SAFETY: owned references of the interfaces asked for.
        unsafe {
            let component = ComPtr::<IComponent>::from_raw(component.cast()).expect("component");
            Self {
                processor: component.cast::<IAudioProcessor>().expect("processor"),
                component,
                controller: ComPtr::<IEditController>::from_raw(controller.cast())
                    .expect("controller"),
                kind,
            }
        }
    }

    /// Initializes, sets up at 48 kHz and activates.
    pub fn start(&self) {
        // SAFETY: plain calls on live objects.
        unsafe {
            assert_eq!(self.component.initialize(std::ptr::null_mut()), kResultOk);
            assert_eq!(self.controller.initialize(std::ptr::null_mut()), kResultOk);
            let mut setup = ProcessSetup {
                processMode: 0,
                symbolicSampleSize: kSample32 as i32,
                maxSamplesPerBlock: 512,
                sampleRate: 48_000.0,
            };
            assert_eq!(self.processor.setupProcessing(&mut setup), kResultOk);
            assert_eq!(self.component.setActive(1), kResultOk);
            assert_eq!(self.processor.setProcessing(1), kResultOk);
        }
    }

    /// Runs one block; `input` is stereo or empty. Returns the result and
    /// the stereo output.
    pub fn process(
        &self,
        frames: usize,
        input: Option<[&[f32]; 2]>,
        events: Vec<Event>,
        params: &[(ParamID, &[(i32, f64)])],
    ) -> (tresult, [Vec<f32>; 2]) {
        let mut ins: [Vec<f32>; 2] = match input {
            Some([l, r]) => [l.to_vec(), r.to_vec()],
            None => [vec![], vec![]],
        };
        let mut out = [vec![0.0_f32; frames], vec![0.0_f32; frames]];
        let mut in_ptrs = [ins[0].as_mut_ptr(), ins[1].as_mut_ptr()];
        let mut out_ptrs = [out[0].as_mut_ptr(), out[1].as_mut_ptr()];
        let mut in_bus = AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: in_ptrs.as_mut_ptr(),
            },
        };
        let mut out_bus = AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: out_ptrs.as_mut_ptr(),
            },
        };
        let list = ComWrapper::new(Notes(events))
            .to_com_ptr::<IEventList>()
            .expect("listed");
        let changes = changes(params);
        let mut data = ProcessData {
            processMode: 0,
            symbolicSampleSize: kSample32 as i32,
            numSamples: frames as i32,
            numInputs: i32::from(input.is_some()),
            numOutputs: 1,
            inputs: if input.is_some() {
                &mut in_bus
            } else {
                std::ptr::null_mut()
            },
            outputs: &mut out_bus,
            inputParameterChanges: changes.as_ptr(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: list.as_ptr(),
            outputEvents: std::ptr::null_mut(),
            processContext: std::ptr::null_mut(),
        };
        // SAFETY: every pointer in `data` outlives the call.
        let result = unsafe { self.processor.process(&mut data) };
        (result, out)
    }

    /// The component's saved state.
    pub fn save(&self) -> (tresult, Vec<u8>) {
        let (stream, shared) = Stream::open(vec![]);
        // SAFETY: a live stream.
        let result = unsafe { self.component.getState(stream.as_ptr()) };
        let bytes = shared.lock().unwrap().0.clone();
        (result, bytes)
    }

    /// Restores the component from saved state.
    pub fn load(&self, bytes: Vec<u8>) -> tresult {
        let (stream, _) = Stream::open(bytes);
        // SAFETY: a live stream.
        unsafe { self.component.setState(stream.as_ptr()) }
    }
}
