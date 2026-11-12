//! Plugin editor windows on Windows (M6 slice 4, ADR-0067 "Editors"; grown
//! from the M6 editor-focus prototype).
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
//! This is the only code in the host that has no test on Linux. The prototype
//! ran on Windows with the test plugin and one real plugin (MT-VST-0
//! "Editors"); this module is checked by the cross-target compiler and by
//! MT-VST-4.
//!
//! The window itself is [`window`], the z-order policy [`raise`] and the
//! keyboard [`keys`]; this file is the message pump and the editor interface.

use std::cell::Cell;
use std::ptr::{NonNull, null, null_mut};
use std::sync::{Arc, Once};

use vst3::ComPtr;
use vst3::Steinberg::IPlugView;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
    PostMessageW, PostQuitMessage, TranslateMessage, WM_APP, WM_TIMER,
};

use self::raise::{RAISE_TIMER, raise_if_in_front, set_dispatcher};
use self::window::{register, wide};
use super::{Editors, Notify};
use crate::protocol::{EditorBounds, EditorStatus};
use crate::server::{Pump, Waker};

mod keys;
mod raise;
mod window;

const DISPATCH_CLASS: &str = "LumenPluginHostDispatch";

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
        set_dispatcher(dispatcher);
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
                if keys::take_key(&msg) {
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        DRAIN.set(None);
    }
}

// ------------------------------------------------------------- the editor --

/// The editors of this process: at most one window, kept in thread-local
/// state (see [`window`]).
#[derive(Default)]
pub struct Windows;

impl Editors for Windows {
    fn open(
        &mut self,
        title: &str,
        view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String> {
        window::open(title, view, wanted)?;
        Ok(window::status())
    }

    fn close(&mut self) -> EditorStatus {
        window::close_window();
        window::status()
    }

    fn status(&mut self) -> EditorStatus {
        window::status()
    }

    fn set_visible(&mut self, visible: bool) -> EditorStatus {
        window::set_visible(visible);
        window::status()
    }

    fn capture_keys(&mut self, keys: Vec<u8>) {
        self::keys::capture(keys);
    }

    fn set_notify(&mut self, notify: Notify) {
        self::keys::notify(notify);
    }
}
