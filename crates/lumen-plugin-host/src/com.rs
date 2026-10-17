//! The COM objects a host gives a plugin: the host context, the handler that
//! hears about edits and restarts, event lists, parameter changes and a
//! memory stream for state. Built on the `vst3` crate's `ComWrapper`; the
//! methods take `&self`, so what changes lives behind a lock or an atomic.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::Vst::{
    Event, IAttributeList, IAttributeListTrait, IComponentHandler, IComponentHandlerTrait,
    IEventList, IEventListTrait, IHostApplication, IHostApplicationTrait, IMessage, IMessageTrait,
    IParamValueQueue, IParamValueQueueTrait, IParameterChanges, IParameterChangesTrait, ParamID,
    ParamValue, String128, TChar,
};
use vst3::Steinberg::{
    IBStream, IBStreamTrait, TUID, int32, int64, kInvalidArgument, kNotImplemented, kResultFalse,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper, Interface};

use crate::protocol::ParamQueue;

/// An attribute's name in a list.
type AttrID = *const c_char;

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

    /// Plugins with a separate controller ask the host for a message and an
    /// attribute list to talk to it with, and not all of them look at what
    /// they get back. These two are the only objects a host has to make.
    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if obj.is_null() || cid.is_null() || iid.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: valid out pointer, and valid ids from the caller.
        let (cid, iid) = unsafe {
            *obj = std::ptr::null_mut();
            (*cid, *iid)
        };
        let same = |id: TUID, wanted: [u8; 16]| id.map(|c| c as u8) == wanted;
        let made: Option<*mut c_void> = if same(cid, IMessage::IID) && same(iid, IMessage::IID) {
            ComWrapper::new(HostMessage::new())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw().cast())
        } else if same(cid, IAttributeList::IID) && same(iid, IAttributeList::IID) {
            ComWrapper::new(HostAttributes::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw().cast())
        } else {
            None
        };
        match made {
            Some(raw) => {
                // SAFETY: the out pointer is valid; the plugin owns the
                // reference `into_raw` handed over.
                unsafe { *obj = raw };
                kResultOk
            }
            None => kNotImplemented,
        }
    }
}

/// A value in an attribute list.
enum Attribute {
    Int(i64),
    Float(f64),
    /// UTF-16 units, without the terminator.
    Text(Vec<u16>),
    Binary(Vec<u8>),
}

/// An attribute list: what a message carries.
#[derive(Default)]
pub struct HostAttributes {
    values: Mutex<HashMap<String, Attribute>>,
}

impl HostAttributes {
    fn set(&self, id: AttrID, value: Attribute) -> tresult {
        if id.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, value);
        kResultOk
    }

    fn get<R>(&self, id: AttrID, read: impl FnOnce(&Attribute) -> Option<R>) -> Option<R> {
        if id.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        values.get(id.as_ref()).and_then(read)
    }
}

impl Class for HostAttributes {
    type Interfaces = (IAttributeList,);
}

