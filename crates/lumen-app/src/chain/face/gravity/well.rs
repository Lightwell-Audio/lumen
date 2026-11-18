use std::sync::OnceLock;

use lumen_devices::MeterReading;

use super::super::Scene;
use super::super::ballistics::{Ballistic, Spring, step_of};
use super::super::cadence;
use super::geometry::{CELLS, COLUMNS, FOCUS, MIDDLE, ROWS, SPREAD, focus_down};
use crate::motion;

const NEIGHBOURS: f32 = 1500.0;
const HOLD: f32 = 42.0;
const DAMPING: f32 = 5.0;
const STEP: f32 = 1.0 / 120.0;
const HELD_SHARE: f32 = 0.112;
const MOST_STEPS: usize = 8;
const MOST_HEIGHT: f32 = 4.0;
const AT_REST_HEIGHT: f32 = 0.004;
const AT_REST_SPEED: f32 = 0.03;
const KICK_COLUMNS: std::ops::RangeInclusive<usize> = 10..=12;
const KICK_ROWS: std::ops::RangeInclusive<usize> = 5..=7;
const KICK_PER_DB: f32 = 2.4;
const KICK_RATE_DB_PER_SECOND: f32 = 42.0;
const KICK_MOST_DB: f32 = 24.0;

const FULL_DEPTH_DB: f32 = 16.0;
const FLASH_NOMINAL_FRAME: f32 = 60.0;
const FLASH_FLOOR_DB: f32 = 0.05;
const FLASH_PER_DB: f32 = 14.0;
const READOUT_ATTACK: f32 = 0.006;
const READOUT_RELEASE: f32 = 0.10;
const AT_REST_DB: f32 = 0.05;
const AT_REST_FLASH: f32 = 0.01;
const AT_REST_SQUASH: f32 = 0.002;

pub(super) const BALL_RESTING_LIFT: f32 = -12.0;
const BALL_RESTING_BELOW: f32 = 0.05;
const SQUASH_PER_SPEED: f32 = 1.0 / 260.0;
const SQUASH_MOST: f32 = 0.28;
const SQUASH_ATTACK: f32 = 0.02;
const SQUASH_RELEASE: f32 = 0.05;
const SILENT_DB: f32 = -120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Heard {
    pub reduction_db: f32,
    pub input_db: f32,
}

impl Heard {
    pub(super) const SILENT: Self = Self {
        reduction_db: 0.0,
        input_db: SILENT_DB,
    };

    pub(super) fn of(reading: MeterReading) -> Self {
        Self {
            reduction_db: reading.reduction_db.max(0.0),
            input_db: reading.input_db,
        }
    }

    pub(super) fn sounding(&self) -> bool {
        self.input_db > motion::SILENCE_DB
    }
}

