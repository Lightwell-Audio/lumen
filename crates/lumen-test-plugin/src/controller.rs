//! The edit controller of both test plugins: parameters and their text.
//! There is no editor view unless the `editor` switch is on (see
//! [`crate::editor`]): hosts must offer their generic controls for a plugin
//! like this.

use std::ffi::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use vst3::Steinberg::Vst::{
    BusDirection, IComponentHandler, IComponentHandler2, IComponentHandler2Trait,
    IComponentHandlerTrait, IEditController, IEditControllerTrait, IMidiMapping, IMidiMappingTrait,
    IUnitInfo, IUnitInfoTrait, MediaType, ParameterInfo, ParameterInfo_, ProgramListID,
    ProgramListInfo, RestartFlags_, String128, TChar, UnitID, UnitInfo,
};
use vst3::Steinberg::{
    FUnknown, IBStream, IPlugView, IPluginBaseTrait, int16, int32, kInvalidArgument, kResultFalse,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComRef};

use crate::faults::{Faults, Kind as Fault};
use crate::params::{Kind, LATENCY, LEVEL, WAVE};
use crate::{state, streams, text};

/// How many parameters `bad_params` claims.
pub const BAD_COUNT: i32 = 4_000_000;
const STATE_LIMIT: usize = 4096;

pub struct Controller {
    kind: Kind,
    faults: Faults,
    values: [AtomicU64; 2],
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
    /// The values of the extra parameters `many_params` adds.
    extra: Vec<AtomicU64>,
    /// The trigger's value (bits), when the `window_edits` fault is on.
    trigger: AtomicU64,
    /// The program-change parameter's value (bits), with `presets`.
    program: AtomicU64,
    /// The controller's own state (bits): a view value, with `presets`.
    view: AtomicU64,
}

/// The first id of the extra parameters.
pub const EXTRA_BASE: u32 = 100;
/// The id of the instrument's trigger, which exists with the `window_edits`
/// fault: stepping it from off to on makes the plugin edit its wave as a
/// gesture of its own, the way a control moved in its window would.
pub const TRIGGER: u32 = 90;
/// The id of the instrument's program-change parameter, with `presets`.
pub const PROGRAM: u32 = 91;
/// The programs the instrument offers with `presets`: name, normalized level
/// and normalized wave.
pub const PROGRAMS: [(&str, f64, f64); 3] = [
    ("Square, quiet", 0.25, 0.0),
    ("Saw, loud", 0.9, 1.0),
    ("Saw, half", 0.5, 1.0),
];
/// What the controller's state holds before a `setState`.
pub const VIEW_DEFAULT: f64 = 0.5;
const VIEW_TAG: [u8; 4] = *b"LTV1";
/// How many groups the extra parameters are spread over (ids 1 to this).
pub const EXTRA_GROUPS: u32 = 4;

impl Class for Controller {
    type Interfaces = (IEditController, IUnitInfo, IMidiMapping);
}

impl Controller {
    pub fn new(kind: Kind, faults: Faults) -> Self {
        let d = kind.defaults();
        let extra = (0..faults.many_params())
            .map(|_| AtomicU64::new(0.5_f64.to_bits()))
            .collect();
        Self {
            kind,
            faults,
            values: [
                AtomicU64::new(d[0].to_bits()),
                AtomicU64::new(d[1].to_bits()),
            ],
            handler: Mutex::new(None),
            extra,
            trigger: AtomicU64::new(0.0_f64.to_bits()),
            program: AtomicU64::new(0.0_f64.to_bits()),
            view: AtomicU64::new(VIEW_DEFAULT.to_bits()),
        }
    }

    /// Whether the instrument offers its program list.
    fn has_programs(&self) -> bool {
        self.kind == Kind::Instrument && self.faults.has(Fault::Presets)
    }

    fn value(&self, i: usize) -> f64 {
        f64::from_bits(self.values[i].load(Ordering::Relaxed))
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.kind.params().iter().position(|p| p.id == id)
    }