impl IAttributeListTrait for HostAttributes {
    unsafe fn setInt(&self, id: AttrID, value: i64) -> tresult {
        self.set(id, Attribute::Int(value))
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut i64) -> tresult {
        if value.is_null() {
            return kInvalidArgument;
        }
        match self.get(id, |a| match a {
            Attribute::Int(v) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                // SAFETY: a valid out pointer.
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attribute::Float(value))
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        if value.is_null() {
            return kInvalidArgument;
        }
        match self.get(id, |a| match a {
            Attribute::Float(v) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                // SAFETY: a valid out pointer.
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut units = Vec::new();
        // SAFETY: a NUL-terminated UTF-16 string from the plugin.
        unsafe {
            let mut at = string;
            while *at != 0 {
                units.push(*at as u16);
                at = at.add(1);
            }
        }
        self.set(id, Attribute::Text(units))
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: u32) -> tresult {
        if string.is_null() || size_in_bytes < 2 {
            return kInvalidArgument;
        }
        let Some(units) = self.get(id, |a| match a {
            Attribute::Text(t) => Some(t.clone()),
            _ => None,
        }) else {
            return kResultFalse;
        };
        let room = (size_in_bytes as usize / 2).saturating_sub(1);
        // SAFETY: the caller's buffer holds `size_in_bytes` bytes.
        unsafe {
            for (i, unit) in units.iter().take(room).enumerate() {
                *string.add(i) = *unit as TChar;
            }
            *string.add(units.len().min(room)) = 0;
        }
        kResultOk
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: u32) -> tresult {
        if data.is_null() && size_in_bytes != 0 {
            return kInvalidArgument;
        }
        let bytes = if size_in_bytes == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin vouches for `size_in_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size_in_bytes as usize) }
                .to_vec()
        };
        self.set(id, Attribute::Binary(bytes))
    }

    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut u32,
    ) -> tresult {
        if data.is_null() || size_in_bytes.is_null() || id.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        match values.get(id.as_ref()) {
            Some(Attribute::Binary(bytes)) => {
                // SAFETY: valid out pointers; the bytes live as long as the
                // list holds the value, which is how long the plugin may use
                // them.
                unsafe {
                    *data = bytes.as_ptr().cast();
                    *size_in_bytes = bytes.len() as u32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// A message: an id and an attribute list.
pub struct HostMessage {
    id: Mutex<CString>,
    attributes: ComPtr<IAttributeList>,
    // Keeps the list's object alive; the message hands out its pointer.
    _list: ComWrapper<HostAttributes>,
}

impl HostMessage {
    fn new() -> Self {
        let list = ComWrapper::new(HostAttributes::default());
        let attributes = list
            .to_com_ptr::<IAttributeList>()
            .expect("the list implements its interface");
        Self {
            id: Mutex::new(CString::default()),
            attributes,
            _list: list,
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> *const c_char {
        // The string lives in the message; the plugin reads it before the id
        // is set again, as with any host.
        self.id.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
    }

    unsafe fn setMessageID(&self, id: *const c_char) {
        if id.is_null() {
            return;
        }
        // SAFETY: a NUL-terminated id from the plugin.
        let id = unsafe { CStr::from_ptr(id) }.to_owned();
        *self.id.lock().unwrap_or_else(|e| e.into_inner()) = id;
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attributes.as_ptr()
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

    /// The points of `params` in `len` frames from `start`, moved to start
    /// at 0: one piece of a block given to the plugin in pieces.
    pub fn set_span(&self, params: &[ParamQueue], start: u32, len: u32) {
        let end = start.saturating_add(len);
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
                data.1.extend(
                    param
                        .points
                        .iter()
                        .filter(|&&(at, _)| (start..end).contains(&at))
                        .map(|&(at, v)| ((at - start) as int32, v)),
                );
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

/// A queue that takes a plugin's reported changes and keeps none of them.
struct Sink;

impl Class for Sink {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Sink {
    unsafe fn getParameterId(&self) -> ParamID {
        0
    }
    unsafe fn getPointCount(&self) -> int32 {
        0
    }
    unsafe fn getPoint(
        &self,
        _index: int32,
        _sample_offset: *mut int32,
        _value: *mut ParamValue,
    ) -> tresult {
        kResultFalse
    }
    unsafe fn addPoint(
        &self,
        _sample_offset: int32,
        _value: ParamValue,
        index: *mut int32,
    ) -> tresult {
        if !index.is_null() {
            // SAFETY: a valid out pointer from the caller.
            unsafe { *index = 0 };
        }
        kResultOk
    }
}

/// The list of output parameter changes a plugin reports its own edits in.
/// It takes them and keeps none: a plugin that reports a change uses the
/// queue it is given without looking, so there must be one.
pub struct NoOutputChanges {
    sink: ComPtr<IParamValueQueue>,
}

impl NoOutputChanges {
    pub fn new() -> Self {
        Self {
            sink: ComWrapper::new(Sink)
                .to_com_ptr::<IParamValueQueue>()
                .expect("interface listed"),
        }
    }
}

impl Default for NoOutputChanges {
    fn default() -> Self {
        Self::new()
    }
}

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
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if !index.is_null() {
            // SAFETY: a valid out pointer from the caller.
            unsafe { *index = 0 };
        }
        self.sink.as_ptr()
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
