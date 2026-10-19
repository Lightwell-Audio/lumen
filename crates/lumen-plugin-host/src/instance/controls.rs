//! The controller's side of a plugin, used without the audio thread's lock.

use super::*;

/// The controller's side of a plugin: its component (for its state), its
/// controller and the handler that hears it, cloned out of the [`Instance`].
/// The host's main thread calls through it without holding the lock the
/// audio thread takes for every block, so a plugin that is slow to make its
/// editor, write its state or list its parameters never stalls its own audio.
/// VST3 allows it: a host calls these on its main thread while the processor
/// runs on the audio thread; only setting the plugin up excludes processing,
/// and that stays under the lock.
pub struct Controls {
    pub(super) component: ComPtr<IComponent>,
    pub(super) controller: Option<ComPtr<IEditController>>,
    pub(super) handler: ComWrapper<Handler>,
}

/// What the processor heard that the controller has not been told, and the
/// controller events no parameter took ([`Instance::take_heard`]).
pub struct Heard {
    pub(super) values: Vec<(u32, f64)>,
    pub(super) ignored: u32,
}

impl Controls {
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
                    (controller.getParameterInfo(i, &mut info) == kResultOk).then(|| {
                        let value = controller.getParamNormalized(info.id);
                        ParamInfo {
                            id: info.id,
                            title: wide(&info.title),
                            units: wide(&info.units),
                            steps: info.stepCount,
                            default: info.defaultNormalizedValue,
                            flags: info.flags as u32,
                            unit: info.unitId,
                            value: if value.is_finite() {
                                value.clamp(0.0, 1.0)
                            } else {
                                info.defaultNormalizedValue
                            },
                        }
                    })
                })
                .collect();
            (total as u32, list)
        }
    }

    /// The groups the parameters belong to, capped. A plugin with none has an empty list.
    pub fn units(&self) -> Vec<UnitInfo> {
        let Some(info) = self.controller.as_ref().and_then(|c| c.cast::<IUnitInfo>()) else {
            return Vec::new();
        };
        // SAFETY: plain calls on a live interface with valid out pointers.
        unsafe {
            let count = info.getUnitCount().max(0) as usize;
            (0..count.min(MAX_UNITS) as i32)
                .filter_map(|i| {
                    let mut unit: VstUnitInfo = std::mem::zeroed();
                    (info.getUnitInfo(i, &mut unit) == kResultOk).then(|| UnitInfo {
                        id: unit.id,
                        parent: unit.parentUnitId,
                        name: wide(&unit.name),
                    })
                })
                .collect()
        }
    }

    /// The edit controller's own state (a window size, a view), apart from
    /// the component's. A controller that keeps none writes nothing.
    pub fn controller_state(&self) -> Result<Vec<u8>, InstanceError> {
        let Some(controller) = &self.controller else {
            return Ok(Vec::new());
        };
        let (stream, ptr) = MemoryStream::open(Vec::new(), MAX_STATE);
        // SAFETY: a live stream object.
        let result = unsafe { controller.getState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!(
                "the controller's getState failed ({result})"
            )));
        }
        Ok(stream.bytes())
    }

    /// Restores the edit controller's own state.
    pub fn set_controller_state(&self, state: &[u8]) -> Result<(), InstanceError> {
        let Some(controller) = &self.controller else {
            return Ok(());
        };
        let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
        // SAFETY: a live stream object.
        let result = unsafe { controller.setState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!(
                "the controller's setState failed ({result})"
            )));
        }
        Ok(())
    }

    /// The program lists the plugin offers, capped, each with the parameter
    /// that selects its programs. A plugin with none has an empty list.
    pub fn programs(&self) -> Vec<ProgramList> {
        let Some(controller) = &self.controller else {
            return Vec::new();
        };
        let Some(info) = controller.cast::<IUnitInfo>() else {
            return Vec::new();
        };
        // The unit each list sits in, and the program-change parameter of each
        // unit.
        let mut unit_of: Vec<(i32, i32)> = Vec::new();
        let mut changers: Vec<(i32, u32)> = Vec::new();
        // SAFETY: plain calls on live interfaces with valid out pointers.
        unsafe {
            let units = info.getUnitCount().max(0) as usize;
            for i in 0..units.min(MAX_UNITS) as i32 {
                let mut unit: VstUnitInfo = std::mem::zeroed();
                if info.getUnitInfo(i, &mut unit) == kResultOk && unit.programListId >= 0 {
                    unit_of.push((unit.programListId, unit.id));
                }
            }
            let params = controller.getParameterCount().max(0) as usize;
            for i in 0..params.min(MAX_PARAMETERS) as i32 {
                let mut p: ParameterInfo = std::mem::zeroed();
                if controller.getParameterInfo(i, &mut p) == kResultOk
                    && p.flags as u32 & crate::protocol::FLAG_PROGRAM_CHANGE != 0
                    && !changers.iter().any(|&(unit, _)| unit == p.unitId)
                {
                    changers.push((p.unitId, p.id));
                }
            }
            let count = info.getProgramListCount().max(0) as usize;
            let mut all = 0;
            let mut lists = Vec::new();
            for i in 0..count.min(MAX_PROGRAM_LISTS) as i32 {
                let mut list: ProgramListInfo = std::mem::zeroed();
                if info.getProgramListInfo(i, &mut list) != kResultOk {
                    continue;
                }
                let n = (list.programCount.max(0) as usize)
                    .min(MAX_PROGRAMS)
                    .min(MAX_PROGRAMS_ALL - all);
                let programs = (0..n as i32)
                    .map(|k| {
                        let mut name: vst3::Steinberg::Vst::String128 = [0; 128];
                        let named = info.getProgramName(list.id, k, &mut name) == kResultOk;
                        let text = if named { wide(&name) } else { String::new() };
                        if text.is_empty() {
                            format!("Program {}", k + 1)
                        } else {
                            text
                        }
                    })
                    .collect::<Vec<_>>();
                all += programs.len();
                let unit = unit_of
                    .iter()
                    .find(|&&(l, _)| l == list.id)
                    .map_or(-1, |&(_, u)| u);
                let parameter = changers
                    .iter()
                    .find(|&&(u, _)| u == unit)
                    .map(|&(_, id)| id);
                lists.push(ProgramList {
                    id: list.id,
                    name: wide(&list.name),
                    unit,
                    parameter,
                    programs,
                });
            }
            lists
        }
    }

    /// The plugin's own text for a normalized value, if it has one.
    pub fn param_text(&self, id: u32, value: f64) -> Option<String> {
        let controller = self.controller.as_ref()?;
        if !value.is_finite() {
            return None;
        }
        let mut text: vst3::Steinberg::Vst::String128 = [0; 128];
        // SAFETY: a valid out pointer to a String128.
        let r = unsafe { controller.getParamStringByValue(id, value.clamp(0.0, 1.0), &mut text) };
        (r == kResultOk).then(|| wide(&text))
    }

    /// The normalized value the plugin reads from `text`, if it understands
    /// it.
    pub fn param_value(&self, id: u32, text: &str) -> Option<f64> {
        let controller = self.controller.as_ref()?;
        // A TChar string, NUL-terminated; one with a NUL inside would end
        // early, so it is refused.
        if text.contains('\0') {
            return None;
        }
        let mut units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut value = 0.0_f64;
        // SAFETY: a NUL-terminated string that outlives the call and a valid
        // out pointer.
        let r = unsafe { controller.getParamValueByString(id, units.as_mut_ptr(), &mut value) };
        (r == kResultOk && value.is_finite()).then(|| value.clamp(0.0, 1.0))
    }

    /// Sets a parameter from Lumen: the controller is told now, and the
    /// processor hears it at the start of the next block.
    pub fn set_param(&self, id: u32, value: f64) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, 1.0);
        if let Some(controller) = &self.controller {
            // SAFETY: a plain call on a live controller.
            unsafe { controller.setParamNormalized(id, value) };
        }
        self.handler.send_to_processor(id, value);
    }

    /// The edits made since the last call and how many were dropped. First
    /// the controller hears what the processor was given in blocks (a host's
    /// duty: a value that reached only the processor leaves the plugin's
    /// window showing the old one).
    pub fn take_edits(&self, heard: Heard) -> Taken {
        if let Some(controller) = &self.controller {
            for (id, value) in heard.values {
                // SAFETY: a plain call on a live controller.
                unsafe { controller.setParamNormalized(id, value) };
            }
        }
        let (list, dropped) = self.handler.take_reports();
        Taken {
            list,
            dropped,
            ignored: heard.ignored,
        }
    }
}

/// The parameter a program change sets, and how many steps it has: the first
/// program-change parameter in the root unit, else the first anywhere.
pub(super) fn program_parameter(controller: &ComPtr<IEditController>) -> Option<(u32, i32)> {
    let mut found: Option<(u32, i32, i32)> = None;
    // SAFETY: plain calls on a live controller with valid out pointers.
    unsafe {
        let count = controller.getParameterCount().max(0) as usize;
        for i in 0..count.min(MAX_PARAMETERS) as i32 {
            let mut p: ParameterInfo = std::mem::zeroed();
            if controller.getParameterInfo(i, &mut p) == kResultOk
                && p.flags as u32 & crate::protocol::FLAG_PROGRAM_CHANGE != 0
                && found.is_none_or(|(_, _, unit)| unit != 0 && p.unitId == 0)
            {
                found = Some((p.id, p.stepCount, p.unitId));
            }
        }
    }
    found.map(|(id, steps, _)| (id, steps))
}

pub(super) fn wide(buf: &[u16]) -> String {
    let units: Vec<u16> = buf.iter().copied().take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&units)
}
