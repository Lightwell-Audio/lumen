//! Plugin editor windows on Windows (M6 slice 4, ADR-0067 "Editors"; the
//! spike's window code, `lumen-editor-spike`, made production).
//!
//! The window is made, pumped and destroyed on the host's main thread, which
//! is also the thread that made the plugin and the one thread allowed to call
//! its view. It is not owned by Lumen's window: a window owned across
//! processes froze Lumen's window moves when a plugin hung. It is owned by a
//! hidden window of its own host instead, so it has no taskbar button and
//! nothing to minimize to. To stay above Lumen without being above other
//! programs it is on top only while Lumen or one of its editors has the
//! foreground (a timer on this thread decides, and changes the order only when
//! that changes, so two editors never fight over which is in front). No
//! shortcut of Lumen reaches a plugin's window, and none of the plugin's
//! reaches Lumen; while Lumen's computer keyboard is on, its note keys pressed
//! alone in the window are taken and told to Lumen (the owner's choice, M6
//! slice 4).
//!
//! Resizing follows the VST3 contract: `checkSizeConstraint` sets the window's
//! tracking limits, `onSize` follows every size, the plugin's `resizeView`
//! moves the window, and `setContentScaleFactor` comes before `attached` and
//! again on every monitor change.
//!
//! This is the only code in the host that has no test on Linux. The spike ran
//! on Windows with the test plugin and one real plugin (MT-VST-0 "Editors");
//! this module is checked by the cross-target compiler and by MT-VST-4.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{NonNull, null, null_mut};
use std::sync::{Arc, Once};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeHWND, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, MONITOR_DEFAULTTONULL, MonitorFromRect};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GA_ROOT,
    GWL_EXSTYLE, GetAncestor, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW,
    IsIconic, IsWindowVisible, KillTimer, LoadCursorW, MINMAXINFO, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SIZE_MINIMIZED, SW_HIDE, SW_SHOWNA, SW_SHOWNORMAL,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_SETFOCUS, WM_SIZE,
    WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

use super::{Editors, Notify};
use crate::protocol::{EditorBounds, EditorStatus, MAX_EDITOR_SIDE, Response};
use crate::server::{Pump, Waker};

const FRAME_CLASS: &str = "LumenPluginEditorFrame";
const OWNER_CLASS: &str = "LumenPluginEditorOwner";
const DISPATCH_CLASS: &str = "LumenPluginHostDispatch";
/// No minimize box: an owned window has no taskbar button to go to, and
/// would shrink to a stub in a corner of the screen.
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN;

/// The timer that raises the editor above Lumen only while Lumen (or an
/// editor) is the program in front, and how often it runs.
const RAISE_TIMER: usize = 1;
const RAISE_EVERY_MS: u32 = 200;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn register(name: &str, proc: Proc, once: &'static Once) {
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

type Proc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn empty_rect() -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    }
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut r = empty_rect();
    // SAFETY: a valid out pointer.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}

fn view_rect(width: i32, height: i32) -> ViewRect {
    ViewRect {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    }
}

fn plugin_size(view: &ComPtr<IPlugView>) -> (i32, i32) {
    let mut r = view_rect(0, 0);
    // SAFETY: a live view and a valid out pointer.
    unsafe { view.getSize(&mut r) };
    (r.right - r.left, r.bottom - r.top)
}

/// The window size that has `client` for its content at `dpi`.
fn outer_for(client: (i32, i32), dpi: u32) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: client.0,
        bottom: client.1,
    };
    // SAFETY: a valid rectangle to adjust.
    unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, dpi) };
    (r.right - r.left, r.bottom - r.top)
}

// ---------------------------------------------------------------- state --

/// The open editor. Window procedures find it here, because they are called
/// by the system with nothing of ours.
struct Open {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    /// Kept alive for as long as the view may call it.
    _frame: ComWrapper<FrameObj>,
    hwnd: HWND,
    attached: bool,
}

thread_local! {
    static EDITOR: RefCell<Option<Open>> = const { RefCell::new(None) };
    /// Where the window last was while it could be asked: what is
    /// remembered when it closes.
    static LAST: Cell<Option<EditorBounds>> = const { Cell::new(None) };
}

/// The pieces a window procedure needs, copied out so no borrow is held while
/// the plugin runs (it calls back into the frame).
#[derive(Clone)]
struct Live {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    hwnd: HWND,
    attached: bool,
}

