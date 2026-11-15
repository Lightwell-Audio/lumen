use lumen_core::pan::PanLaw;

use crate::ParamBlock;
use crate::params::ParamInfo;

pub type Values<'a> = &'a [(u32, f32)];

#[derive(Clone, Copy)]
pub struct Description {
    pub name: &'static str,
    pub table: &'static [ParamInfo],
    pub latency: fn(Values<'_>) -> u32,
    pub tail: fn(Values<'_>) -> u32,
}

#[derive(Debug)]
pub struct Stereo<'a> {
    pub left: &'a mut [f32],
    pub right: &'a mut [f32],
}

impl Stereo<'_> {
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    pub fn split_at_mut(&mut self, until: usize) -> (Stereo<'_>, Stereo<'_>) {
        let (left, left_rest) = self.left.split_at_mut(until);
        let (right, right_rest) = self.right.split_at_mut(until);
        (
            Stereo { left, right },
            Stereo {
                left: left_rest,
                right: right_rest,
            },
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Key<'a> {
    pub left: &'a [f32],
    pub right: &'a [f32],
}

#[derive(Debug)]
pub struct Block<'a> {
    pub rate: u32,
    pub start: i64,
    pub bpm: f32,
    pub law: PanLaw,
    pub events: &'a ParamBlock,
}

pub trait Effect: Send {
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, key: Option<Key<'_>>);

    fn latency(&self) -> u32 {
        0
    }

    fn tail(&self) -> u32 {
        0
    }

    fn reset(&mut self) {}
}

pub const MOST_FRAMES: usize = crate::MOST_FRAMES;

#[cfg(test)]
mod tests {
    use super::*;

    struct Counted(usize);

    impl Effect for Counted {
        fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, key: Option<Key<'_>>) {
            assert_eq!(block.events.len(), 0);
            assert!(key.is_none());
            self.0 += io.frames();
        }
    }

    #[test]
    fn a_device_sees_the_whole_block_and_its_latency_and_tail_default_to_none() {
        let mut device = Counted(0);
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        let block = Block {
            rate: 48_000,
            start: 0,
            bpm: 120.0,
            law: PanLaw::Balance,
            events: &ParamBlock::none(),
        };
        device.process(
            &block,
            Stereo {
                left: &mut left,
                right: &mut right,
            },
            None,
        );
        assert_eq!(device.0, 64);
        assert_eq!(device.latency(), 0);
        assert_eq!(device.tail(), 0);
    }

    #[test]
    fn a_block_splits_into_stretches_of_the_same_frames() {
        let (mut left, mut right) = ([0.0; 8], [0.0; 8]);
        let mut io = Stereo {
            left: &mut left,
            right: &mut right,
        };
        let (head, tail) = io.split_at_mut(3);
        assert_eq!((head.frames(), tail.frames()), (3, 5));
    }
}
