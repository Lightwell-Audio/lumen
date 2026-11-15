use eframe::egui::{self, Vec2};

use crate::App;

const LEAST: Vec2 = Vec2::new(960.0, 540.0);

fn min_window_size(scale: f32, monitor: Option<Vec2>) -> Vec2 {
    let least = LEAST * scale;
    let Some(monitor) = monitor.filter(|m| m.x >= 1.0 && m.y >= 1.0) else {
        return least;
    };
    least.min(monitor.max(LEAST))
}

fn must_send(last: Option<(Vec2, f32)>, size: Vec2, ppp: f32) -> bool {
    last != Some((size, ppp))
}

impl App {
    pub(crate) fn window_minimum_stale(&self, ctx: &egui::Context) -> bool {
        self.min_window.is_none()
            || ctx.input(|i| {
                let monitor = i.viewport().monitor_size;
                monitor != self.min_window_monitor
                    || (monitor.is_none() && self.min_window_ppp != Some(i.pixels_per_point()))
            })
    }

    pub(crate) fn apply_window_minimum(&mut self, ctx: &egui::Context) {
        let scale = self.settings.appearance.scale();
        let (ppp, monitor, zoom_then) = ctx.input(|i| {
            let viewport = i.viewport();
            let zoom_then = match viewport.native_pixels_per_point {
                Some(native) if native > 0.0 => i.pixels_per_point() / native,
                _ => scale,
            };
            (i.pixels_per_point(), viewport.monitor_size, zoom_then)
        });
        self.min_window_monitor = monitor;
        let size = min_window_size(scale, monitor.map(|m| m * zoom_then)) / scale;
        if must_send(self.min_window.zip(self.min_window_ppp), size, ppp) {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(size));
            self.min_window = Some(size);
            self.min_window_ppp = Some(ppp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_window_is_960_by_540_ui_points_at_every_scale() {
        for scale in [1.0, 1.5, 2.0] {
            assert_eq!(min_window_size(scale, None), LEAST * scale);
            assert_eq!(min_window_size(scale, None) / scale, LEAST);
        }
    }

    #[test]
    fn a_small_monitor_caps_the_least_window_but_not_below_960_by_540() {
        assert_eq!(
            min_window_size(2.0, Some(Vec2::new(1_280.0, 720.0))),
            Vec2::new(1_280.0, 720.0)
        );
        assert_eq!(min_window_size(2.0, Some(Vec2::new(800.0, 500.0))), LEAST);
        assert_eq!(
            min_window_size(1.5, Some(Vec2::new(1_500.0, 600.0))),
            Vec2::new(1_440.0, 600.0)
        );
    }

    #[test]
    fn the_least_size_is_sent_again_when_the_zoom_it_is_converted_with_changes() {
        let size = LEAST;
        assert!(must_send(None, size, 1.0), "the first command");
        assert!(!must_send(Some((size, 1.0)), size, 1.0), "nothing changed");
        assert!(
            must_send(Some((size, 1.0)), size, 2.0),
            "the same value at another zoom"
        );
        assert!(
            must_send(Some((size, 1.0)), size * 2.0, 1.0),
            "another size"
        );
    }

    #[test]
    fn a_monitor_with_room_does_not_grow_the_window_past_the_interface() {
        assert_eq!(
            min_window_size(1.0, Some(Vec2::new(3_440.0, 1_440.0))),
            LEAST
        );
        assert_eq!(
            min_window_size(2.0, Some(Vec2::new(3_840.0, 2_160.0))),
            LEAST * 2.0
        );
    }
}
