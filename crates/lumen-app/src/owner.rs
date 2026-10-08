//! Lumen's main window as the owner of native dialogs (open, save), and
//! where the system's cursor is over it while files are dragged in.
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

    /// Where the system's cursor is in the window, in points of
    /// `pixels_per_point` physical pixels. Read while files are dragged in
    /// from the system, when the window gets no pointer moves (see
    /// `App::drag_pointer`).
    pub fn cursor(&self, pixels_per_point: f32) -> Option<eframe::egui::Pos2> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{HWND, POINT};
            use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
            use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

            let RawWindowHandle::Win32(window) = self.0 else {
                return None;
            };
            let mut at = POINT { x: 0, y: 0 };
            // SAFETY: both calls only write the point passed; the handle is
            // Lumen's main window, which lives as long as the app.
            let found = unsafe {
                GetCursorPos(&mut at) != 0
                    && ScreenToClient(window.hwnd.get() as HWND, &mut at) != 0
            };
            found.then(|| {
                eframe::egui::pos2(
                    at.x as f32 / pixels_per_point,
                    at.y as f32 / pixels_per_point,
                )
            })
        }
        #[cfg(not(windows))]
        {
            // Only Windows windows are kept (`of`).
            let _ = pixels_per_point;
            None
        }
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
