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
