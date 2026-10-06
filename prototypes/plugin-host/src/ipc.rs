//! Shared-memory transport between Lumen and a plugin-host process, adapted
//! from P-ISO (`prototypes/isolation-ipc`). M6 extracts one shared crate.

use std::io;
use std::process::{Child, Command, Stdio};
use std::ptr::{self, addr_of, addr_of_mut};
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CREATE_WAITABLE_TIMER_MANUAL_RESET, CreateEventW,
    CreateWaitableTimerExW, EVENT_ALL_ACCESS, INFINITE, OpenEventW, SetEvent, SetWaitableTimer,
    TIMER_ALL_ACCESS, WaitForMultipleObjects, WaitForSingleObject,
};

pub const MAX_BLOCK: usize = 1024;
pub const STATE_CAPACITY: usize = 4 << 20;

pub const CMD_PROCESS: u32 = 0;
pub const CMD_QUIT: u32 = 1;
pub const CMD_GET_STATE: u32 = 2;
pub const CMD_SET_STATE: u32 = 3;
/// Fault injection: the host process dereferences null, like a crashing plugin.
pub const CMD_CRASH: u32 = 4;

#[repr(C, align(64))]
pub struct Line<T>(pub T);

#[repr(C)]
pub struct Shared {
    pub request_seq: Line<AtomicU64>,
    pub done_seq: Line<AtomicU64>,
    pub command: Line<AtomicU32>,
    pub frames: Line<AtomicU32>,
    /// Note for this block: 0 none, +pitch note on, -pitch note off.
    pub note: Line<AtomicI32>,
    pub ready: Line<AtomicU32>,
    /// 0 on success, otherwise a VST3 result code or -1.
    pub status: Line<AtomicI32>,
    pub state_len: Line<AtomicU32>,
    pub output: [[f32; MAX_BLOCK * 2]; 2],
    pub state: [u8; STATE_CAPACITY],
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check(handle: HANDLE, what: &str) -> io::Result<HANDLE> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::other(format!(
            "{what}: {}",
            io::Error::last_os_error()
        )))
    } else {
        Ok(handle)
    }
}

pub struct Mapping {
    handle: HANDLE,
    view: *mut Shared,
}

impl Mapping {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: page-file-backed named mapping sized for `Shared`.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                size_of::<Shared>() as u32,
                name.as_ptr(),
            )
        };
        Self::map(check(handle, "CreateFileMappingW")?)
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: valid NUL-terminated name.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr()) };
        Self::map(check(handle, "OpenFileMappingW")?)
    }

    fn map(handle: HANDLE) -> io::Result<Self> {
        // SAFETY: the mapping is at least size_of::<Shared>() bytes.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<Shared>()) };
        if view.Value.is_null() {
            // SAFETY: closing our own handle.
            unsafe { CloseHandle(handle) };
            return Err(io::Error::other("MapViewOfFile failed"));
        }
        Ok(Self {
            handle,
            view: view.Value.cast(),
        })
    }

    /// Atomics only; buffers go through the raw-pointer accessors.
    pub fn shared(&self) -> &Shared {
        // SAFETY: the view stays mapped while `self` lives; zero-filled memory is a valid `Shared`.
        unsafe { &*self.view }
    }

    pub fn output(&self, slot: u64) -> *mut f32 {
        // SAFETY: in-bounds projection on the live view.
        unsafe { addr_of_mut!((*self.view).output[(slot & 1) as usize]).cast() }
    }

    pub fn state(&self) -> *mut u8 {
        // SAFETY: in-bounds projection on the live view.
        unsafe { addr_of_mut!((*self.view).state).cast() }
    }

    pub fn state_const(&self) -> *const u8 {
        // SAFETY: in-bounds projection on the live view.
        unsafe { addr_of!((*self.view).state).cast() }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping and closing what `map` created, once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

pub struct Event(HANDLE);

// SAFETY: kernel event handles are usable from any thread.
unsafe impl Send for Event {}

impl Event {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: named auto-reset event.
        Ok(Self(check(
            unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) },
            "CreateEventW",
        )?))
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: valid name.
        Ok(Self(check(
            unsafe { OpenEventW(EVENT_ALL_ACCESS, 0, name.as_ptr()) },
            "OpenEventW",
        )?))
    }

    pub fn set(&self) {
        // SAFETY: valid handle.
        unsafe { SetEvent(self.0) };
    }

    pub fn wait(&self) {
        // SAFETY: valid handle.
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: our own handle.
        unsafe { CloseHandle(self.0) };
    }
}

