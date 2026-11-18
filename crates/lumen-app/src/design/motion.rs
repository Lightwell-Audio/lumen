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

pub const FACE_FRAME: std::time::Duration = std::time::Duration::from_millis(16);

pub const METER_ATTACK: f32 = 0.004;
pub const METER_RELEASE: f32 = 0.16;
pub const PEAK_HOLD: f32 = 0.70;
pub const PEAK_FALL: f32 = 1.1;
#[allow(dead_code)]
pub const FLASH_RISE: f32 = 0.010;
pub const FLASH_TAU: f32 = 0.12;

pub const SILENCE_DB: f32 = -66.0;
pub const SLEEP_HOLD: f32 = 0.45;
pub const SLEEP_TAU: f32 = 0.30;
pub const WAKE_TAU: f32 = 0.05;
pub const DT_MAX: f32 = 1.0 / 20.0;

pub const GLOW_REST: f32 = 0.10;
#[allow(dead_code)]
pub const GLOW_LIVE: f32 = 0.30;
#[allow(dead_code)]
pub const GLOW_ENERGY: f32 = 0.50;
#[allow(dead_code)]
pub const GLOW_HIT: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringKind {
    pub omega: f32,
    pub zeta: f32,
}

#[allow(dead_code)]
pub const SNAP: SpringKind = SpringKind {
    omega: 46.0,
    zeta: 0.55,
};
pub const POP: SpringKind = SpringKind {
    omega: 34.0,
    zeta: 0.38,
};
#[allow(dead_code)]
pub const SOFT: SpringKind = SpringKind {
    omega: 32.0,
    zeta: 1.0,
};

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

#[allow(dead_code)]
pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let u = t.clamp(0.0, 1.0) - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
}

#[allow(dead_code)]
pub fn ease_out_expo(t: f32) -> f32 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2.0_f32.powf(-10.0 * t.clamp(0.0, 1.0))
    }
}

#[allow(dead_code)]
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

pub fn follow(current: f32, target: f32, tau_up: f32, tau_down: f32, dt: f32) -> f32 {
    let tau = if target > current { tau_up } else { tau_down };
    if tau <= 0.0 {
        return target;
    }
    current + (target - current) * (1.0 - (-dt.max(0.0) / tau).exp())
}

#[allow(dead_code)]
pub fn flash(age: f32) -> f32 {
    if age <= 0.0 {
        0.0
    } else if age < FLASH_RISE {
        age / FLASH_RISE
    } else {
        (-(age - FLASH_RISE) / FLASH_TAU).exp()
    }
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
    fn the_landing_curves_start_at_zero_and_end_on_one() {
        for curve in [ease_out_back, ease_out_expo, ease_in_out] {
            assert!(curve(0.0).abs() < 1e-3, "{}", curve(0.0));
            assert!((curve(1.0) - 1.0).abs() < 1e-6);
        }
        assert!(
            (0..100).any(|i| ease_out_back(i as f32 / 100.0) > 1.0),
            "out_back overshoots"
        );
        assert!(ease_out_expo(0.2) > 0.7, "thrown outward, then long settle");
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn follow_is_fast_up_slow_down_and_frame_rate_independent() {
        let up = follow(0.0, 1.0, METER_ATTACK, METER_RELEASE, 0.016);
        let down = follow(1.0, 0.0, METER_ATTACK, METER_RELEASE, 0.016);
        assert!(up > 0.95, "{up}");
        assert!(down > 0.85 && down < 0.95, "{down}");
        let mut slow = 1.0;
        for _ in 0..10 {
            slow = follow(slow, 0.0, METER_ATTACK, METER_RELEASE, 0.02);
        }
        let mut fast = 1.0;
        for _ in 0..40 {
            fast = follow(fast, 0.0, METER_ATTACK, METER_RELEASE, 0.005);
        }
        assert!((slow - fast).abs() < 1e-5, "{slow} vs {fast}");
        assert_eq!(follow(0.3, 0.8, 0.0, 0.1, 0.01), 0.8);
    }

    #[test]
    fn a_flash_rises_in_a_frame_and_falls_like_a_decay() {
        assert_eq!(flash(-0.1), 0.0);
        assert_eq!(flash(0.0), 0.0);
        assert!((flash(FLASH_RISE) - 1.0).abs() < 1e-6);
        let later = flash(FLASH_RISE + FLASH_TAU);
        assert!((later - (-1.0_f32).exp()).abs() < 1e-6, "{later}");
    }

    #[test]
    fn the_spring_presets_overshoot_as_named() {
        let overshoot =
            |k: SpringKind| (-std::f32::consts::PI * k.zeta / (1.0 - k.zeta * k.zeta).sqrt()).exp();
        assert!((overshoot(SNAP) - 0.126).abs() < 0.01);
        assert!((overshoot(POP) - 0.28).abs() < 0.02);
        assert_eq!(SOFT.zeta, 1.0);
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
