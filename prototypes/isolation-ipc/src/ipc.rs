//! Transport primitives: shared memory, events, timers, host processes.

use std::io;
use std::process::{Child, Command, Stdio};
use std::ptr::{self, addr_of, addr_of_mut};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
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
    AvSetMmThreadCharacteristicsW, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
    CREATE_WAITABLE_TIMER_MANUAL_RESET, CreateEventW, CreateWaitableTimerExW, EVENT_ALL_ACCESS,
    INFINITE, OpenEventW, SetEvent, SetWaitableTimer, TIMER_ALL_ACCESS, WaitForMultipleObjects,
    WaitForSingleObject,
};

pub const MAX_FRAMES: usize = 4096;
pub const CHANNELS: usize = 2;
const SAMPLES: usize = MAX_FRAMES * CHANNELS;

// Commands; 0 (the zero-filled initial state) means "process normally".
pub const CMD_QUIT: u32 = 1;
pub const CMD_CRASH: u32 = 2;
pub const CMD_HANG: u32 = 3;

/// One cache line per control word, so the two processes do not false-share.
#[repr(C, align(64))]
pub struct Line<T>(pub T);

/// Memory shared between Lumen and one plugin host process.
///
/// Audio buffers are double-buffered by request parity, so a late host never
/// reads the slot the parent is filling for the next block.
#[repr(C)]
pub struct Shared {
    pub request_seq: Line<AtomicU64>,
    pub done_seq: Line<AtomicU64>,
    pub command: Line<AtomicU32>,
    pub frames: Line<AtomicU32>,
    pub ready: Line<AtomicU32>,
    pub input: [[f32; SAMPLES]; 2],
    pub output: [[f32; SAMPLES]; 2],
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check_handle(handle: HANDLE, what: &str) -> io::Result<HANDLE> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::other(format!(
            "{what} failed: {}",
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
        let size = size_of::<Shared>() as u32;
        // SAFETY: page-file-backed named mapping; all pointer args valid or null.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                size,
                name.as_ptr(),
            )
        };
        Self::map(check_handle(handle, "CreateFileMappingW")?)
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: valid NUL-terminated name.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr()) };
        Self::map(check_handle(handle, "OpenFileMappingW")?)
    }

    fn map(handle: HANDLE) -> io::Result<Self> {
        // SAFETY: `handle` is a valid mapping at least size_of::<Shared>() bytes long.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<Shared>()) };
        if view.Value.is_null() {
            // SAFETY: closing the handle we own.
            unsafe { CloseHandle(handle) };
            return Err(io::Error::other("MapViewOfFile failed"));
        }
        // Fresh page-file mappings are zero-filled, which is a valid `Shared`.
        Ok(Self {
            handle,
            view: view.Value.cast(),
        })
    }

    pub fn shared(&self) -> &Shared {
        // SAFETY: the view stays mapped for the life of `self`. Only atomics are
        // accessed through this reference; buffers go through raw pointers.
        unsafe { &*self.view }
    }

    pub fn input_slot(&self, slot: usize) -> *mut f32 {
        // SAFETY: in-bounds field projection on the live view.
        unsafe { addr_of_mut!((*self.view).input[slot & 1]).cast() }
    }

    pub fn output_slot(&self, slot: usize) -> *const f32 {
        // SAFETY: in-bounds field projection on the live view.
        unsafe { addr_of!((*self.view).output[slot & 1]).cast() }
    }

    pub fn output_slot_mut(&self, slot: usize) -> *mut f32 {
        // SAFETY: in-bounds field projection on the live view.
        unsafe { addr_of_mut!((*self.view).output[slot & 1]).cast() }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping and closing what `map` created, exactly once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

/// Named auto-reset event.
pub struct Event(HANDLE);

impl Event {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: auto-reset, initially unsignalled, named event.
        let handle = unsafe { CreateEventW(ptr::null(), 0, 0, name.as_ptr()) };
        Ok(Self(check_handle(handle, "CreateEventW")?))
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // SAFETY: valid NUL-terminated name.
        let handle = unsafe { OpenEventW(EVENT_ALL_ACCESS, 0, name.as_ptr()) };
        Ok(Self(check_handle(handle, "OpenEventW")?))
    }

    pub fn set(&self) {
        // SAFETY: valid event handle.
        unsafe { SetEvent(self.0) };
    }

    pub fn wait_forever(&self) {
        // SAFETY: valid event handle.
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own.
        unsafe { CloseHandle(self.0) };
    }
}