fn live() -> Option<Live> {
    EDITOR.with(|e| {
        e.borrow().as_ref().map(|o| Live {
            view: o.view.clone(),
            scale: o.scale.clone(),
            hwnd: o.hwnd,
            attached: o.attached,
        })
    })
}

/// Reads where the window is now into [`LAST`]. A minimized window has no
/// place worth keeping.
fn note_bounds(hwnd: HWND) {
    // SAFETY: a window this thread made.
    if unsafe { IsIconic(hwnd) } != 0 {
        return;
    }
    let mut r = empty_rect();
    // SAFETY: a valid out pointer.
    unsafe { GetWindowRect(hwnd, &mut r) };
    let (width, height) = client_size(hwnd);
    // SAFETY: a window this thread made.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    LAST.set(Some(EditorBounds {
        x: r.left,
        y: r.top,
        width: width.max(0) as u32,
        height: height.max(0) as u32,
        dpi,
    }));
}

fn set_client_size(width: i32, height: i32) {
    let Some(live) = live() else {
        return;
    };
    // SAFETY: a window this thread made.
    let dpi = unsafe { GetDpiForWindow(live.hwnd) };
    let (ow, oh) = outer_for((width, height), dpi);
    // SAFETY: as above.
    unsafe {
        SetWindowPos(
            live.hwnd,
            null_mut(),
            0,
            0,
            ow,
            oh,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

// ---------------------------------------------------------- frame object --

struct FrameObj;

impl Class for FrameObj {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for FrameObj {
    /// The plugin asks for a different view size. The window follows, and its
    /// `WM_SIZE` tells the view.
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the plugin passes a valid rectangle.
        let r = unsafe { &*new_size };
        set_client_size(r.right - r.left, r.bottom - r.top);
        kResultTrue
    }
}

// ------------------------------------------------------- the frame window --

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_GETMINMAXINFO => {
            if let Some(live) = live() {
                // SAFETY: `lparam` is the system's `MINMAXINFO`.
                let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                let ask = |w: i32, h: i32| {
                    let mut r = view_rect(w, h);
                    // SAFETY: a live view and a valid rectangle.
                    unsafe { live.view.checkSizeConstraint(&mut r) };
                    (r.right - r.left, r.bottom - r.top)
                };
                // SAFETY: a window this thread made.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                let min = outer_for(ask(1, 1), dpi);
                let max = outer_for(ask(100_000, 100_000), dpi);
                info.ptMinTrackSize = POINT { x: min.0, y: min.1 };
                info.ptMaxTrackSize = POINT { x: max.0, y: max.1 };
                return 0;
            }
        }
        WM_SIZE if wparam as u32 != SIZE_MINIMIZED => {
            if let Some(live) = live().filter(|l| l.attached) {
                let (w, h) = client_size(hwnd);
                let mut r = view_rect(w, h);
                // SAFETY: a live view and a valid rectangle.
                unsafe { live.view.onSize(&mut r) };
            }
            note_bounds(hwnd);
        }
        WM_SETFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                // SAFETY: a live view.
                unsafe { live.view.onFocus(1) };
            }
        }
        WM_KILLFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                // SAFETY: a live view.
                unsafe { live.view.onFocus(0) };
            }
        }
        // Another window has the keyboard now: its key-ups will not come
        // here, so the keys taken and still down are let go of.
        WM_ACTIVATE if (wparam & 0xFFFF) as u32 == WA_INACTIVE => release_keys(),
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            let scale = dpi as f32 / 96.0;
            if let Some(live) = live() {
                if let Some(support) = &live.scale {
                    // SAFETY: a live interface.
                    unsafe { support.setContentScaleFactor(scale) };
                }
                // SAFETY: `lparam` is the system's suggested rectangle.
                let r = unsafe { &*(lparam as *const RECT) };
                // SAFETY: a window this thread made.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                let (w, h) = plugin_size(&live.view);
                set_client_size(w, h);
                note_bounds(hwnd);
            }
            return 0;
        }
        WM_CLOSE => {
            // The user closed it. The host's status says so by the window
            // being gone.
            close_window();
            return 0;
        }
        _ => {}
    }
    // SAFETY: default handling for a message not finished above.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Takes the editor down: the view is told it is gone before its window is.
