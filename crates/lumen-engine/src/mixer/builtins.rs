use lumen_devices::hosted::{QUIET_LEVEL, QUIET_SECONDS};
use lumen_devices::{Block, Effect, Key, Stereo};

use lumen_core::pan::PanLaw;

use super::delay::Lines;
use super::hosts::{HostScratch, Hosting};
use super::plan::CHUNK;

pub const MAX_BUILTINS: usize = 2048;

pub(super) const NO_BUILTIN: u64 = u64::MAX;

const TAIL_MOST_SECONDS: u32 = 10;

pub(super) enum DeviceMsg {
    Attach(u64, Box<Slot>),
    Detach(u64),
}

pub struct Slot {
    effect: Box<dyn Effect>,
    quiet: u64,
    asked: bool,
    was_off: bool,
}

impl Slot {
    pub fn new(effect: Box<dyn Effect>) -> Box<Self> {
        Box::new(Self {
            effect,
            quiet: u64::MAX,
            asked: false,
            was_off: false,
        })
    }

    fn heard(&mut self, left: &[f32], right: &[f32]) {
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(left.len() as u64);
        }
    }
}

pub(super) struct BuiltinPool {
    slots: Vec<Option<(u64, Box<Slot>)>>,
    used: usize,
}

impl BuiltinPool {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    dev: &mut super::plan::Dev,
    lines: &mut Lines,
    scratch: &mut HostScratch,
    hosting: &mut Hosting<'_>,
    (left, right): (&mut [f32], &mut [f32]),
    key: Option<(&[f32], &[f32])>,
    (rate, start, bpm, advance): (u32, i64, f32, bool),
    law: PanLaw,
) {
    let n = left.len();
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
        dev.values_last.fill(f32::NAN);
        dev.wet.set(0.0);
        dev.wet.settle();
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        return;
    };
    if !advance {
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        slot.asked = true;
        return;
    }
    if dev.bypass && dev.wet.settled() && dev.wet.value() == 0.0 {
        dev.values_last.fill(f32::NAN);
        slot.was_off = true;
        left.copy_from_slice(dry_left);
        right.copy_from_slice(dry_right);
        return;
    }
    if slot.was_off {
        slot.effect.reset();
        slot.was_off = false;
        slot.quiet = u64::MAX;
    }
    if !dev.bypass && dev.wet.settled() && dev.wet.value() == 1.0 {
        let block = Block {
            rate,
            start,
            bpm,
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
        return;
    }
    let (wet_left, wet_right) = scratch.wet.split_at_mut(CHUNK);
    let (wet_left, wet_right) = (&mut wet_left[..n], &mut wet_right[..n]);
    wet_left.copy_from_slice(left);
    wet_right.copy_from_slice(right);
    let block = Block {
        rate,
        start,
        bpm,
        law,
        events: &scratch.params,
    };
    let key = key.map(|(left, right)| Key { left, right });
    slot.effect.process(
        &block,
        Stereo {
            left: wet_left,
            right: wet_right,
        },
        key,
    );
    slot.asked = true;
    slot.heard(wet_left, wet_right);
    dev.wet.set(if dev.bypass { 0.0 } else { 1.0 });
    if dev.wet.settled() {
        left.copy_from_slice(wet_left);
        right.copy_from_slice(wet_right);
    } else {
        let gain = &mut scratch.gain[..n];
        dev.wet.fill(gain, rate, true);
        for k in 0..n {
            left[k] = dry_left[k] + (wet_left[k] - dry_left[k]) * gain[k];
            right[k] = dry_right[k] + (wet_right[k] - dry_right[k]) * gain[k];
        }
    }
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
            bypass: false,
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
        assert!(!pool.sounding(48_000));
        assert!(pool.get_mut(9).is_none());
        assert!(pool.insert(8, Slot::new(Box::new(Endless))).is_ok());
        assert!(!pool.sounding(48_000));
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
