//! How the host process prepares itself. Everything here runs once, at
//! start, before any plugin code.

use std::fs::File;

/// Takes the process's real stdout for the protocol and points the stdout
/// that plugin code writes to at stderr, so a plugin that prints cannot
/// corrupt a frame.
#[cfg(unix)]
pub fn take_stdout() -> File {
    use std::os::fd::FromRawFd;
    // SAFETY: duplicating and redirecting our own standard descriptors at
    // start; the duplicate is owned by the returned file.
    unsafe {
        let protocol = libc::dup(1);
        assert!(protocol >= 0, "could not duplicate stdout");
        libc::dup2(2, 1);
        File::from_raw_fd(protocol)
    }
}

#[cfg(windows)]
pub fn take_stdout() -> File {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    // SAFETY: swapping our own standard handles at start; the original
    // stdout handle is owned by the returned file.
    unsafe {
        let protocol = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
        File::from_raw_handle(protocol as _)
    }
}

/// The environment variable that carries the id of the process that started
/// a host. A host whose parent is gone before it can watch for that must
/// notice, and an orphan is not always adopted by process 1: another
/// ancestor that takes orphans in (WSL's session init, a service manager,
/// a container's init) adopts it under its own id.
pub const PARENT_ENV: &str = "LUMEN_HOST_PARENT";

/// Marks `command` as started by this process, for [`die_with_parent`].
pub fn started_by_this_process(command: &mut std::process::Command) {
    command.env(PARENT_ENV, std::process::id().to_string());
}

/// Whether a process whose parent is now `parent` has lost the one that
/// started it, `started_by` (from [`PARENT_ENV`], when the spawner set it).
#[cfg(unix)]
fn orphaned(parent: i64, started_by: Option<i64>) -> bool {
    parent <= 1 || started_by.is_some_and(|id| id != parent)
}

/// The id the spawner put in [`PARENT_ENV`], if any.
#[cfg(unix)]
fn started_by() -> Option<i64> {
    std::env::var(PARENT_ENV).ok()?.trim().parse().ok()
}