/// High-resolution manual-reset waitable timer: sub-millisecond deadlines
/// without raising the system timer resolution.
pub struct Timer(HANDLE);

impl Timer {
    pub fn new() -> io::Result<Self> {
        // SAFETY: unnamed timer with default security.
        let handle = unsafe {
            CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_MANUAL_RESET | CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        Ok(Self(check_handle(handle, "CreateWaitableTimerExW")?))
    }

    /// Arms the timer `after` from now (also clears its signalled state).
    pub fn arm(&self, after: Duration) {
        // Negative = relative, in 100 ns units.
        let due = -((after.as_nanos() / 100).max(1) as i64);
        // SAFETY: valid timer handle and due-time pointer; no completion routine.
        unsafe { SetWaitableTimer(self.0, &due, 0, None, ptr::null(), 0) };
    }

    pub fn wait(&self) {
        // SAFETY: valid timer handle.
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own.
        unsafe { CloseHandle(self.0) };
    }
}

/// Waits until `first` or `second` is signalled. Returns true for `first`.
pub fn wait_either(first: HANDLE, second: HANDLE) -> bool {
    let handles = [first, second];
    // SAFETY: two valid handles; wait-any.
    unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) == WAIT_OBJECT_0 }
}

/// Joins the current thread to the MMCSS "Pro Audio" class, as audio threads should.
pub fn pro_audio_priority() -> bool {
    let task = wide("Pro Audio");
    let mut index = 0_u32;
    // SAFETY: valid task name and out pointer. The handle is intentionally kept.
    !unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) }.is_null()
}

/// Job object that kills every assigned process when Lumen exits, so plugin
/// hosts can never outlive it.
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for Job {}
// SAFETY: `assign` only passes the handle to thread-safe kernel calls.
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: unnamed job with default security.
        let handle = check_handle(
            unsafe { CreateJobObjectW(ptr::null(), ptr::null()) },
            "CreateJobObjectW",
        )?;
        // SAFETY: plain-data struct; zero is a valid starting state.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: correct struct and size for this information class.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    pub fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are valid; std's child handle has full access.
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own; this kills any remaining hosts.
        unsafe { CloseHandle(self.0) };
    }
}

/// Parent-side handle to one plugin host process.
pub struct Host {
    pub id: usize,
    pub generation: u32,
    child: Child,
    mapping: Mapping,
    go: Event,
    done: Event,
}

// SAFETY: the raw handles and view pointer are process-wide and usable from
// any thread; a `Host` is only ever used by one thread at a time.
unsafe impl Send for Host {}

