#![allow(
    clippy::unnecessary_cast,
    reason = "the bindings' enum constants are `i32` on Windows and `u32` elsewhere, so the cast one platform needs is a no-op on the other"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use vst3::Steinberg::Vst::{
    AudioBusBuffers, BusDirection, BusDirections, BusDirections_, BusInfo, BusInfo_, BusTypes_,
    Event,
    Event_::EventTypes_::{kNoteOffEvent, kNoteOnEvent},
    IAttributeListTrait, IAudioProcessor, IAudioProcessorTrait, IComponent, IComponentTrait,
    IConnectionPoint, IConnectionPointTrait, IEventListTrait, IHostApplication,
    IHostApplicationTrait, IMessage, IMessageTrait, IParamValueQueueTrait, IParameterChangesTrait,
    IoMode, MediaType, MediaTypes, MediaTypes_, ProcessData, ProcessSetup, RoutingInfo, SpeakerArr,
    SpeakerArrangement, SymbolicSampleSizes_,
};
use vst3::Steinberg::{
    FUnknown, IBStream, IPluginBaseTrait, TBool, TUID, kInternalError, kInvalidArgument,
    kNotImplemented, kResultFalse, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComRef, Interface};

use crate::dsp::MAX_BUSES;
use crate::faults::{self, Faults, Kind as Fault};
use crate::params::Kind;
use crate::render::{Buses, Change, Dsp, Note, Timed};
use crate::{state, streams, text};

pub static EXTRA_OUTPUT: AtomicBool = AtomicBool::new(false);

const POINTS: usize = 64;
const NOTES: usize = 128;
const STATE_LIMIT: usize = 4096;

pub struct Processor {
    kind: Kind,
    controller: TUID,
    faults: Faults,
    calls: AtomicU64,
    latency: AtomicU32,
    state_calls: AtomicU64,
    dsp: Mutex<Dsp>,
}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor, IConnectionPoint);
}

impl Processor {
    pub fn new(kind: Kind, controller: TUID, faults: Faults) -> Self {
        faults::lifecycle_reset(&faults);
        Self {
            kind,
            controller,
            faults,
            calls: AtomicU64::new(0),
            latency: AtomicU32::new(0),
            state_calls: AtomicU64::new(0),
            dsp: Mutex::new(Dsp::new(kind)),
        }
    }

    fn dsp(&self) -> std::sync::MutexGuard<'_, Dsp> {
        self.dsp.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn event_buses(&self, dir: BusDirection) -> i32 {
        i32::from(self.kind == Kind::Instrument && dir == BusDirections_::kInput as BusDirection)
    }

    fn audio_buses(&self, dir: BusDirection) -> i32 {
        let extra =
            i32::from(self.faults.has(Fault::BusChange) && EXTRA_OUTPUT.load(Ordering::Relaxed));
        match (self.kind, dir as BusDirections) {
            (Kind::Instrument, BusDirections_::kInput) => 0,
            (Kind::Instrument, BusDirections_::kOutput) if self.faults.has(Fault::AuxBuses) => 4,
            (Kind::Instrument, BusDirections_::kOutput) => {
                self.faults.multi_out().map_or(1, |n| n as i32) + extra
            }
            (Kind::Effect, BusDirections_::kInput) if self.faults.has(Fault::Sidechain) => 2,
            _ => 1,
        }
    }
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        if self.faults.has(Fault::NeedsMessage) {
            let host =
                unsafe { ComRef::from_raw(context) }.and_then(|c| c.cast::<IHostApplication>());
            let Some(host) = host else {
                faults::crash();
            };
            let mut object = std::ptr::null_mut();
            unsafe {
                host.createInstance(
                    IMessage::IID.as_ptr().cast_mut().cast(),
                    IMessage::IID.as_ptr().cast_mut().cast(),
                    &mut object,
                )
            };
            let Some(message) = (unsafe { ComPtr::<IMessage>::from_raw(object.cast()) }) else {
                faults::crash();
            };
            let attributes = unsafe { message.getAttributes() };
            let Some(attributes) = (unsafe { ComRef::from_raw(attributes) }) else {
                faults::crash();
            };
            unsafe { attributes.setInt(c"count".as_ptr(), 1) };
        }
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        if !faults::lifecycle_has(
            &self.faults,
            faults::LIFECYCLE_COMPONENT_CONNECTED
                | faults::LIFECYCLE_CONTROLLER_CONNECTED
                | faults::LIFECYCLE_COMPONENT_DISCONNECTED
                | faults::LIFECYCLE_CONTROLLER_DISCONNECTED,
        ) {
            faults::crash();
        }
        faults::lifecycle_record(&self.faults, faults::LIFECYCLE_COMPONENT_TERMINATED);
        kResultOk
    }
}

