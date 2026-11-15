use std::time::{Duration, Instant};

use lumen_devices::{Guard, Outcome, ParamBlock};

use super::builtins::BuiltinPool;
use super::delay::Lines;
use super::plan::{CHUNK, Dev, Plan};

pub const MAX_HOSTS: usize = 256;

pub(super) const NO_HOST: u64 = u64::MAX;

pub(super) enum HostMsg {
    Attach(u64, Guard),
    Detach(u64),
}

pub(super) struct HostPool {
    slots: Vec<Option<(u64, Guard)>>,
    used: usize,
}

impl HostPool {
    pub fn new() -> Self {
        Self {
            slots: (0..MAX_HOSTS).map(|_| None).collect(),
            used: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.used
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Guard> {
        if self.used == 0 {
            return None;
        }
        self.slots
            .iter_mut()
            .flatten()
            .find(|(slot, _)| *slot == id)
            .map(|(_, guard)| guard)
    }

    pub fn insert(&mut self, id: u64, guard: Guard) -> Result<Option<Guard>, Guard> {
        if let Some((_, old)) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|(slot, _)| *slot == id)
        {
            return Ok(Some(std::mem::replace(old, guard)));
        }
        match self.slots.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some((id, guard));
                self.used += 1;
                Ok(None)
            }
            None => Err(guard),
        }
    }

    pub fn sounding(&self, rate: u32) -> bool {
        self.used > 0
            && self
                .slots
                .iter()
                .flatten()
                .any(|(_, guard)| !guard.bypassed() && guard.sounding(rate))
    }

    pub fn age(&mut self, frames: usize) {
        if self.used == 0 {
            return;
        }
        for (_, guard) in self.slots.iter_mut().flatten() {
            guard.age(frames);
        }
    }

    pub fn flush(&mut self) {
        if self.used == 0 {
            return;
        }
        for (_, guard) in self.slots.iter_mut().flatten() {
            guard.flush();
        }
    }

    pub fn remove(&mut self, id: u64) -> Option<Guard> {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(slot, _)| *slot == id) {
                self.used -= 1;
                return slot.take().map(|(_, guard)| guard);
            }
        }
        None
    }
}

pub(super) struct Hosting<'a> {
    pub pool: Option<&'a mut HostPool>,
    pub builtins: Option<&'a mut BuiltinPool>,
    pub fading: Option<&'a Plan>,
    pub deadline: Option<Instant>,
}

impl Hosting<'static> {
    #[allow(dead_code)]
    pub fn none() -> Self {
        Self {
            pool: None,
            builtins: None,
            fading: None,
            deadline: None,
        }
    }
}

pub(super) struct HostScratch {
    pub input: [f32; 2 * CHUNK],
    pub dry: [f32; 2 * CHUNK],
    pub wet: [f32; 2 * CHUNK],
    pub gain: [f32; CHUNK],
    pub params: ParamBlock,
}

impl HostScratch {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            input: [0.0; 2 * CHUNK],
            dry: [0.0; 2 * CHUNK],
            wet: [0.0; 2 * CHUNK],
            gain: [0.0; CHUNK],
            params: ParamBlock::new(),
        })
    }
}

fn wanted(guard: &Guard, dev: &Dev) -> bool {
    !guard.failed() && (!guard.bypassed() || dev.wet.value() > 0.0)
}

pub(super) fn begin_early(
    dev: &mut Dev,
    hosting: &mut Hosting<'_>,
    scratch: &HostScratch,
    frames: usize,
) {
    let Some(pool) = hosting.pool.as_deref_mut() else {
        return;
    };
    if let Some(guard) = pool.get_mut(dev.host)
        && wanted(guard, dev)
    {
        let (left, right) = scratch.input.split_at(CHUNK);
        dev.began = guard.begin(
            frames,
            &[&left[..frames], &right[..frames]],
            &[],
            &scratch.params,
        );
        dev.refused = !dev.began && !guard.failed();
    }
}

pub(super) fn run(
    dev: &mut Dev,
    lines: &mut Lines,
    scratch: &mut HostScratch,
    hosting: &mut Hosting<'_>,
    (left, right): (&mut [f32], &mut [f32]),
    key: Option<(&[f32], &[f32])>,
    (rate, advance): (u32, bool),
) {
    let n = left.len();
    let HostScratch {
        dry,
        wet,
        gain,
        params,
        ..
    } = scratch;
    let (dry_left, dry_right) = dry.split_at_mut(CHUNK);
    let (dry_left, dry_right) = (&mut dry_left[..n], &mut dry_right[..n]);
    dry_left.copy_from_slice(left);
    dry_right.copy_from_slice(right);
    lines.run(dev.line, dry_left, dry_right);
    if !advance {
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        return;
    }

    let (wet_left, wet_right) = wet.split_at_mut(CHUNK);
    let (wet_left, wet_right) = (&mut wet_left[..n], &mut wet_right[..n]);
    let mut ready = false;
    let mut target = 0.0;
    if let Some(pool) = hosting.pool.as_deref_mut()
        && let Some(guard) = pool.get_mut(dev.host)
        && (dev.began || wanted(guard, dev))
    {
        if !dev.began {
            let again = std::mem::take(&mut dev.refused);
            let begin = |guard: &mut Guard, input: &[&[f32]]| {
                if again {
                    guard.begin_again(n, input, &[], params)
                } else {
                    guard.begin(n, input, &[], params)
                }
            };
            dev.began = match key {
                Some((key_left, key_right)) => {
                    begin(guard, &[&*left, &*right, key_left, key_right])
                }
                None => begin(guard, &[&*left, &*right]),
            };
        }
        let deadline = hosting.deadline.unwrap_or_else(|| {
            Instant::now() + Duration::from_micros(n as u64 * 750_000 / u64::from(rate.max(1)))
        });
        ready = guard.finish(deadline, wet_left, wet_right) == Outcome::Ready;
        target = if guard.bypassed() { 0.0 } else { 1.0 };
    }
    dev.began = false;
    dev.refused = false;

    if !ready {
        dev.wet.set(0.0);
        dev.wet.settle();
    } else {
        dev.wet.set(target);
    }
    if !ready || (dev.wet.settled() && dev.wet.value() == 0.0) {
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
    } else if dev.wet.settled() {
        left.copy_from_slice(wet_left);
        right.copy_from_slice(wet_right);
    } else {
        let gain = &mut gain[..n];
        dev.wet.fill(gain, rate, true);
        for k in 0..n {
            left[k] = dry_left[k] + (wet_left[k] - dry_left[k]) * gain[k];
            right[k] = dry_right[k] + (wet_right[k] - dry_right[k]) * gain[k];
        }
    }
}
