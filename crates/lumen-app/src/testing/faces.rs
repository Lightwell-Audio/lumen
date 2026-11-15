use std::time::Duration;

use eframe::egui;

use crate::chain::cadence;

pub fn face_repaint_ask(ctx: &egui::Context) -> Option<Duration> {
    cadence::asked(ctx)
}

pub fn forget_face_repaint_asks(ctx: &egui::Context) {
    cadence::forget_asks(ctx);
}

pub fn face_frame() -> Duration {
    crate::motion::FACE_FRAME
}
