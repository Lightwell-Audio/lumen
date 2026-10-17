//! The edit controller of both test plugins: parameters and their text.
//! There is no editor view unless the `editor` switch is on (see
//! [`crate::editor`]): hosts must offer their generic controls for a plugin
//! like this.

use std::ffi::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use vst3::Steinberg::Vst::{
    BusDirection, IComponentHandler, IComponentHandlerTrait, IEditController, IEditControllerTrait,
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
}

/// The first id of the extra parameters.
pub const EXTRA_BASE: u32 = 100;
/// How many groups the extra parameters are spread over (ids 1 to this).
pub const EXTRA_GROUPS: u32 = 4;

impl Class for Controller {
    type Interfaces = (IEditController, IUnitInfo);
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
        }
    }

    fn value(&self, i: usize) -> f64 {
        f64::from_bits(self.values[i].load(Ordering::Relaxed))
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.kind.params().iter().position(|p| p.id == id)
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

    unsafe fn setState(&self, _stream: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _stream: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        if self.faults.has(Fault::BadParams) {
            BAD_COUNT
        } else {
            (self.kind.params().len() + self.extra.len()) as i32
        }
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let params = self.kind.params();
        let bad = self.faults.has(Fault::BadParams);
        let past = (index.max(0) as usize).saturating_sub(params.len());
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
        if let Some(k) = self.extra_index(id) {
            return f64::from_bits(self.extra[k].load(Ordering::Relaxed));
        }
        self.index(id).map_or(0.0, |i| self.value(i))
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
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
        if self.faults.has(Fault::WindowEdits)
            && self.kind == Kind::Instrument
            && id == LEVEL
            && before != value
        {
            // Stands in for the plugin's window: moving the level also
            // moves the wave, as the plugin's own gesture, told to the host.
            if let Some(handler) = self.handler() {
                // SAFETY: a live handler the host gave us.
                unsafe {
                    handler.beginEdit(WAVE);
                    handler.performEdit(WAVE, value);
                    handler.endEdit(WAVE);
                }
            }
        }
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
            0
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
        info.programListId = -1;
        kResultOk
    }

    unsafe fn getProgramListCount(&self) -> int32 {
        0
    }

    unsafe fn getProgramListInfo(&self, _index: int32, _info: *mut ProgramListInfo) -> tresult {
        kInvalidArgument
    }

    unsafe fn getProgramName(
        &self,
        _list: ProgramListID,
        _index: int32,
        _name: *mut String128,
    ) -> tresult {
        kInvalidArgument
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
