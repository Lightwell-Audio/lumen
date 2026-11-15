use std::fs::File;
use std::time::Instant;

#[cfg(unix)]
pub fn take_stdout() -> File {
    use std::os::fd::FromRawFd;
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
    unsafe {
        let protocol = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
        File::from_raw_handle(protocol as _)
    }
}

pub const PARENT_ENV: &str = "LUMEN_HOST_PARENT";

pub fn started_by_this_process(command: &mut std::process::Command) {
    command.env(PARENT_ENV, std::process::id().to_string());
}

#[cfg(unix)]
fn orphaned(parent: i64, started_by: Option<i64>) -> bool {
    parent <= 1 || started_by.is_some_and(|id| id != parent)
}

#[cfg(unix)]
fn started_by() -> Option<i64> {
    std::env::var(PARENT_ENV).ok()?.trim().parse().ok()
}

pub fn die_with_parent() {
    #[cfg(target_os = "linux")]
    {
        let started_by = started_by();
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

    pub(super) fn die_with_parent(started_by: Option<i64>) {
        let parent = unsafe { libc::getppid() };
        if super::orphaned(i64::from(parent), started_by) {
            unsafe { libc::_exit(0) };
        }
        let queue = match arm_parent(parent) {
            Ok(queue) => queue,
            Err(error) => {
                eprintln!("{}parent monitor failed: {error}", super::log_prefix());
                unsafe { libc::_exit(1) };
            }
        };
        let started = thread::Builder::new()
            .name("lumen-parent-watch".into())
            .spawn(move || watch_parent(queue));
        if let Err(error) = started {
            eprintln!(
                "{}could not start parent monitor: {error}",
                super::log_prefix()
            );
            unsafe { libc::_exit(1) };
        }
    }

    fn arm_parent(parent: libc::pid_t) -> io::Result<std::fs::File> {
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        let _queue = unsafe { std::fs::File::from_raw_fd(kq) };
        if unsafe { libc::getppid() } != parent {
            unsafe { libc::_exit(0) };
        }

        let change = libc::kevent {
            ident: parent as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let mut registration_event: libc::kevent = unsafe { std::mem::zeroed() };
        let no_wait = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let registered =
            unsafe { libc::kevent(kq, &change, 1, &mut registration_event, 1, &no_wait) };
        if registered < 0 {
            return Err(io::Error::last_os_error());
        }
        if registered == 1 && registration_event.flags & libc::EV_ERROR != 0 {
            return Err(io::Error::from_raw_os_error(registration_event.data as i32));
        }
        if registered == 1 && registration_event.fflags & libc::NOTE_EXIT != 0 {
            unsafe { libc::_exit(0) };
        }
        if unsafe { libc::getppid() } != parent {
            unsafe { libc::_exit(0) };
        }
        Ok(_queue)
    }

    fn watch_parent(queue: std::fs::File) {
        use std::os::fd::AsRawFd;

        let kq = queue.as_raw_fd();
        loop {
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            let ready =
                unsafe { libc::kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                eprintln!("{}parent monitor failed: {error}", super::log_prefix());
                unsafe { libc::_exit(1) };
            }
            if ready == 1 && event.fflags & libc::NOTE_EXIT != 0 {
                unsafe { libc::_exit(0) };
            }
            if ready == 1 && event.flags & libc::EV_ERROR != 0 {
                let error = io::Error::from_raw_os_error(event.data as i32);
                eprintln!("{}parent monitor failed: {error}", super::log_prefix());
                unsafe { libc::_exit(1) };
            }
        }
    }
}

#[cfg(unix)]
pub fn end_now(code: u32) -> ! {
    unsafe { libc::_exit(code as libc::c_int) }
}

#[cfg(windows)]
pub fn end_now(code: u32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}

pub const NONCE_ENV: &str = "LUMEN_HOST_LOG";

pub fn new_nonce() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};

    let here = 0_u8;
    let mut hasher = RandomState::new().build_hasher();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (std::ptr::from_ref(&here) as usize).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn mark(nonce: &str) -> String {
    format!("lumen-host[{nonce}]: ")
}

pub fn log_prefix() -> &'static str {
    static MARK: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MARK.get_or_init(|| mark(&std::env::var(NONCE_ENV).unwrap_or_default()))
}

pub fn log(line: &str) {
    eprintln!("{}{line}", log_prefix());
}

#[cfg(windows)]
pub fn run_at_full_speed() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, HIGH_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetPriorityClass,
        SetProcessInformation,
    };
    let me = unsafe { GetCurrentProcess() };
    if unsafe { SetPriorityClass(me, HIGH_PRIORITY_CLASS) } == 0 {
        log("could not raise the process priority");
    }
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
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

pub fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    {
        let mut csr: u32 = 0;
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack));
            csr |= 0x8040;
            std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack));
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            let mut fpcr: u64;
            std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack));
            fpcr |= 1 << 24;
            std::arch::asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack));
        }
    }
}

#[cfg(windows)]
fn probe(at: *const u8, out: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut read = 0;
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

#[cfg(windows)]
fn read_u32(at: *const u8) -> Option<u32> {
    let mut word = [0_u8; 4];
    probe(at, &mut word).then(|| u32::from_le_bytes(word))
}

#[cfg(windows)]
fn image_size(header: *const u8) -> usize {
    let Some(pe) = read_u32(header.wrapping_add(0x3c)) else {
        return 0;
    };
    read_u32(header.wrapping_add(pe as usize + 0x18 + 0x38)).unwrap_or(0) as usize
}

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

    let _ = log_prefix();

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
        let (code, at, record) = unsafe {
            let record = &*(*info).ExceptionRecord;
            (
                record.ExceptionCode as u32,
                record.ExceptionAddress as usize,
                record,
            )
        };
        let mark = log_prefix();
        let mut line = Line {
            bytes: [0; 512],
            len: 0,
        };
        let _ = write!(line, "{mark}crash in ");
        let mut module: HMODULE = std::ptr::null_mut();
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
            let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) }
                as usize;
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
        unsafe {
            let mut written = 0;
            WriteFile(
                GetStdHandle(STD_ERROR_HANDLE),
                line.bytes.as_ptr(),
                line.len as u32,
                &mut written,
                std::ptr::null_mut(),
            );
            #[cfg(target_arch = "x86_64")]
            {
                let context = &*(*info).ContextRecord;
                let mut detail = Line {
                    bytes: [0; 512],
                    len: 0,
                };
                let _ = writeln!(
                    detail,
                    "{mark}regs rax={:x} rcx={:x} rdx={:x} rbx={:x} rsi={:x} rdi={:x} r8={:x} r9={:x}",
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
                let _ = write!(detail, "{mark}code ");
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
                detail.len = 0;
                let _ = write!(detail, "{mark}stack");
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
        0
    }

    unsafe { SetUnhandledExceptionFilter(Some(filter)) };
}

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
    unsafe { CoInitializeEx(std::ptr::null(), mode as u32) };
}

#[cfg(windows)]
pub fn quiet_crashes() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };
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
