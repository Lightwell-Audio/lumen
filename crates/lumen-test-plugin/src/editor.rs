//! The test plugins' editor view, for the editor-focus spike (M6 slice 0) and
//! slice 4. Switched on by the `editor` entry in `LUMEN_TEST_PLUGIN_FAULT`
//! (see [`crate::faults`]); without it there is no view, as before.
//!
//! The view is a child window of whatever window the host gives it, holding
//! a text field (what typing is checked against), a button that asks the host
//! to grow the view (plugin-initiated resizing), and a label with the view's
//! size and content scale. It accepts user resizing within limits and, with
//! `editor_hang=MS`, stops answering its window's messages `MS` milliseconds
//! after it is attached, the way a plugin stuck in its own UI code does.
//!
//! Windows and macOS provide a small native view; elsewhere `create` returns
//! no view.

use vst3::Steinberg::IPlugView;

use crate::faults::Faults;

/// The view's size when the content scale is 1, in pixels.
pub const BASE_WIDTH: i32 = 320;
pub const BASE_HEIGHT: i32 = 200;

/// A new view object for `createView`, or null where there is no editor.
/// The caller owns the reference.
pub fn create(faults: &Faults) -> *mut IPlugView {
    #[cfg(windows)]
    {
        windows::create(faults)
    }
    #[cfg(target_os = "macos")]
    {
        macos::create(faults)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = faults;
        std::ptr::null_mut()
    }
}

#[cfg(target_os = "macos")]
#[path = "editor/macos.rs"]
mod macos;

