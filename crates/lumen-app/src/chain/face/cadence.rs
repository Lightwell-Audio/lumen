use std::time::Duration;

use eframe::egui::{Context, Id};

use super::ballistics::{Rest, db_of_level};
use crate::motion;

#[derive(Debug, Clone, Copy)]
struct Sleeper {
    rest: Rest,
    at: f64,
}

fn asked_key() -> Id {
    Id::new("face repaint asked")
}

pub(crate) fn wake(ctx: &Context, id: Id, level: f32, still: bool) -> f32 {
    let key = id.with("face rest");
    let now = ctx.input(|i| i.time);
    let mut sleeper = ctx.data(|d| d.get_temp::<Sleeper>(key)).unwrap_or(Sleeper {
        rest: Rest::default(),
        at: now,
    });
    let dt = (now - sleeper.at) as f32;
    let awake = sleeper.rest.step(db_of_level(level), dt, still);
    sleeper.at = now;
    if sleeper.rest.asleep() {
        ctx.data_mut(|d| d.remove::<Sleeper>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, sleeper));
        again(ctx, still);
    }
    awake
}

pub(crate) fn again(ctx: &Context, still: bool) {
    if !still {
        ask(ctx, motion::FACE_FRAME);
    }
}

fn ask(ctx: &Context, wait: Duration) {
    let key = asked_key();
    let kept = ctx
        .data(|d| d.get_temp::<Duration>(key))
        .map_or(wait, |known| known.min(wait));
    ctx.data_mut(|d| d.insert_temp(key, kept));
    ctx.request_repaint_after(wait);
}

pub(crate) fn asked(ctx: &Context) -> Option<Duration> {
    ctx.data(|d| d.get_temp::<Duration>(asked_key()))
}

pub(crate) fn forget_asks(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Duration>(asked_key()));
}

#[cfg(test)]
mod tests;
