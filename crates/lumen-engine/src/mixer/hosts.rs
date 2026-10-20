//! Hosted effects in the mixer (M6 slice 2b, ADR-0069).
//!
//! A plugin effect runs in a host process; the mixer holds the host in a
//! fixed-size [`HostPool`], found by the id a [`Device::Hosted`] names, and
//! never calls plugin code. The plan refers to a host by id, so attaching,
//! detaching or losing one never swaps a plan.
//!
//! Each hosted device always runs a **fallback**: the node's input through a
//! delay line of the plugin's reported latency. While the host is bypassed,
//! failed, absent or late, that is what the node hears, so nothing shifts in
//! time (the owner's D1 and D7). The host's output replaces it through a
//! 20 ms glide (`wet`), so a bypass or a recovery never steps; a host that
//! fails mid-glide has nothing to fade from, so that block simply falls back.
//!
//! To let the hosts work while the mixer does, a node that nothing feeds
//! hands its first hosted device its block before the node loop
//! ([`begin_scaled`]); the loop then collects it where it reaches it. Other
//! devices submit and collect in place and so run one after another (the
//! owner's B1).
//!
//! [`Device::Hosted`]: super::spec::Device::Hosted

use std::time::{Duration, Instant};

use lumen_devices::{Guard, Outcome, ParamBlock};

use super::delay::Lines;
use super::plan::{CHUNK, Dev};

/// How many hosts one mixer holds.
pub const MAX_HOSTS: usize = 256;

/// A device with no host: a built-in one.
pub(super) const NO_HOST: u64 = u64::MAX;

/// What the interface side asks of the pool.
pub(super) enum HostMsg {
    Attach(u64, Guard),
    Detach(u64),
}

/// The attached hosts, by device id. Fixed size, so nothing allocates while
/// the audio thread uses it.
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

    /// Attaches `guard` as `id`: the one it replaces comes back, or the new
    /// one when the pool is full.
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

    /// Whether a host that is on may still be heard at `rate`: what it
    /// played has not been quiet for long (its tail, after a Stop).
    pub fn sounding(&self, rate: u32) -> bool {
        self.used > 0
            && self
                .slots
                .iter()
                .flatten()
                .any(|(_, guard)| !guard.bypassed() && guard.sounding(rate))
    }

    /// `frames` were rendered: hosts not asked for a block meanwhile have
    /// been quiet for them.
    pub fn age(&mut self, frames: usize) {
        if self.used == 0 {
            return;
        }
        for (_, guard) in self.slots.iter_mut().flatten() {
            guard.age(frames);
        }
    }

    /// Every effect forgets what it holds (see [`Guard::flush`]).
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

/// What one render of a plan may ask of the hosts: the pool and the
/// callback's deadline. A plan fading out is given none, so it plays its
/// fallbacks and a host is never asked twice for one block.
pub(super) struct Hosting<'a> {
    pub pool: Option<&'a mut HostPool>,
    pub deadline: Option<Instant>,
}

impl Hosting<'static> {
    pub fn none() -> Self {
        Self {
            pool: None,
            deadline: None,
        }
    }
}

/// Buffers a plan's hosted devices work in, held by the plan so nothing is
/// zeroed on the stack every block.
pub(super) struct HostScratch {
    /// A node's input, as handed to its host ahead of the node loop.
    pub input: [f32; 2 * CHUNK],
    /// The input through the device's delay line: the fallback.
    dry: [f32; 2 * CHUNK],
    /// The host's output.
    wet: [f32; 2 * CHUNK],
    gain: [f32; CHUNK],
    /// The parameters the lanes change in the block, for the device being
    /// handed one (M7 slice A3).
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

/// Whether the host of `dev` is asked for a block: it works, and is not
/// bypassed, or is still fading out.
fn wanted(guard: &Guard, dev: &Dev) -> bool {
    !guard.failed() && (!guard.bypassed() || dev.wet.value() > 0.0)
}

/// Hands the host of `dev` the first `frames` of `scratch.input` (both sides,
/// `CHUNK` apart) now, ahead of the node loop.
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

/// Runs a hosted device over a node's `left` and `right`: the host's output,
/// or the fallback, or a glide between them. `key` is what arrives for the
/// node's sidechain, given to the plugin as its sidechain input. `advance` is false for a tail
/// rendered ahead of a jump, which plays the fallback and leaves the host
/// and the glide alone (the audio is not what the host should hear next).
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
    // The delay line runs every block, so it holds the input it should when
    // the fallback is needed.
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
            // A block refused early is tried once more here, and is one
            // missed block however often it is tried.
            let again = std::mem::take(&mut dev.refused);
            let begin = |guard: &mut Guard, input: &[&[f32]]| {
                if again {
                    guard.begin_again(n, input, &[], params)
                } else {
                    guard.begin(n, input, &[], params)
                }
            };
            dev.began = match key {
                // The plugin's sidechain follows its audio in the block.
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
        // Nothing to fade from: the fallback, at once.
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