impl IConnectionPointTrait for Processor {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        if other.is_null() {
            return kInvalidArgument;
        }
        faults::lifecycle_record(&self.faults, faults::LIFECYCLE_COMPONENT_CONNECTED);
        kResultOk
    }

    unsafe fn disconnect(&self, other: *mut IConnectionPoint) -> tresult {
        if other.is_null() {
            return kInvalidArgument;
        }
        faults::lifecycle_record(&self.faults, faults::LIFECYCLE_COMPONENT_DISCONNECTED);
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kNotImplemented
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = self.controller };
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        match media as MediaTypes {
            MediaTypes_::kAudio => self.audio_buses(dir),
            MediaTypes_::kEvent => self.event_buses(dir),
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        let bus = unsafe { &mut *bus };
        let count = match media as MediaTypes {
            MediaTypes_::kAudio => self.audio_buses(dir),
            MediaTypes_::kEvent => self.event_buses(dir),
            _ => 0,
        };
        if index < 0 || index >= count {
            return kInvalidArgument;
        }
        bus.mediaType = media;
        bus.direction = dir;
        bus.busType = BusTypes_::kMain as i32;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        if media as MediaTypes == MediaTypes_::kAudio && index > 0 {
            bus.busType = BusTypes_::kAux as i32;
            bus.flags = 0;
        }
        if media as MediaTypes == MediaTypes_::kEvent {
            bus.channelCount = 1;
            text::copy_wstring("Notes", &mut bus.name);
        } else {
            let output = dir as BusDirections == BusDirections_::kOutput;
            bus.channelCount = if output && self.faults.has(Fault::BadBuses) {
                9_999
            } else {
                2
            };
            let name = match (output, index) {
                (true, 0) => "Output".to_owned(),
                (true, n) => format!("Output {}", n + 1),
                (false, 0) => "Input".to_owned(),
                (false, _) => "Sidechain".to_owned(),
            };
            text::copy_wstring(&name, &mut bus.name);
        }
        kResultOk
    }

    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, active: TBool) -> tresult {
        if active != 0 {
            self.calls.store(0, Ordering::Relaxed);
            self.dsp().reset();
        }
        kResultOk
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let refusals = self.faults.state_refusals();
        if refusals > 0 && self.state_calls.fetch_add(1, Ordering::Relaxed) < refusals {
            return kResultFalse;
        }
        let Some(bytes) = (unsafe { streams::read_all(stream, STATE_LIMIT) }) else {
            return kResultFalse;
        };
        let Some(values) = state::decode(self.kind, &bytes) else {
            return kResultFalse;
        };
        let mut dsp = self.dsp();
        dsp.values = values;
        self.latency.store(dsp.latency(), Ordering::Relaxed);
        kResultOk
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        if self.faults.has(Fault::StateHang) {
            faults::hang();
        }
        if self.faults.has(Fault::StateFail) {
            return kInternalError;
        }
        let bytes = if self.faults.has(Fault::StateGarbage) {
            [0xAB; state::LEN]
        } else {
            state::encode(self.kind, self.dsp().values)
        };
        if unsafe { streams::write_all(stream, &bytes) } {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins != self.audio_buses(BusDirections_::kInput as BusDirection)
            || num_outs != self.audio_buses(BusDirections_::kOutput as BusDirection)
        {
            return kResultFalse;
        }
        let all_stereo = unsafe {
            (0..num_ins as usize).all(|i| *inputs.add(i) == SpeakerArr::kStereo)
                && (0..num_outs as usize).all(|i| *outputs.add(i) == SpeakerArr::kStereo)
        };
        if all_stereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if index < 0 || index >= self.audio_buses(dir) {
            return kInvalidArgument;
        }
        unsafe { *arrangement = SpeakerArr::kStereo };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let setup = unsafe { &*setup };
        if setup.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32
            || !(setup.sampleRate.is_finite() && setup.sampleRate > 0.0)
        {
            return kResultFalse;
        }
        let mut dsp = self.dsp();
        dsp.rate = setup.sampleRate;
        if let Some(release) = self.faults.release() {
            dsp.set_release(release.as_secs_f64());
        }
        if let Some(samples) = self.faults.instrument_latency() {
            dsp.set_instrument_latency(samples);
            self.latency.store(dsp.latency(), Ordering::Relaxed);
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        self.dsp().reset();
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        if self.faults.at_call(Fault::ProcessCrash, call) {
            faults::crash();
        }
        if self.faults.at_call(Fault::ProcessHang, call) {
            faults::hang();
        }
        if self.faults.from_call(Fault::ProcessError, call) {
            return kInternalError;
        }
        if let Some(wait) = self.faults.slow() {
            faults::busy_wait(wait);
        }
        let data = unsafe { &*data };
        if self.faults.has(Fault::AuxBuses) && data.numOutputs != 4 {
            faults::crash();
        }
        if self.faults.has(Fault::AuxBuses) {
            for bus in 0..4 {
                let list = unsafe { (*data.outputs.add(bus)).__field0.channelBuffers32 };
                if list.is_null() {
                    faults::crash();
                }
            }
        }
        if self.faults.has(Fault::ReportsParams) {
            let list = unsafe { ComRef::from_raw(data.outputParameterChanges) };
            let Some(list) = list else {
                faults::crash();
            };
            let (id, mut index) = (7_u32, 0_i32);
            let queue = unsafe { list.addParameterData(&id, &mut index) };
            let Some(queue) = (unsafe { ComRef::from_raw(queue) }) else {
                faults::crash();
            };
            let mut at = 0_i32;
            unsafe { queue.addPoint(0, 0.5, &mut at) };
        }
        let frames = data.numSamples.max(0) as usize;

        let mut changes = [Timed {
            at: 0,
            what: Change { id: 0, value: 0.0 },
        }; POINTS];
        let mut notes = [Timed {
            at: 0,
            what: Note::Off {
                channel: 0,
                pitch: 0,
                id: -1,
            },
        }; NOTES];
        let (n_changes, n_notes) = unsafe { gather(data, &mut changes, &mut notes) };

        let (input, output) = unsafe { channels(data) };
        let buses = unsafe { collect_buses(data) };
        let mut dsp = self.dsp();
        dsp.set_output_buses(
            self.audio_buses(BusDirections_::kOutput as BusDirection)
                .max(1) as usize,
        );
        unsafe {
            dsp.render_buses(
                frames,
                input,
                buses,
                &changes[..n_changes],
                &notes[..n_notes],
            )
        };
        self.latency.store(dsp.latency(), Ordering::Relaxed);
        drop(dsp);

        if self.faults.has(Fault::Prints) {
            println!("test plugin: stray output from process");
        }
        if self.faults.from_call(Fault::ProcessNan, call) {
            for p in output.into_iter().filter(|p| !p.is_null()) {
                for i in 0..frames {
                    unsafe { p.add(i).write(f32::NAN) };
                }
            }
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }
}

unsafe fn channels(data: &ProcessData) -> ([*const f32; 2], [*mut f32; 2]) {
    fn first(buses: *mut AudioBusBuffers, count: i32) -> [*mut f32; 2] {
        let mut out = [std::ptr::null_mut(); 2];
        if buses.is_null() || count < 1 {
            return out;
        }
        unsafe {
            let bus = &*buses;
            let list = bus.__field0.channelBuffers32;
            if !list.is_null() {
                for (c, slot) in out
                    .iter_mut()
                    .enumerate()
                    .take(bus.numChannels.max(0) as usize)
                {
                    *slot = *list.add(c);
                }
            }
        }
        out
    }
    let input = first(data.inputs, data.numInputs).map(<*mut f32>::cast_const);
    (input, first(data.outputs, data.numOutputs))
}

unsafe fn collect_buses(data: &ProcessData) -> Buses {
    let pair = |buses: *mut AudioBusBuffers, count: i32, index: usize| -> [*mut f32; 2] {
        let mut out = [std::ptr::null_mut(); 2];
        if buses.is_null() || index as i32 >= count {
            return out;
        }
        unsafe {
            let bus = &*buses.add(index);
            let list = bus.__field0.channelBuffers32;
            if !list.is_null() {
                for (c, slot) in out
                    .iter_mut()
                    .enumerate()
                    .take(bus.numChannels.max(0) as usize)
                {
                    *slot = *list.add(c);
                }
            }
        }
        out
    };
    let mut outputs = [[std::ptr::null_mut(); 2]; MAX_BUSES];
    for (index, slot) in outputs.iter_mut().enumerate() {
        *slot = pair(data.outputs, data.numOutputs, index);
    }
    Buses {
        outputs,
        sidechain: pair(data.inputs, data.numInputs, 1).map(<*mut f32>::cast_const),
    }
}

unsafe fn gather(
    data: &ProcessData,
    changes: &mut [Timed<Change>; POINTS],
    notes: &mut [Timed<Note>; NOTES],
) -> (usize, usize) {
    let (mut nc, mut nn) = (0, 0);
    if let Some(list) = unsafe { ComRef::from_raw(data.inputParameterChanges) } {
        let queues = unsafe { list.getParameterCount() };
        for q in 0..queues {
            let Some(queue) = (unsafe { ComRef::from_raw(list.getParameterData(q)) }) else {
                continue;
            };
            let id = unsafe { queue.getParameterId() };
            if id > 1 {
                continue;
            }
            let points = unsafe { queue.getPointCount() };
            for i in 0..points {
                let (mut at, mut value) = (0, 0.0);
                if unsafe { queue.getPoint(i, &mut at, &mut value) } == kResultOk && nc < POINTS {
                    changes[nc] = Timed {
                        at,
                        what: Change { id, value },
                    };
                    nc += 1;
                }
            }
        }
    }
    changes[..nc].sort_by_key(|c| c.at);

    if let Some(list) = unsafe { ComRef::from_raw(data.inputEvents) } {
        let count = unsafe { list.getEventCount() };
        for i in 0..count {
            let mut event: Event = unsafe { std::mem::zeroed() };
            if unsafe { list.getEvent(i, &mut event) } != kResultOk || nn == NOTES {
                continue;
            }
            let what = unsafe {
                match event.r#type as i32 {
                    t if t == kNoteOnEvent as i32 => Note::On {
                        channel: event.__field0.noteOn.channel,
                        pitch: event.__field0.noteOn.pitch,
                        velocity: event.__field0.noteOn.velocity,
                        id: event.__field0.noteOn.noteId,
                    },
                    t if t == kNoteOffEvent as i32 => Note::Off {
                        channel: event.__field0.noteOff.channel,
                        pitch: event.__field0.noteOff.pitch,
                        id: event.__field0.noteOff.noteId,
                    },
                    _ => continue,
                }
            };
            notes[nn] = Timed {
                at: event.sampleOffset,
                what,
            };
            nn += 1;
        }
    }
    notes[..nn].sort_by_key(|n| n.at);
    (nc, nn)
}
