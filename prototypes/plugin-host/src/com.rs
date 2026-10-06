//! The COM objects a VST3 host must provide: host context, event list,
//! parameter changes and a memory stream for state. Built on the `vst3`
//! crate's `ComWrapper`. Methods take `&self`, so state lives in a `Mutex`.

use std::ffi::c_void;
use std::sync::Mutex;

use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::Vst::{
    Event, IEventList, IEventListTrait, IHostApplication, IHostApplicationTrait, IParamValueQueue,
    IParameterChanges, IParameterChangesTrait, ParamID, String128,
};
use vst3::Steinberg::{
    IBStream, IBStreamTrait, TUID, int32, int64, kInvalidArgument, kNotImplemented, kResultOk,
    tresult,
};
use vst3::{Class, ComPtr, ComWrapper};

pub struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let text: Vec<u16> = "Lumen".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the plugin passes a String128 buffer; "Lumen\0" fits.
        unsafe {
            for (i, unit) in text.iter().enumerate() {
                (*name)[i] = *unit;
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
            // SAFETY: valid out pointer from the caller.
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

/// Events for one block. Filled by the host before `process`.
pub struct EventList(pub Mutex<Vec<Event>>);

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.0.lock().map(|e| e.len() as int32).unwrap_or(0)
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        let Ok(events) = self.0.lock() else {
            return kInvalidArgument;
        };
        match events.get(index as usize) {
            Some(event) if !e.is_null() => {
                // SAFETY: valid out pointer from the plugin.
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
        if let Ok(mut events) = self.0.lock() {
            // SAFETY: valid event from the plugin.
            events.push(unsafe { *e });
        }
        kResultOk
    }
}

/// No parameter changes. Some plugins crash on a null pointer, so the host
/// passes an empty list instead.
pub struct NoParameterChanges;

impl Class for NoParameterChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for NoParameterChanges {
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

/// In-memory `IBStream` used for plugin state.
pub struct MemoryStream(pub Mutex<(Vec<u8>, usize)>);

impl MemoryStream {
    pub fn open(data: Vec<u8>) -> ComPtr<IBStream> {
        ComWrapper::new(Self(Mutex::new((data, 0))))
            .to_com_ptr::<IBStream>()
            .expect("interface listed")
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
        let Ok(mut guard) = self.0.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = (num_bytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        // SAFETY: the plugin's buffer holds `num_bytes` bytes; n <= num_bytes.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer.cast::<u8>(), n) };
        *pos += n;
        if !num_bytes_read.is_null() {
            // SAFETY: valid out pointer.
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
        let Ok(mut guard) = self.0.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let n = num_bytes.max(0) as usize;
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        // SAFETY: the plugin's buffer holds `num_bytes` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer.cast::<u8>(), data.as_mut_ptr().add(*pos), n)
        };
        *pos += n;
        if !num_bytes_written.is_null() {
            // SAFETY: valid out pointer.
            unsafe { *num_bytes_written = n as int32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut guard) = self.0.lock() else {
            return kInvalidArgument;
        };
        let (data, pos) = &mut *guard;
        let base = match mode {
            m if m == kIBSeekSet as int32 => 0,
            m if m == kIBSeekCur as int32 => *pos as int64,
            m if m == kIBSeekEnd as int32 => data.len() as int64,
            _ => return kInvalidArgument,
        };
        let target = base + offset;
        if target < 0 {
            return kInvalidArgument;
        }
        *pos = target as usize;
        if !result.is_null() {
            // SAFETY: valid out pointer.
            unsafe { *result = target };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(guard) = self.0.lock() else {
            return kInvalidArgument;
        };
        if !pos.is_null() {
            // SAFETY: valid out pointer.
            unsafe { *pos = guard.1 as int64 };
        }
        kResultOk
    }
}

/// Reads the whole contents back out of a stream created by [`MemoryStream::open`].
pub fn stream_bytes(stream: &ComPtr<IBStream>) -> Vec<u8> {
    // Seek to the start and read everything through the interface itself.
    let mut out = Vec::new();
    let mut chunk = [0_u8; 4096];
    // SAFETY: plain calls on our own live stream object.
    unsafe {
        stream.seek(0, kIBSeekSet as int32, std::ptr::null_mut());
        loop {
            let mut read = 0;
            stream.read(chunk.as_mut_ptr().cast(), chunk.len() as int32, &mut read);
            if read <= 0 {
                break;
            }
            out.extend_from_slice(&chunk[..read as usize]);
        }
    }
    out
}
