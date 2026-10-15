//! The edit controller of both test plugins: parameters and their text.
//! There is no editor view on purpose: hosts must offer their generic
//! controls for a plugin like this.

use std::ffi::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use vst3::Steinberg::Vst::{
    IComponentHandler, IComponentHandlerTrait, IEditController, IEditControllerTrait,
    ParameterInfo, ParameterInfo_, RestartFlags_, String128, TChar,
};
use vst3::Steinberg::{
    FUnknown, IBStream, IPlugView, IPluginBaseTrait, kInvalidArgument, kResultFalse, kResultOk,
    tresult,
};
use vst3::{Class, ComPtr, ComRef};

use crate::faults::{Faults, Kind as Fault};
use crate::params::{Kind, LATENCY};
use crate::{state, streams, text};

/// How many parameters `bad_params` claims.
pub const BAD_COUNT: i32 = 4_000_000;
const STATE_LIMIT: usize = 4096;

pub struct Controller {
    kind: Kind,
    faults: Faults,
    values: [AtomicU64; 2],
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
}

impl Class for Controller {
    type Interfaces = (IEditController,);
}

impl Controller {
    pub fn new(kind: Kind, faults: Faults) -> Self {
        let d = kind.defaults();
        Self {
            kind,
            faults,
            values: [
                AtomicU64::new(d[0].to_bits()),
                AtomicU64::new(d[1].to_bits()),
            ],
            handler: Mutex::new(None),
        }
    }

    fn value(&self, i: usize) -> f64 {
        f64::from_bits(self.values[i].load(Ordering::Relaxed))
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.kind.params().iter().position(|p| p.id == id)
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
            self.kind.params().len() as i32
        }
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let params = self.kind.params();
        let bad = self.faults.has(Fault::BadParams);
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
        self.index(id).map_or(0.0, |i| self.value(i))
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
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

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        std::ptr::null_mut::<IPlugView>()
    }
}
