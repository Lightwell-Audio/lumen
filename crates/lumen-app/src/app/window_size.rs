//! The main window's least inner size (F5): the interface needs 960 x 540 UI
//! points at every UI scale, so the window's least size in native points
//! grows with the scale rather than the interface shrinking into it. The UI
//! scale is egui's zoom factor (`ctx.set_zoom_factor`), which divides native
//! points: at 200% a 960 native-point window leaves 480 UI points across.

use eframe::egui::{self, Vec2};

use crate::App;

/// The least window the interface is designed for, in UI points (DESIGN.md:
/// nothing overlaps at the 960 × 540 minimum window).
const LEAST: Vec2 = Vec2::new(960.0, 540.0);

/// The main window's least inner size in native points for UI `scale`
/// (egui's zoom factor): 960 x 540 UI points, which is `960 * scale` native
/// points, capped by `monitor` (native points, [`None`] while it is not
/// known) but never below 960 x 540 native points. Note that
/// [`egui::ViewportCommand::MinInnerSize`] takes UI points, so the caller
/// divides this by `scale` (egui-winit multiplies it by the zoom factor and
/// the native pixels per point).
fn min_window_size(scale: f32, monitor: Option<Vec2>) -> Vec2 {
    let least = LEAST * scale;
    // A monitor of zero or a nonsense size (some Wayland setups report one)
    // cannot cap anything.
    let Some(monitor) = monitor.filter(|m| m.x >= 1.0 && m.y >= 1.0) else {
        return least;
    };
    // The monitor's size wins when it is smaller than the scale asks for,
    // but a window below 960 x 540 native points cuts the interface too.
    least.min(monitor.max(LEAST))
}

/// Whether the least inner size must be sent: the first time, and whenever
/// its value or the pixels per point it is sent with differs from the last
/// command's.
///
/// The value alone is not enough. The value is in UI points, and egui-winit
/// multiplies it by the zoom factor and the native pixels per point when the
/// command is processed, which is after the pass that sent it, at the zoom
/// that pass had. At 100% and at 200% the value is the same 960 x 540 UI
/// points, so a scale change would send nothing and the window would keep the
/// least size of the old scale. Remembering the pixels per point the command
/// was converted with makes the same value go again once the new zoom is the
/// one in effect.
fn must_send(last: Option<(Vec2, f32)>, size: Vec2, ppp: f32) -> bool {
    last != Some((size, ppp))
}

impl App {
    /// Whether the window's least size must be worked out again: it has not
    /// been yet, or the monitor's size (in UI points, which also moves when
    /// the zoom does) is not the one the last size was worked out for.
    pub(crate) fn window_minimum_stale(&self, ctx: &egui::Context) -> bool {
        self.min_window.is_none()
            || ctx.input(|i| {
                let monitor = i.viewport().monitor_size;
                // With no monitor reported there is no cap to measure with a
                // stale zoom, so a zoom change is seen at once through the
                // pixels per point; with one, the monitor's own change is the
                // signal that the zoom has reached it.
                monitor != self.min_window_monitor
                    || (monitor.is_none() && self.min_window_ppp != Some(i.pixels_per_point()))
            })
    }

    /// Sends the window's least inner size for the current UI scale. Called
    /// from the frame loop whenever [`App::window_minimum_stale`] and where
    /// the zoom factor is applied; the command goes only when what it would
    /// send changed ([`must_send`]), never every frame.
    ///
    /// Two things here are about the zoom arriving one pass late.
    /// `ctx.set_zoom_factor` takes effect at the start of the next pass, while
    /// the viewport commands of a pass are processed at its end, so a value
    /// sent in the pass that changes the zoom is converted with the old one:
    /// hence the pixels per point in [`must_send`], which sends the same value
    /// again once the new zoom is in effect (the monitor size egui reports
    /// then has moved too, which is what calls this back in).
    ///
    /// The other is the first pass: the monitor's size egui reports was
    /// measured with the zoom the pass began with (the one before the saved
    /// scale is applied), so the cap it gives can be over-estimated for that
    /// pass. [`App::window_minimum_stale`] sees the corrected monitor size on
    /// the next pass and works the size out again, so it lasts one pass only.
    pub(crate) fn apply_window_minimum(&mut self, ctx: &egui::Context) {
        let scale = self.settings.appearance.scale();
        let (ppp, monitor, zoom_then) = ctx.input(|i| {
            let viewport = i.viewport();
            // `monitor_size` is in UI points of the zoom this pass began
            // with, which is not `scale` in the pass that changes it:
            // `pixels_per_point` is that zoom times the OS scale.
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
            // Native points: the window grows with the interface.
            assert_eq!(min_window_size(scale, None), LEAST * scale);
            // What the command takes: UI points, so the zoom factor is
            // already divided out and the interface keeps its size.
            assert_eq!(min_window_size(scale, None) / scale, LEAST);
        }
    }

    #[test]
    fn a_small_monitor_caps_the_least_window_but_not_below_960_by_540() {
        // A 1,280 x 720 monitor at 200%: the 1,920 x 1,080 the scale asks
        // for does not fit, so the monitor wins...
        assert_eq!(
            min_window_size(2.0, Some(Vec2::new(1_280.0, 720.0))),
            Vec2::new(1_280.0, 720.0)
        );
        // ...but never below 960 x 540 native points.
        assert_eq!(min_window_size(2.0, Some(Vec2::new(800.0, 500.0))), LEAST);
        // Each side is capped on its own: a 1,500 x 600 monitor at 150% keeps
        // the width the scale asks for and only shortens the height.
        assert_eq!(
            min_window_size(1.5, Some(Vec2::new(1_500.0, 600.0))),
            Vec2::new(1_440.0, 600.0)
        );
    }

    /// F5 (review): the same 960 x 540 UI points is sent again when the zoom
    /// the command will be converted with changed, and not otherwise.
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
