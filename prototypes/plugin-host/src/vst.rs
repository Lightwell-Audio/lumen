//! A minimal VST3 instrument host: load a module, instantiate its audio
//! processor, play notes into stereo output, save and restore state.
//! Controller, editor and parameters are out of scope for P-HOST.

use std::ffi::c_char;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use vst3::Steinberg::Vst::BusDirections_::{kInput, kOutput};
use vst3::Steinberg::Vst::Event_::EventTypes_::{kNoteOffEvent, kNoteOnEvent};
use vst3::Steinberg::Vst::MediaTypes_::{kAudio, kEvent};
use vst3::Steinberg::Vst::ProcessModes_::kRealtime;
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, Event, Event__type0, IAudioProcessor,
    IAudioProcessorTrait, IComponent, IComponentTrait, IEventList, IHostApplication,
    IParameterChanges, NoteOffEvent, NoteOnEvent, ProcessContext, ProcessData, ProcessSetup,
    SpeakerArr, SpeakerArrangement,
};
use vst3::Steinberg::{
    IPluginBaseTrait, IPluginFactory, IPluginFactoryTrait, PClassInfo, kResultOk,
};
use vst3::{ComPtr, ComWrapper, Interface};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};

use crate::com::{self, EventList, MemoryStream, NoParameterChanges};

#[derive(Debug, Clone, Copy)]
pub enum Note {
    On { pitch: i16, velocity: f32 },
    Off { pitch: i16 },
}

pub struct Plugin {
    pub name: String,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    _factory: ComPtr<IPluginFactory>,
    _host: ComPtr<IHostApplication>,
    events: ComWrapper<EventList>,
    events_ptr: ComPtr<IEventList>,
    params: ComPtr<IParameterChanges>,
    context: ProcessContext,
    active: bool,
}

