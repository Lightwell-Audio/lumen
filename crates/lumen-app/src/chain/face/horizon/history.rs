use lumen_devices::MeterReading;

use super::super::Scene;
use super::super::ballistics::{Ballistic, step_of};
use super::geometry::{COLUMNS, TAKE_EVERY};
use crate::motion;

const READOUT_RELEASE: f32 = 0.12;
const FLASH_FULL_DB: f32 = 5.0;
const SILENT_DB: f32 = -120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Heard {
    pub input_db: f32,
    pub output_db: f32,
    pub reduction_db: f32,
}

impl Heard {
    pub(super) const SILENT: Self = Self {
        input_db: SILENT_DB,
        output_db: SILENT_DB,
        reduction_db: 0.0,
    };

    pub(super) fn of(reading: MeterReading) -> Self {
        Self {
            input_db: reading.input_db,
            output_db: reading.output_db,
            reduction_db: reading.reduction_db.max(0.0),
        }
    }

    pub(super) fn sounding(&self) -> bool {
        self.input_db > motion::SILENCE_DB
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Skyline {
    input: [f32; COLUMNS],
    output: [f32; COLUMNS],
    count: usize,
    gathered: f32,
    most_input: f32,
    most_output: f32,
    flash: f32,
    pulled: Ballistic,
    at: f64,
}

impl Skyline {
    pub(super) fn new(at: f64) -> Self {
        Self {
            input: [SILENT_DB; COLUMNS],
            output: [SILENT_DB; COLUMNS],
            count: 0,
            gathered: 0.0,
            most_input: SILENT_DB,
            most_output: SILENT_DB,
            flash: 0.0,
            pulled: Ballistic::new(motion::METER_ATTACK, READOUT_RELEASE),
            at,
        }
    }

    pub(super) fn step(&mut self, heard: Heard, dt: f32, still: bool) {
        let dt = step_of(dt);
        self.pulled.step(heard.reduction_db, dt, still);
        if still {
            self.flash = 0.0;
            self.count = 0;
            self.gathered = 0.0;
            return;
        }
        let hit = (heard.reduction_db / FLASH_FULL_DB).clamp(0.0, 1.0);
        self.flash = (self.flash * (-dt / motion::FLASH_TAU).exp()).max(hit);
        self.most_input = self.most_input.max(heard.input_db);
        self.most_output = self.most_output.max(heard.output_db);
        self.gathered += dt;
        while self.gathered >= TAKE_EVERY {
            self.gathered -= TAKE_EVERY;
            self.take();
        }
    }

    fn take(&mut self) {
        if self.count == COLUMNS {
            self.input.copy_within(1.., 0);
            self.output.copy_within(1.., 0);
        } else {
            self.count += 1;
        }
        let last = self.count - 1;
        self.input[last] = self.most_input;
        self.output[last] = self.most_output;
        self.most_input = SILENT_DB;
        self.most_output = SILENT_DB;
    }

    pub(super) fn input(&self) -> &[f32] {
        &self.input[..self.count]
    }

    pub(super) fn output(&self) -> &[f32] {
        &self.output[..self.count]
    }

    pub(super) fn shift(&self) -> f32 {
        (self.gathered / TAKE_EVERY).clamp(0.0, 1.0)
    }

    pub(super) fn flash(&self) -> f32 {
        self.flash
    }

    pub(super) fn pulled(&self) -> f32 {
        self.pulled.value()
    }
}

pub(super) fn advance(scene: &Scene<'_>, heard: Heard) -> Skyline {
    let ctx = scene.painter.ctx();
    let key = scene.id.with("skyline");
    let now = ctx.input(|i| i.time);
    let mut sky = ctx
        .data(|d| d.get_temp::<Skyline>(key))
        .unwrap_or_else(|| Skyline::new(now - f64::from(motion::DT_MAX)));
    let dt = (now - sky.at) as f32;
    sky.at = now;
    sky.step(heard, dt, scene.still);
    if scene.awake <= 0.0 && !scene.still {
        ctx.data_mut(|d| d.remove::<Skyline>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, sky));
    }
    sky
}