/// A host whose Lumen died must not linger, hung inside a plugin: ask the
/// kernel to end this process with its parent: a parent-death signal on Linux
/// and a process-exit event on macOS. Windows uses a job object on the Lumen
/// side.
pub fn die_with_parent() {
    #[cfg(target_os = "linux")]
    {
        let started_by = started_by();
        // SAFETY: a plain prctl call. If the parent is already gone the
        // signal would never come, so check for that after arming it.
        unsafe {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            if orphaned(i64::from(libc::getppid()), started_by) {
                libc::_exit(0);
            }
        }
    }

    #[cfg(target_os = "macos")]
    macos::die_with_parent(started_by());
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io;
    use std::os::fd::FromRawFd;
    use std::thread;

    /// Darwin has no parent-death signal. A separate thread waits in the
    /// kernel for the original parent to exit, so a plugin hung on the host's
    /// main thread cannot keep this process alive.
    pub(super) fn die_with_parent(started_by: Option<i64>) {
        // SAFETY: reads this process's current parent id.
        let parent = unsafe { libc::getppid() };
        if super::orphaned(i64::from(parent), started_by) {
            // SAFETY: a process with no stable parent cannot be monitored.
            unsafe { libc::_exit(0) };
        }
        // Arm the kernel event synchronously before returning to main, which
        // may load or call plugin code immediately afterward.
        let queue = match arm_parent(parent) {
            Ok(queue) => queue,
            Err(error) => {
                eprintln!("lumen-host: parent monitor failed: {error}");
                // SAFETY: do not continue into plugin code without the guard.
                unsafe { libc::_exit(1) };
            }
        };
        let started = thread::Builder::new()
            .name("lumen-parent-watch".into())
            .spawn(move || watch_parent(queue));
        if let Err(error) = started {
            eprintln!("lumen-host: could not start parent monitor: {error}");
            // SAFETY: do not continue into plugin code without the guard.
            unsafe { libc::_exit(1) };
        }
    }

    fn arm_parent(parent: libc::pid_t) -> io::Result<std::fs::File> {
        // SAFETY: kqueue creates a descriptor that is moved to the watcher.
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: ownership of the new descriptor is transferred to File.
        let _queue = unsafe { std::fs::File::from_raw_fd(kq) };
        // SAFETY: reads this process's current parent id.
        if unsafe { libc::getppid() } != parent {
            // SAFETY: this host was orphaned before its event was registered.
            unsafe { libc::_exit(0) };
        }

        // Register before rechecking getppid: if the parent exited before the
        // registration, the changed parent id catches that race; if it exits
        // after the check, EVFILT_PROC reports NOTE_EXIT.
        let change = libc::kevent {
            ident: parent as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: kq is valid and `change` points to one initialized event.
        let mut registration_event: libc::kevent = unsafe { std::mem::zeroed() };
        let no_wait = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // A zero timeout collects an immediately-ready exit event or an
        // EV_ERROR registration result without waiting for the live parent.
        // SAFETY: live queue, one initialized change and result slot, and a
        // valid zero timeout so registration cannot wait for the parent's exit.
        let registered =
            unsafe { libc::kevent(kq, &change, 1, &mut registration_event, 1, &no_wait) };
        if registered < 0 {
            return Err(io::Error::last_os_error());
        }
        if registered == 1 && registration_event.flags & libc::EV_ERROR != 0 {
            return Err(io::Error::from_raw_os_error(registration_event.data as i32));
        }
        if registered == 1 && registration_event.fflags & libc::NOTE_EXIT != 0 {
            // SAFETY: the kernel reports that the watched parent has exited.
            unsafe { libc::_exit(0) };
        }
        // SAFETY: reads this process's current parent id.
        if unsafe { libc::getppid() } != parent {
            // SAFETY: the parent exited between the first check and the
            // completed registration; no plugin code has run yet.
            unsafe { libc::_exit(0) };
        }
        Ok(_queue)
    }

    fn watch_parent(queue: std::fs::File) {
        use std::os::fd::AsRawFd;

        let kq = queue.as_raw_fd();
        loop {
            // SAFETY: zero is a valid initial state for a kevent result slot.
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: kq is valid and `event` is a writable result slot.
            let ready =
                unsafe { libc::kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                eprintln!("lumen-host: parent monitor failed: {error}");
                // SAFETY: stopping the host avoids leaving an orphan.
                unsafe { libc::_exit(1) };
            }
            if ready == 1 && event.fflags & libc::NOTE_EXIT != 0 {
                // Do not unwind through plugin code or wait for a hung thread.
                // SAFETY: this is the process-exit notification for the parent.
                unsafe { libc::_exit(0) };
            }
            if ready == 1 && event.flags & libc::EV_ERROR != 0 {
                let error = io::Error::from_raw_os_error(event.data as i32);
                eprintln!("lumen-host: parent monitor failed: {error}");
                // SAFETY: stopping the host avoids leaving an orphan.
                unsafe { libc::_exit(1) };
            }
        }
    }
}

/// Ends this process now, with `code`, without unloading modules or running
/// any exit code of theirs. A scan's answer is complete once it is written;
/// what a plugin does while it is torn down (real ones hang there, or leave a
/// failure code) says nothing about the plugin and must not decide the
/// outcome.
#[cfg(unix)]
pub fn end_now(code: u32) -> ! {
    // SAFETY: ends the process at once; nothing is left to run.
    unsafe { libc::_exit(code as libc::c_int) }
}

#[cfg(windows)]
pub fn end_now(code: u32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // SAFETY: terminates our own process; the call does not return.
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}

/// What the host says about itself on stderr, where Lumen collects it: one
/// line, prefixed so it is told from what a plugin prints.
pub const LOG_PREFIX: &str = "lumen-host: ";

pub fn log(line: &str) {
    eprintln!("{LOG_PREFIX}{line}");
}

/// Asks Windows to run this process at full speed: high priority, so a
/// plugin's own worker threads are not kept waiting behind ordinary programs
/// while the audio thread waits on them, and no power throttling, which
/// Windows may apply to a process without a window (lower clocks, coarser
/// timers). The audio thread has its own scheduling class (MMCSS); this is
/// for everything else the plugin runs. Says on stderr what was refused.
#[cfg(windows)]
pub fn run_at_full_speed() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, HIGH_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetPriorityClass,
        SetProcessInformation,
    };
    // SAFETY: the pseudo-handle of this process, which needs no closing.
    let me = unsafe { GetCurrentProcess() };
    // SAFETY: a valid process handle and priority class.
    if unsafe { SetPriorityClass(me, HIGH_PRIORITY_CLASS) } == 0 {
        log("could not raise the process priority");
    }
    // Controlled and off: never throttled, whatever Windows decides.
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    // SAFETY: a valid handle, and a pointer to a state of the size given.
    let set = unsafe {
        SetProcessInformation(
            me,
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if set == 0 {
        log("could not turn off power throttling");
    }
}

/// Makes denormal floats flush to zero on the calling thread (the audio
/// thread). Hosts do this, and plugins with feedback paths (physical models,
/// reverbs, filters) are written to expect it: without it their tails decay
/// through denormals, which cost a hundred times as much, and a block that
/// should take a fraction of a millisecond takes several.
pub fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    {
        let mut csr: u32 = 0;
        // SAFETY: reads and writes this thread's SSE control register; the
        // bits set are flush-to-zero (15) and denormals-are-zero (6).
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack));
            csr |= 0x8040;
            std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack));
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: sets the flush-to-zero bit (24) of this thread's FPCR.
        unsafe {
            let mut fpcr: u64;
            std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack));
            fpcr |= 1 << 24;
            std::arch::asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack));
        }
    }
}

