use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Id};

pub const FAST: f32 = 0.10;
pub const EDGE_SCROLL_SPEED: f32 = 900.0;
pub const BASE: f32 = 0.18;
pub const NOTE: std::time::Duration = std::time::Duration::from_millis(1400);
pub const GLIDE: f32 = 0.22;
pub const WASH: f32 = 0.6;
pub const BREATH: f32 = 2.4;
pub const BREATH_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

pub const TEXT_EVERY: std::time::Duration = std::time::Duration::from_millis(50);
pub const TEXT_AGAIN: std::time::Duration = std::time::Duration::from_secs(1);
pub const TEXT_FRAME: std::time::Duration = std::time::Duration::from_millis(60);
pub const STEADY_FLASH: f64 = 0.15;
pub const FADE_FRAME: std::time::Duration = std::time::Duration::from_millis(16);
pub const MENU_LEAVE_DELAY: f64 = 0.4;
pub const SCROLL_BAR_LINGER: f64 = 0.8;
pub const PROGRESS_FRAME: std::time::Duration = std::time::Duration::from_millis(100);
pub const RECHECK: std::time::Duration = std::time::Duration::from_millis(50);
pub const PROGRESS_FAST: std::time::Duration = std::time::Duration::from_millis(50);
pub const WORK_FRAME: std::time::Duration = std::time::Duration::from_millis(30);
pub const WAIT_FRAME: std::time::Duration = std::time::Duration::from_millis(100);
pub const METER_FRAME: std::time::Duration = std::time::Duration::from_millis(33);
pub const SCAN_FRAME: std::time::Duration = std::time::Duration::from_millis(250);
pub const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);
pub const ENGINE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);
pub const DEVICE_POLL: std::time::Duration = std::time::Duration::from_millis(300);
pub const IDLE_TICK: std::time::Duration = std::time::Duration::from_secs(1);
pub const AGE_FRAME: std::time::Duration = std::time::Duration::from_secs(1);

static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn set_reduced(reduced: bool) {
    REDUCED.store(reduced, Ordering::Relaxed);
}

pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

pub fn time(duration: f32) -> f32 {
    if reduced() { 0.0 } else { duration }
}

pub fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

pub fn spring(t: f32) -> f32 {
    const OMEGA: f32 = 7.0;
    let t = t.clamp(0.0, 1.0);
    let raw = |t: f32| 1.0 - (1.0 + OMEGA * t) * (-OMEGA * t).exp();
    raw(t) / raw(1.0)
}

pub fn fade(ctx: &egui::Context, id: Id, visible: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, visible, time(BASE), ease_out)
}

pub fn hover(ctx: &egui::Context, id: Id, on: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, on, time(FAST), ease_out)
}

pub fn appear(ctx: &egui::Context, since: f64) -> f32 {
    let duration = time(BASE);
    if duration == 0.0 {
        return 1.0;
    }
    let age = (ctx.input(|i| i.time) - since) as f32;
    if age < duration {
        ctx.request_repaint();
    }
    ease_out(age / duration)
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    from: f32,
    to: f32,
    start: f64,
}

pub fn glide(ctx: &egui::Context, id: Id, target: f32, animate: bool) -> f32 {
    glide_over(ctx, id, target, animate, GLIDE)
}

pub fn glide_over(ctx: &egui::Context, id: Id, target: f32, animate: bool, duration: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let duration = time(duration);
    let previous: Option<Glide> = ctx.data(|d| d.get_temp(id));
    let state = match previous {
        Some(g) if animate && duration > 0.0 && g.to != target => Glide {
            from: shown(g, now, duration),
            to: target,
            start: now,
        },
        Some(g) if animate && duration > 0.0 => g,
        _ => Glide {
            from: target,
            to: target,
            start: now,
        },
    };
    ctx.data_mut(|d| d.insert_temp(id, state));
    let value = shown(state, now, duration);
    if value != state.to {
        ctx.request_repaint();
    }
    value
}

fn shown(g: Glide, now: f64, duration: f32) -> f32 {
    if duration <= 0.0 {
        return g.to;
    }
    let t = ((now - g.start) as f32 / duration).clamp(0.0, 1.0);
    if t >= 1.0 {
        g.to
    } else {
        g.from + (g.to - g.from) * spring(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_and_land_exactly_on_one() {
        for curve in [ease_out, spring] {
            assert_eq!(curve(0.0), 0.0);
            assert!((curve(1.0) - 1.0).abs() < 1e-6);
            let mut last = 0.0;
            for i in 1..=100 {
                let v = curve(i as f32 / 100.0);
                assert!(v >= last && v <= 1.0 + 1e-6, "{v} after {last}");
                last = v;
            }
        }
        assert!(spring(0.33) > 0.6 && ease_out(0.33) > 0.6);
    }

    #[test]
    fn a_glide_follows_exactly_when_not_animating() {
        let ctx = egui::Context::default();
        let id = Id::new("playhead");
        assert_eq!(glide(&ctx, id, 4.0, false), 4.0);
        assert_eq!(glide(&ctx, id, 9.0, false), 9.0);
        let first = glide(&ctx, id, 20.0, true);
        assert!((9.0..20.0).contains(&first), "{first}");

        set_reduced(true);
        assert_eq!(time(BASE), 0.0);
        assert_eq!(glide(&ctx, id, 30.0, true), 30.0);
        set_reduced(false);
        assert_eq!(time(BASE), BASE);
    }
}
