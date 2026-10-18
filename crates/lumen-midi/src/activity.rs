//! Traffic counters for the activity lights (M5 slice 2d): one counter per
//! input slot and per output, bumped with relaxed atomics (no locks, no
//! allocation) where messages pass; the UI compares them from frame to
//! frame, and a change is a flash.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
    /// The keys held down on each input slot, one bit a key (any channel):
    /// a light stays lit for as long as a key is down.
    held: [[AtomicU64; 2]; SLOTS],
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            inputs: std::array::from_fn(|_| AtomicU32::new(0)),
            outputs: std::array::from_fn(|_| AtomicU32::new(0)),
            held: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
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
        self.track_held(slot, bytes);
    }

    /// Keeps the keys held on `slot`: a note-on sets its key, a note-off
    /// (or a note-on of velocity 0) clears it, All Notes Off and All Sound
    /// Off clear them all.
    fn track_held(&self, slot: u16, bytes: &[u8]) {
        let Some(words) = self.held.get(usize::from(slot)) else {
            return;
        };
        let [status, first, rest @ ..] = bytes else {
            return;
        };
        let (word, bit) = (usize::from(*first >> 6 & 1), 1_u64 << (first & 63));
        match status & 0xF0 {
            0x90 if rest.first().is_some_and(|v| *v > 0) => {
                words[word].fetch_or(bit, Ordering::Relaxed);
            }
            0x80 | 0x90 => {
                words[word].fetch_and(!bit, Ordering::Relaxed);
            }
            0xB0 if matches!(first, 120 | 123) => self.release(slot),
            _ => {}
        }
    }

    /// Lets go of every key held on input `slot` (its port closed).
    pub fn release(&self, slot: u16) {
        if let Some(words) = self.held.get(usize::from(slot)) {
            for word in words {
                word.store(0, Ordering::Relaxed);
            }
        }
    }

    /// Whether a key is held down on input `slot`.
    pub fn input_held(&self, slot: u16) -> bool {
        self.held
            .get(usize::from(slot))
            .is_some_and(|w| w.iter().any(|w| w.load(Ordering::Relaxed) != 0))
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

    #[test]
    fn a_key_is_held_from_its_note_on_to_its_note_off() {
        let a = Activity::default();
        assert!(!a.input_held(2));
        a.input(2, &[0x90, 60, 100]);
        a.input(2, &[0x90, 100, 90]);
        assert!(a.input_held(2));
        assert!(!a.input_held(3));
        a.input(2, &[0x80, 60, 0]);
        assert!(a.input_held(2), "key 100 is still down");
        // A note-on of velocity 0 is a note-off.
        a.input(2, &[0x91, 100, 0]);
        assert!(!a.input_held(2));
        // Controllers and a stray note-off change nothing.
        a.input(2, &[0xB0, 64, 127]);
        a.input(2, &[0x80, 10, 0]);
        assert!(!a.input_held(2));
    }

    #[test]
    fn all_notes_off_and_closing_let_go() {
        let a = Activity::default();
        a.input(1, &[0x90, 5, 1]);
        a.input(1, &[0x90, 70, 1]);
        a.input(1, &[0xB0, 123, 0]);
        assert!(!a.input_held(1));
        a.input(1, &[0x90, 5, 1]);
        a.release(1);
        assert!(!a.input_held(1));
    }
}