/// Copies `out.len()` bytes at `at` into `out`, or says it could not. A crash
/// filter that faults again loses the report, so every read that may point
/// anywhere (the faulting instruction, the stack) goes through this: the
/// kernel refuses memory that is not mapped instead of raising a second
/// exception.
#[cfg(windows)]
fn probe(at: *const u8, out: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut read = 0;
    // SAFETY: reads this process's own memory (the pseudo-handle needs no
    // closing) into a buffer of the length given; an address that cannot be
    // read is reported, not faulted.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            at.cast(),
            out.as_mut_ptr().cast(),
            out.len(),
            &mut read,
        )
    } != 0;
    ok && read == out.len()
}

/// A little-endian `u32` at `at`, or `None` when it cannot be read.
#[cfg(windows)]
fn read_u32(at: *const u8) -> Option<u32> {
    let mut word = [0_u8; 4];
    probe(at, &mut word).then(|| u32::from_le_bytes(word))
}

/// The size the loaded image says it has, from its own PE header. 0 when the
/// header cannot be read, which matches nothing in the stack scan.
#[cfg(windows)]
fn image_size(header: *const u8) -> usize {
    let Some(pe) = read_u32(header.wrapping_add(0x3c)) else {
        return 0;
    };
    read_u32(header.wrapping_add(pe as usize + 0x18 + 0x38)).unwrap_or(0) as usize
}

