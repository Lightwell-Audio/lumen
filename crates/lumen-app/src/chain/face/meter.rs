use super::Scene;
use crate::motion;

const FALL: f32 = 5.0;
const AT_REST: f32 = 0.05;
const LONGEST_STEP: f32 = 0.1;

pub(super) fn is_none(decibels: f32) -> bool {
    decibels < AT_REST
}

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
        ctx.request_repaint_after(motion::METER_FRAME);
    }
    kept
}