fn close_window() {
    let Some(editor) = EDITOR.with(|e| e.borrow_mut().take()) else {
        return;
    };
    note_bounds(editor.hwnd);
    stop_raising();
    release_keys();
    // SAFETY: the view and window this thread made.
    unsafe {
        editor.view.removed();
        editor.view.setFrame(null_mut());
        DestroyWindow(editor.hwnd);
    }
}

// ------------------------------------------------------------ the raising --

thread_local! {
    static DISPATCHER: Cell<HWND> = const { Cell::new(null_mut()) };
}

fn start_raising() {
    let dispatcher = DISPATCHER.get();
    if !dispatcher.is_null() {
        // SAFETY: a timer on the dispatcher window of this thread.
        unsafe { SetTimer(dispatcher, RAISE_TIMER, RAISE_EVERY_MS, None) };
        raise_if_in_front();
    }
}

fn stop_raising() {
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
fn raise_if_in_front() {
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

// ------------------------------------------------------------- the owner --

thread_local! {
    static OWNER: Cell<HWND> = const { Cell::new(null_mut()) };
}

unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: default handling; the owner is never shown.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The hidden window that owns the editor: an owned window has no taskbar
/// button. Made once, on this thread, and never shown.
fn owner() -> HWND {
    let made = OWNER.get();
    if !made.is_null() {
        return made;
    }
    static CLASS: Once = Once::new();
    register(OWNER_CLASS, owner_proc, &CLASS);
    let class = wide(OWNER_CLASS);
    // SAFETY: a hidden top-level window on this thread; a tool window, so
    // nothing lists it even if something showed it.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    OWNER.set(hwnd);
    hwnd
}

// -------------------------------------------------------------- the keys --

thread_local! {
    /// The keys Lumen asked for (virtual-key codes).
    static CAPTURE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The keys taken that are down now: their key-ups are taken too.
    static HELD: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static NOTIFY: RefCell<Option<Notify>> = const { RefCell::new(None) };
}

fn tell(code: u8, down: bool) {
    NOTIFY.with(|n| {
        if let Some(notify) = &*n.borrow() {
            notify(Response::Key { code, down });
        }
    });
}

/// Lets go of every key taken and still down, telling Lumen.
fn release_keys() {
    for code in HELD.with(|h| std::mem::take(&mut *h.borrow_mut())) {
        tell(code, false);
    }
}

/// Whether Ctrl, Alt, Shift or a Windows key is down: a chord is the
/// plugin's, never a note.
fn modifier_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .into_iter()
        // SAFETY: a plain query of this thread's keyboard state.
        .any(|vk| unsafe { GetKeyState(i32::from(vk)) } < 0)
}

/// Takes a key message meant for the editor if Lumen asked for its key: the
/// plugin never sees it and Lumen is told of the press or release (not of a
/// repeat). True when taken.
fn take_key(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN && msg.message != WM_KEYUP {
        return false;
    }
    let Ok(code) = u8::try_from(msg.wParam) else {
        return false;
    };
    let Some(live) = live() else {
        return false;
    };
    // The editor's frame or a window inside it; a plugin's own pop-up windows
    // (a dialog, a menu) keep their keys.
    // SAFETY: a plain query about a window of this thread.
    if msg.hwnd.is_null() || unsafe { GetAncestor(msg.hwnd, GA_ROOT) } != live.hwnd {
        return false;
    }
    let held = HELD.with(|h| h.borrow().contains(&code));
    if msg.message == WM_KEYUP {
        if held {
            HELD.with(|h| h.borrow_mut().retain(|&c| c != code));
            tell(code, false);
        }
        return held;
    }
    if !held && (!CAPTURE.with(|c| c.borrow().contains(&code)) || modifier_down()) {
        return false;
    }
    // Bit 30: the key was already down (a repeat).
    let repeat = (msg.lParam >> 30) & 1 != 0;
    if !held && !repeat {
        HELD.with(|h| h.borrow_mut().push(code));
        tell(code, true);
    }
    true
}

// ------------------------------------------------------------- the editor --

/// The editors of this process: at most one window, kept in thread-local
/// state (see [`Open`]).
#[derive(Default)]
pub struct Windows;

impl Editors for Windows {
    fn open(
        &mut self,
        title: &str,
        view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String> {
        if EDITOR.with(|e| e.borrow().is_some()) {
            return Err("an editor is already open".into());
        }
        let view = view.ok_or("the plugin has no editor")?;
        // SAFETY: a live view; a constant platform name.
        if unsafe { view.isPlatformTypeSupported(kPlatformTypeHWND) } != kResultTrue {
            return Err("the editor cannot be shown in a window".into());
        }
        static CLASS: Once = Once::new();
        register(FRAME_CLASS, frame_proc, &CLASS);
        let class = wide(FRAME_CLASS);
        let title = wide(title);
        // Where it was, if that place is still on a screen; else wherever the
        // system puts a new window.
        let (x, y) = wanted
            .filter(on_a_screen)
            .map_or((CW_USEDEFAULT, CW_USEDEFAULT), |b| (b.x, b.y));
        // SAFETY: a top-level window on this (the window) thread, owned by
        // this host's hidden window (never by Lumen's).
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                STYLE,
                x,
                y,
                400,
                300,
                owner(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not make the editor's window".into());
        }
        // SAFETY: a window this thread made.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = dpi as f32 / 96.0;
        let support = view.cast::<IPlugViewContentScaleSupport>();
        if let Some(support) = &support {
            // SAFETY: a live interface, before the view is attached.
            unsafe { support.setContentScaleFactor(scale) };
        }
        let frame = ComWrapper::new(FrameObj);
        let Some(frame_ptr) = frame.to_com_ptr::<IPlugFrame>() else {
            // SAFETY: our window.
            unsafe { DestroyWindow(hwnd) };
            return Err("could not give the editor a frame".into());
        };
        // SAFETY: a live view and frame.
        unsafe { view.setFrame(frame_ptr.as_ptr()) };
        // The size it had, carried to this screen's scale; else its own.
        let client = wanted
            .filter(|b| b.width > 0 && b.height > 0)
            .map(|b| {
                let per = if b.dpi > 0 {
                    dpi as f64 / b.dpi as f64
                } else {
                    1.0
                };
                let side =
                    |v: u32| ((v as f64 * per).round() as i32).clamp(1, MAX_EDITOR_SIDE as i32);
                (side(b.width), side(b.height))
            })
            .unwrap_or_else(|| plugin_size(&view));
        let (ow, oh) = outer_for(client, dpi);
        // SAFETY: our window.
        unsafe {
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                ow,
                oh,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        // The editor is registered before it is attached: the plugin may call
        // the frame, and the window may message, while `attached` runs.
        EDITOR.with(|e| {
            *e.borrow_mut() = Some(Open {
                view: view.clone(),
                scale: support,
                _frame: frame,
                hwnd,
                attached: false,
            });
        });
        // SAFETY: a live view and our window as its parent.
        let result = unsafe { view.attached(hwnd.cast::<c_void>(), kPlatformTypeHWND) };
        if result != kResultOk {
            close_window();
            return Err(format!("the editor could not be attached ({result})"));
        }
        EDITOR.with(|e| {
            if let Some(open) = e.borrow_mut().as_mut() {
                open.attached = true;
            }
        });
        // Size the plugin to what the frame really is (the first `onSize`).
        let (cw, ch) = client_size(hwnd);
        let mut r = view_rect(cw, ch);
        // SAFETY: a live, attached view.
        unsafe { view.onSize(&mut r) };
        // SAFETY: our window. Lumen allowed this process to take the focus
        // when it asked.
        unsafe {
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
        }
        note_bounds(hwnd);
        start_raising();
        Ok(self.status())
    }

    fn close(&mut self) -> EditorStatus {
        close_window();
        self.status()
    }

    fn status(&mut self) -> EditorStatus {
        let open = live().map(|l| l.hwnd);
        if let Some(hwnd) = open {
            note_bounds(hwnd);
        }
        EditorStatus {
            open: open.is_some(),
            // SAFETY: a plain query about a window this thread made.
            visible: open.is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) } != 0),
            bounds: LAST.get(),
        }
    }

    fn set_visible(&mut self, visible: bool) -> EditorStatus {
        if let Some(live) = live() {
            if visible {
                // SAFETY: our window. Shown where it was, without taking the
                // keyboard from Lumen, which has it: the user just chose a
                // track.
                unsafe { ShowWindow(live.hwnd, SW_SHOWNA) };
                raise_if_in_front();
            } else {
                // The keys taken from it are let go of, as when it loses the
                // keyboard.
                release_keys();
                // SAFETY: our window.
                unsafe { ShowWindow(live.hwnd, SW_HIDE) };
            }
        }
        self.status()
    }

    fn capture_keys(&mut self, keys: Vec<u8>) {
        // A key down now that is no longer asked for is let go of; its
        // key-up then goes to the plugin, which never saw it go down.
        let dropped: Vec<u8> = HELD.with(|h| {
            let mut held = h.borrow_mut();
            let gone = held.iter().copied().filter(|c| !keys.contains(c)).collect();
            held.retain(|c| keys.contains(c));
            gone
        });
        for code in dropped {
            tell(code, false);
        }
        CAPTURE.with(|c| *c.borrow_mut() = keys);
    }

    fn set_notify(&mut self, notify: Notify) {
        NOTIFY.with(|n| *n.borrow_mut() = Some(notify));
    }
}