#[cfg(windows)]
mod windows {
    use std::ffi::{c_char, c_void};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Once};

    use vst3::Steinberg::{
        IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
        IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
        kPlatformTypeHWND, kResultFalse, kResultOk, kResultTrue, tresult,
    };
    use vst3::{Class, ComPtr, ComRef, ComWrapper};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BS_PUSHBUTTON, CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        ES_AUTOVSCROLL, ES_MULTILINE, GWLP_USERDATA, GetWindowLongPtrW, IDC_ARROW, KillTimer,
        LoadCursorW, MoveWindow, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowTextW,
        WM_COMMAND, WM_NCCREATE, WM_NCDESTROY, WM_SETFOCUS, WM_SIZE, WM_TIMER, WNDCLASSEXW,
        WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use super::{BASE_HEIGHT, BASE_WIDTH};
    use crate::faults::{self, Faults};

    const CLASS: &str = "LumenTestPluginEditor";
    const MIN: (i32, i32) = (240, 160);
    const MAX: (i32, i32) = (1600, 1200);
    const GROW: (i32, i32) = (40, 30);
    const HANG_TIMER: usize = 1;
    const BN_CLICKED: usize = 0;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// What the window procedure and the view object share.
    struct Shared {
        /// The window the view made (a child of the host's), or 0.
        hwnd: AtomicUsize,
        edit: AtomicUsize,
        label: AtomicUsize,
        button: AtomicUsize,
        width: AtomicI32,
        height: AtomicI32,
        scale: AtomicU32,
        /// The view's COM pointer, for `resizeView`. Not an owning reference.
        view: AtomicUsize,
        frame: Mutex<Option<ComPtr<IPlugFrame>>>,
        hang_after_ms: AtomicU32,
    }

    impl Shared {
        fn scale(&self) -> f32 {
            f32::from_bits(self.scale.load(Ordering::Relaxed))
        }

        fn px(&self, n: i32) -> i32 {
            (f64::from(n) * f64::from(self.scale())).round() as i32
        }

        fn size(&self) -> (i32, i32) {
            (
                self.width.load(Ordering::Relaxed),
                self.height.load(Ordering::Relaxed),
            )
        }

        fn clamp(&self, (w, h): (i32, i32)) -> (i32, i32) {
            (
                w.clamp(self.px(MIN.0), self.px(MAX.0)),
                h.clamp(self.px(MIN.1), self.px(MAX.1)),
            )
        }

        /// Puts the children in their places for the current size and scale.
        fn layout(&self) {
            let (w, h) = self.size();
            let (label, edit, button) = (
                self.label.load(Ordering::Relaxed) as HWND,
                self.edit.load(Ordering::Relaxed) as HWND,
                self.button.load(Ordering::Relaxed) as HWND,
            );
            if label.is_null() {
                return;
            }
            let (bar, foot) = (self.px(24), self.px(32));
            // SAFETY: windows this view made, on its own thread.
            unsafe {
                MoveWindow(label, 0, 0, w, bar, 1);
                MoveWindow(edit, 0, bar, w, (h - bar - foot).max(1), 1);
                MoveWindow(
                    button,
                    self.px(6),
                    h - foot + self.px(4),
                    self.px(96),
                    self.px(24),
                    1,
                );
                let text = wide(&format!(
                    "Lumen test editor - {w}x{h} px - scale {:.2}",
                    self.scale()
                ));
                SetWindowTextW(label, text.as_ptr());
            }
        }
    }

    pub fn create(faults: &Faults) -> *mut IPlugView {
        let shared = Arc::new(Shared {
            hwnd: AtomicUsize::new(0),
            edit: AtomicUsize::new(0),
            label: AtomicUsize::new(0),
            button: AtomicUsize::new(0),
            width: AtomicI32::new(BASE_WIDTH),
            height: AtomicI32::new(BASE_HEIGHT),
            scale: AtomicU32::new(1.0_f32.to_bits()),
            view: AtomicUsize::new(0),
            frame: Mutex::new(None),
            hang_after_ms: AtomicU32::new(
                faults
                    .editor_hang()
                    .map_or(0, |d| d.as_millis().min(u128::from(u32::MAX)) as u32 + 1),
            ),
        });
        let view = ComWrapper::new(Editor {
            shared: Arc::clone(&shared),
        });
        let Some(ptr) = view.to_com_ptr::<IPlugView>() else {
            return null_mut();
        };
        shared.view.store(ptr.as_ptr() as usize, Ordering::Relaxed);
        ptr.into_raw()
    }

    struct Editor {
        shared: Arc<Shared>,
    }

    impl Class for Editor {
        type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
    }

    fn register_class() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let name = wide(CLASS);
            // SAFETY: a window class naming a procedure in this module; the
            // strings live for the call, the system copies them.
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

    fn shared_of(hwnd: HWND) -> Option<&'static Shared> {
        // SAFETY: the pointer was stored at creation from a leaked `Arc` and
        // is released only at `WM_NCDESTROY`, after which no message arrives.
        let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
        // SAFETY: a non-null pointer is the leaked `Arc` described above.
        (!p.is_null()).then(|| unsafe { &*p })
    }

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCCREATE => {
                // SAFETY: `lparam` is the system's `CREATESTRUCTW`.
                let params = unsafe { (*(lparam as *const CREATESTRUCTW)).lpCreateParams };
                // SAFETY: storing the pointer `attached` passed.
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, params as isize) };
            }
            WM_NCDESTROY => {
                // SAFETY: reading back the pointer stored at `WM_NCCREATE`.
                let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
                if !p.is_null() {
                    // SAFETY: balances the `Arc::into_raw` of `attached`.
                    drop(unsafe { Arc::from_raw(p) });
                    // SAFETY: clearing the slot so no later message reads it.
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
                }
            }
            WM_SIZE => {
                if let Some(shared) = shared_of(hwnd) {
                    shared.layout();
                }
            }
            WM_SETFOCUS => {
                if let Some(shared) = shared_of(hwnd) {
                    let edit = shared.edit.load(Ordering::Relaxed) as HWND;
                    if !edit.is_null() {
                        // SAFETY: a window this view made.
                        unsafe { SetFocus(edit) };
                    }
                }
            }
            WM_TIMER if wparam == HANG_TIMER => {
                // The plugin's UI code gets stuck: this thread never pumps
                // another message.
                // SAFETY: a timer this view set.
                unsafe { KillTimer(hwnd, HANG_TIMER) };
                faults::hang();
            }
            WM_COMMAND => {
                if let Some(shared) = shared_of(hwnd)
                    && (wparam >> 16) == BN_CLICKED
                    && lparam as usize == shared.button.load(Ordering::Relaxed)
                {
                    grow(shared);
                }
            }
            _ => {}
        }
        // SAFETY: the default handling for a message we did not finish.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// The plugin asks its host for a bigger view.
    fn grow(shared: &Shared) {
        let frame = shared
            .frame
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(frame) = frame else {
            return;
        };
        let (w, h) = shared.clamp({
            let (w, h) = shared.size();
            (w + shared.px(GROW.0), h + shared.px(GROW.1))
        });
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        // SAFETY: a live frame the host gave us; `view` is this object.
        unsafe {
            frame.resizeView(
                shared.view.load(Ordering::Relaxed) as *mut IPlugView,
                &mut rect,
            )
        };
    }

    impl IPlugViewTrait for Editor {
        unsafe fn isPlatformTypeSupported(&self, kind: *const c_char) -> tresult {
            if kind.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: a NUL-terminated platform name per the VST3 ABI.
            let (given, want) = unsafe {
                (
                    std::ffi::CStr::from_ptr(kind),
                    std::ffi::CStr::from_ptr(kPlatformTypeHWND),
                )
            };
            if given == want {
                kResultTrue
            } else {
                kResultFalse
            }
        }

        unsafe fn attached(&self, parent: *mut c_void, kind: *const c_char) -> tresult {
            // SAFETY: as above.
            if unsafe { self.isPlatformTypeSupported(kind) } != kResultTrue || parent.is_null() {
                return kInvalidArgument;
            }
            let shared = &self.shared;
            if shared.hwnd.load(Ordering::Relaxed) != 0 {
                return kResultFalse;
            }
            register_class();
            let (w, h) = (shared.px(BASE_WIDTH), shared.px(BASE_HEIGHT));
            shared.width.store(w, Ordering::Relaxed);
            shared.height.store(h, Ordering::Relaxed);
            let class = wide(CLASS);
            let empty = wide("");
            // SAFETY: windows made here, children of the host's `parent`,
            // owned and used on the calling (window) thread. The leaked
            // `Arc` is released when the window is destroyed.
            unsafe {
                let instance = GetModuleHandleW(null());
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    empty.as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                    0,
                    0,
                    w,
                    h,
                    parent as HWND,
                    null_mut(),
                    instance,
                    Arc::into_raw(Arc::clone(shared)).cast(),
                );
                if hwnd.is_null() {
                    return kResultFalse;
                }
                let make = |class: &str, style: u32, text: &str| {
                    let (class, text) = (wide(class), wide(text));
                    CreateWindowExW(
                        0,
                        class.as_ptr(),
                        text.as_ptr(),
                        WS_CHILD | WS_VISIBLE | style,
                        0,
                        0,
                        10,
                        10,
                        hwnd,
                        null_mut(),
                        instance,
                        null(),
                    )
                };
                let label = make("STATIC", 0, "");
                let edit = make(
                    "EDIT",
                    WS_BORDER | WS_TABSTOP | WS_VSCROLL | (ES_MULTILINE | ES_AUTOVSCROLL) as u32,
                    "",
                );
                let button = make("BUTTON", WS_TABSTOP | BS_PUSHBUTTON as u32, "Grow view");
                shared.label.store(label as usize, Ordering::Relaxed);
                shared.edit.store(edit as usize, Ordering::Relaxed);
                shared.button.store(button as usize, Ordering::Relaxed);
                shared.hwnd.store(hwnd as usize, Ordering::Relaxed);
                shared.layout();
                let after = shared.hang_after_ms.load(Ordering::Relaxed);
                if after > 0 {
                    SetTimer(hwnd, HANG_TIMER, (after - 1).max(10), None);
                }
            }
            kResultOk
        }

        unsafe fn removed(&self) -> tresult {
            let hwnd = self.shared.hwnd.swap(0, Ordering::Relaxed) as HWND;
            for slot in [&self.shared.label, &self.shared.edit, &self.shared.button] {
                slot.store(0, Ordering::Relaxed);
            }
            if !hwnd.is_null() {
                // SAFETY: the window this view made, on its own thread.
                unsafe { DestroyWindow(hwnd) };
            }
            kResultOk
        }

        unsafe fn onWheel(&self, _distance: f32) -> tresult {
            kResultFalse
        }

        unsafe fn onKeyDown(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
            kResultFalse
        }

        unsafe fn onKeyUp(&self, _key: u16, _code: i16, _modifiers: i16) -> tresult {
            kResultFalse
        }

        unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
            if size.is_null() {
                return kInvalidArgument;
            }
            let (w, h) = if self.shared.hwnd.load(Ordering::Relaxed) == 0 {
                (self.shared.px(BASE_WIDTH), self.shared.px(BASE_HEIGHT))
            } else {
                self.shared.size()
            };
            // SAFETY: the host passes a valid out pointer.
            unsafe {
                *size = ViewRect {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                };
            }
            kResultOk
        }

        unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
            if new_size.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: the host passes a valid rectangle.
            let r = unsafe { &*new_size };
            let (w, h) = self.shared.clamp((r.right - r.left, r.bottom - r.top));
            self.shared.width.store(w, Ordering::Relaxed);
            self.shared.height.store(h, Ordering::Relaxed);
            let hwnd = self.shared.hwnd.load(Ordering::Relaxed) as HWND;
            if !hwnd.is_null() {
                // SAFETY: the window this view made, on its own thread.
                unsafe { MoveWindow(hwnd, 0, 0, w, h, 1) };
            }
            kResultOk
        }

        unsafe fn onFocus(&self, state: u8) -> tresult {
            let edit = self.shared.edit.load(Ordering::Relaxed) as HWND;
            if state != 0 && !edit.is_null() {
                // SAFETY: a window this view made.
                unsafe { SetFocus(edit) };
            }
            kResultOk
        }

        unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
            // SAFETY: the host passes a live frame or null.
            let frame = unsafe { ComRef::from_raw(frame) }.map(|f| f.to_com_ptr());
            *self.shared.frame.lock().unwrap_or_else(|e| e.into_inner()) = frame;
            kResultOk
        }

        unsafe fn canResize(&self) -> tresult {
            kResultTrue
        }

        unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
            if rect.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: the host passes a valid rectangle to adjust.
            let r = unsafe { &mut *rect };
            let (w, h) = self.shared.clamp((r.right - r.left, r.bottom - r.top));
            r.right = r.left + w;
            r.bottom = r.top + h;
            kResultOk
        }
    }

    impl IPlugViewContentScaleSupportTrait for Editor {
        unsafe fn setContentScaleFactor(&self, factor: f32) -> tresult {
            if !(factor.is_finite() && factor > 0.0) {
                return kInvalidArgument;
            }
            let before = self.shared.scale();
            self.shared.scale.store(factor.to_bits(), Ordering::Relaxed);
            if self.shared.hwnd.load(Ordering::Relaxed) != 0 {
                // The same view in new pixels: the size follows the scale, as
                // a plugin's own would. The host asks `getSize` and follows.
                let ratio = f64::from(factor) / f64::from(before);
                let (w, h) = self.shared.size();
                let now = self.shared.clamp((
                    (f64::from(w) * ratio).round() as i32,
                    (f64::from(h) * ratio).round() as i32,
                ));
                self.shared.width.store(now.0, Ordering::Relaxed);
                self.shared.height.store(now.1, Ordering::Relaxed);
                self.shared.layout();
            }
            kResultOk
        }
    }
}