fn text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Plugin {
    /// Loads `path` and instantiates its first audio processor class (or
    /// the one whose name contains `class_filter`).
    pub fn load(path: &Path, class_filter: Option<&str>) -> Result<Self, String> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: NUL-terminated path. The module is never unloaded (some
        // plugins crash on unload); the process owns it until exit.
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if module.is_null() {
            return Err(format!(
                "LoadLibrary failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: NUL-terminated export names; signatures per the VST3 module ABI.
        unsafe {
            if let Some(init) = GetProcAddress(module, c"InitDll".as_ptr().cast()) {
                let init: unsafe extern "system" fn() -> bool = std::mem::transmute(init);
                init();
            }
        }
        // SAFETY: as above.
        let get_factory = unsafe { GetProcAddress(module, c"GetPluginFactory".as_ptr().cast()) }
            .ok_or("no GetPluginFactory export")?;
        // SAFETY: the VST3 factory export signature.
        let get_factory: unsafe extern "system" fn() -> *mut IPluginFactory =
            unsafe { std::mem::transmute(get_factory) };
        // SAFETY: returns an owned reference or null.
        let factory =
            unsafe { ComPtr::from_raw(get_factory()) }.ok_or("GetPluginFactory returned null")?;

        // SAFETY: plain factory queries with valid out pointers.
        let (cid, name) = unsafe {
            (0..factory.countClasses())
                .find_map(|i| {
                    let mut info: PClassInfo = std::mem::zeroed();
                    if factory.getClassInfo(i, &mut info) != kResultOk {
                        return None;
                    }
                    let name = text(&info.name);
                    let wanted = text(&info.category) == "Audio Module Class"
                        && class_filter.is_none_or(|f| name.contains(f));
                    wanted.then_some((info.cid, name))
                })
                .ok_or("no matching audio processor class")?
        };

        let mut object = ptr::null_mut();
        // SAFETY: valid class and interface ids; `object` receives an owned reference.
        let result = unsafe {
            factory.createInstance(cid.as_ptr(), IComponent::IID.as_ptr().cast(), &mut object)
        };
        // SAFETY: on success `object` is an owned IComponent reference.
        let component = unsafe { ComPtr::from_raw(object.cast::<IComponent>()) }
            .filter(|_| result == kResultOk)
            .ok_or(format!("createInstance failed ({result})"))?;
        let host = com::host_application();
        // SAFETY: the host context outlives the component (both owned by `Plugin`).
        let result = unsafe { component.initialize(host.as_ptr().cast()) };
        if result != kResultOk {
            return Err(format!("initialize failed ({result})"));
        }
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or("no IAudioProcessor")?;
        let events = ComWrapper::new(EventList(std::sync::Mutex::new(Vec::new())));
        let events_ptr = events.to_com_ptr::<IEventList>().expect("interface listed");
        let params = ComWrapper::new(NoParameterChanges)
            .to_com_ptr::<IParameterChanges>()
            .expect("interface listed");
        Ok(Self {
            name,
            component,
            processor,
            _factory: factory,
            _host: host,
            events,
            events_ptr,
            params,
            // SAFETY: plain C struct; zero is a valid "nothing known" context.
            context: unsafe { std::mem::zeroed() },
            active: false,
        })
    }

    /// Stereo output, event input, real-time processing at `rate` and `block`.
    pub fn activate(&mut self, rate: f64, block: i32) -> Result<(), String> {
        // SAFETY: valid pointers to locals for the duration of each call.
        unsafe {
            let mut outputs: [SpeakerArrangement; 1] = [SpeakerArr::kStereo];
            self.processor
                .setBusArrangements(ptr::null_mut(), 0, outputs.as_mut_ptr(), 1);
            let mut setup = ProcessSetup {
                processMode: kRealtime,
                symbolicSampleSize: kSample32,
                maxSamplesPerBlock: block,
                sampleRate: rate,
            };
            let result = self.processor.setupProcessing(&mut setup);
            if result != kResultOk {
                return Err(format!("setupProcessing failed ({result})"));
            }
            if self.component.getBusCount(kAudio, kOutput) > 0 {
                self.component.activateBus(kAudio, kOutput, 0, 1);
            }
            if self.component.getBusCount(kEvent, kInput) > 0 {
                self.component.activateBus(kEvent, kInput, 0, 1);
            }
            self.component.setActive(1);
            self.processor.setProcessing(1);
        }
        self.context.sampleRate = rate;
        self.context.tempo = 120.0;
        self.active = true;
        Ok(())
    }

    /// Renders one block into `left`/`right` with `notes` at their sample offsets.
    pub fn process(
        &mut self,
        notes: &[(i32, Note)],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), String> {
        if let Ok(mut events) = self.events.0.lock() {
            events.clear();
            events.extend(notes.iter().map(|&(offset, note)| event(offset, note)));
        }
        let mut channels = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut output = AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: channels.as_mut_ptr(),
            },
        };
        let mut data = ProcessData {
            processMode: kRealtime,
            symbolicSampleSize: kSample32,
            numSamples: left.len() as i32,
            numInputs: 0,
            numOutputs: 1,
            inputs: ptr::null_mut(),
            outputs: &mut output,
            inputParameterChanges: self.params.as_ptr(),
            outputParameterChanges: ptr::null_mut(),
            inputEvents: self.events_ptr.as_ptr(),
            outputEvents: ptr::null_mut(),
            processContext: &mut self.context,
        };
        // SAFETY: every pointer in `data` refers to live locals or owned objects.
        let result = unsafe { self.processor.process(&mut data) };
        self.context.projectTimeSamples += left.len() as i64;
        if result == kResultOk {
            Ok(())
        } else {
            Err(format!("process failed ({result})"))
        }
    }

    /// The processor's state blob.
    pub fn state(&self) -> Result<Vec<u8>, String> {
        let stream = MemoryStream::open(Vec::new());
        // SAFETY: live stream object.
        let result = unsafe { self.component.getState(stream.as_ptr()) };
        if result != kResultOk {
            return Err(format!("getState failed ({result})"));
        }
        Ok(com::stream_bytes(&stream))
    }

    pub fn set_state(&self, state: &[u8]) -> Result<(), String> {
        let stream = MemoryStream::open(state.to_vec());
        // SAFETY: live stream object.
        let result = unsafe { self.component.setState(stream.as_ptr()) };
        if result == kResultOk {
            Ok(())
        } else {
            Err(format!("setState failed ({result})"))
        }
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        // SAFETY: shutdown sequence on live objects.
        unsafe {
            if self.active {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.component.terminate();
        }
    }
}

fn event(offset: i32, note: Note) -> Event {
    let (kind, body) = match note {
        Note::On { pitch, velocity } => (
            kNoteOnEvent,
            Event__type0 {
                noteOn: NoteOnEvent {
                    channel: 0,
                    pitch,
                    tuning: 0.0,
                    velocity,
                    length: 0,
                    noteId: -1,
                },
            },
        ),
        Note::Off { pitch } => (
            kNoteOffEvent,
            Event__type0 {
                noteOff: NoteOffEvent {
                    channel: 0,
                    pitch,
                    velocity: 0.0,
                    noteId: -1,
                    tuning: 0.0,
                },
            },
        ),
    };
    Event {
        busIndex: 0,
        sampleOffset: offset,
        ppqPosition: 0.0,
        flags: 0,
        r#type: kind as u16,
        __field0: body,
    }
}