/// On Windows, says where a crash happened: the exception, the module the
/// faulting code is in and its offset into it, and for an access violation
/// what was touched. It tells Lumen whether the plugin or the host died, and
/// where. The process then ends with the exception code as its exit code, as
/// it would have without this. Written without allocating, with whatever
/// state the crash left.
#[cfg(windows)]
pub fn report_crashes() {
    use std::fmt::Write as _;
    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

    /// A line built in a fixed buffer.
    struct Line {
        bytes: [u8; 512],
        len: usize,
    }
    impl std::fmt::Write for Line {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            let room = self.bytes.len() - self.len;
            let take = s.len().min(room);
            self.bytes[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
            self.len += take;
            Ok(())
        }
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes valid exception pointers.
        let (code, at, record) = unsafe {
            let record = &*(*info).ExceptionRecord;
            (
                record.ExceptionCode as u32,
                record.ExceptionAddress as usize,
                record,
            )
        };
        let mut line = Line {
            bytes: [0; 512],
            len: 0,
        };
        let _ = write!(line, "{LOG_PREFIX}crash in ");
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: a plain call; the flags ask for the module holding `at`
        // without taking a reference.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                at as *const u16,
                &mut module,
            )
        } != 0;
        if found {
            let mut name = [0_u16; 260];
            // SAFETY: a buffer of the length given.
            let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) }
                as usize;
            // The file's own name: after the last separator.
            let start = name[..n]
                .iter()
                .rposition(|&c| c == u16::from(b'\\') || c == u16::from(b'/'))
                .map_or(0, |i| i + 1);
            for c in char::decode_utf16(name[start..n].iter().copied()) {
                let _ = line.write_char(c.unwrap_or('?'));
            }
            let _ = write!(line, "+{:#x}", at - module as usize);
        } else {
            let _ = write!(line, "an unknown module at {at:#x}");
        }
        // An access violation says whether it read, wrote or ran, and where.
        if code == 0xC000_0005 && record.NumberParameters >= 2 {
            let what = match record.ExceptionInformation[0] {
                0 => "reading",
                1 => "writing",
                _ => "running",
            };
            let _ = write!(
                line,
                ", {what} address {:#x}",
                record.ExceptionInformation[1]
            );
        }
        let _ = line.write_char('\n');
        // SAFETY: the writes are plain calls over buffers that hold `len`
        // bytes; `TerminateProcess` ends this process, which is its own.
        unsafe {
            let mut written = 0;
            WriteFile(
                GetStdHandle(STD_ERROR_HANDLE),
                line.bytes.as_ptr(),
                line.len as u32,
                &mut written,
                std::ptr::null_mut(),
            );
            // What the plugin held and ran at the fault, and where it was
            // called from, for a plugin that cannot be looked at any other
            // way (x86-64 only).
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: the system passes the faulting thread's context
                // with the exception pointers.
                let context = &*(*info).ContextRecord;
                let mut detail = Line {
                    bytes: [0; 512],
                    len: 0,
                };
                let _ = writeln!(
                    detail,
                    "{LOG_PREFIX}regs rax={:x} rcx={:x} rdx={:x} rbx={:x} rsi={:x} rdi={:x} r8={:x} r9={:x}",
                    context.Rax,
                    context.Rcx,
                    context.Rdx,
                    context.Rbx,
                    context.Rsi,
                    context.Rdi,
                    context.R8,
                    context.R9
                );
                WriteFile(
                    GetStdHandle(STD_ERROR_HANDLE),
                    detail.bytes.as_ptr(),
                    detail.len as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
                detail.len = 0;
                let _ = write!(detail, "{LOG_PREFIX}code ");
                // Some of the bytes before the fault and the ones after. The
                // instruction pointer may be in memory that cannot be read at
                // all (a jump to a wild address), so the window is copied
                // through `probe`; a byte that cannot be read prints as `??`.
                let mut window = [0_u8; 16];
                let readable = probe(
                    (context.Rip as usize).wrapping_sub(8) as *const u8,
                    &mut window,
                );
                for i in -8_isize..8 {
                    if readable {
                        let _ = write!(detail, "{:02x}", window[(i + 8) as usize]);
                    } else {
                        let _ = detail.write_str("??");
                    }
                    if i == -1 {
                        let _ = detail.write_char('|');
                    }
                }
                let _ = detail.write_char('\n');
                WriteFile(
                    GetStdHandle(STD_ERROR_HANDLE),
                    detail.bytes.as_ptr(),
                    detail.len as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
                // Words on the stack that point into the module that died:
                // the calls it was inside. The stack is what a plugin may have
                // damaged, so it is copied one page at a time through `probe`:
                // a page that cannot be read is skipped.
                detail.len = 0;
                let _ = write!(detail, "{LOG_PREFIX}stack");
                let (base, size) = if found {
                    (module as usize, image_size(module as *const u8))
                } else {
                    (0, 0)
                };
                let mut seen = 0;
                let mut page = [0_u8; 512];
                let stack = context.Rsp as *const u8;
                for chunk in 0..8 {
                    let at = stack.wrapping_add(chunk * page.len());
                    if !probe(at, &mut page) {
                        continue;
                    }
                    for word in page.as_chunks::<{ size_of::<usize>() }>().0 {
                        let word = usize::from_le_bytes(*word);
                        if word > base && word < base + size && seen < 14 {
                            let _ = write!(detail, " +{:x}", word - base);
                            seen += 1;
                        }
                    }
                }
                let _ = detail.write_char('\n');
                WriteFile(
                    GetStdHandle(STD_ERROR_HANDLE),
                    detail.bytes.as_ptr(),
                    detail.len as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
            }
            TerminateProcess(GetCurrentProcess(), code);
        }
        // Not reached; if it were, the system would end the process.
        0
    }

    // SAFETY: installs the filter for the process; it is our own.
    unsafe { SetUnhandledExceptionFilter(Some(filter)) };
}

