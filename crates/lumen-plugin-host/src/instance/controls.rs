use super::*;

pub struct Controls {
    pub(super) component: ComPtr<IComponent>,
    pub(super) controller: Option<ComPtr<IEditController>>,
    pub(super) handler: ComWrapper<Handler>,
}

pub struct Heard {
    pub(super) values: Vec<(u32, f64)>,
    pub(super) ignored: u32,
}

impl Controls {
    pub fn create_editor_view(&self) -> Option<ComPtr<IPlugView>> {
        let controller = self.controller.as_ref()?;
        let raw = unsafe { controller.createView(c"editor".as_ptr()) };
        unsafe { ComPtr::from_raw(raw) }
    }

    pub fn state(&self) -> Result<Vec<u8>, InstanceError> {
        let (stream, ptr) = MemoryStream::open(Vec::new(), MAX_STATE);
        let result = unsafe { self.component.getState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!("getState failed ({result})")));
        }
        Ok(stream.bytes())
    }

    pub fn set_state(&self, state: &[u8]) -> Result<(), InstanceError> {
        let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
        let result = unsafe { self.component.setState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!("setState failed ({result})")));
        }
        if let Some(controller) = &self.controller {
            let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
            unsafe { controller.setComponentState(ptr.as_ptr()) };
        }
        Ok(())
    }

    pub fn parameters(&self) -> (u32, Vec<ParamInfo>) {
        let Some(controller) = &self.controller else {
            return (0, Vec::new());
        };
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

    pub fn units(&self) -> Vec<UnitInfo> {
        let Some(info) = self.controller.as_ref().and_then(|c| c.cast::<IUnitInfo>()) else {
            return Vec::new();
        };
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

    pub fn controller_state(&self) -> Result<Vec<u8>, InstanceError> {
        let Some(controller) = &self.controller else {
            return Ok(Vec::new());
        };
        let (stream, ptr) = MemoryStream::open(Vec::new(), MAX_STATE);
        let result = unsafe { controller.getState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!(
                "the controller's getState failed ({result})"
            )));
        }
        Ok(stream.bytes())
    }

    pub fn set_controller_state(&self, state: &[u8]) -> Result<(), InstanceError> {
        let Some(controller) = &self.controller else {
            return Ok(());
        };
        let (_stream, ptr) = MemoryStream::open(state.to_vec(), MAX_STATE);
        let result = unsafe { controller.setState(ptr.as_ptr()) };
        if result != kResultOk {
            return Err(plugin(format!(
                "the controller's setState failed ({result})"
            )));
        }
        Ok(())
    }

    pub fn programs(&self) -> Vec<ProgramList> {
        let Some(controller) = &self.controller else {
            return Vec::new();
        };
        let Some(info) = controller.cast::<IUnitInfo>() else {
            return Vec::new();
        };
        let mut unit_of: Vec<(i32, i32)> = Vec::new();
        let mut changers: Vec<(i32, u32)> = Vec::new();
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

    pub fn param_text(&self, id: u32, value: f64) -> Option<String> {
        let controller = self.controller.as_ref()?;
        if !value.is_finite() {
            return None;
        }
        let mut text: vst3::Steinberg::Vst::String128 = [0; 128];
        let r = unsafe { controller.getParamStringByValue(id, value.clamp(0.0, 1.0), &mut text) };
        (r == kResultOk).then(|| wide(&text))
    }

    pub fn param_value(&self, id: u32, text: &str) -> Option<f64> {
        let controller = self.controller.as_ref()?;
        if text.contains('\0') {
            return None;
        }
        let mut units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut value = 0.0_f64;
        let r = unsafe { controller.getParamValueByString(id, units.as_mut_ptr(), &mut value) };
        (r == kResultOk && value.is_finite()).then(|| value.clamp(0.0, 1.0))
    }

    pub fn set_param(&self, id: u32, value: f64) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, 1.0);
        if let Some(controller) = &self.controller {
            unsafe { controller.setParamNormalized(id, value) };
        }
        self.handler.send_to_processor(id, value);
    }

    pub fn take_edits(&self, heard: Heard) -> Taken {
        if let Some(controller) = &self.controller {
            for (id, value) in heard.values {
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

pub(super) fn program_parameter(controller: &ComPtr<IEditController>) -> Option<(u32, i32)> {
    let mut found: Option<(u32, i32, i32)> = None;
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
