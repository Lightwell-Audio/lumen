//! Controller mappings (format v23, M7 slice A5): which control a MIDI
//! controller number moves. A mapping binds a controller number on a channel,
//! from any input, to one value of one track: the owner's N1.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::automation::Target;
use crate::model::Id;

/// The most mappings a project holds.
pub const MAX_MAPPINGS: usize = 1024;

/// A controller number on a channel, and the control it moves. A controller
/// moves at most one control, and a control is moved by at most one
/// controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mapping {
    /// The MIDI channel, 0 to 15.
    pub channel: u8,
    /// The controller number, 0 to 127.
    pub number: u8,
    /// The track whose value it moves (the master's too).
    pub track: Id,
    pub target: Target,
}

impl Mapping {
    /// Whether a message on `channel` for controller `number` is this one's.
    pub fn hears(&self, channel: u8, number: u8) -> bool {
        self.channel == channel && self.number == number
    }

    /// Whether the numbers are in the range MIDI has.
    pub fn is_valid(&self) -> bool {
        self.channel < 16 && self.number < 128
    }
}

/// Checks a project's mappings: in range, few, and no controller or control
/// used twice. A mapping to a track or device that is gone is kept, as a lane
/// is: the list says so and the user clears it.
pub fn check(mappings: &[Mapping]) -> Result<(), String> {
    if mappings.len() > MAX_MAPPINGS {
        return Err(format!("more than {MAX_MAPPINGS} controller mappings"));
    }
    if mappings.iter().any(|m| !m.is_valid()) {
        return Err("a controller mapping is out of MIDI's range".into());
    }
    let mut keys = HashSet::new();
    let mut controls = HashSet::new();
    for m in mappings {
        if !keys.insert((m.channel, m.number)) {
            return Err("two mappings share a controller".into());
        }
        if !controls.insert((m.track, m.target)) {
            return Err("two mappings move the same control".into());
        }
    }
    Ok(())
}

/// `mappings` with `new` added: any mapping on the same controller or the same
/// control is replaced. Returns `None` when the mapping is out of range or the
/// list is full.
pub fn with(mappings: &[Mapping], new: Mapping) -> Option<Vec<Mapping>> {
    if !new.is_valid() {
        return None;
    }
    let mut out: Vec<Mapping> = mappings
        .iter()
        .copied()
        .filter(|m| {
            !m.hears(new.channel, new.number) && (m.track, m.target) != (new.track, new.target)
        })
        .collect();
    if out.len() >= MAX_MAPPINGS {
        return None;
    }
    out.push(new);
    Some(out)
}

/// `mappings` without the one that moves `track`'s `target`.
pub fn without(mappings: &[Mapping], track: Id, target: Target) -> Vec<Mapping> {
    mappings
        .iter()
        .copied()
        .filter(|m| (m.track, m.target) != (track, target))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(channel: u8, number: u8, track: u64, target: Target) -> Mapping {
        Mapping {
            channel,
            number,
            track: Id(track),
            target,
        }
    }

    #[test]
    fn a_new_mapping_replaces_one_on_its_controller_or_its_control() {
        let a = map(0, 7, 1, Target::Volume);
        let b = map(0, 10, 1, Target::Pan);
        let list = with(&with(&[], a).unwrap(), b).unwrap();
        assert_eq!(list, vec![a, b]);
        // Learning controller 7 for the pan moves it off the fader and drops
        // the pan's old controller.
        let c = map(0, 7, 1, Target::Pan);
        assert_eq!(with(&list, c).unwrap(), vec![c]);
        // The same channel and number on another channel is another controller.
        let d = map(1, 7, 1, Target::Volume);
        assert_eq!(with(&list, d).unwrap().len(), 2);
    }

    #[test]
    fn out_of_range_numbers_are_refused() {
        assert!(with(&[], map(16, 0, 1, Target::Volume)).is_none());
        assert!(with(&[], map(0, 128, 1, Target::Volume)).is_none());
        assert!(check(&[map(0, 200, 1, Target::Volume)]).is_err());
    }

    #[test]
    fn check_refuses_duplicates_and_too_many() {
        let a = map(0, 7, 1, Target::Volume);
        assert!(check(&[a, map(0, 7, 2, Target::Volume)]).is_err());
        assert!(check(&[a, map(0, 8, 1, Target::Volume)]).is_err());
        assert!(check(&[a, map(0, 8, 1, Target::Pan)]).is_ok());
        let many: Vec<Mapping> = (0..=MAX_MAPPINGS as u64)
            .map(|i| map((i % 16) as u8, (i / 16 % 128) as u8, i, Target::Volume))
            .collect();
        assert!(check(&many).is_err());
    }

    #[test]
    fn without_removes_one_control() {
        let a = map(0, 7, 1, Target::Volume);
        let b = map(0, 8, 1, Target::Pan);
        assert_eq!(without(&[a, b], Id(1), Target::Volume), vec![b]);
        assert_eq!(without(&[a, b], Id(2), Target::Volume), vec![a, b]);
    }
}
