use eframe::egui;

pub const WINDOW_ICON: u32 = 128;

pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: lumen_brand::app_icon(WINDOW_ICON),
        width: WINDOW_ICON,
        height: WINDOW_ICON,
    }
}
