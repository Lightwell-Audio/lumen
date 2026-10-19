//! The instance's tests.

use super::*;

#[test]
fn a_note_off_carries_the_id_its_note_on_was_given() {
    let mut ids = NoteIds::new();
    let a = ids.on(0, 60);
    let b = ids.on(0, 64);
    assert_ne!(a, b);
    assert_eq!(ids.off(0, 64), b);
    assert_eq!(ids.off(0, 60), a);
    // Ended, or never started: not known.
    assert_eq!(ids.off(0, 60), -1);
    assert_eq!(ids.off(3, 10), -1);
    // Out of range is allowed, and has no id to find again.
    let wild = ids.on(40, 300);
    assert!(wild >= 0);
    assert_eq!(ids.off(40, 300), -1);
    ids.on(1, 1);
    ids.clear();
    assert_eq!(ids.off(1, 1), -1);
}

#[test]
fn controller_events_past_the_room_are_dropped_and_counted() {
    let mut mapped: Vec<u32> = (0..600).collect();
    assert_eq!(keep_most(&mut mapped, 512), 88);
    assert_eq!(mapped.len(), 512);
    assert_eq!(mapped[511], 511, "the first ones are the ones kept");
    assert_eq!(keep_most(&mut mapped, 512), 0);
}