fn shapes() -> &'static [f32; CELLS] {
    static SHAPES: OnceLock<[f32; CELLS]> = OnceLock::new();
    SHAPES.get_or_init(|| {
        std::array::from_fn(|k| {
            let (i, j) = (k % COLUMNS, k / COLUMNS);
            let dx = (i as f32 - MIDDLE.0) / SPREAD.0;
            let dy = (j as f32 - MIDDLE.1) / SPREAD.1;
            (-(dx * dx + dy * dy)).exp()
        })
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Membrane {
    height: [f32; CELLS],
    speed: [f32; CELLS],
    owed: f32,
}

impl Membrane {
    pub(super) fn flat() -> Self {
        Self {
            height: [0.0; CELLS],
            speed: [0.0; CELLS],
            owed: 0.0,
        }
    }

    pub(super) fn height(&self, column: usize, row: usize) -> f32 {
        self.height[row * COLUMNS + column]
    }

    pub(super) fn focus(&self) -> f32 {
        self.height(FOCUS.0, FOCUS.1)
    }

    pub(super) fn kick(&mut self, strength: f32) {
        for row in KICK_ROWS {
            for column in KICK_COLUMNS {
                self.speed[row * COLUMNS + column] -= strength;
            }
        }
    }

    pub(super) fn hold(&mut self, depth: f32) {
        let shapes = shapes();
        for row in 1..ROWS - 1 {
            for column in 1..COLUMNS - 1 {
                let k = row * COLUMNS + column;
                self.height[k] = -depth * HELD_SHARE * shapes[k];
                self.speed[k] = 0.0;
            }
        }
        self.owed = 0.0;
    }

    pub(super) fn step(&mut self, depth: f32, dt: f32) {
        let shapes = shapes();
        self.owed += dt;
        let mut taken = 0;
        while self.owed >= STEP && taken < MOST_STEPS {
            self.owed -= STEP;
            taken += 1;
            for row in 1..ROWS - 1 {
                for column in 1..COLUMNS - 1 {
                    let k = row * COLUMNS + column;
                    let h = &self.height;
                    let laplacian =
                        h[k - 1] + h[k + 1] + h[k - COLUMNS] + h[k + COLUMNS] - 4.0 * h[k];
                    let target = -depth * shapes[k];
                    self.speed[k] +=
                        (NEIGHBOURS * laplacian - HOLD * (h[k] - target) - DAMPING * self.speed[k])
                            * STEP;
                }
            }
            for (height, speed) in self.height.iter_mut().zip(&self.speed) {
                *height = (*height + speed * STEP).clamp(-MOST_HEIGHT, MOST_HEIGHT);
            }
        }
        self.owed = self.owed.min(STEP);
    }

    pub(super) fn at_rest(&self) -> bool {
        self.height.iter().all(|h| h.abs() < AT_REST_HEIGHT)
            && self.speed.iter().all(|v| v.abs() < AT_REST_SPEED)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Well {
    membrane: Membrane,
    reduction: f32,
    pulled: Ballistic,
    flash: f32,
    ball: Spring,
    squash: f32,
    focus_down: f32,
    at: f64,
}

fn ball_target(awake: f32) -> f32 {
    if awake < BALL_RESTING_BELOW {
        BALL_RESTING_LIFT
    } else {
        0.0
    }
}

impl Well {
    pub(super) fn new(at: f64, awake: f32) -> Self {
        Self {
            membrane: Membrane::flat(),
            reduction: 0.0,
            pulled: Ballistic::new(READOUT_ATTACK, READOUT_RELEASE),
            flash: 0.0,
            ball: Spring::new(motion::POP, ball_target(awake)),
            squash: 0.0,
            focus_down: focus_down(0.0),
            at,
        }
    }

    pub(super) fn step(&mut self, heard: Heard, awake: f32, dt: f32, still: bool) {
        let dt = step_of(dt);
        let reduction = heard.reduction_db;
        let rise = (reduction - self.reduction).max(0.0);
        self.reduction = reduction;
        self.pulled.step(reduction, dt, still);
        let target = ball_target(awake);
        if still {
            self.flash = 0.0;
            self.squash = 0.0;
            self.membrane.hold(depth_of(reduction));
            self.ball.step(target, dt, true);
            self.focus_down = focus_down(self.membrane.focus());
            return;
        }
        if dt > 0.0 {
            let rate = rise / dt;
            if rate > KICK_RATE_DB_PER_SECOND {
                self.membrane.kick(rise.min(KICK_MOST_DB) * KICK_PER_DB);
            }
            let hit =
                ((rate / FLASH_NOMINAL_FRAME - FLASH_FLOOR_DB) * FLASH_PER_DB).clamp(0.0, 1.0);
            self.flash = (self.flash * (-dt / motion::FLASH_TAU).exp()).max(hit);
        }
        self.membrane.step(depth_of(reduction), dt);
        self.ball.step(target, dt, false);
        let down = focus_down(self.membrane.focus());
        if dt > 0.0 {
            let speed = (down - self.focus_down) / dt.max(1e-3);
            self.squash = motion::follow(
                self.squash,
                (speed * SQUASH_PER_SPEED).clamp(-SQUASH_MOST, SQUASH_MOST),
                SQUASH_ATTACK,
                SQUASH_RELEASE,
                dt,
            );
        }
        self.focus_down = down;
    }

    pub(super) fn idle(&self, awake: f32) -> bool {
        self.reduction <= 0.0
            && self.pulled.value() < AT_REST_DB
            && self.flash < AT_REST_FLASH
            && self.squash.abs() < AT_REST_SQUASH
            && self.membrane.at_rest()
            && self.ball.settled_on(ball_target(awake))
    }

    pub(super) fn membrane(&self) -> &Membrane {
        &self.membrane
    }

    pub(super) fn depth(&self) -> f32 {
        depth_of(self.reduction)
    }

    pub(super) fn pulled(&self) -> f32 {
        self.pulled.value()
    }

    pub(super) fn flash(&self) -> f32 {
        self.flash
    }

    pub(super) fn lift(&self) -> f32 {
        self.ball.at()
    }

    pub(super) fn squash(&self) -> f32 {
        self.squash
    }

    pub(super) fn focus_down(&self) -> f32 {
        self.focus_down
    }
}

pub(super) fn depth_of(reduction_db: f32) -> f32 {
    (reduction_db / FULL_DEPTH_DB).clamp(0.0, 1.0)
}

pub(super) fn advance(scene: &Scene<'_>, heard: Heard, awake: f32) -> Well {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("well");
    let now = ctx.input(|i| i.time);
    let mut well = ctx
        .data(|d| d.get_temp::<Well>(key))
        .unwrap_or_else(|| Well::new(now - f64::from(motion::DT_MAX), awake));
    let dt = (now - well.at) as f32;
    well.at = now;
    well.step(heard, awake, dt, scene.still);
    if well.idle(awake) && awake <= 0.0 && !scene.still {
        ctx.data_mut(|d| d.remove::<Well>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, well));
        if !well.idle(awake) {
            cadence::again(ctx, scene.still);
        }
    }
    well
}
