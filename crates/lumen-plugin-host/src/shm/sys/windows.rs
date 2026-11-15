use std::cell::RefCell;
use std::io;
use std::process::Child;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE, SetLastError, WAIT_OBJECT_0,
};
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

fn created(handle: HANDLE, what: &str) -> io::Result<HANDLE> {
    let handle = checked(handle, what)?;
    if io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        unsafe { CloseHandle(handle) };
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{what}: the name is already in use"),
        ));
    }
    Ok(handle)
}

pub struct Mapping {
    handle: HANDLE,
    ptr: *mut u8,
}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn create(name: &str, len: usize) -> io::Result<Self> {
        let name = wide(name);
        unsafe { SetLastError(0) };
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
        Self::map(created(handle, "CreateFileMappingW")?, len)
    }

    pub fn open(name: &str, len: usize) -> io::Result<Self> {
        let name = wide(name);
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr()) };
        Self::map(checked(handle, "OpenFileMappingW")?, len)
    }

    fn map(handle: HANDLE, len: usize) -> io::Result<Self> {
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len) };
        if view.Value.is_null() {
            let error = io::Error::last_os_error();
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
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

pub struct Signal {
    event: HANDLE,
    timer: HANDLE,
}

unsafe impl Send for Signal {}
unsafe impl Sync for Signal {}

impl Signal {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        unsafe { SetLastError(0) };
        let event = unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) };
        Ok(Self::with_timer(created(event, "CreateEventW")?))
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        let event = unsafe { OpenEventW(EVENT_ALL_ACCESS, 0, name.as_ptr()) };
        Ok(Self::with_timer(checked(event, "OpenEventW")?))
    }

    fn with_timer(event: HANDLE) -> Self {
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
        unsafe { SetEvent(self.event) };
    }

    pub fn wait(&self, timeout: Duration) -> bool {
        if self.timer.is_null() {
            let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            return unsafe { WaitForSingleObject(self.event, ms) } == WAIT_OBJECT_0;
        }
        let due = -((timeout.as_nanos() / 100).clamp(1, i64::MAX as u128) as i64);
        let armed = unsafe { SetWaitableTimer(self.timer, &due, 0, None, ptr::null(), 0) };
        if armed == 0 {
            let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            return unsafe { WaitForSingleObject(self.event, ms) } == WAIT_OBJECT_0;
        }
        let handles = [self.event, self.timer];
        unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, u32::MAX) == WAIT_OBJECT_0 }
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.event);
            if !self.timer.is_null() {
                CloseHandle(self.timer);
            }
        }
    }
}

pub fn remove_names(_names: &Names) {}

struct AudioTaskRegistration(HANDLE);

impl Drop for AudioTaskRegistration {
    fn drop(&mut self) {
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
        let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
        if handle.is_null() {
            return false;
        }
        *registration = Some(AudioTaskRegistration(handle));
        true
    })
}

pub struct Job(HANDLE);

unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Self> {
        let handle = checked(
            unsafe { CreateJobObjectW(ptr::null(), ptr::null()) },
            "CreateJobObjectW",
        )?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self(handle))
    }

    pub fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn a_mapping_name_that_is_already_taken_is_refused() {
        let names = Names::new(&format!("dup-mapping-{}", std::process::id()));
        let first = Mapping::create(&names.region, 4096).expect("the first mapping");
        let again = Mapping::create(&names.region, 4096)
            .err()
            .expect("the second is refused");
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists, "{again}");
        drop(first);
        Mapping::create(&names.region, 4096).expect("the name is free again");
    }

    #[test]
    fn an_event_name_that_is_already_taken_is_refused() {
        let names = Names::new(&format!("dup-event-{}", std::process::id()));
        let first = Signal::create(&names.go).expect("the first event");
        let again = Signal::create(&names.go)
            .err()
            .expect("the second is refused");
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists, "{again}");
        drop(first);
        Signal::create(&names.go).expect("the name is free again");
    }

    #[test]
    fn a_wait_ends_at_its_timeout_even_when_the_timer_cannot_be_armed() {
        let event = Signal::create("lumen-test-timer-arm-event").expect("an event");
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
