//! Windows file mappings, named events, the audio scheduling class and the
//! job object that ends hosts with Lumen.

use std::cell::RefCell;
use std::io;
use std::process::Child;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CREATE_WAITABLE_TIMER_MANUAL_RESET, CreateEventW,
    CreateWaitableTimerExW, EVENT_ALL_ACCESS, OpenEventW, SetEvent, SetWaitableTimer,
    TIMER_ALL_ACCESS, WaitForMultipleObjects, WaitForSingleObject,
};

use super::Names;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn checked(handle: HANDLE, what: &str) -> io::Result<HANDLE> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::other(format!(
            "{what} failed: {}",
            io::Error::last_os_error()
        )))
    } else {
        Ok(handle)
    }
}

/// A mapped block of shared memory, zero-filled when created.
pub struct Mapping {
    handle: HANDLE,
    ptr: *mut u8,
}

// SAFETY: a mapping is plain memory addressed through a pointer that is
// valid for the object's life; sharing it is the point.
unsafe impl Send for Mapping {}
// SAFETY: as above; all access goes through atomics or raw copies.
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn create(name: &str, len: usize) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: a page-file-backed named mapping; the pointers are valid.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                ((len as u64) >> 32) as u32,
                len as u32,
                name.as_ptr(),
            )
        };
        Self::map(checked(handle, "CreateFileMappingW")?, len)
    }

    pub fn open(name: &str, len: usize) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: a valid NUL-terminated name.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr()) };
        Self::map(checked(handle, "OpenFileMappingW")?, len)
    }

    fn map(handle: HANDLE, len: usize) -> io::Result<Self> {
        // SAFETY: `handle` is a mapping of at least `len` bytes (a smaller
        // one makes the call fail).
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len) };
        if view.Value.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: closing the handle this function owns.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self {
            handle,
            ptr: view.Value.cast(),
        })
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping and closing what `map` created, once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

/// A wake-up: an auto-reset event with a high-resolution timer for waits
/// that must end on time. The millisecond timeout of a plain wait is rounded
/// to the system tick, which is too coarse for a deadline of a few
/// milliseconds.
pub struct Signal {
    event: HANDLE,
    timer: HANDLE,
}

// SAFETY: kernel handles may be used from any thread.
unsafe impl Send for Signal {}
// SAFETY: setting and waiting on kernel objects is thread-safe.
unsafe impl Sync for Signal {}

impl Signal {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: an auto-reset, initially unsignalled, named event.
        let event = unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) };
        Ok(Self::with_timer(checked(event, "CreateEventW")?))
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: a valid NUL-terminated name.
        let event = unsafe { OpenEventW(EVENT_ALL_ACCESS, 0, name.as_ptr()) };
        Ok(Self::with_timer(checked(event, "OpenEventW")?))
    }

    fn with_timer(event: HANDLE) -> Self {
        // SAFETY: an unnamed timer with default security. If the system has
        // no high-resolution timers, waits fall back to millisecond timeouts.
        let timer = unsafe {
            CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_MANUAL_RESET | CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        Self { event, timer }
    }

    pub fn set(&self) {
        // SAFETY: a live event handle.
        unsafe { SetEvent(self.event) };
    }

    /// Waits for the event for up to `timeout`; true if it was set.
    pub fn wait(&self, timeout: Duration) -> bool {
        if self.timer.is_null() {
            let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: a live event handle.
            return unsafe { WaitForSingleObject(self.event, ms) } == WAIT_OBJECT_0;
        }
        // Negative means relative, in 100 ns units.
        let due = -((timeout.as_nanos() / 100).clamp(1, i64::MAX as u128) as i64);
        // SAFETY: a live timer and a valid due-time pointer; no completion routine.
        let armed = unsafe { SetWaitableTimer(self.timer, &due, 0, None, ptr::null(), 0) };
        if armed == 0 {
            // Without a running timer the wait below would have no end.
            let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: a live event handle.
            return unsafe { WaitForSingleObject(self.event, ms) } == WAIT_OBJECT_0;
        }
        let handles = [self.event, self.timer];
        // SAFETY: two live handles; wait for either. The event is first, so
        // it wins when both are ready.
        unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, u32::MAX) == WAIT_OBJECT_0 }
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        // SAFETY: closing the handles this object owns.
        unsafe {
            CloseHandle(self.event);
            if !self.timer.is_null() {
                CloseHandle(self.timer);
            }
        }
    }
}

/// Windows removes a name with its last handle.
pub fn remove_names(_names: &Names) {}

/// Joins the calling thread to the Pro Audio class of the multimedia
/// scheduler, as audio threads should. The registration lasts for the
/// thread's life.
struct AudioTaskRegistration(HANDLE);

impl Drop for AudioTaskRegistration {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by AvSetMmThreadCharacteristicsW
        // on this same thread and is reverted when the thread-local drops.
        unsafe {
            AvRevertMmThreadCharacteristics(self.0);
        }
    }
}

thread_local! {
    static AUDIO_TASK: RefCell<Option<AudioTaskRegistration>> = const { RefCell::new(None) };
}

pub fn audio_thread_priority() -> bool {
    AUDIO_TASK.with(|registration| {
        let mut registration = registration.borrow_mut();
        if registration.is_some() {
            return true;
        }

        let task = wide("Pro Audio");
        let mut index = 0_u32;
        // SAFETY: a valid task name and out pointer.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
        if handle.is_null() {
            return false;
        }
        *registration = Some(AudioTaskRegistration(handle));
        true
    })
}

/// A job that kills every process assigned to it when Lumen's handle closes,
/// which includes Lumen being killed, so a host hung in a plugin cannot
/// outlive the session.
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for Job {}
// SAFETY: `assign` only passes the handle to thread-safe kernel calls.
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: an unnamed job with default security.
        let handle = checked(
            unsafe { CreateJobObjectW(ptr::null(), ptr::null()) },
            "CreateJobObjectW",
        )?;
        // SAFETY: a plain-data struct; zero is a valid starting state.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the structure and size match this information class.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: closing the handle this function owns.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self(handle))
    }

    pub fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are live; std's child handle has full access.
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing the handle this object owns; the kernel ends what is left.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    /// A signal whose timer cannot be armed (the handle is an event, which
    /// `SetWaitableTimer` refuses) still gives up at the timeout.
    #[test]
    fn a_wait_ends_at_its_timeout_even_when_the_timer_cannot_be_armed() {
        let event = Signal::create("lumen-test-timer-arm-event").expect("an event");
        // SAFETY: an unnamed auto-reset event, standing in for a bad timer.
        let not_a_timer = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        assert!(!not_a_timer.is_null());
        let signal = Signal {
            event: event.event,
            timer: not_a_timer,
        };
        std::mem::forget(event);
        let (done, waited) = mpsc::channel();
        std::thread::spawn(move || {
            let began = Instant::now();
            let set = signal.wait(Duration::from_millis(80));
            let _ = done.send((set, began.elapsed()));
        });
        let (set, took) = waited
            .recv_timeout(Duration::from_secs(5))
            .expect("the wait ended");
        assert!(!set);
        assert!(took >= Duration::from_millis(60), "{took:?}");
    }
}
