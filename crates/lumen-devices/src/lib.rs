//! Lumen's built-in devices (ADR-0045). Pure, real-time-safe DSP that
//! depends on `lumen-core` alone: the engine runs it, the app builds it.
//! Instruments first (M5 slice 1); the effects of M8 join them (ADR-0145).
//!
//! Everything here follows the audio thread's rules: no allocation, locks
//! or blocking while rendering. Devices allocate in their constructors,
//! which run on the UI side.

pub mod delay;
pub mod effect;
pub mod hosted;
pub mod instrument;
mod line;
pub mod params;
pub mod reverb;
pub mod synth;
pub mod utility;

pub use effect::{Block, Description, Effect, Key, Stereo, Values};
pub use hosted::{
    Cost, FAILED_AFTER_FRAMES, Guard, Hosted, HostedInstrument, MOST_FRAMES, MOST_POINTS,
    MOST_QUEUES, Mark, Outcome, ParamBlock, Refused, Reply, Status,
};
pub use instrument::Instrument;
pub use params::ParamInfo;
pub use synth::{Preset, Synth};
pub use utility::Utility;

/// Every built-in effect this build knows, in the order they were added.
/// Each one has a personality in the interface (`lumen-app`'s
/// `builtin::identity`), which a test keeps in step with this list.
pub static EFFECTS: &[&Description] = &[
    &utility::DESCRIPTION,
    &delay::DESCRIPTION,
    &reverb::DESCRIPTION,
];

/// The description of the built-in effect called `name`, or `None` when no
/// device has that name: the one table the card, the lanes and the engine's
/// values all read, and the one place its latency and tail are worked out.
pub fn effect_description(name: &str) -> Option<&'static Description> {
    EFFECTS
        .iter()
        .copied()
        .find(|description| description.name == name)
}

/// The parameters of the built-in effect called `name`, or `None` when no
/// device has that name.
pub fn effect_params(name: &str) -> Option<&'static [ParamInfo]> {
    effect_description(name).map(|description| description.table)
}

/// Frames the effect called `name` holds its output back by at `values`, or
/// `None` when no device has that name. A pure function of the values, so the
/// routing and the running device agree and an export needs no device built.
pub fn effect_latency(name: &str, values: Values<'_>) -> Option<u32> {
    effect_description(name).map(|description| (description.latency)(values))
}

/// Frames the effect called `name` rings on after its input stops at
/// `values`, or `None` when no device has that name.
pub fn effect_tail(name: &str, values: Values<'_>) -> Option<u32> {
    effect_description(name).map(|description| (description.tail)(values))
}

/// Frames the effect called `name` rings on after its input stops at `values`,
/// at a tempo of `bpm` and a rate of `rate`, or `None` when no device has that
/// name. A device whose tail is a number of seconds (a delay whose time is in
/// notes follows the tempo) needs both; a device whose tail does not look at
/// them gives what [`effect_tail`] gives.
pub fn effect_tail_at(name: &str, values: Values<'_>, bpm: f32, rate: u32) -> Option<u32> {
    match name {
        delay::NAME => Some(delay::tail_frames(values, bpm, rate)),
        reverb::NAME => Some(reverb::tail_frames(values, rate)),
        _ => effect_tail(name, values)
            // A device that reports frames at the reference rate (48 kHz, the
            // contract `effect_tail` documents) is put on the rate in hand.
            .map(|frames| (u64::from(frames) * u64::from(rate) / 48_000) as u32),
    }
}

/// A built-in effect by the name a project saves. The constructor allocates:
/// call it on the interface side, then hand the device to the audio thread.
pub fn effect(name: &str) -> Option<Box<dyn Effect>> {
    match name {
        utility::NAME => Some(Box::new(Utility::new())),
        delay::NAME => Some(Box::new(delay::Delay::new())),
        reverb::NAME => Some(Box::new(reverb::Reverb::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_effect_is_in_both_tables() {
        let name = utility::NAME;
        assert!(effect_params(name).is_some(), "{name} has a table");
        assert!(effect(name).is_some(), "{name} is built");
        assert!(effect_params("nothing").is_none());
        assert!(effect("nothing").is_none());
    }

    #[test]
    fn a_description_names_the_device_it_describes() {
        assert!(!EFFECTS.is_empty());
        for description in EFFECTS {
            let name = description.name;
            assert_eq!(effect_description(name).map(|d| d.name), Some(name));
            assert_eq!(description.table.len(), effect_params(name).unwrap().len());
            // The default values: what a device nothing has set plays.
            let defaults: Vec<(u32, f32)> = description
                .table
                .iter()
                .map(|p| (p.id, p.normalised(p.default)))
                .collect();
            // A fresh device holds its defaults, and its running instance
            // reports what its description works out for them (re-check N2):
            // the routing reads the one, the mixer's fallback and the tail
            // after Stop the other.
            let fresh = effect(name).unwrap();
            assert_eq!(effect_latency(name, &defaults), Some(fresh.latency()));
            assert_eq!(effect_tail(name, &defaults), Some(fresh.tail()));
        }
        assert_eq!(effect_latency("nothing", &[]), None);
        assert_eq!(effect_tail("nothing", &[]), None);
    }

    /// L6: a device's own table fits the room one block holds, so opening a
    /// queue for each of its parameters cannot fail. (A device's table and the
    /// lanes that drive it share that room; a device with as many parameters as
    /// lanes would not fit — see the ADR.)
    #[test]
    fn every_built_in_s_table_fits_one_block() {
        for description in EFFECTS {
            assert!(
                description.table.len() <= MOST_QUEUES,
                "{} names {} parameters of {MOST_QUEUES}",
                description.name,
                description.table.len()
            );
        }
    }
}