/// Whether some screen shows the top-left of `bounds`: a window remembered on
/// a monitor that is no longer there goes where the system puts it instead.
fn on_a_screen(bounds: &EditorBounds) -> bool {
    let r = RECT {
        left: bounds.x,
        top: bounds.y,
        right: bounds.x + 64,
        bottom: bounds.y + 32,
    };
    // SAFETY: a query with a valid rectangle.
    !unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONULL) }.is_null()
}

// --------------------------------------------------------------- the pump --

type Drain = dyn FnMut() -> bool;

thread_local! {
    /// The function the dispatcher window calls when woken; set for as long
    /// as [`WindowPump::run`] runs.
    static DRAIN: Cell<Option<NonNull<Drain>>> = const { Cell::new(None) };
    /// Whether a drain is running now (the plugin may pump messages inside
    /// one, and the dispatcher must not start another inside it).
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn dispatch_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP => {
            drain_now();
            return 0;
        }
        WM_TIMER if wparam == RAISE_TIMER => {
            raise_if_in_front();
            return 0;
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Hands the queued requests to the host's handler. When it is done for good
/// the message loop ends.
fn drain_now() {
    if BUSY.replace(true) {
        // Inside a request that pumps messages: the loop that is running it
        // takes whatever was queued meanwhile.
        return;
    }
    if let Some(mut drain) = DRAIN.get() {
        // SAFETY: set by `run` for the length of its loop, on this thread,
        // and not borrowed anywhere else (BUSY says so).
        let more = unsafe { drain.as_mut()() };
        if !more {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
        }
    }
    BUSY.set(false);
}

/// The main thread of a host that can show windows: a message loop, woken by
/// a message-only window when the reader queues a request.
pub struct WindowPump {
    dispatcher: HWND,
}

impl WindowPump {
    pub fn new() -> Self {
        // SAFETY: a process-wide setting made once at start, before any
        // window: the editors are per-monitor DPI aware, as Lumen is.
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        static CLASS: Once = Once::new();
        register(DISPATCH_CLASS, dispatch_proc, &CLASS);
        let name = wide(DISPATCH_CLASS);
        // SAFETY: a message-only window for requests, on this thread.
        let dispatcher = unsafe {
            CreateWindowExW(
                0,
                name.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        DISPATCHER.set(dispatcher);
        Self { dispatcher }
    }
}

impl Default for WindowPump {
    fn default() -> Self {
        Self::new()
    }
}

impl Pump for WindowPump {
    fn waker(&self) -> Waker {
        // A window handle is only a number to post to; it is valid from any
        // thread for as long as the window lives, which is as long as the
        // host's loop.
        let target = self.dispatcher as isize;
        Arc::new(move || {
            // SAFETY: posting to a window of the main thread; a failure
            // (the window is gone) means the loop has ended.
            unsafe { PostMessageW(target as HWND, WM_APP, 0, 0) };
        })
    }

    fn run(&mut self, drain: &mut dyn FnMut() -> bool) {
        // SAFETY: the pointer is cleared before this function returns, so it
        // never outlives `drain`; only this thread reads it.
        let erased: NonNull<Drain> = unsafe {
            std::mem::transmute::<NonNull<dyn FnMut() -> bool + '_>, _>(NonNull::from(drain))
        };
        DRAIN.set(Some(erased));
        // Whatever was queued before the loop began.
        drain_now();
        let mut msg = MSG {
            hwnd: null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };
        // SAFETY: the standard message loop of this thread. A key Lumen asked
        // for is taken before the plugin's window sees it (and before it can
        // become a character).
        unsafe {
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if take_key(&msg) {
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        DRAIN.set(None);
    }
}
