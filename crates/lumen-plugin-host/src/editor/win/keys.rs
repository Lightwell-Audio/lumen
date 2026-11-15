use std::cell::RefCell;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, MSG, WM_KEYDOWN, WM_KEYUP,
};

use super::window::live;
use crate::editor::Notify;
use crate::protocol::Response;

thread_local! {
    static CAPTURE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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

pub(super) fn release_keys() {
    for code in HELD.with(|h| std::mem::take(&mut *h.borrow_mut())) {
        tell(code, false);
    }
}

pub(super) fn capture(keys: Vec<u8>) {
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

pub(super) fn notify(notify: Notify) {
    NOTIFY.with(|n| *n.borrow_mut() = Some(notify));
}

fn modifier_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(|vk| unsafe { GetKeyState(i32::from(vk)) } < 0)
}

pub(super) fn take_key(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN && msg.message != WM_KEYUP {
        return false;
    }
    let Ok(code) = u8::try_from(msg.wParam) else {
        return false;
    };
    let Some(live) = live() else {
        return false;
    };
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
    let repeat = (msg.lParam >> 30) & 1 != 0;
    if !held && !repeat {
        HELD.with(|h| h.borrow_mut().push(code));
        tell(code, true);
    }
    true
}
