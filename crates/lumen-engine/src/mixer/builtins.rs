//! Built-in effects in the mixer (M8 slice B0, ADR-XXXX).
//!
//! A built-in effect is Lumen's own code, so the mixer runs it in place on the
//! audio thread: it is held in a fixed-size [`BuiltinPool`], found by the id a
//! [`Device::Builtin`] names, and its parameters arrive with the plan (the
//! node's [`NodeSpec::values`]) and as the block's lane points, so nothing has
//! to be sent to it while it runs.
//!
//! Every built-in device always has a **fallback**: the node's input through a
//! delay line as long as the effect's reported latency. A device that is not
//! attached yet (the interface is still building it) or a block rendered ahead
//! of a jump plays that, so nothing shifts in time and the device's state never
//! moves on audio it should not hear. Unlike a plugin there is no glide between
//! the two: a built-in is attached while stopped, when a project opens.
//!
//! [`Device::Builtin`]: super::spec::Device::Builtin
//! [`NodeSpec::values`]: super::spec::NodeSpec::values

use lumen_devices::hosted::{QUIET_LEVEL, QUIET_SECONDS};
use lumen_devices::{Block, Effect, Key, Stereo};

use lumen_core::pan::PanLaw;

use super::delay::Lines;
use super::hosts::{HostScratch, Hosting};
use super::plan::CHUNK;

/// How many built-in effects one mixer holds: the most devices a project's
/// tracks and buses can carry in one plan.
pub const MAX_BUILTINS: usize = 256;

/// A device with no built-in: one in a host, or the synthetic delay.
pub(super) const NO_BUILTIN: u64 = u64::MAX;

/// The longest a reported tail is waited through: after it, only the output
/// decides (a device may report an endless one).
const TAIL_MOST_SECONDS: u32 = 10;

/// What the interface side asks of the pool.
pub(super) enum DeviceMsg {
    Attach(u64, Box<Slot>),
    Detach(u64),
}

/// One running built-in effect, with what the engine has learned about it.
pub struct Slot {
    effect: Box<dyn Effect>,
    /// Frames since its output was last heard above [`QUIET_LEVEL`].
    quiet: u64,
    /// It was given a block since the last [`BuiltinPool::age`].
    asked: bool,
}

impl Slot {
    /// A slot around `effect`. Allocates: call it on the interface side.
    pub fn new(effect: Box<dyn Effect>) -> Box<Self> {
        Box::new(Self {
            effect,
            quiet: u64::MAX,
            asked: false,
        })
    }

    /// Follows how loud the block's output was.
    fn heard(&mut self, left: &[f32], right: &[f32]) {
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(left.len() as u64);
        }
    }
}

/// The attached built-in effects, by device id. Fixed size, so nothing
/// allocates while the audio thread uses it.
pub(super) struct BuiltinPool {
    slots: Vec<Option<(u64, Box<Slot>)>>,
    used: usize,
}

impl BuiltinPool {
    /// A pool with room for [`MAX_BUILTINS`]. Allocates: call it off the audio
    /// thread.
    pub fn new() -> Self {
        Self {
            slots: (0..MAX_BUILTINS).map(|_| None).collect(),
            used: 0,
        }
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Slot> {
        if self.used == 0 {
            return None;
        }
        self.slots
            .iter_mut()
            .flatten()
            .find(|(slot, _)| *slot == id)
            .map(|(_, slot)| slot.as_mut())
    }

    /// Attaches `slot` as `id`: the one it replaces comes back, or the new one
    /// when the pool is full.
    pub fn insert(&mut self, id: u64, slot: Box<Slot>) -> Result<Option<Box<Slot>>, Box<Slot>> {
        if let Some((_, old)) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|(slot, _)| *slot == id)
        {
            return Ok(Some(std::mem::replace(old, slot)));
        }
        match self.slots.iter_mut().find(|slot| slot.is_none()) {
            Some(free) => {
                *free = Some((id, slot));
                self.used += 1;
                Ok(None)
            }
            None => Err(slot),
        }
    }

