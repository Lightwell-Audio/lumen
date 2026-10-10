//! The mouse wheel on value controls (M4 slice 5, the owner's B3: a
//! latching wheel): a wheel gesture that starts on a value control (a
//! slider, knob, fader or field) adjusts it, a notch a step; one that
//! started scrolling keeps scrolling, even across controls, until the wheel
//! rests. Ctrl and Alt keep the wheel for zooming and track heights.
//!
//! Controls draw after the arrangement has scrolled, so the decision is
//! made first thing each frame ([`route`]) against where the controls were
//! last frame; a control then takes its steps as it draws ([`steps`]). When
//! a control has the gesture the scroll is taken out of the input, so
//! nothing under it scrolls.

use eframe::egui::{self, Context, Id, Rect, Ui};

/// Wheel events closer than this (seconds) are one gesture.
const GAP: f64 = 0.3;
/// Scroll points to a notch of the wheel (as the navigator's Alt+wheel).
const NOTCH: f32 = 40.0;

/// Where the wheel's gesture goes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Owner {
    #[default]
    Nobody,
    /// The view under the pointer scrolls.
    Scrolling,
    /// This control takes the notches.
    Control(Id),
}

#[derive(Debug, Clone, Default)]
struct Latch {
    owner: Owner,
    /// When the last wheel event came.
    last: f64,
    /// Notches waiting for the owning control (fractions add up).
    pending: f32,
    /// The value controls drawn last frame, and this frame's so far.
    targets: Vec<(Id, Rect)>,
    next: Vec<(Id, Rect)>,
}

fn latch_id() -> Id {
    Id::new("wheel latch")
}

/// Decides where this frame's wheel goes. Call once per frame, before
/// anything reads the scroll.
pub fn route(ctx: &Context) {
    let mut latch: Latch = ctx.data_mut(|d| d.get_temp(latch_id())).unwrap_or_default();
    latch.targets = std::mem::take(&mut latch.next);
    let (delta, now, pointer, held) = ctx.input(|i| {
        (
            i.smooth_scroll_delta,
            i.time,
            i.pointer.hover_pos(),
            i.modifiers.command || i.modifiers.alt,
        )
    });
    let wheel = delta.x + delta.y;
    if wheel != 0.0 && !held {
        let under = pointer.and_then(|p| {
            latch
                .targets
                .iter()
                .rev()
                .find(|(_, r)| r.contains(p))
                .map(|(id, _)| *id)
        });
        let fresh = now - latch.last > GAP || latch.owner == Owner::Nobody;
        latch.owner = match (fresh, latch.owner, under) {
            // A new gesture: the control under the pointer, or scrolling.
            (true, _, Some(id)) => Owner::Control(id),
            (true, _, None) => Owner::Scrolling,
            // A control's gesture ends when the pointer leaves it.
            (false, Owner::Control(id), Some(now_under)) if now_under == id => Owner::Control(id),
            (false, Owner::Control(_), _) => Owner::Scrolling,
            (false, owner, _) => owner,
        };
        latch.last = now;
        if let Owner::Control(_) = latch.owner {
            latch.pending += wheel / NOTCH;
            // Taken: nothing under the control scrolls.
            ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
        }
    } else if now - latch.last > GAP {
        latch.owner = Owner::Nobody;
        latch.pending = 0.0;
    }
    ctx.data_mut(|d| d.insert_temp(latch_id(), latch));
}

/// A control has the wheel's gesture now: edits it makes merge into one
/// undo step until the wheel rests, as a drag's do until it is let go.
pub fn adjusting(ctx: &Context) -> bool {
    ctx.data_mut(|d| {
        d.get_temp::<Latch>(latch_id())
            .is_some_and(|l| matches!(l.owner, Owner::Control(_)))
    })
}

/// A value control `id` at `rect`: registers it for the wheel, and returns
/// the whole notches it takes this frame (positive up, away from the user).
/// Shift for fine steps is the control's to apply.
pub fn steps(ui: &Ui, id: Id, rect: Rect) -> f32 {
    let ctx = ui.ctx();
    ctx.data_mut(|d| {
        let latch: &mut Latch = d.get_temp_mut_or_default(latch_id());
        latch.next.push((id, rect));
        if latch.owner != Owner::Control(id) {
            return 0.0;
        }
        let whole = latch.pending.trunc();
        latch.pending -= whole;
        whole
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `route` with a wheel of `dy` at `at`, `t` seconds in, the
    /// controls of the frame before being `targets`.
    fn frame(ctx: &Context, t: f64, at: egui::Pos2, dy: f32, targets: &[(Id, Rect)]) -> f32 {
        let mut input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(at));
        if dy != 0.0 {
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let mut took = 0.0;
        let mut left = 0.0;
        let mut output = ctx.run_ui(input, |ui| {
            route(ui.ctx());
            left = ui.input(|i| i.smooth_scroll_delta.y);
            for &(id, rect) in targets {
                took += steps(ui, id, rect);
            }
        });
        output.textures_delta.clear();
        // Steps taken by a control, or the scroll left for the view.
        if took != 0.0 { took } else { -left.abs() }
    }

    #[test]
    fn a_gesture_that_starts_on_a_control_adjusts_it() {
        let ctx = Context::default();
        let control = (
            Id::new("fader"),
            Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 100.0)),
        );
        let on = egui::pos2(20.0, 50.0);
        // The first frame only learns where the control is.
        frame(&ctx, 0.0, on, 0.0, &[control]);
        let mut taken = 0.0;
        for n in 0..10 {
            taken += frame(&ctx, 0.1 + n as f64 * 0.016, on, 40.0, &[control]).max(0.0);
        }
        assert!(taken >= 3.0, "{taken}");
    }

    #[test]
    fn a_gesture_that_started_scrolling_keeps_scrolling_over_controls() {
        let ctx = Context::default();
        let control = (
            Id::new("fader"),
            Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 100.0)),
        );
        let off = egui::pos2(200.0, 50.0);
        let on = egui::pos2(20.0, 50.0);
        frame(&ctx, 0.0, off, 0.0, &[control]);
        frame(&ctx, 0.1, off, 40.0, &[control]);
        // Still scrolling as the pointer crosses the control.
        let mut taken = 0.0;
        for n in 0..10 {
            taken += frame(&ctx, 0.12 + n as f64 * 0.016, on, 40.0, &[control]).max(0.0);
        }
        assert_eq!(taken, 0.0);
        // After a rest, a new gesture on the control adjusts it.
        let mut taken = 0.0;
        for n in 0..10 {
            taken += frame(&ctx, 2.0 + n as f64 * 0.016, on, 40.0, &[control]).max(0.0);
        }
        assert!(taken >= 3.0, "{taken}");
    }
}
