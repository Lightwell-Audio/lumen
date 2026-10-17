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

use crate::protocol::{Edit, EditKind, MAX_EDITS, ParamQueue};

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
///
/// An edit is kept twice: once for Lumen, in order and with its gesture marks
/// ([`Handler::take_reports`]), and once for the processor, which hears the
/// latest value of each parameter at the start of its next block
/// ([`Handler::drain_to_processor`]): a host's duty, because a controller
/// only tells the host; the processor knows nothing until it is told.
#[derive(Default)]
pub struct Handler {
    pub restart: AtomicU32,
    reports: Mutex<Reports>,
    to_processor: Mutex<Vec<(u32, f64)>>,
}

#[derive(Default)]
struct Reports {
    list: Vec<Edit>,
    dropped: u32,
}

impl Handler {
    /// Records an edit for Lumen. A value that follows a value of the same
    /// parameter replaces it, so a fast drag between two polls is one entry;
    /// gesture marks are never merged.
    pub fn report(&self, edit: Edit) {
        let Ok(mut reports) = self.reports.lock() else {
            return;
        };
        if let Some(last) = reports.list.last_mut()
            && edit.kind == last.kind
            && matches!(edit.kind, EditKind::Perform | EditKind::Output)
            && edit.id == last.id
        {
            last.value = edit.value;
            return;
        }
        if reports.list.len() >= MAX_EDITS {
            reports.dropped = reports.dropped.saturating_add(1);
            return;
        }
        reports.list.push(edit);
    }

    /// The edits since the last call, and how many were dropped for want of room.
    pub fn take_reports(&self) -> (Vec<Edit>, u32) {
        match self.reports.lock() {
            Ok(mut r) => (std::mem::take(&mut r.list), std::mem::take(&mut r.dropped)),
            Err(_) => (Vec::new(), 0),
        }
    }

    /// Queues a value for the processor's next block (the latest of each
    /// parameter wins).
    pub fn send_to_processor(&self, id: u32, value: f64) {
        let Ok(mut queued) = self.to_processor.lock() else {
            return;
        };
        match queued.iter().position(|(i, _)| *i == id) {
            Some(at) => queued[at].1 = value,
            None if queued.len() < MOST_PENDING => queued.push((id, value)),
            None => {}
        }
    }

    /// Moves the queued values into `into` (which keeps its room, so a
    /// steady stream of blocks does not allocate).
    pub fn drain_to_processor(&self, into: &mut Vec<(u32, f64)>) {
        if let Ok(mut queued) = self.to_processor.lock() {
            into.extend(queued.drain(..));
        }
    }
}

