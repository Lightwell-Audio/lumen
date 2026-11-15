#![allow(
    clippy::unnecessary_cast,
    reason = "the bindings' enum constants are `i32` on Windows and `u32` elsewhere, so the cast one platform needs is a no-op on the other"
)]

use super::*;

impl Instance {
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

        let host = com::host_application();
        if let Some(factory) = module.factory.cast::<IPluginFactory3>() {
            unsafe { factory.setHostContext(host.as_ptr().cast()) };
        }

        let create =
            |cid: &ClassId, iid: &[u8; 16]| -> Result<*mut std::ffi::c_void, InstanceError> {
                let mut object = ptr::null_mut();
                let cid: TUID = to_tuid(cid);
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
        let component = unsafe { ComPtr::<IComponent>::from_raw(object.cast()) }
            .ok_or_else(|| plugin("no component"))?;
        let result = unsafe { component.initialize(host.as_ptr().cast()) };
        if result != kResultOk {
            return Err(plugin(format!("initialize failed ({result})")));
        }
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or_else(|| plugin("the component has no audio processor"))?;

        let mut separate_controller = false;
        let mut controller = component.cast::<IEditController>();
        if controller.is_none() {
            let mut cid: TUID = [0; 16];
            if unsafe { component.getControllerClassId(&mut cid) } == kResultOk {
                let id: ClassId = cid.map(|c| c as u8);
                if let Ok(object) = create(&id, &IEditController::IID) {
                    controller = unsafe { ComPtr::<IEditController>::from_raw(object.cast()) };
                    if let Some(c) = &controller {
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
                unsafe { c.setComponentHandler(h.as_ptr()) };
            }
            if separate_controller
                && let (Some(a), Some(b)) = (
                    component.cast::<IConnectionPoint>(),
                    c.cast::<IConnectionPoint>(),
                )
            {
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
        }

        let mapping = controller.as_ref().and_then(|c| c.cast::<IMidiMapping>());
        let program_param = controller.as_ref().and_then(program_parameter);
        let events_in = ComWrapper::new(EventList::default());
        let events_in_ptr = events_in.to_com_ptr::<IEventList>().expect("listed");
        let events_out = ComWrapper::new(EventList::default());
        let events_out_ptr = events_out.to_com_ptr::<IEventList>().expect("listed");
        let changes = ComWrapper::new(Changes::new());
        let changes_ptr = changes.to_com_ptr::<IParameterChanges>().expect("listed");
        let outputs = ComWrapper::new(OutputChanges::new());
        let outputs_ptr = outputs.to_com_ptr::<IParameterChanges>().expect("listed");

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
            outputs,
            outputs_ptr,
            now: Vec::with_capacity(MOST_PENDING),
            mapping,
            program_param,
            midi_map: vec![MAP_UNKNOWN; MAP_CHANNELS * MAP_NUMBERS],
            mapped: Vec::with_capacity(256),
            ignored: 0,
            reported: Vec::with_capacity(MOST_PENDING),
            for_controller: Vec::with_capacity(MOST_PENDING),
            context: unsafe { std::mem::zeroed() },
            in_buses: 0,
            out_buses: 0,
            in_slots: Vec::new(),
            out_slots: Vec::new(),
            notes: NoteIds::new(),
            spare: Vec::new(),
            format: None,
            processing: false,
            latency: 0,
            _host: host,
        })
    }

    pub(super) fn stop(&mut self) {
        self.notes.clear();
        if self.processing {
            unsafe {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.processing = false;
        }
    }

    pub fn configure(&mut self, format: Format) -> Result<(), InstanceError> {
        self.configure_with(format, Aux::default())
    }

    pub fn buses(&self) -> (Vec<crate::protocol::BusInfo>, Vec<crate::protocol::BusInfo>) {
        let list = |direction: i32| -> Vec<crate::protocol::BusInfo> {
            let count = unsafe { self.component.getBusCount(kAudio as i32, direction) }
                .clamp(0, MAX_BUSES as i32);
            (0..count)
                .filter_map(|bus| {
                    let mut info: BusInfo = unsafe { std::mem::zeroed() };
                    let got = unsafe {
                        self.component
                            .getBusInfo(kAudio as i32, direction, bus, &mut info)
                    };
                    (got == kResultOk).then(|| crate::protocol::BusInfo {
                        name: wide(&info.name),
                        channels: u8::try_from(info.channelCount.max(0)).unwrap_or(u8::MAX),
                        main: info.busType == vst3::Steinberg::Vst::BusTypes_::kMain as i32,
                        default_active: info.flags
                            & vst3::Steinberg::Vst::BusInfo_::BusFlags_::kDefaultActive as u32
                            != 0,
                    })
                })
                .collect()
        };
        (list(kInput as i32), list(kOutput as i32))
    }

    fn bus_count(&self, direction: i32) -> usize {
        unsafe { self.component.getBusCount(kAudio as i32, direction) }.max(0) as usize
    }

    fn bus_width(&self, direction: i32, bus: usize) -> usize {
        unsafe {
            let mut info: BusInfo = std::mem::zeroed();
            if self
                .component
                .getBusInfo(kAudio as i32, direction, bus as i32, &mut info)
                == kResultOk
            {
                usize::try_from(info.channelCount).unwrap_or(usize::MAX)
            } else {
                usize::MAX
            }
        }
    }

    pub fn configure_with(&mut self, format: Format, aux: Aux) -> Result<(), InstanceError> {
        self.stop();
        let ins = usize::from(format.inputs);
        let outs = usize::from(format.outputs);
        if ins > MAX_CHANNELS || outs > MAX_CHANNELS {
            return Err(plugin("too many channels"));
        }
        let (in_buses, out_buses) = (
            self.bus_count(kInput as i32),
            self.bus_count(kOutput as i32),
        );
        if in_buses > MAX_DECLARED_BUSES || out_buses > MAX_DECLARED_BUSES {
            return Err(plugin(format!(
                "the plugin declares {} audio buses; at most {MAX_DECLARED_BUSES} are supported",
                in_buses.max(out_buses)
            )));
        }
        let mut wanted_aux = [Vec::new(), Vec::new()];
        for (side, (name, direction, buses, mask)) in [
            ("input", kInput as i32, in_buses, aux.inputs),
            ("output", kOutput as i32, out_buses, aux.outputs),
        ]
        .into_iter()
        .enumerate()
        {
            for bus in (1..MAX_BUSES).filter(|bus| mask & (1 << bus) != 0) {
                if bus >= buses {
                    return Err(plugin(format!("the plugin has no {name} bus {bus}")));
                }
                let channels = self.bus_width(direction, bus);
                if !(1..=2).contains(&channels) {
                    return Err(plugin(format!(
                        "{name} bus {bus} has {channels} channels; only mono and stereo are supported"
                    )));
                }
                wanted_aux[side].push((bus, channels as u8));
            }
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
        for (direction, wanted, aux_buses) in [
            (kInput as i32, ins, &wanted_aux[0]),
            (kOutput as i32, outs, &wanted_aux[1]),
        ] {
            if wanted > 0 {
                let have = if self.bus_count(direction) > 0 {
                    self.bus_width(direction, 0)
                } else {
                    0
                };
                if have != wanted {
                    return Err(plugin(format!(
                        "the plugin's main bus has {have} channels, not {wanted}"
                    )));
                }
            }
            for &(bus, channels) in aux_buses {
                let have = self.bus_width(direction, bus);
                if have != usize::from(channels) {
                    return Err(plugin(format!(
                        "bus {bus} has {have} channels, not {channels}"
                    )));
                }
            }
        }
        let slots = |main: usize, aux_buses: &[(usize, u8)]| -> Vec<Slot> {
            let mut list = Vec::with_capacity(1 + aux_buses.len());
            let mut first = 0;
            if main > 0 {
                list.push(Slot {
                    bus: 0,
                    channels: main as u8,
                    first,
                });
                first += main;
            }
            for &(bus, channels) in aux_buses {
                list.push(Slot {
                    bus,
                    channels,
                    first,
                });
                first += usize::from(channels);
            }
            list
        };
        let in_slots = slots(ins, &wanted_aux[0]);
        let out_slots = slots(outs, &wanted_aux[1]);
        let total = |list: &[Slot]| list.iter().map(|s| usize::from(s.channels)).sum::<usize>();
        let (total_in, total_out) = (total(&in_slots), total(&out_slots));
        if total_in > MAX_CHANNELS || total_out > MAX_CHANNELS {
            return Err(plugin("too many channels in all"));
        }
        self.in_slots = in_slots;
        self.out_slots = out_slots;
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
            for (direction, buses, slots) in [
                (kInput as i32, in_buses, &self.in_slots),
                (kOutput as i32, out_buses, &self.out_slots),
            ] {
                for bus in 1..buses {
                    let on = slots.iter().any(|slot| slot.bus == bus);
                    self.component
                        .activateBus(kAudio as i32, direction, bus as i32, u8::from(on));
                }
            }
            let result = self.component.setActive(1);
            if result != kResultOk {
                return Err(plugin(format!("setActive failed ({result})")));
            }
            let result = self.processor.setProcessing(1);
            if result != kResultOk && result != vst3::Steinberg::kNotImplemented {
                self.component.setActive(0);
                return Err(plugin(format!("setProcessing failed ({result})")));
            }
            self.latency = self.processor.getLatencySamples();
        }
        self.context.sampleRate = format.sample_rate;
        self.context.tempo = 120.0;
        self.context.timeSigNumerator = 4;
        self.context.timeSigDenominator = 4;
        self.context.state = (kPlaying | kTempoValid | kTimeSigValid) as u32;
        self.context.projectTimeSamples = 0;
        self.handler.restart.store(0, Ordering::Release);
        self.format = Some(Format {
            inputs: total_in as u8,
            outputs: total_out as u8,
            ..format
        });
        self.processing = true;
        Ok(())
    }
}