    pub fn remove(&mut self, id: u64) -> Option<Box<Slot>> {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(slot, _)| *slot == id) {
                self.used -= 1;
                return slot.take().map(|(_, slot)| slot);
            }
        }
        None
    }

    /// Whether anything a built-in plays may still be heard at `rate`: its
    /// output has not been quiet for long, or its tail has not run out. What
    /// the engine asks after a Stop, before it stops rendering.
    pub fn sounding(&self, rate: u32) -> bool {
        if self.used == 0 {
            return false;
        }
        let rate = u64::from(rate);
        self.slots.iter().flatten().any(|(_, slot)| {
            let tail = u64::from(slot.effect.tail()).min(rate * u64::from(TAIL_MOST_SECONDS));
            let window = (rate * u64::from(QUIET_SECONDS)).max(tail);
            slot.quiet < window
        })
    }

    /// `frames` were rendered: a built-in not asked for a block meanwhile has
    /// been quiet for them.
    pub fn age(&mut self, frames: usize) {
        if self.used == 0 {
            return;
        }
        for (_, slot) in self.slots.iter_mut().flatten() {
            if !std::mem::take(&mut slot.asked) {
                slot.quiet = slot.quiet.saturating_add(frames as u64);
            }
        }
    }

    /// Every effect forgets what it holds: playback jumps to another place in
    /// the song.
    pub fn reset(&mut self) {
        if self.used == 0 {
            return;
        }
        for (_, slot) in self.slots.iter_mut().flatten() {
            slot.effect.reset();
            slot.quiet = u64::MAX;
        }
    }
}

