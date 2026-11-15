use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;

use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeHWND, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, MONITOR_DEFAULTTONULL, MonitorFromRect};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowRect,
    IDC_ARROW, IsIconic, IsWindowVisible, LoadCursorW, MINMAXINFO, RegisterClassExW,
    SIZE_MINIMIZED, SW_HIDE, SW_SHOWNA, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_KILLFOCUS, WM_SETFOCUS, WM_SIZE, WNDCLASSEXW, WS_CAPTION,
    WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

use super::keys::release_keys;
use super::raise::{raise_if_in_front, start_raising, stop_raising};
use crate::protocol::{EditorBounds, EditorStatus, MAX_EDITOR_SIDE};

pub(super) const FRAME_CLASS: &str = "LumenPluginEditorFrame";
const OWNER_CLASS: &str = "LumenPluginEditorOwner";
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN;

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub(super) type Proc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub(super) fn register(name: &str, proc: Proc, once: &'static Once) {
    once.call_once(|| {
        let name = wide(name);
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
    unsafe { view.getSize(&mut r) };
    (r.right - r.left, r.bottom - r.top)
}

fn outer_for(client: (i32, i32), dpi: u32) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: client.0,
        bottom: client.1,
    };
    unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, dpi) };
    (r.right - r.left, r.bottom - r.top)
}

struct Open {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    _frame: ComWrapper<FrameObj>,
    hwnd: HWND,
    attached: bool,
}

thread_local! {
    static EDITOR: RefCell<Option<Open>> = const { RefCell::new(None) };
    static LAST: Cell<Option<EditorBounds>> = const { Cell::new(None) };
}

pub(super) struct Live {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    pub(super) hwnd: HWND,
    attached: bool,
}

pub(super) fn live() -> Option<Live> {
    EDITOR.with(|e| {
        e.borrow().as_ref().map(|o| Live {
            view: o.view.clone(),
            scale: o.scale.clone(),
            hwnd: o.hwnd,
            attached: o.attached,
        })
    })
}

fn note_bounds(hwnd: HWND) {
    if unsafe { IsIconic(hwnd) } != 0 {
        return;
    }
    let mut r = empty_rect();
    unsafe { GetWindowRect(hwnd, &mut r) };
    let (width, height) = client_size(hwnd);
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
    let dpi = unsafe { GetDpiForWindow(live.hwnd) };
    let (ow, oh) = outer_for((width, height), dpi);
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

struct FrameObj;

impl Class for FrameObj {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for FrameObj {
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let r = unsafe { &*new_size };
        set_client_size(r.right - r.left, r.bottom - r.top);
        kResultTrue
    }
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_GETMINMAXINFO => {
            if let Some(live) = live() {
                let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                let ask = |w: i32, h: i32| {
                    let mut r = view_rect(w, h);
                    unsafe { live.view.checkSizeConstraint(&mut r) };
                    (r.right - r.left, r.bottom - r.top)
                };
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
                unsafe { live.view.onSize(&mut r) };
            }
            note_bounds(hwnd);
        }
        WM_SETFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                unsafe { live.view.onFocus(1) };
            }
        }
        WM_KILLFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                unsafe { live.view.onFocus(0) };
            }
        }
        WM_ACTIVATE if (wparam & 0xFFFF) as u32 == WA_INACTIVE => release_keys(),
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            let scale = dpi as f32 / 96.0;
            if let Some(live) = live() {
                if let Some(support) = &live.scale {
                    unsafe { support.setContentScaleFactor(scale) };
                }
                let r = unsafe { &*(lparam as *const RECT) };
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
            close_window();
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub(super) fn close_window() {
    let Some(editor) = EDITOR.with(|e| e.borrow_mut().take()) else {
        return;
    };
    note_bounds(editor.hwnd);
    stop_raising();
    release_keys();
    unsafe {
        editor.view.removed();
        editor.view.setFrame(null_mut());
        DestroyWindow(editor.hwnd);
    }
}

thread_local! {
    static OWNER: Cell<HWND> = const { Cell::new(null_mut()) };
}

unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn owner() -> HWND {
    let made = OWNER.get();
    if !made.is_null() {
        return made;
    }
    static CLASS: Once = Once::new();
    register(OWNER_CLASS, owner_proc, &CLASS);
    let class = wide(OWNER_CLASS);
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

pub(super) fn open(
    title: &str,
    view: Option<ComPtr<IPlugView>>,
    wanted: Option<EditorBounds>,
) -> Result<(), String> {
    if EDITOR.with(|e| e.borrow().is_some()) {
        return Err("an editor is already open".into());
    }
    let view = view.ok_or("the plugin has no editor")?;
    if unsafe { view.isPlatformTypeSupported(kPlatformTypeHWND) } != kResultTrue {
        return Err("the editor cannot be shown in a window".into());
    }
    static CLASS: Once = Once::new();
    register(FRAME_CLASS, frame_proc, &CLASS);
    let class = wide(FRAME_CLASS);
    let title = wide(title);
    let (x, y) = wanted
        .filter(on_a_screen)
        .map_or((CW_USEDEFAULT, CW_USEDEFAULT), |b| (b.x, b.y));
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
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let scale = dpi as f32 / 96.0;
    let support = view.cast::<IPlugViewContentScaleSupport>();
    if let Some(support) = &support {
        unsafe { support.setContentScaleFactor(scale) };
    }
    let frame = ComWrapper::new(FrameObj);
    let Some(frame_ptr) = frame.to_com_ptr::<IPlugFrame>() else {
        unsafe { DestroyWindow(hwnd) };
        return Err("could not give the editor a frame".into());
    };
    unsafe { view.setFrame(frame_ptr.as_ptr()) };
    let client = wanted
        .filter(|b| b.width > 0 && b.height > 0)
        .map(|b| {
            let per = if b.dpi > 0 {
                dpi as f64 / b.dpi as f64
            } else {
                1.0
            };
            let side = |v: u32| ((v as f64 * per).round() as i32).clamp(1, MAX_EDITOR_SIDE as i32);
            (side(b.width), side(b.height))
        })
        .unwrap_or_else(|| plugin_size(&view));
    let (ow, oh) = outer_for(client, dpi);
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
    EDITOR.with(|e| {
        *e.borrow_mut() = Some(Open {
            view: view.clone(),
            scale: support,
            _frame: frame,
            hwnd,
            attached: false,
        });
    });
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
    let (cw, ch) = client_size(hwnd);
    let mut r = view_rect(cw, ch);
    unsafe { view.onSize(&mut r) };
    unsafe {
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
    }
    note_bounds(hwnd);
    start_raising();
    Ok(())
}

pub(super) fn status() -> EditorStatus {
    let open = live().map(|l| l.hwnd);
    if let Some(hwnd) = open {
        note_bounds(hwnd);
    }
    EditorStatus {
        open: open.is_some(),
        visible: open.is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) } != 0),
        bounds: LAST.get(),
    }
}

pub(super) fn set_visible(visible: bool) {
    if let Some(live) = live() {
        if visible {
            unsafe { ShowWindow(live.hwnd, SW_SHOWNA) };
            raise_if_in_front();
        } else {
            release_keys();
            unsafe { ShowWindow(live.hwnd, SW_HIDE) };
        }
    }
}

fn on_a_screen(bounds: &EditorBounds) -> bool {
    let r = RECT {
        left: bounds.x,
        top: bounds.y,
        right: bounds.x + 64,
        bottom: bounds.y + 32,
    };
    !unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONULL) }.is_null()
}
