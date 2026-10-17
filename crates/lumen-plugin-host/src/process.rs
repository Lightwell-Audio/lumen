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

/// A host whose Lumen died must not linger, hung inside a plugin: ask the
/// kernel to kill this process with its parent. Where the platform has no
/// such request (Windows uses a job object on the Lumen side), this does
/// nothing.
pub fn die_with_parent() {
    #[cfg(target_os = "linux")]
    // SAFETY: a plain prctl call. If the parent is already gone the signal
    // would never come, so check for that after arming it.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
        if libc::getppid() == 1 {
            libc::_exit(0);
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
        // SAFETY: plain calls; the buffer holds `len` bytes.
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
                let code_at = context.Rip as *const u8;
                // Some of the bytes before the fault and the ones after.
                for i in -8_isize..8 {
                    let _ = write!(detail, "{:02x}", *code_at.offset(i));
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
                // the calls it was inside.
                detail.len = 0;
                let _ = write!(detail, "{LOG_PREFIX}stack");
                let (base, size) = if found {
                    let mut size = 0;
                    let header = module as *const u8;
                    // The PE header's image size, at a fixed offset.
                    let pe = *(header.add(0x3c) as *const u32) as usize;
                    size = size.max(*(header.add(pe + 0x18 + 0x38) as *const u32) as usize);
                    (module as usize, size)
                } else {
                    (0, 0)
                };
                let sp = context.Rsp as *const usize;
                let mut seen = 0;
                for i in 0..512 {
                    let word = *sp.add(i);
                    if word > base && word < base + size && seen < 14 {
                        let _ = write!(detail, " +{:x}", word - base);
                        seen += 1;
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
