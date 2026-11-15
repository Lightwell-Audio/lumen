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

type Drain = dyn FnMut() -> bool;

thread_local! {
    static DRAIN: Cell<Option<NonNull<Drain>>> = const { Cell::new(None) };
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
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn drain_now() {
    if BUSY.replace(true) {
        return;
    }
    if let Some(mut drain) = DRAIN.get() {
        let more = unsafe { drain.as_mut()() };
        if !more {
            unsafe { PostQuitMessage(0) };
        }
    }
    BUSY.set(false);
}

pub struct WindowPump {
    dispatcher: HWND,
}

impl WindowPump {
    pub fn new() -> Self {
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        static CLASS: Once = Once::new();
        register(DISPATCH_CLASS, dispatch_proc, &CLASS);
        let name = wide(DISPATCH_CLASS);
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
        let target = self.dispatcher as isize;
        Arc::new(move || {
            unsafe { PostMessageW(target as HWND, WM_APP, 0, 0) };
        })
    }

    fn run(&mut self, drain: &mut dyn FnMut() -> bool) {
        let erased: NonNull<Drain> = unsafe {
            std::mem::transmute::<NonNull<dyn FnMut() -> bool + '_>, _>(NonNull::from(drain))
        };
        DRAIN.set(Some(erased));
        drain_now();
        let mut msg = MSG {
            hwnd: null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };
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
