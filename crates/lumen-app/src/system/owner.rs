use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, WindowHandle,
};

#[derive(Debug, Clone, Copy)]
pub struct Owner(RawWindowHandle);

unsafe impl Send for Owner {}

impl Owner {
    pub fn of(frame: &eframe::Frame) -> Option<Self> {
        let raw = frame.window_handle().ok()?.as_raw();
        matches!(raw, RawWindowHandle::Win32(_)).then_some(Self(raw))
    }

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
            let _ = pixels_per_point;
            None
        }
    }
}

impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

impl HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}
