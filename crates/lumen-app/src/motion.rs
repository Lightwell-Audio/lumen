//! The motion system (`docs/DESIGN.md` §6): durations, easing curves and
//! helpers that animate UI state.
//!
//! Rules the helpers enforce:
//! - **Input is never delayed.** Callers change state at once; these
//!   functions only decide how the change is shown.
//! - **Idle stays idle.** A helper requests frames only while its animation
//!   runs.
//! - **Reduce motion** makes every duration zero, so things change in place
//!   without travelling.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Id};

/// Hover and press feedback.
pub const FAST: f32 = 0.10;
/// Popovers, windows, notices, panels.
pub const BASE: f32 = 0.18;
/// How long a short note (undo, save, open) stays before fading.
pub const NOTE: std::time::Duration = std::time::Duration::from_millis(1400);
/// Things moving to a new place (the playhead after a locate).
pub const GLIDE: f32 = 0.22;
/// What undo or redo changed: an accent wash that fades over this long.
pub const WASH: f32 = 0.6;
/// One breath of the armed ring: the one deliberate loop.
pub const BREATH: f32 = 2.4;
/// Frame interval for the armed ring's slow breathing (about 30 fps): it
/// moves too slowly to need the display's full rate.
pub const BREATH_FRAME: std::time::Duration = std::time::Duration::from_millis(33);

static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn set_reduced(reduced: bool) {
    REDUCED.store(reduced, Ordering::Relaxed);
}

pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// `duration` in seconds, or zero with Reduce motion on.
pub fn time(duration: f32) -> f32 {
    if reduced() { 0.0 } else { duration }
}

/// Cubic ease-out: starts fast, settles gently. Entrances.
pub fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// A critically damped spring (no overshoot), normalised to reach exactly 1
/// at `t = 1`: quick to start, soft to land. Movement.
pub fn spring(t: f32) -> f32 {
    const OMEGA: f32 = 7.0;
    let t = t.clamp(0.0, 1.0);
    let raw = |t: f32| 1.0 - (1.0 + OMEGA * t) * (-OMEGA * t).exp();
    raw(t) / raw(1.0)
}

/// 0 → 1 as `visible` turns on (ease-out), 1 → 0 as it turns off (the
/// mirror, so exits accelerate away).
pub fn fade(ctx: &egui::Context, id: Id, visible: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, visible, time(BASE), ease_out)
}

/// Like [`fade`], for hover and press feedback.
pub fn hover(ctx: &egui::Context, id: Id, on: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, on, time(FAST), ease_out)
}

/// 0 → 1 over [`BASE`] from `since` (seconds, `ctx` input time): for things
/// that appear once, such as a clip after an import.
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

/// A value that glides (spring, [`GLIDE`]) to each new `target` when
/// `animate` is true, and follows it exactly otherwise (for example the
/// playhead: it glides after a locate while stopped, and tracks the audio
/// exactly while playing).
pub fn glide(ctx: &egui::Context, id: Id, target: f32, animate: bool) -> f32 {
    glide_over(ctx, id, target, animate, GLIDE)
}

/// [`glide`] over `duration` seconds (before Reduce motion): `FAST` for a
/// highlight following the arrow keys.
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
            // Monotonic: no overshoot, no bounce.
            let mut last = 0.0;
            for i in 1..=100 {
                let v = curve(i as f32 / 100.0);
                assert!(v >= last && v <= 1.0 + 1e-6, "{v} after {last}");
                last = v;
            }
        }
        // Responsive: most of the way there in the first third.
        assert!(spring(0.33) > 0.6 && ease_out(0.33) > 0.6);
    }

    #[test]
    fn a_glide_follows_exactly_when_not_animating() {
        let ctx = egui::Context::default();
        let id = Id::new("playhead");
        assert_eq!(glide(&ctx, id, 4.0, false), 4.0);
        assert_eq!(glide(&ctx, id, 9.0, false), 9.0);
        // Animating from a standstill: starts where it was, not at the target.
        let first = glide(&ctx, id, 20.0, true);
        assert!((9.0..20.0).contains(&first), "{first}");

        // Reduce motion: every duration is zero, and a glide lands at once.
        // (In this test, not its own, because the setting is global.)
        set_reduced(true);
        assert_eq!(time(BASE), 0.0);
        assert_eq!(glide(&ctx, id, 30.0, true), 30.0);
        set_reduced(false);
        assert_eq!(time(BASE), BASE);
    }
}