/// Gives the calling thread COM, as a plugin on Windows expects of its host
/// (the VST3 hosting notes ask for it): single-threaded for the thread that
/// creates and sets up plugins, multi-threaded for the one that processes
/// audio. Several plugins call into COM or OLE while they are created and
/// die with an access violation when the thread has none. A thread that
/// already has COM in the other mode is left as it is.
#[cfg(windows)]
pub fn init_com(single_threaded: bool) {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx,
    };
    let mode = if single_threaded {
        COINIT_APARTMENTTHREADED
    } else {
        COINIT_MULTITHREADED
    };
    // SAFETY: a plain call on this thread; the result (S_OK, S_FALSE when it
    // already had COM, or a mode clash) needs no action: the thread has COM
    // in some mode either way, or the plugin runs as it did before.
    unsafe { CoInitializeEx(std::ptr::null(), mode as u32) };
}

/// Keeps a crash from parking the process behind an error dialog, which
/// would turn it into a hang.
#[cfg(windows)]
pub fn quiet_crashes() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };
    // SAFETY: a plain call.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
}

#[cfg(all(test, unix))]
mod tests {
    use super::orphaned;

    #[test]
    fn a_host_adopted_by_process_one_is_orphaned() {
        assert!(orphaned(1, None));
        assert!(orphaned(1, Some(4242)));
    }

    #[test]
    fn a_host_adopted_by_another_ancestor_is_orphaned_when_it_knows_its_parent() {
        assert!(orphaned(7, Some(4242)));
        assert!(!orphaned(4242, Some(4242)));
    }

    #[test]
    fn a_host_with_no_parent_id_trusts_only_process_one() {
        assert!(!orphaned(7, None));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{probe, read_u32};

    /// The crash filter's reads may point anywhere. `probe` reports memory it
    /// cannot read instead of raising a second exception.
    #[test]
    fn a_read_the_crash_filter_cannot_make_is_reported() {
        let mut out = [0_u8; 8];
        assert!(!probe(std::ptr::null(), &mut out));
        assert!(!probe(4 as *const u8, &mut out));
    }

    #[test]
    fn a_read_the_crash_filter_can_make_copies_the_bytes() {
        let value = 0x1234_5678_u32;
        let mut out = [0_u8; 4];
        assert!(probe((&raw const value).cast(), &mut out));
        assert_eq!(u32::from_le_bytes(out), value);
        assert_eq!(read_u32((&raw const value).cast()), Some(value));
    }
}
