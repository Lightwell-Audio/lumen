//! The COM objects a host gives a plugin: the host context, the handler that
//! hears about edits and restarts, event lists, parameter changes and a
//! memory stream for state. Built on the `vst3` crate's `ComWrapper`; the
//! methods take `&self`, so what changes lives behind a lock or an atomic.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::Vst::{
    Event, IComponentHandler, IComponentHandlerTrait, IEventList, IEventListTrait,
    IHostApplication, IHostApplicationTrait, IParamValueQueue, IParamValueQueueTrait,
    IParameterChanges, IParameterChangesTrait, ParamID, ParamValue, String128,
};
use vst3::Steinberg::{
    IBStream, IBStreamTrait, TUID, int32, int64, kInvalidArgument, kNotImplemented, kResultOk,
    tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

use crate::protocol::ParamQueue;

pub struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the plugin passes a String128 buffer; "Lumen\0" fits.
        unsafe {
            for (i, unit) in "Lumen".encode_utf16().chain(Some(0)).enumerate() {
                (*name)[i] = unit;
            }
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if !obj.is_null() {
            // SAFETY: a valid out pointer from the caller.
            unsafe { *obj = std::ptr::null_mut() };
        }
        kNotImplemented
    }
}

pub fn host_application() -> ComPtr<IHostApplication> {
    ComWrapper::new(HostApplication)
        .to_com_ptr::<IHostApplication>()
        .expect("interface listed")
}

/// Hears the controller: restart requests (latency or bus changes) and
/// parameter edits made in its own windows.
#[derive(Default)]
pub struct Handler {
    pub restart: AtomicU32,
    pub edits: Mutex<Vec<(u32, f64)>>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if let Ok(mut edits) = self.edits.lock()
            && edits.len() < 4096
        {
            edits.push((id, value));
        }
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags as u32, Ordering::AcqRel);
        kResultOk
    }
}

/// Events for one block. The host fills it before `process`; a plugin's
/// output events are appended to a second list that nothing reads yet.
#[derive(Default)]
pub struct EventList(Mutex<Vec<Event>>);

impl EventList {
    pub fn set(&self, events: impl IntoIterator<Item = Event>) {
        if let Ok(mut list) = self.0.lock() {
            list.clear();
            list.extend(events);
        }
    }
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.0.lock().map_or(0, |e| e.len() as int32)
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        let Ok(events) = self.0.lock() else {
            return kInvalidArgument;
        };
        match events.get(index as usize) {
            Some(event) if !e.is_null() => {
                // SAFETY: a valid out pointer from the plugin.
                unsafe { *e = *event };
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        if let Ok(mut events) = self.0.lock()
            && events.len() < 4096
        {
            // SAFETY: a valid event from the plugin.
            events.push(unsafe { *e });
        }
        kResultOk
    }
}

/// One parameter's points inside a block. The objects are made once and
/// refilled, so a steady stream of blocks does not allocate.
struct Queue {
    data: Mutex<(ParamID, Vec<(int32, ParamValue)>)>,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.data.lock().map_or(0, |d| d.0)
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.data.lock().map_or(0, |d| d.1.len() as int32)
    }

