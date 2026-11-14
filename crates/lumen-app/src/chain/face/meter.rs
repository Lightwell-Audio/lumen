//! What a dynamics device's gain-reduction meter says, as a face shows it.

use super::Scene;
use crate::motion;

/// The gain reduction falls back at this rate (a share a second) once the
/// device lets go, and a reading this small is none, in decibels.
const FALL: f32 = 5.0;
const AT_REST: f32 = 0.05;
/// The longest a frame's step is taken as, so a stalled frame does not jump.
const LONGEST_STEP: f32 = 0.1;

/// Whether a reduction is worth drawing: a quieter one is a still picture.
pub(super) fn is_none(decibels: f32) -> bool {
    decibels < AT_REST
}

/// How much the device is taking away now, in decibels: it follows the meter
/// up at once and falls back as the device lets go. The state is the card's
/// own, kept between frames; with motion reduced (no reading) there is none.
pub(super) fn pulled(scene: &Scene<'_>) -> f32 {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("pulled");
    let Some(reading) = scene.reading else {
        ctx.data_mut(|d| d.remove::<(f32, f64)>(key));
        return 0.0;
    };
    let now = ctx.input(|i| i.time);
    let (was, at) = ctx
        .data(|d| d.get_temp::<(f32, f64)>(key))
        .unwrap_or((0.0, now));
    let step = ((now - at) as f32).clamp(0.0, LONGEST_STEP);
    let fallen = was * (-step * FALL).exp();
    let pulled = reading.reduction_db.max(fallen);
    let kept = if is_none(pulled) { 0.0 } else { pulled };
    ctx.data_mut(|d| d.insert_temp(key, (kept, now)));
    if kept > 0.0 {
        // Asks for the next frame while the picture is moving, and not otherwise.
        ctx.request_repaint_after(motion::METER_FRAME);
    }
    kept
}
