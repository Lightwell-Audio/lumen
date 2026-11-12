//! Keeping the editor above Lumen while Lumen is the program in front, and
//! letting it sink behind whichever program is.

use std::cell::Cell;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_NOTOPMOST,
    HWND_TOPMOST, KillTimer, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetTimer, SetWindowPos,
    WS_EX_TOPMOST,
};

use super::window::live;

/// The timer that raises the editor above Lumen only while Lumen (or an
/// editor) is the program in front, and how often it runs.
pub(super) const RAISE_TIMER: usize = 1;
const RAISE_EVERY_MS: u32 = 200;

thread_local! {
    static DISPATCHER: Cell<HWND> = const { Cell::new(null_mut()) };
}

/// Remembers the pump's message-only window, which carries the raising timer.
pub(super) fn set_dispatcher(hwnd: HWND) {
    DISPATCHER.set(hwnd);
}

pub(super) fn start_raising() {
    let dispatcher = DISPATCHER.get();
    if !dispatcher.is_null() {
        // SAFETY: a timer on the dispatcher window of this thread.
        unsafe { SetTimer(dispatcher, RAISE_TIMER, RAISE_EVERY_MS, None) };
        raise_if_in_front();
    }
}

pub(super) fn stop_raising() {
    let dispatcher = DISPATCHER.get();
    if !dispatcher.is_null() {
        // SAFETY: as above.
        unsafe { KillTimer(dispatcher, RAISE_TIMER) };
    }
}

/// Whether the program in front is Lumen or one of its hosts: this process,
/// its parent, or a process with the same parent.
fn lumen_is_in_front() -> bool {
    // SAFETY: read-only queries about the foreground window.
    let front = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid
    };
    // SAFETY: a plain query.
    let me = unsafe { GetCurrentProcessId() };
    if front == 0 {
        return false;
    }
    if front == me {
        return true;
    }
    let (mut mine, mut theirs) = (None, None);
    // SAFETY: a snapshot of the process list, walked with correctly sized
    // entries and closed after.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            if entry.th32ProcessID == me {
                mine = Some(entry.th32ParentProcessID);
            }
            if entry.th32ProcessID == front {
                theirs = Some(entry.th32ParentProcessID);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    match (mine, theirs) {
        // Lumen itself, or another of its hosts.
        (Some(parent), Some(other)) => front == parent || other == parent,
        _ => false,
    }
}

/// Whether `hwnd` is on top of every window that is not.
fn is_topmost(hwnd: HWND) -> bool {
    // SAFETY: a plain query about a window.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    style & WS_EX_TOPMOST != 0
}

/// Puts the editor above Lumen while Lumen is in front, and lets it sink
/// behind the program in front when another is. Only a change is made: an
/// editor already where it should be is left alone, so two editors on top
/// keep the order the user clicked them into instead of trading places on
/// every tick.
pub(super) fn raise_if_in_front() {
    let Some(live) = live() else {
        return;
    };
    let topmost = is_topmost(live.hwnd);
    let after = match (lumen_is_in_front(), topmost) {
        (true, false) => HWND_TOPMOST,
        (false, true) => {
            // Behind the program in front (a window placed after one that is
            // not on top stops being on top), not above it.
            // SAFETY: a read-only query.
            let front = unsafe { GetForegroundWindow() };
            if front.is_null() || is_topmost(front) {
                HWND_NOTOPMOST
            } else {
                front
            }
        }
        _ => return,
    };
    // SAFETY: our window.
    unsafe {
        SetWindowPos(
            live.hwnd,
            after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}
