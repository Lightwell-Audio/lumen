//! What the engine asks of a built-in effect (M8 slice B0, ADR-0145).
//!
//! A built-in effect is Lumen's own code: it runs **in-process on the audio
//! thread**, never in a plugin host, under the same real-time rules as the
//! engine (no allocation, no lock, no blocking while [`Effect::process`] runs;
//! devices allocate in their constructors, which run on the interface side).
//!
//! The engine hands a device one block at a time: the frames to read and write,
//! the sidechain that keys it when its track has one, and the parameter
//! changes of the block with the frame each lands on. Parameter values are
//! normalised, 0 to 1, from the table the device declares
//! ([`crate::params`]), so automation lanes, overrides and MIDI learn are the
//! same for a built-in as for a hosted plugin.

use lumen_core::pan::PanLaw;

use crate::ParamBlock;
use crate::params::ParamInfo;

/// A device's parameter values as the project holds them: normalised, 0 to 1,
/// by the table's stable id, every parameter the table names (a parameter the
/// project does not is its default).
///
/// [`Description::latency`] and [`Description::tail`] are worked out from
/// these, never from a running instance: a device's timing is a pure function
/// of what its settings say (M8 slice B0, review H1), so changing a setting
/// changes the routing's timing exactly as a plugin's reported latency change
/// does, and an export or a bounce gets the same answer from the project with
/// no device built.
pub type Values<'a> = &'a [(u32, f32)];

/// What a built-in effect is, apart from the audio it makes: its stable saved
/// name, the parameter table the card and the lanes read, and how its latency
/// and tail follow its values.
#[derive(Clone, Copy)]
pub struct Description {
    /// The name a project saves (`PluginRef::Builtin.name`), never changed.
    pub name: &'static str,
    /// Its parameters, in the order its card shows them.
    pub table: &'static [ParamInfo],
    /// Frames the device's output trails its input at `values`, at the rate it
    /// is running. Delay compensation counts it, and it is the length of the
    /// node's fallback line, so it must be the device's own latency for the
    /// same values.
    pub latency: fn(Values<'_>) -> u32,
    /// Frames it rings on after its input stops at `values`.
    pub tail: fn(Values<'_>) -> u32,
}

/// A block's frames, left and right, to read and write.
#[derive(Debug)]
pub struct Stereo<'a> {
    pub left: &'a mut [f32],
    pub right: &'a mut [f32],
}

impl Stereo<'_> {
    /// Frames in the block: both sides are the same length.
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    /// Splits the block at `until`: the frames before it, and those from it on.
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

/// What arrives for a device's sidechain: a compressor's key, the same length
/// as the block.
#[derive(Debug, Clone, Copy)]
pub struct Key<'a> {
    pub left: &'a [f32],
    pub right: &'a [f32],
}

/// One block as a built-in effect sees it.
#[derive(Debug)]
pub struct Block<'a> {
    /// The device's sample rate.
    pub rate: u32,
    /// The timeline sample the block's first frame plays at.
    pub start: i64,
    /// The song's tempo at the block's first frame, in beats per minute (M8
    /// slice B3): what a delay whose time is in notes follows. The mixer reads
    /// it from the timeline map it plays, so it is the tempo the clips follow,
    /// and a tempo change reaches a device with the first block after it (a
    /// block is at most 256 frames, so the change is inaudible for a delay
    /// time). 120.0 for a mixer with no timeline.
    pub bpm: f32,
    /// The pan law of the track the device is on, for a device that pans.
    pub law: PanLaw,
    /// The parameters the block moves, at their frames: the value the project
    /// holds for each (one point on the first frame), and a lane's points
    /// where one plays. Read with [`crate::params::values`].
    pub events: &'a ParamBlock,
}

/// A device that processes stereo audio. Every method except the constructor is
/// real-time safe.
pub trait Effect: Send {
    /// Processes `io` in place: it holds the block's frames going in and the
    /// same frames coming out. `key` is what arrives for the track's sidechain,
    /// when it has one; a device that takes no sidechain ignores it.
    fn process(&mut self, block: &Block<'_>, io: Stereo<'_>, key: Option<Key<'_>>);

    /// Frames the device's output trails its input, at the rate it is running:
    /// delay compensation lines every other path up to it.
    ///
    /// The plan reads this from the device's [`Description`] and the values
    /// the project holds, not from a running instance (M8 slice B0, review
    /// H1), so a setting that changes the latency is a change of the routing's
    /// timing — exactly as a plugin's reported latency change is. A device that
    /// reports a latency here must report the same one from its description for
    /// the same values, or its fallback line and the running instance disagree.
    fn latency(&self) -> u32 {
        0
    }

    /// Frames the device rings on after its input stops (a delay's echoes, a
    /// reverb's tail), at the rate it is running; 0 when it stops with the
    /// input. The engine keeps what it plays after a Stop for this long, and a
    /// bounce and an export add it as the automatic tail. The mixer reads the
    /// running instance's; its [`Description`] must give the same number for
    /// the same values (a test holds every device to it at its defaults).
    fn tail(&self) -> u32 {
        0
    }

    /// Forgets what it holds (its filters, its tail): playback jumps to another
    /// place in the song, and nothing of the old place carries over.
    fn reset(&mut self) {}
}

/// The most frames one block holds: the engine's chunk (`MOST_FRAMES`).
pub const MOST_FRAMES: usize = crate::MOST_FRAMES;

#[cfg(test)]
mod tests {
    use super::*;

    /// A device that only counts: the trait's defaults are quiet.
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