/// Runs a built-in device over a node's `left` and `right`: its output, or the
/// fallback (the input through the device's delay line) when it is not attached
/// or the block is being rendered ahead of a jump. `key` is what arrives for
/// the node's sidechain. `scratch.params` holds the values the node's
/// parameters hold and the points the block's lanes give them.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    dev: &mut super::plan::Dev,
    lines: &mut Lines,
    scratch: &mut HostScratch,
    hosting: &mut Hosting<'_>,
    (left, right): (&mut [f32], &mut [f32]),
    key: Option<(&[f32], &[f32])>,
    (rate, start, advance): (u32, i64, bool),
    law: PanLaw,
) {
    let n = left.len();
    // The fallback line runs every block, so it holds the input it should
    // when the device is missing.
    let (dry_left, dry_right) = scratch.dry.split_at_mut(CHUNK);
    let (dry_left, dry_right) = (&mut dry_left[..n], &mut dry_right[..n]);
    dry_left.copy_from_slice(left);
    dry_right.copy_from_slice(right);
    lines.run(dev.line, dry_left, dry_right);
    let Some(slot) = hosting
        .builtins
        .as_deref_mut()
        .and_then(|pool| pool.get_mut(dev.builtin))
    else {
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        return;
    };
    if !advance {
        // A tail rendered ahead of a jump: the device must not hear it, so the
        // fallback plays and its state is left where the song left it.
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        slot.asked = true;
        return;
    }
    let block = Block {
        rate,
        start,
        law,
        events: &scratch.params,
    };
    let key = key.map(|(left, right)| Key { left, right });
    slot.effect.process(
        &block,
        Stereo {
            left: &mut *left,
            right: &mut *right,
        },
        key,
    );
    slot.asked = true;
    slot.heard(left, right);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clips::{ClipAudio, clip_player};
    use crate::mixer::{Destination, NodeSpec, Routing, mixer};
    use crate::source::{Mode, Output, RenderContext, Source};
    use crate::timeline::Timeline;
    use lumen_core::time::Ticks;
    use std::sync::Arc;

    const RATE: u32 = 48_000;

    struct Quiet;

    impl Effect for Quiet {
        fn process(&mut self, _: &Block<'_>, io: Stereo<'_>, _: Option<Key<'_>>) {
            io.left.fill(0.0);
            io.right.fill(0.0);
        }
    }

    struct Endless;

    impl Effect for Endless {
        fn process(&mut self, _: &Block<'_>, _: Stereo<'_>, _: Option<Key<'_>>) {}
        fn tail(&self) -> u32 {
            u32::MAX
        }
    }

    /// A device that answers with a decaying tone, to hear a tail.
    struct Ring {
        level: f32,
        tail: u32,
    }

    impl Effect for Ring {
        fn process(&mut self, _: &Block<'_>, io: Stereo<'_>, _: Option<Key<'_>>) {
            for k in 0..io.left.len() {
                self.level *= 0.999;
                io.left[k] = self.level;
                io.right[k] = self.level;
            }
        }

        fn tail(&self) -> u32 {
            self.tail
        }
    }

    /// Whether a mixer playing `value` through a built-in `effect` still has
    /// something to say after `seconds` of it, as the engine asks after a Stop
    /// and an export does when it adds a tail.
    fn rings(value: f32, effect: Box<dyn Effect>, seconds: f32) -> bool {
        let audio = Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![vec![value; 48_000], vec![value; 48_000]],
        });
        let clip = crate::clips::Clip {
            id: 1,
            track: 1,
            start: Ticks::ZERO,
            offset: 0,
            length: 48_000,
            gain: 1.0,
            audio,
            shape: Default::default(),
            envelope: Default::default(),
        };
        let (mut clips, player) = clip_player();
        clips.set(vec![clip]).expect("room for a clip list");
        let (mut handle, mut source) = mixer(player);
        let mut track = NodeSpec::unity(1, Destination::Node(100));
        track.devices = vec![crate::mixer::Device::Builtin {
            device: 7,
            latency: 0,
        }];
        assert!(
            handle
                .set(&Routing {
                    nodes: vec![NodeSpec::unity(100, Destination::Hardware(0)), track],
                    cue: None,
                    measure: None,
                })
                .expect("a valid routing")
        );
        handle
            .attach_builtin(7, Slot::new(effect))
            .unwrap_or_else(|_| panic!("room in the pool"));
        source.idle(RATE, 128);
        let timeline = Timeline::default();
        let mut data = vec![0.0; 2 * 128];
        let blocks = (seconds * RATE as f32 / 128.0).max(1.0) as usize;
        for k in 0..blocks {
            let cx = RenderContext {
                timeline: &timeline,
                sample_rate: RATE,
                start: k as i64 * 128,
                tail: false,
                stopping: false,
                mode: Mode::Play,
                stream: k as u64 * 128,
                device: k as u64 * 128,
                recording: false,
                looping: None,
                deadline: None,
            };
            let mut out = Output::new(&mut data, 128, 0, 128, 2);
            source.render(&cx, &mut out);
        }
        source.ringing()
    }

    #[test]
    fn a_pool_holds_its_slots_and_says_when_nothing_may_still_sound() {
        let mut pool = BuiltinPool::new();
        assert!(!pool.sounding(48_000));
        assert!(pool.insert(7, Slot::new(Box::new(Quiet))).is_ok());
        assert!(pool.get_mut(7).is_some());
        // Nothing has played yet, so nothing sounds.
        assert!(!pool.sounding(48_000));
        assert!(pool.get_mut(9).is_none());
        // A device with an endless tail is neither heard nor quiet until it
        // has been given a block.
        assert!(pool.insert(8, Slot::new(Box::new(Endless))).is_ok());
        assert!(!pool.sounding(48_000));
        // Replacing gives the old one back; removing takes it away.
        let old = pool.insert(7, Slot::new(Box::new(Quiet)));
        assert!(matches!(old, Ok(Some(_))));
        assert!(pool.remove(7).is_some());
        assert!(pool.remove(7).is_none());
        assert!(pool.get_mut(8).is_some());
    }

    #[test]
    fn a_full_pool_refuses_the_device_it_cannot_hold() {
        let mut pool = BuiltinPool::new();
        for id in 0..MAX_BUILTINS as u64 {
            assert!(pool.insert(id, Slot::new(Box::new(Quiet))).is_ok());
        }
        assert!(
            pool.insert(u64::MAX - 1, Slot::new(Box::new(Quiet)))
                .is_err()
        );
        assert!(pool.get_mut(MAX_BUILTINS as u64 - 1).is_some());
    }

    #[test]
    fn a_device_with_a_tail_rings_on_after_a_stop() {
        // The answer to "built-in effects have no tails": the trait says how
        // long one rings, and the engine keeps what it plays.
        assert!(rings(
            0.25,
            Box::new(Ring {
                level: 0.5,
                tail: RATE,
            }),
            0.2
        ));
    }

    #[test]
    fn a_device_that_stops_with_its_input_stops_sounding() {
        assert!(!rings(0.0, Box::new(Quiet), 3.0));
    }
}
