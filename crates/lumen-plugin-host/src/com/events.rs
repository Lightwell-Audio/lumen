//! Event lists and parameter changes, in and out, for one block.

use super::*;

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