    /// Whether the controller lists the trigger.
    fn has_trigger(&self) -> bool {
        self.kind == Kind::Instrument
            && (self.faults.has(Fault::WindowEdits)
                || self.faults.has(Fault::SetDirty)
                || self.faults.has(Fault::ValuesChanged)
                || self.faults.has(Fault::BusChange))
    }

    /// Which extra parameter `id` is, if it is one.
    fn extra_index(&self, id: u32) -> Option<usize> {
        let k = id.checked_sub(EXTRA_BASE)? as usize;
        (k < self.extra.len()).then_some(k)
    }

    fn handler(&self) -> Option<ComPtr<IComponentHandler>> {
        // Cloned out so no lock is held over the host's call.
        self.handler
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        *self.handler.lock().unwrap_or_else(|e| e.into_inner()) = None;
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: the host passes a live stream or null.
        let Some(bytes) = (unsafe { streams::read_all(stream, STATE_LIMIT) }) else {
            return kResultFalse;
        };
        let Some(values) = state::decode(self.kind, &bytes) else {
            return kResultFalse;
        };
        for (slot, v) in self.values.iter().zip(values) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        kResultOk
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        if !self.faults.has(Fault::Presets) {
            return kResultOk;
        }
        // SAFETY: the host passes a live stream or null.
        let Some(bytes) = (unsafe { streams::read_all(stream, STATE_LIMIT) }) else {
            return kResultFalse;
        };
        if bytes.len() != 12 || bytes[..4] != VIEW_TAG {
            return kResultFalse;
        }
        let value = f64::from_le_bytes(bytes[4..].try_into().expect("8 bytes"));
        if !(0.0..=1.0).contains(&value) {
            return kResultFalse;
        }
        self.view.store(value.to_bits(), Ordering::Relaxed);
        kResultOk
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        if !self.faults.has(Fault::Presets) {
            return kResultOk;
        }
        let mut bytes = VIEW_TAG.to_vec();
        bytes.extend_from_slice(&f64::from_bits(self.view.load(Ordering::Relaxed)).to_le_bytes());
        // SAFETY: the host passes a live stream or null.
        if unsafe { streams::write_all(stream, &bytes) } {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn getParameterCount(&self) -> i32 {
        if self.faults.has(Fault::BadParams) {
            BAD_COUNT
        } else {
            (self.kind.params().len()
                + self.extra.len()
                + usize::from(self.has_trigger())
                + usize::from(self.has_programs())) as i32
        }
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let params = self.kind.params();
        let bad = self.faults.has(Fault::BadParams);
        let past = (index.max(0) as usize).saturating_sub(params.len());
        if index >= 0
            && !bad
            && self.has_programs()
            && index as usize == params.len() + self.extra.len() + usize::from(self.has_trigger())
        {
            // SAFETY: the host passes a valid out pointer.
            let info = unsafe { &mut *info };
            info.id = PROGRAM;
            text::copy_wstring("Program", &mut info.title);
            text::copy_wstring("Prog", &mut info.shortTitle);
            text::copy_wstring("", &mut info.units);
            info.stepCount = PROGRAMS.len() as i32 - 1;
            info.defaultNormalizedValue = 0.0;
            info.unitId = 0;
            info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate
                | ParameterInfo_::ParameterFlags_::kIsProgramChange;
            return kResultOk;
        }
        if index >= 0
            && index as usize >= params.len()
            && !bad
            && self.has_trigger()
            && past == self.extra.len()
        {
            // SAFETY: the host passes a valid out pointer.
            let info = unsafe { &mut *info };
            info.id = TRIGGER;
            text::copy_wstring("Trigger", &mut info.title);
            text::copy_wstring("Trig", &mut info.shortTitle);
            text::copy_wstring("", &mut info.units);
            info.stepCount = 1;
            info.defaultNormalizedValue = 0.0;
            info.unitId = 0;
            info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
            return kResultOk;
        }
        if index >= 0 && index as usize >= params.len() && !bad && past < self.extra.len() {
            // SAFETY: the host passes a valid out pointer.
            let info = unsafe { &mut *info };
            let k = past as u32;
            info.id = EXTRA_BASE + k;
            text::copy_wstring(&format!("Param {k}"), &mut info.title);
            text::copy_wstring(&format!("P{k}"), &mut info.shortTitle);
            text::copy_wstring("", &mut info.units);
            info.stepCount = 0;
            info.defaultNormalizedValue = 0.5;
            info.unitId = 1 + (k % EXTRA_GROUPS) as i32;
            info.flags = if k % 7 == 3 {
                ParameterInfo_::ParameterFlags_::kIsHidden
            } else if k % 11 == 5 {
                ParameterInfo_::ParameterFlags_::kIsReadOnly
            } else {
                ParameterInfo_::ParameterFlags_::kCanAutomate
            };
            return kResultOk;
        }
        if index < 0 || (index as usize >= params.len() && !bad) || index >= BAD_COUNT {
            return kInvalidArgument;
        }
        // Past the real parameters, `bad_params` repeats the first one.
        let p = params.get(index as usize).unwrap_or(&params[0]);
        // SAFETY: the host passes a valid out pointer.
        let info = unsafe { &mut *info };
        info.id = p.id;
        text::copy_wstring(p.title, &mut info.title);
        text::copy_wstring(p.title, &mut info.shortTitle);
        text::copy_wstring(p.units, &mut info.units);
        info.stepCount = p.steps;
        info.defaultNormalizedValue = p.default;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, id: u32, value: f64, string: *mut String128) -> tresult {
        if id == PROGRAM && self.has_programs() {
            let k = (value.clamp(0.0, 1.0) * (PROGRAMS.len() - 1) as f64).round() as usize;
            // SAFETY: the host passes a valid String128.
            text::copy_wstring(PROGRAMS[k].0, unsafe { &mut *string });
            return kResultOk;
        }
        if id == TRIGGER && self.has_trigger() {
            // SAFETY: the host passes a valid String128.
            text::copy_wstring(if value < 0.5 { "Off" } else { "On" }, unsafe {
                &mut *string
            });
            return kResultOk;
        }
        if self.extra_index(id).is_some() {
            // SAFETY: the host passes a valid String128.
            text::copy_wstring(&format!("{:.1} %", value * 100.0), unsafe { &mut *string });
            return kResultOk;
        }
        if self.index(id).is_none() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid String128.
        text::copy_wstring(&self.kind.display(id, value), unsafe { &mut *string });
        kResultOk
    }

    unsafe fn getParamValueByString(
        &self,
        id: u32,
        string: *mut TChar,
        value: *mut f64,
    ) -> tresult {
        if self.index(id).is_none() || string.is_null() {
            return kInvalidArgument;
        }
        let mut units = Vec::new();
        // SAFETY: the host passes a NUL-terminated string; reading stops at
        // the NUL or after a generous cap.
        unsafe {
            while units.len() < 256 && *string.add(units.len()) != 0 {
                units.push(*string.add(units.len()));
            }
        }
        match self.kind.parse(id, &String::from_utf16_lossy(&units)) {
            Some(v) => {
                // SAFETY: the host passes a valid out pointer.
                unsafe { *value = v };
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn normalizedParamToPlain(&self, id: u32, value: f64) -> f64 {
        self.kind.plain(id, value)
    }

    unsafe fn plainParamToNormalized(&self, id: u32, plain: f64) -> f64 {
        self.kind.normalized(id, plain)
    }

    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        if id == PROGRAM && self.has_programs() {
            return f64::from_bits(self.program.load(Ordering::Relaxed));
        }
        if id == TRIGGER && self.has_trigger() {
            return f64::from_bits(self.trigger.load(Ordering::Relaxed));
        }
        if let Some(k) = self.extra_index(id) {
            return f64::from_bits(self.extra[k].load(Ordering::Relaxed));
        }
        self.index(id).map_or(0.0, |i| self.value(i))
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        if id == PROGRAM && self.has_programs() {
            if !value.is_finite() {
                return kInvalidArgument;
            }
            let value = value.clamp(0.0, 1.0);
            self.program.store(value.to_bits(), Ordering::Relaxed);
            let k = (value * (PROGRAMS.len() - 1) as f64).round() as usize;
            let (_, level, wave) = PROGRAMS[k];
            self.values[0].store(level.to_bits(), Ordering::Relaxed);
            self.values[1].store(wave.to_bits(), Ordering::Relaxed);
            // A program changes other parameters: the plugin tells the host,
            // as a gesture of its own, so the processor hears them too.
            if let Some(handler) = self.handler() {
                // SAFETY: a live handler the host gave us.
                unsafe {
                    for (id, v) in [(LEVEL, level), (WAVE, wave)] {
                        handler.beginEdit(id);
                        handler.performEdit(id, v);
                        handler.endEdit(id);
                    }
                    handler.restartComponent(RestartFlags_::kParamValuesChanged);
                }
            }
            return kResultOk;
        }
        if id == TRIGGER && self.has_trigger() {
            if !value.is_finite() {
                return kInvalidArgument;
            }
            let value = value.clamp(0.0, 1.0);
            let before = f64::from_bits(self.trigger.swap(value.to_bits(), Ordering::Relaxed));
            if before < 0.5 && value >= 0.5 {
                // Stands in for the plugin's window: a control moved there is
                // a gesture of the plugin's own, told to the host.
                if let Some(handler) = self.handler() {
                    // SAFETY: a live handler the host gave us.
                    unsafe {
                        if self.faults.has(Fault::WindowEdits) {
                            handler.beginEdit(WAVE);
                            handler.performEdit(WAVE, 1.0);
                            handler.endEdit(WAVE);
                        }
                        if self.faults.has(Fault::SetDirty)
                            && let Some(second) = handler.cast::<IComponentHandler2>()
                        {
                            second.setDirty(1);
                        }
                        if self.faults.has(Fault::ValuesChanged) {
                            handler.restartComponent(RestartFlags_::kParamValuesChanged);
                        }
                        if self.faults.has(Fault::BusChange) {
                            // The instrument now has one output more.
                            crate::component::EXTRA_OUTPUT.store(true, Ordering::Relaxed);
                            handler.restartComponent(RestartFlags_::kIoChanged);
                        }
                    }
                }
            }
            return kResultOk;
        }
        if let Some(k) = self.extra_index(id) {
            if !value.is_finite() {
                return kInvalidArgument;
            }
            self.extra[k].store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            return kResultOk;
        }
        let Some(i) = self.index(id) else {
            return kInvalidArgument;
        };
        if !value.is_finite() {
            return kInvalidArgument;
        }
        let value = value.clamp(0.0, 1.0);
        let before = f64::from_bits(self.values[i].swap(value.to_bits(), Ordering::Relaxed));
        if self.kind == Kind::Effect && id == LATENCY && before != value {
            // The delay is the plugin's reported latency: tell the host to
            // ask again. The handler is cloned out so no lock is held over
            // the host's call.
            let handler = self
                .handler
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(handler) = handler {
                // SAFETY: a live handler the host gave us.
                unsafe { handler.restartComponent(RestartFlags_::kLatencyChanged) };
            }
        }
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        // SAFETY: the host passes a live handler or null.
        let handler = unsafe { ComRef::from_raw(handler) }.map(|h| h.to_com_ptr());
        *self.handler.lock().unwrap_or_else(|e| e.into_inner()) = handler;
        kResultOk
    }

    unsafe fn createView(&self, name: *const c_char) -> *mut IPlugView {
        // SAFETY: a NUL-terminated name or null, per the VST3 ABI.
        let wanted = !name.is_null() && unsafe { std::ffi::CStr::from_ptr(name) } == c"editor";
        if wanted && self.faults.editor() {
            crate::editor::create(&self.faults)
        } else {
            std::ptr::null_mut::<IPlugView>()
        }
    }
}

impl IUnitInfoTrait for Controller {
    unsafe fn getUnitCount(&self) -> int32 {
        if self.extra.is_empty() {
            int32::from(self.has_programs())
        } else {
            1 + EXTRA_GROUPS as int32
        }
    }

    unsafe fn getUnitInfo(&self, index: int32, info: *mut UnitInfo) -> tresult {
        // SAFETY: a plain call on this object.
        let count = unsafe { self.getUnitCount() };
        if index < 0 || index >= count || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid out pointer.
        let info = unsafe { &mut *info };
        info.id = index;
        // The root has no parent (`kNoParentUnitId`).
        info.parentUnitId = if index == 0 { -1 } else { 0 };
        let name = if index == 0 {
            "Root".to_string()
        } else {
            format!("Group {index}")
        };
        text::copy_wstring(&name, &mut info.name);
        info.programListId = if index == 0 && self.has_programs() {
            0
        } else {
            -1
        };
        kResultOk
    }

    unsafe fn getProgramListCount(&self) -> int32 {
        int32::from(self.has_programs())
    }

    unsafe fn getProgramListInfo(&self, index: int32, info: *mut ProgramListInfo) -> tresult {
        if index != 0 || !self.has_programs() || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid out pointer.
        let info = unsafe { &mut *info };
        info.id = 0;
        text::copy_wstring("Factory", &mut info.name);
        info.programCount = PROGRAMS.len() as int32;
        kResultOk
    }

    unsafe fn getProgramName(
        &self,
        list: ProgramListID,
        index: int32,
        name: *mut String128,
    ) -> tresult {
        if list != 0 || !self.has_programs() || name.is_null() {
            return kInvalidArgument;
        }
        let Some((title, _, _)) = usize::try_from(index).ok().and_then(|k| PROGRAMS.get(k)) else {
            return kInvalidArgument;
        };
        // SAFETY: the host passes a valid out pointer.
        text::copy_wstring(title, unsafe { &mut *name });
        kResultOk
    }

    unsafe fn getProgramInfo(
        &self,
        _list: ProgramListID,
        _index: int32,
        _attribute: *const c_char,
        _value: *mut String128,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn hasProgramPitchNames(&self, _list: ProgramListID, _index: int32) -> tresult {
        kResultFalse
    }

    unsafe fn getProgramPitchName(
        &self,
        _list: ProgramListID,
        _index: int32,
        _pitch: int16,
        _name: *mut String128,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn getSelectedUnit(&self) -> UnitID {
        0
    }

    unsafe fn selectUnit(&self, _unit: UnitID) -> tresult {
        kResultFalse
    }

    unsafe fn getUnitByBus(
        &self,
        _type: MediaType,
        _dir: BusDirection,
        _bus: int32,
        _channel: int32,
        _unit: *mut UnitID,
    ) -> tresult {
        kResultFalse
    }

    unsafe fn setUnitProgramData(
        &self,
        _list: int32,
        _index: int32,
        _data: *mut IBStream,
    ) -> tresult {
        kResultFalse
    }
}

impl IMidiMappingTrait for Controller {
    /// Channel 0 only: the level follows controller 7 (volume) and the wave
    /// follows pitch bend (129). Pressure (128) and everything else map to
    /// nothing, so a host's handling of an unmapped controller can be tested.
    unsafe fn getMidiControllerAssignment(
        &self,
        bus: int32,
        channel: int16,
        controller: i16,
        id: *mut u32,
    ) -> tresult {
        if bus != 0 || channel != 0 || id.is_null() {
            return kResultFalse;
        }
        let mapped = match controller {
            7 => LEVEL,
            129 => WAVE,
            _ => return kResultFalse,
        };
        // SAFETY: the host passes a valid out pointer.
        unsafe { *id = mapped };
        kResultOk
    }
}