impl Host {
    /// Spawns a host and waits until it reports ready. Not real-time safe.
    pub fn spawn(id: usize, generation: u32, spin: bool, job: &Job) -> io::Result<Self> {
        let base = format!("Local\\lumen-iso-{}-{id}-{generation}", std::process::id());
        let mapping = Mapping::create(&format!("{base}-shm"))?;
        let go = Event::create(&format!("{base}-go"))?;
        let done = Event::create(&format!("{base}-done"))?;
        let child = Command::new(std::env::current_exe()?)
            .args(["child", &base, if spin { "spin" } else { "event" }])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        job.assign(&child)?;
        let host = Self {
            id,
            generation,
            child,
            mapping,
            go,
            done,
        };
        let started = Instant::now();
        while host.mapping.shared().ready.0.load(Ordering::Acquire) == 0 {
            if started.elapsed() > Duration::from_secs(10) {
                return Err(io::Error::other("host did not become ready"));
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        Ok(host)
    }

    /// Hands one block to the host. Real-time safe.
    pub fn submit(&self, seq: u64, input: &[f32], frames: usize, wake: bool) {
        let shared = self.mapping.shared();
        // SAFETY: the slot holds SAMPLES floats; `input` is at most that long.
        unsafe {
            ptr::copy_nonoverlapping(
                input.as_ptr(),
                self.mapping.input_slot(seq as usize),
                input.len().min(SAMPLES),
            )
        };
        shared.frames.0.store(frames as u32, Ordering::Relaxed);
        shared.request_seq.0.store(seq, Ordering::Release);
        if wake {
            self.go.set();
        }
    }

    pub fn is_done(&self, seq: u64) -> bool {
        self.mapping.shared().done_seq.0.load(Ordering::Acquire) == seq
    }

    /// Copies the host's output for `seq`. Only call after `is_done(seq)`.
    pub fn read_output(&self, seq: u64, out: &mut [f32]) {
        // SAFETY: the slot holds SAMPLES floats; `out` is at most that long.
        unsafe {
            ptr::copy_nonoverlapping(
                self.mapping.output_slot(seq as usize),
                out.as_mut_ptr(),
                out.len().min(SAMPLES),
            )
        };
    }

    pub fn done_handle(&self) -> HANDLE {
        self.done.handle()
    }

    /// Fault injection: the host acts on this at its next block.
    pub fn inject(&self, command: u32) {
        self.mapping
            .shared()
            .command
            .0
            .store(command, Ordering::Release);
    }

    /// Exit code if the process has ended. Allocation-free.
    pub fn exit_code(&mut self) -> Option<i32> {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.code().unwrap_or(-1))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if self.exit_code().is_none() {
            self.inject(CMD_QUIT);
            self.mapping
                .shared()
                .request_seq
                .0
                .fetch_add(1, Ordering::Release);
            self.go.set();
            std::thread::sleep(Duration::from_millis(20));
            if self.exit_code().is_none() {
                self.kill();
            }
        }
    }
}

/// The plugin host process: waits for blocks, applies 0.5 gain, reports done.
pub fn child_main(base: &str, spin: bool) -> ! {
    // A crashing plugin must terminate the host at once, never park it
    // behind a Windows Error Reporting dialog.
    // SAFETY: process-wide error-mode flags.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
    let mapping = Mapping::open(&format!("{base}-shm")).expect("open mapping");
    let go = Event::open(&format!("{base}-go")).expect("open go event");
    let done = Event::open(&format!("{base}-done")).expect("open done event");
    pro_audio_priority();
    let shared = mapping.shared();
    shared.ready.0.store(1, Ordering::Release);

    let mut seen = 0_u64;
    loop {
        let seq = loop {
            let seq = shared.request_seq.0.load(Ordering::Acquire);
            if seq != seen {
                break seq;
            }
            if spin {
                std::hint::spin_loop();
            } else {
                go.wait_forever();
            }
        };
        seen = seq;
        match shared.command.0.load(Ordering::Acquire) {
            CMD_QUIT => std::process::exit(0),
            // SAFETY: deliberately not safe. This simulates a plugin crashing
            // with an access violation, as real native code does.
            CMD_CRASH => unsafe { ptr::null_mut::<u32>().write_volatile(0xDEAD) },
            CMD_HANG => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            _ => {}
        }
        let samples = (shared.frames.0.load(Ordering::Relaxed) as usize * CHANNELS).min(SAMPLES);
        let input = mapping.input_slot(seq as usize);
        let output = mapping.output_slot_mut(seq as usize);
        for i in 0..samples {
            // SAFETY: `i < samples <= SAMPLES` for both slots.
            unsafe { output.add(i).write(input.add(i).read() * 0.5) };
        }
        shared.done_seq.0.store(seq, Ordering::Release);
        if !spin {
            done.set();
        }
    }
}