/// High-resolution manual-reset timer for pacing and deadlines.
pub struct Timer(HANDLE);

impl Timer {
    pub fn new() -> io::Result<Self> {
        // SAFETY: unnamed timer.
        let handle = unsafe {
            CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_MANUAL_RESET | CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        Ok(Self(check(handle, "CreateWaitableTimerExW")?))
    }

    pub fn arm(&self, after: Duration) {
        let due = -((after.as_nanos() / 100).max(1) as i64);
        // SAFETY: valid handle and due-time pointer.
        unsafe { SetWaitableTimer(self.0, &due, 0, None, ptr::null(), 0) };
    }

    pub fn wait(&self) {
        // SAFETY: valid handle.
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: our own handle.
        unsafe { CloseHandle(self.0) };
    }
}

pub struct Job(HANDLE);

// SAFETY: kernel handle, thread-safe.
unsafe impl Send for Job {}
// SAFETY: only passed to thread-safe kernel calls.
unsafe impl Sync for Job {}

impl Job {
    /// Kills every assigned process when Lumen exits.
    pub fn new() -> io::Result<Self> {
        // SAFETY: unnamed job; plain-data limits struct.
        unsafe {
            let handle = check(
                CreateJobObjectW(ptr::null(), ptr::null()),
                "CreateJobObjectW",
            )?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Ok(Self(handle))
        }
    }

    fn assign(&self, child: &Child) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are valid.
        unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) };
    }
}

/// Lumen's side of one plugin-host process.
pub struct HostProcess {
    child: Child,
    pub mapping: Mapping,
    go: Event,
    pub done: Event,
    seq: u64,
}

// SAFETY: handles and the view pointer are process-wide; one owner at a time.
unsafe impl Send for HostProcess {}

impl HostProcess {
    /// Spawns a host for `module` and waits until the plugin is active. Not real-time safe.
    pub fn spawn(module: &str, generation: u32, job: &Job) -> io::Result<Self> {
        let base = format!("Local\\lumen-phost-{}-{generation}", std::process::id());
        let mapping = Mapping::create(&format!("{base}-shm"))?;
        let go = Event::create(&format!("{base}-go"))?;
        let done = Event::create(&format!("{base}-done"))?;
        let child = Command::new(std::env::current_exe()?)
            .args(["child", &base, module])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        job.assign(&child);
        let host = Self {
            child,
            mapping,
            go,
            done,
            seq: 0,
        };
        let started = Instant::now();
        loop {
            match host.mapping.shared().ready.0.load(Ordering::Acquire) {
                1 => return Ok(host),
                2 => {
                    return Err(io::Error::other(
                        "plugin failed to load in the host process",
                    ));
                }
                _ if started.elapsed() > Duration::from_secs(20) => {
                    return Err(io::Error::other("host did not become ready"));
                }
                _ => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    /// Issues a command and returns its sequence number.
    pub fn submit(&mut self, command: u32, frames: usize, note: i32) -> u64 {
        self.seq += 1;
        let shared = self.mapping.shared();
        shared.command.0.store(command, Ordering::Relaxed);
        shared.frames.0.store(frames as u32, Ordering::Relaxed);
        shared.note.0.store(note, Ordering::Relaxed);
        shared.request_seq.0.store(self.seq, Ordering::Release);
        self.go.set();
        self.seq
    }

    pub fn is_done(&self, seq: u64) -> bool {
        self.mapping.shared().done_seq.0.load(Ordering::Acquire) == seq
    }

    /// Blocking command with a generous timeout (non-real-time paths only).
    pub fn call(&mut self, command: u32, timeout: Duration) -> io::Result<i32> {
        let seq = self.submit(command, 0, 0);
        let started = Instant::now();
        while !self.is_done(seq) {
            if started.elapsed() > timeout || self.exited() {
                return Err(io::Error::other("host did not answer"));
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        Ok(self.mapping.shared().status.0.load(Ordering::Acquire))
    }

    pub fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub fn exit_code(&mut self) -> Option<i32> {
        self.child.try_wait().ok().flatten().and_then(|s| s.code())
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn done_handle(&self) -> HANDLE {
        self.done.0
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        if !self.exited() {
            self.submit(CMD_QUIT, 0, 0);
            std::thread::sleep(Duration::from_millis(50));
            self.kill();
        }
    }
}

/// Waits until the host signals `done` or `deadline` fires. True for done.
pub fn wait_done(host: &HostProcess, deadline: &Timer) -> bool {
    let handles = [host.done_handle(), deadline.0];
    // SAFETY: two valid handles; wait-any.
    unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) == WAIT_OBJECT_0 }
}
