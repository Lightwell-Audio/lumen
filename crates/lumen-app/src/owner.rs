//! Lumen's main window as the owner of native dialogs (open, save).
//!
//! An owned dialog stays above Lumen, minimizes and restores with it, and
//! comes forward when Lumen is clicked, so it can never get lost behind
//! other windows (owner request, MT-APP-8). The dialogs run on their own
//! thread, so the window is passed there as a plain handle value.

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, WindowHandle,
};

/// Lumen's window, as a value that can cross threads.
#[derive(Debug, Clone, Copy)]
pub struct Owner(RawWindowHandle);

// SAFETY: a window handle is an identifier (an HWND on Windows), not a
// pointer that is dereferenced here; the dialog only passes it to the OS,
// which accepts it from any thread.
unsafe impl Send for Owner {}

impl Owner {
    /// The window behind `frame`, where the platform supports owning
    /// dialogs.
    pub fn of(frame: &eframe::Frame) -> Option<Self> {
        let raw = frame.window_handle().ok()?.as_raw();
        matches!(raw, RawWindowHandle::Win32(_)).then_some(Self(raw))
    }
}

impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the handle came from Lumen's main window, which outlives
        // every dialog: dialogs are modal to it and return before it closes.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

impl HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}
