use super::Scene;
use super::ballistics::Ballistic;
use super::cadence;
use crate::motion;

const AT_REST: f32 = 0.05;

pub(super) fn is_none(decibels: f32) -> bool {
    decibels < AT_REST
}

pub(super) fn pulled(scene: &Scene<'_>) -> f32 {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("pulled");
    let Some(reading) = scene.reading else {
        ctx.data_mut(|d| d.remove::<(Ballistic, f64)>(key));
        return 0.0;
    };
    let now = ctx.input(|i| i.time);
    let (mut meter, at) = ctx
        .data(|d| d.get_temp::<(Ballistic, f64)>(key))
        .unwrap_or((Ballistic::default(), now - f64::from(motion::DT_MAX)));
    let shown = meter.step(reading.reduction_db, (now - at) as f32, scene.still);
    if is_none(shown) {
        ctx.data_mut(|d| d.remove::<(Ballistic, f64)>(key));
        return 0.0;
    }
    ctx.data_mut(|d| d.insert_temp(key, (meter, now)));
    cadence::again(ctx, scene.still);
    shown
}
