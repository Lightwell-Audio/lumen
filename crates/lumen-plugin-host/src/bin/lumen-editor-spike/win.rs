//! Small Win32 helpers shared by both roles.

use std::ptr::{null, null_mut};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetClientRect, GetWindowRect, IDC_ARROW, LoadCursorW,
    RegisterClassExW, WNDCLASSEXW,
};

pub type Proc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Both roles are per-monitor DPI aware, as Lumen will be.
pub fn per_monitor_dpi() {
    // SAFETY: a process-wide setting made once at start.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// Registers a window class once per process.
pub fn register(name: &str, proc: Proc, once: &'static Once) {
    once.call_once(|| {
        let name = wide(name);
        // SAFETY: a class naming a procedure in this program; the system
        // copies the strings.
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(null()),
                hIcon: null_mut(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_WINDOW + 1) as usize as _,
                lpszMenuName: null(),
                lpszClassName: name.as_ptr(),
                hIconSm: null_mut(),
            };
            RegisterClassExW(&class);
        }
    });
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 64];
    // SAFETY: a buffer of the length given.
    let n = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

/// The first descendant of `parent` whose class is `class` (case-blind).
pub fn find_descendant(parent: HWND, class: &str) -> Option<HWND> {
    struct Search<'a> {
        class: &'a str,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the `Search` below, alive for the enumeration.
        let search = unsafe { &mut *(lparam as *mut Search<'_>) };
        if class_name(hwnd).eq_ignore_ascii_case(search.class) {
            search.found = hwnd;
            return 0;
        }
        1
    }
    let mut search = Search {
        class,
        found: null_mut(),
    };
    // SAFETY: enumerates windows of the calling process's thread, with a
    // pointer that outlives the call.
    unsafe { EnumChildWindows(parent, Some(visit), &raw mut search as LPARAM) };
    (!search.found.is_null()).then_some(search.found)
}

pub fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a valid out pointer.
    unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a valid out pointer.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}
