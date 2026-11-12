//! Lumen's window icon (ADR-0043): the app icon drawn by `lumen_brand`,
//! which also draws the executable's icons on Windows (`build.rs`,
//! ADR-0117), so the window, the taskbar and Explorer show one drawing.

use eframe::egui;

/// The window icon's size in pixels.
pub const WINDOW_ICON: u32 = 128;

/// The window icon.
pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: lumen_brand::app_icon(WINDOW_ICON),
        width: WINDOW_ICON,
        height: WINDOW_ICON,
    }
}