/// The most distinct parameters waiting for a block, and the most a block
/// is given at its start.
pub const MOST_PENDING: usize = 512;

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        self.report(Edit {
            id,
            value: 0.0,
            kind: EditKind::Begin,
        });
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if value.is_finite() {
            self.report(Edit {
                id,
                value,
                kind: EditKind::Perform,
            });
            self.send_to_processor(id, value);
        }
        kResultOk
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        self.report(Edit {
            id,
            value: 0.0,
            kind: EditKind::End,
        });
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
    ///
    /// `now` are values to apply at the start of the piece (edits made in a
    /// plugin's window, or set from Lumen): each joins its parameter's queue
    /// at offset 0, or gets a queue of its own.
    pub fn set_span(
        &self,
        params: &[ParamQueue],
        now: &[(u32, f64)],
        mapped: &[(u32, u32, f64)],
        start: u32,
        len: u32,
    ) {
        let end = start.saturating_add(len);
        let Ok(mut queues) = self.queues.lock() else {
            return;
        };
        let room = params.len() + now.len().min(MOST_PENDING) + mapped.len().min(MOST_PENDING);
        while queues.len() < room {
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
        let mut used = params.len();
        for &(id, value) in now.iter().take(MOST_PENDING) {
            let found = queues[..used]
                .iter()
                .find(|q| q.data.lock().is_ok_and(|d| d.0 == id));
            if let Some(queue) = found {
                if let Ok(mut data) = queue.data.lock() {
                    data.1.insert(0, (0, value));
                }
            } else {
                if let Ok(mut data) = queues[used].data.lock() {
                    data.0 = id;
                    data.1.clear();
                    data.1.push((0, value));
                }
                used += 1;
            }
        }
        // Controller events, in the time order they came in: each joins its
        // parameter's queue after the points before it.
        for &(id, at, value) in mapped.iter().take(MOST_PENDING) {
            if !(start..end).contains(&at) {
                continue;
            }
            let at = (at - start) as int32;
            let found = queues[..used]
                .iter()
                .find(|q| q.data.lock().is_ok_and(|d| d.0 == id));
            if let Some(queue) = found {
                if let Ok(mut data) = queue.data.lock() {
                    let place = data.1.partition_point(|&(t, _)| t <= at);
                    data.1.insert(place, (at, value));
                }
            } else {
                if let Ok(mut data) = queues[used].data.lock() {
                    data.0 = id;
                    data.1.clear();
                    data.1.push((at, value));
                }
                used += 1;
            }
        }
        self.live.store(used as u32, Ordering::Release);
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

/// A queue that takes a plugin's reported changes and keeps none of them:
/// the answer to a plugin that reports more parameters than there is room for.
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

/// The most points one output queue keeps in a block.
const MOST_OUT_POINTS: usize = 256;

/// One parameter's reported points inside a block.
struct OutQueue {
    data: Mutex<(ParamID, Vec<(int32, ParamValue)>)>,
}

impl Class for OutQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for OutQueue {
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

    unsafe fn addPoint(&self, at: int32, value: ParamValue, index: *mut int32) -> tresult {
        let Ok(mut data) = self.data.lock() else {
            return kInvalidArgument;
        };
        if data.1.len() >= MOST_OUT_POINTS {
            // Full: the newest value replaces the last, so the end state is kept.
            if let Some(last) = data.1.last_mut() {
                *last = (at, value);
            }
        } else {
            data.1.push((at, value));
        }
        if !index.is_null() {
            // SAFETY: a valid out pointer from the caller.
            unsafe { *index = data.1.len() as int32 - 1 };
        }
        kResultOk
    }
}

/// The list of output parameter changes a plugin reports its own values in
/// (a meter, a changed program). Queues are made once and reused; after each
/// block [`OutputChanges::take`] reads the last value of each parameter.
pub struct OutputChanges {
    queues: Mutex<Vec<ComWrapper<OutQueue>>>,
    live: AtomicU32,
    sink: ComPtr<IParamValueQueue>,
}

impl OutputChanges {
    pub fn new() -> Self {
        Self {
            queues: Mutex::new(Vec::new()),
            live: AtomicU32::new(0),
            sink: ComWrapper::new(Sink)
                .to_com_ptr::<IParamValueQueue>()
                .expect("interface listed"),
        }
    }

    /// Appends `(id, last value)` of each parameter reported since the last
    /// call to `into`, and clears the list for the next block.
    pub fn take(&self, into: &mut Vec<(u32, f64)>) {
        let live = self.live.swap(0, Ordering::AcqRel) as usize;
        let Ok(queues) = self.queues.lock() else {
            return;
        };
        for queue in queues.iter().take(live) {
            if let Ok(data) = queue.data.lock()
                && let Some(&(_, value)) = data.1.last()
                && value.is_finite()
            {
                into.push((data.0, value));
            }
        }
    }
}

impl Default for OutputChanges {
    fn default() -> Self {
        Self::new()
    }
}

impl Class for OutputChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for OutputChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.live.load(Ordering::Acquire) as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        let Ok(queues) = self.queues.lock() else {
            return std::ptr::null_mut();
        };
        if index < 0 || index as u32 >= self.live.load(Ordering::Acquire) {
            return std::ptr::null_mut();
        }
        queues
            .get(index as usize)
            .and_then(|q| q.to_com_ptr::<IParamValueQueue>())
            .map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: a valid id pointer from the plugin.
        let id = unsafe { *id };
        let fallback = |index: *mut int32| {
            if !index.is_null() {
                // SAFETY: a valid out pointer from the caller.
                unsafe { *index = 0 };
            }
            self.sink.as_ptr()
        };
        let Ok(mut queues) = self.queues.lock() else {
            return fallback(index);
        };
        let live = self.live.load(Ordering::Acquire) as usize;
        let at = match queues
            .iter()
            .take(live)
            .position(|q| q.data.lock().is_ok_and(|d| d.0 == id))
        {
            Some(at) => at,
            None if live < MOST_OUT_QUEUES => {
                if queues.len() <= live {
                    queues.push(ComWrapper::new(OutQueue {
                        data: Mutex::new((id, Vec::new())),
                    }));
                }
                if let Ok(mut data) = queues[live].data.lock() {
                    data.0 = id;
                    data.1.clear();
                }
                self.live.store(live as u32 + 1, Ordering::Release);
                live
            }
            None => return fallback(index),
        };
        if !index.is_null() {
            // SAFETY: a valid out pointer from the caller.
            unsafe { *index = at as int32 };
        }
        queues[at]
            .to_com_ptr::<IParamValueQueue>()
            .map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }
}

