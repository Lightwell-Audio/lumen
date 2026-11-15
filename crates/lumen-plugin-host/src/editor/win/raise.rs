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

pub(super) const RAISE_TIMER: usize = 1;
const RAISE_EVERY_MS: u32 = 200;

thread_local! {
    static DISPATCHER: Cell<HWND> = const { Cell::new(null_mut()) };
}

pub(super) fn set_dispatcher(hwnd: HWND) {
    DISPATCHER.set(hwnd);
}

pub(super) fn start_raising() {
    let dispatcher = DISPATCHER.get();
    if !dispatcher.is_null() {
        unsafe { SetTimer(dispatcher, RAISE_TIMER, RAISE_EVERY_MS, None) };
        raise_if_in_front();
    }
}

pub(super) fn stop_raising() {
    let dispatcher = DISPATCHER.get();
    if !dispatcher.is_null() {
        unsafe { KillTimer(dispatcher, RAISE_TIMER) };
    }
}

fn lumen_is_in_front() -> bool {
    let front = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid
    };
    let me = unsafe { GetCurrentProcessId() };
    if front == 0 {
        return false;
    }
    if front == me {
        return true;
    }
    let (mut mine, mut theirs) = (None, None);
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
        (Some(parent), Some(other)) => front == parent || other == parent,
        _ => false,
    }
}

fn is_topmost(hwnd: HWND) -> bool {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    style & WS_EX_TOPMOST != 0
}

pub(super) fn raise_if_in_front() {
    let Some(live) = live() else {
        return;
    };
    let topmost = is_topmost(live.hwnd);
    let after = match (lumen_is_in_front(), topmost) {
        (true, false) => HWND_TOPMOST,
        (false, true) => {
            let front = unsafe { GetForegroundWindow() };
            if front.is_null() || is_topmost(front) {
                HWND_NOTOPMOST
            } else {
                front
            }
        }
        _ => return,
    };
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
