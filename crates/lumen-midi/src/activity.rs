//! Traffic counters for the activity lights (M5 slice 2d): one counter per
//! input slot and per output, bumped with relaxed atomics (no locks, no
//! allocation) where messages pass; the UI compares them from frame to
//! frame, and a change is a flash.

use std::sync::atomic::{AtomicU32, Ordering};

/// Counters on each side. Input slots stay below the live feed's keyboard
/// source (63).
pub const SLOTS: usize = 64;

/// Whether a message is playing, not housekeeping: note, controller, pitch
/// bend, pressure and program messages count; clock, active sensing and
/// the other system messages do not.
pub fn counts(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|s| (0x80..0xF0).contains(s))
}

pub struct Activity {
    inputs: [AtomicU32; SLOTS],
    outputs: [AtomicU32; SLOTS],
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            inputs: std::array::from_fn(|_| AtomicU32::new(0)),
            outputs: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl Activity {
    /// A message arrived on input `slot`; real-time safe.
    pub fn input(&self, slot: u16, bytes: &[u8]) {
        if counts(bytes)
            && let Some(c) = self.inputs.get(usize::from(slot))
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A message was sent on output `slot`.
    pub fn output(&self, slot: u16, bytes: &[u8]) {
        if counts(bytes)
            && let Some(c) = self.outputs.get(usize::from(slot))
        {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn input_count(&self, slot: u16) -> u32 {
        self.inputs
            .get(usize::from(slot))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    pub fn output_count(&self, slot: u16) -> u32 {
        self.outputs
            .get(usize::from(slot))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_playing_messages_count() {
        let a = Activity::default();
        a.input(2, &[0x90, 60, 100]);
        a.input(2, &[0xE0, 0, 64]);
        a.input(2, &[0xF8]);
        a.input(2, &[0xFE]);
        assert_eq!(a.input_count(2), 2);
        assert_eq!(a.input_count(3), 0);
        a.output(1, &[0xB0, 7, 1]);
        a.output(1, &[0xF8]);
        assert_eq!(a.output_count(1), 1);
        a.input(900, &[0x90, 1, 1]);
    }
}