/// The most parameters a plugin may report in one block.
const MOST_OUT_QUEUES: usize = 1024;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(id: u32, value: f64, kind: EditKind) -> Edit {
        Edit { id, value, kind }
    }

    #[test]
    fn values_of_one_gesture_are_merged_but_its_marks_are_kept() {
        let h = Handler::default();
        h.report(edit(1, 0.0, EditKind::Begin));
        for v in [0.1, 0.2, 0.3] {
            h.report(edit(1, v, EditKind::Perform));
        }
        h.report(edit(1, 0.0, EditKind::End));
        // A second gesture on the same parameter is not merged into the first.
        h.report(edit(1, 0.0, EditKind::Begin));
        h.report(edit(1, 0.9, EditKind::Perform));
        h.report(edit(2, 0.4, EditKind::Perform));
        h.report(edit(2, 0.5, EditKind::Perform));
        let (list, dropped) = h.take_reports();
        assert_eq!(dropped, 0);
        assert_eq!(
            list,
            vec![
                edit(1, 0.0, EditKind::Begin),
                edit(1, 0.3, EditKind::Perform),
                edit(1, 0.0, EditKind::End),
                edit(1, 0.0, EditKind::Begin),
                edit(1, 0.9, EditKind::Perform),
                edit(2, 0.5, EditKind::Perform),
            ]
        );
        assert!(h.take_reports().0.is_empty());
    }

    #[test]
    fn a_plugin_that_floods_edits_is_bounded_and_the_loss_is_counted() {
        let h = Handler::default();
        for i in 0..(MAX_EDITS as u32 + 10) {
            // Alternating parameters, so nothing merges.
            h.report(edit(i % 2, 0.5, EditKind::Begin));
        }
        let (list, dropped) = h.take_reports();
        assert_eq!(list.len(), MAX_EDITS);
        assert_eq!(dropped, 10);
        assert_eq!(h.take_reports().1, 0, "the count starts again");
    }

    #[test]
    fn the_processor_is_given_the_latest_value_of_each_parameter() {
        let h = Handler::default();
        h.send_to_processor(1, 0.1);
        h.send_to_processor(2, 0.2);
        h.send_to_processor(1, 0.7);
        let mut now = Vec::new();
        h.drain_to_processor(&mut now);
        assert_eq!(now, vec![(1, 0.7), (2, 0.2)]);
        now.clear();
        h.drain_to_processor(&mut now);
        assert!(now.is_empty());
        // Distinct parameters past the bound are not kept.
        for id in 0..(MOST_PENDING as u32 + 5) {
            h.send_to_processor(id, 0.5);
        }
        h.drain_to_processor(&mut now);
        assert_eq!(now.len(), MOST_PENDING);
    }

    #[test]
    fn a_value_that_is_not_a_number_is_never_kept() {
        let h = Handler::default();
        // SAFETY: plain calls on a live object.
        unsafe { h.performEdit(3, f64::NAN) };
        assert!(h.take_reports().0.is_empty());
        let mut now = Vec::new();
        h.drain_to_processor(&mut now);
        assert!(now.is_empty());
    }
}
