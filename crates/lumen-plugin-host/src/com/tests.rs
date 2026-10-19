//! The COM objects' tests.

use super::*;

fn edit(id: u32, value: f64, kind: EditKind) -> Edit {
    Edit { id, value, kind }
}

#[test]
fn a_plugin_saying_it_changed_inside_is_told_once_and_other_news_is_not() {
    let h = Handler::default();
    let dirty = vec![edit(0, 0.0, EditKind::Dirty)];
    // SAFETY: plain calls on an object of this process.
    unsafe {
        // Saved by the plugin itself: not a change.
        h.setDirty(0);
        // A latency change is for the host, not the project.
        h.restartComponent(1 << 3);
        assert!(h.take_reports().0.is_empty());
        // A change, said twice before anyone asks, is one.
        h.setDirty(1);
        h.setDirty(1);
        assert_eq!(h.take_reports().0, dirty, "setDirty");
        // A patch loaded (its values changed), and a reload.
        h.restartComponent(1 << 2);
        assert_eq!(h.take_reports().0, dirty, "values changed");
        h.restartComponent(1);
        assert_eq!(h.take_reports().0, dirty, "reload");
    }
    // Every restart is still kept for the host to act on.
    assert_eq!(h.restart.load(Ordering::Acquire), (1 << 3) | (1 << 2) | 1);
}

#[test]
fn values_of_one_gesture_are_merged_but_its_marks_are_kept() {
    let h = Handler::default();
    h.report(edit(1, 0.0, EditKind::Begin));
    for v in [0.1, 0.2, 0.3] {
        h.report(edit(1, v, EditKind::Perform));
    }
    h.report(edit(1, 0.0, EditKind::End));
    // A second gesture on the same parameter is not merged into the first.
    h.report(edit(1, 0.0, EditKind::Begin));
    h.report(edit(1, 0.9, EditKind::Perform));
    h.report(edit(2, 0.4, EditKind::Perform));
    h.report(edit(2, 0.5, EditKind::Perform));
    let (list, dropped) = h.take_reports();
    assert_eq!(dropped, 0);
    assert_eq!(
        list,
        vec![
            edit(1, 0.0, EditKind::Begin),
            edit(1, 0.3, EditKind::Perform),
            edit(1, 0.0, EditKind::End),
            edit(1, 0.0, EditKind::Begin),
            edit(1, 0.9, EditKind::Perform),
            edit(2, 0.5, EditKind::Perform),
        ]
    );
    assert!(h.take_reports().0.is_empty());
}

#[test]
fn a_plugin_that_floods_edits_is_bounded_and_the_loss_is_counted() {
    let h = Handler::default();
    for i in 0..(MAX_EDITS as u32 + 10) {
        // Alternating parameters, so nothing merges.
        h.report(edit(i % 2, 0.5, EditKind::Begin));
    }
    let (list, dropped) = h.take_reports();
    assert_eq!(list.len(), MAX_EDITS);
    assert_eq!(dropped, 10);
    assert_eq!(h.take_reports().1, 0, "the count starts again");
}

#[test]
fn the_processor_is_given_the_latest_value_of_each_parameter() {
    let h = Handler::default();
    h.send_to_processor(1, 0.1);
    h.send_to_processor(2, 0.2);
    h.send_to_processor(1, 0.7);
    let mut now = Vec::new();
    h.drain_to_processor(&mut now);
    assert_eq!(now, vec![(1, 0.7), (2, 0.2)]);
    now.clear();
    h.drain_to_processor(&mut now);
    assert!(now.is_empty());
    // Distinct parameters past the bound are not kept.
    for id in 0..(MOST_PENDING as u32 + 5) {
        h.send_to_processor(id, 0.5);
    }
    h.drain_to_processor(&mut now);
    assert_eq!(now.len(), MOST_PENDING);
}

#[test]
fn a_value_that_is_not_a_number_is_never_kept() {
    let h = Handler::default();
    // SAFETY: plain calls on a live object.
    unsafe { h.performEdit(3, f64::NAN) };
    assert!(h.take_reports().0.is_empty());
    let mut now = Vec::new();
    h.drain_to_processor(&mut now);
    assert!(now.is_empty());
}
