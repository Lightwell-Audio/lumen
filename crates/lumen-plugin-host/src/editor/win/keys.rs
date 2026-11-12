//! The computer keyboard as Lumen's, while it is on and the editor has the
//! focus: the keys Lumen asked for are taken before the plugin sees them, and
//! the ones taken and still down are always let go of.

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
pub(super) fn release_keys() {
    for code in HELD.with(|h| std::mem::take(&mut *h.borrow_mut())) {
        tell(code, false);
    }
}

/// Takes `keys` (virtual-key codes) from the plugin's window, or gives every
/// key back when empty; the keys arrive through [`crate::server::Notify`].
pub(super) fn capture(keys: Vec<u8>) {
    // A key down now that is no longer asked for is let go of; its key-up
    // then goes to the plugin, which never saw it go down.
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

/// Where the keys taken are told to Lumen, from the thread that reads its
/// answers.
pub(super) fn notify(notify: Notify) {
    NOTIFY.with(|n| *n.borrow_mut() = Some(notify));
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