    unsafe fn getPoint(&self, index: int32, at: *mut int32, value: *mut ParamValue) -> tresult {
        let Ok(data) = self.data.lock() else {
            return kInvalidArgument;
        };
        match data.1.get(index as usize) {
            Some(&(a, v)) if !at.is_null() && !value.is_null() => {
                // SAFETY: valid out pointers from the plugin.
                unsafe {
                    *at = a;
                    *value = v;
                }
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn addPoint(&self, _at: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kInvalidArgument
    }
}

/// The parameter changes of one block.
pub struct Changes {
    queues: Mutex<Vec<ComWrapper<Queue>>>,
    live: AtomicU32,
}

impl Changes {
    pub fn new() -> Self {
        Self {
            queues: Mutex::new(Vec::new()),
            live: AtomicU32::new(0),
        }
    }

    pub fn set(&self, params: &[ParamQueue]) {
        let Ok(mut queues) = self.queues.lock() else {
            return;
        };
        while queues.len() < params.len() {
            queues.push(ComWrapper::new(Queue {
                data: Mutex::new((0, Vec::new())),
            }));
        }
        for (queue, param) in queues.iter().zip(params) {
            if let Ok(mut data) = queue.data.lock() {
                data.0 = param.id;
                data.1.clear();
                data.1
                    .extend(param.points.iter().map(|&(at, v)| (at as int32, v)));
            }
        }
        self.live.store(params.len() as u32, Ordering::Release);
    }
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.live.load(Ordering::Acquire) as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index < 0 || index as u32 >= self.live.load(Ordering::Acquire) {
            return std::ptr::null_mut();
        }
        let Ok(queues) = self.queues.lock() else {
            return std::ptr::null_mut();
        };
        queues
            .get(index as usize)
            .and_then(|q| q.to_com_ptr::<IParamValueQueue>())
            // The object stays alive in `queues`; the plugin borrows it for
            // the call, as the interface says.
            .map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }

    unsafe fn addParameterData(
        &self,
        _id: *const ParamID,
        _index: *mut int32,
    ) -> *mut IParamValueQueue {
        // Output parameter changes are not collected yet; a plugin may
        // return when told there is no room.
        std::ptr::null_mut()
    }
}

/// An empty list of output parameter changes.
pub struct NoOutputChanges;

impl Class for NoOutputChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for NoOutputChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        0
    }
    unsafe fn getParameterData(&self, _index: int32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
    unsafe fn addParameterData(
        &self,
        _id: *const ParamID,
        _index: *mut int32,
    ) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

/// An in-memory `IBStream` for plugin state. The size is capped, so a plugin
/// that writes without end cannot take the host's memory.
pub struct MemoryStream {
    inner: Mutex<(Vec<u8>, usize)>,
    cap: usize,
}

impl MemoryStream {
    pub fn open(data: Vec<u8>, cap: usize) -> (ComWrapper<Self>, ComPtr<IBStream>) {
        let wrapper = ComWrapper::new(Self {
            inner: Mutex::new((data, 0)),
            cap,
        });
        let ptr = wrapper.to_com_ptr::<IBStream>().expect("interface listed");
        (wrapper, ptr)
    }

    /// Everything written so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.inner.lock().map(|g| g.0.clone()).unwrap_or_default()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_read: *mut int32,
    ) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = (num_bytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        // SAFETY: the plugin's buffer holds `num_bytes` bytes and n <= num_bytes.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer.cast::<u8>(), n) };
        *pos += n;
        if !num_bytes_read.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *num_bytes_read = n as int32 };
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_written: *mut int32,
    ) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = num_bytes.max(0) as usize;
        if pos.saturating_add(n) > self.cap {
            return kInvalidArgument;
        }
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        // SAFETY: the plugin's buffer holds `num_bytes` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer.cast::<u8>(), data.as_mut_ptr().add(*pos), n)
        };
        *pos += n;
        if !num_bytes_written.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *num_bytes_written = n as int32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let base = match mode {
            m if m == kIBSeekSet as int32 => 0,
            m if m == kIBSeekCur as int32 => *pos as int64,
            m if m == kIBSeekEnd as int32 => data.len() as int64,
            _ => return kInvalidArgument,
        };
        let target = base.saturating_add(offset);
        if target < 0 || target as u64 > self.cap as u64 {
            return kInvalidArgument;
        }
        *pos = target as usize;
        if !result.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *result = target };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(guard) = self.inner.lock() else {
            return kInvalidArgument;
        };
        if !pos.is_null() {
            // SAFETY: a valid out pointer.
            unsafe { *pos = guard.1 as int64 };
        }
        kResultOk
    }
}
