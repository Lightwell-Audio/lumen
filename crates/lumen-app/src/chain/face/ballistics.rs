use crate::motion::{self, SpringKind};
use crate::theme::personality::card as c;

const SPRING_SETTLED: f32 = 0.001;
const ASLEEP_BELOW: f32 = 0.005;

pub(crate) fn step_of(dt: f32) -> f32 {
    dt.clamp(0.0, motion::DT_MAX)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spring {
    kind: SpringKind,
    at: f32,
    speed: f32,
}

impl Spring {
    pub(crate) fn new(kind: SpringKind, at: f32) -> Self {
        Self {
            kind,
            at,
            speed: 0.0,
        }
    }

    pub(crate) fn at(&self) -> f32 {
        self.at
    }

    pub(crate) fn settled_on(&self, target: f32) -> bool {
        (self.at - target).abs() < SPRING_SETTLED && self.speed.abs() < SPRING_SETTLED * 10.0
    }

    pub(crate) fn snap(&mut self, at: f32) {
        self.at = at;
        self.speed = 0.0;
    }

    pub(crate) fn step(&mut self, target: f32, dt: f32, still: bool) -> f32 {
        if still {
            self.snap(target);
            return self.at;
        }
        let t = step_of(dt);
        let SpringKind { omega, zeta } = self.kind;
        let (off, speed) = (self.at - target, self.speed);
        let (off, speed) = if zeta >= 1.0 {
            let k = speed + omega * off;
            let decay = (-omega * t).exp();
            (decay * (off + k * t), decay * (speed - omega * k * t))
        } else {
            let alpha = zeta * omega;
            let damped = omega * (1.0 - zeta * zeta).sqrt();
            let decay = (-alpha * t).exp();
            let (sin, cos) = (damped * t).sin_cos();
            let b = (speed + alpha * off) / damped;
            (
                decay * (off * cos + b * sin),
                decay * (speed * cos - (damped * off + alpha * b) * sin),
            )
        };
        self.at = target + off;
        self.speed = speed;
        self.at
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ballistic {
    value: f32,
    peak: f32,
    held: f32,
    tau_up: f32,
    tau_down: f32,
}

impl Default for Ballistic {
    fn default() -> Self {
        Self::new(motion::METER_ATTACK, motion::METER_RELEASE)
    }
}

impl Ballistic {
    pub(crate) fn new(tau_up: f32, tau_down: f32) -> Self {
        Self {
            value: 0.0,
            peak: 0.0,
            held: 0.0,
            tau_up,
            tau_down,
        }
    }

    pub(crate) fn value(&self) -> f32 {
        self.value
    }

    #[allow(dead_code)]
    pub(crate) fn peak(&self) -> f32 {
        self.peak
    }

    #[allow(dead_code)]
    pub(crate) fn rests_at(&self, floor: f32) -> bool {
        self.value <= floor && self.peak <= floor
    }

    pub(crate) fn step(&mut self, target: f32, dt: f32, still: bool) -> f32 {
        if still {
            self.value = target;
            self.peak = target;
            self.held = 0.0;
            return target;
        }
        let dt = step_of(dt);
        self.value = motion::follow(self.value, target, self.tau_up, self.tau_down, dt);
        if self.value >= self.peak {
            self.peak = self.value;
            self.held = 0.0;
        } else {
            let falling = (self.held + dt - motion::PEAK_HOLD).clamp(0.0, dt);
            self.held += dt;
            self.peak = (self.peak - motion::PEAK_FALL * falling).max(self.value);
        }
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rest {
    quiet: f32,
    awake: f32,
}

impl Default for Rest {
    fn default() -> Self {
        Self {
            quiet: motion::SLEEP_HOLD,
            awake: 0.0,
        }
    }
}

impl Rest {
    #[allow(dead_code)]
    pub(crate) fn awake(&self) -> f32 {
        self.awake
    }

    pub(crate) fn asleep(&self) -> bool {
        self.awake == 0.0 && self.quiet >= motion::SLEEP_HOLD
    }

    pub(crate) fn step(&mut self, db: f32, dt: f32, still: bool) -> f32 {
        let dt = step_of(dt);
        if db > motion::SILENCE_DB {
            self.quiet = 0.0;
        } else {
            self.quiet = (self.quiet + dt).min(motion::SLEEP_HOLD);
        }
        let target = if self.quiet < motion::SLEEP_HOLD {
            1.0
        } else {
            0.0
        };
        self.awake = if still {
            target
        } else {
            motion::follow(self.awake, target, motion::WAKE_TAU, motion::SLEEP_TAU, dt)
        };
        if target == 0.0 && self.awake < ASLEEP_BELOW {
            self.awake = 0.0;
        }
        self.awake
    }
}

pub(crate) fn db_of_level(level: f32) -> f32 {
    if level > 0.0 {
        c::LEVEL_FLOOR_DB * (1.0 - level.min(1.0))
    } else {
        f32::NEG_INFINITY
    }
}

#[allow(dead_code)]
pub(crate) fn glow_share(awake: f32, energy: f32, hit: f32) -> f32 {
    let live = motion::GLOW_LIVE + motion::GLOW_ENERGY * energy.clamp(0.0, 1.0);
    let held = motion::GLOW_REST + (live - motion::GLOW_REST) * awake.clamp(0.0, 1.0);
    held.max(hit.clamp(0.0, 1.0) * motion::GLOW_HIT)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
