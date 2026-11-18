use super::super::Scene;
use super::super::ballistics::{Spring, db_of_level, step_of};
use super::super::cadence;
use crate::motion;

const FLASH_AT: f32 = 0.6;
const BLOOM_ON: f32 = 0.5;
const BLOOM_PER_SECOND: f32 = 6.0;
const BLOOM_MOST: f32 = 1.5;
const AT_REST_ENERGY: f32 = 0.002;
const AT_REST_FLASH: f32 = 0.01;

pub(super) fn amplitude_of(level: f32) -> f32 {
    let db = db_of_level(level);
    if db.is_finite() {
        10.0_f32.powf(db / 20.0)
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Light {
    energy: f32,
    flash: f32,
    bloom: Spring,
    at: f64,
}

impl Light {
    pub(super) fn new(at: f64) -> Self {
        Self {
            energy: 0.0,
            flash: 0.0,
            bloom: Spring::new(motion::POP, 0.0),
            at,
        }
    }

    pub(super) fn step(&mut self, amplitude: f32, dt: f32, still: bool) {
        if still {
            self.energy = amplitude;
            self.flash = 0.0;
            self.bloom.snap(0.0);
            return;
        }
        let dt = step_of(dt);
        self.energy = motion::follow(
            self.energy,
            amplitude,
            motion::METER_ATTACK,
            motion::METER_RELEASE,
            dt,
        );
        let hit = if amplitude > FLASH_AT { 1.0 } else { 0.0 };
        self.flash = (self.flash * (-dt / motion::FLASH_TAU).exp()).max(hit);
        if self.flash > BLOOM_ON {
            self.bloom.raise(BLOOM_PER_SECOND * dt, BLOOM_MOST);
        }
        self.bloom.step(0.0, dt, false);
    }

    pub(super) fn idle(&self) -> bool {
        self.energy < AT_REST_ENERGY && self.flash < AT_REST_FLASH && self.bloom.settled_on(0.0)
    }

    pub(super) fn energy(&self) -> f32 {
        self.energy
    }

    pub(super) fn flash(&self) -> f32 {
        self.flash
    }

    pub(super) fn bloom(&self) -> f32 {
        self.bloom.at()
    }
}

pub(super) fn advance(scene: &Scene<'_>) -> Light {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("fan light");
    let now = ctx.input(|i| i.time);
    let mut light = ctx
        .data(|d| d.get_temp::<Light>(key))
        .unwrap_or_else(|| Light::new(now - f64::from(motion::DT_MAX)));
    let dt = (now - light.at) as f32;
    light.at = now;
    light.step(amplitude_of(scene.level), dt, scene.still);
    if light.idle() && scene.awake <= 0.0 && !scene.still {
        ctx.data_mut(|d| d.remove::<Light>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, light));
        if !light.idle() {
            cadence::again(ctx, scene.still);
        }
    }
    light
}
