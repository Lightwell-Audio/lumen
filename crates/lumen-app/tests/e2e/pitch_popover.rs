//! The Pitch chip's popover (M9.5 S7): the chip and the popover hanging under
//! it keep their place while Transpose is dragged (F1); a pitch envelope the
//! clip carries is shown even while it cannot be heard, and can be removed in
//! one step (F2); and Repitch says why the pitch does not change and offers
//! Melodic (F3).

use eframe::egui::{Key, Modifiers, Pos2, Rect, accesskit};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::model::{Id, WarpMode};

use super::Lumen;
use super::audio_clip_properties::type_into;
use super::audio_editor_handles::{clip, shown, x_of};
use super::warp_chip::{close_chip, open_chip, opened_click_track};

/// A warped clip in `mode`, opened in the Clip tab, with every popover shut.
fn warped_in(name: &str, mode: &str) -> (Lumen, Id) {
    let (mut lumen, id) = opened_click_track(name);
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    lumen.harness.get_by_label_contains("Warp mode: ").click();
    lumen.frames(3);
    lumen.click(mode);
    close_chip(&mut lumen);
    (lumen, id)
}

/// The Pitch chip, opened.
fn open_pitch(lumen: &mut Lumen) {
    lumen
        .harness
        .query_all_by_label_contains("Pitch: ")
        .next()
        .expect("the Pitch chip is in the strip")
        .click();
    lumen.frames(4);
}

fn shows(lumen: &Lumen, label: &str) -> bool {
    lumen
        .harness
        .query_all_by_label_contains(label)
        .next()
        .is_some()
}

/// The chip's place on screen, as a screen reader reads it.
fn chip_rect(lumen: &Lumen) -> Rect {
    lumen
        .harness
        .query_all_by_label_contains("Pitch: ")
        .next()
        .expect("the Pitch chip is in the strip")
        .rect()
}

/// The Transpose field: its left edge is the popover's, moved.
fn transpose_rect(lumen: &Lumen) -> Rect {
    lumen.harness.get_by_label_contains("Transpose: ").rect()
}

fn transpose_disabled(lumen: &Lumen) -> bool {
    lumen
        .harness
        .get_by_label_contains("Transpose: ")
        .accesskit_node()
        .is_disabled()
}

/// The popover's Pitch envelope switch: the header's pill has the same name,
/// and is a button.
fn press_envelope_switch(lumen: &mut Lumen) {
    let switch = lumen
        .harness
        .get_all_by_label("Pitch envelope")
        .find(|node| node.accesskit_node().role() == accesskit::Role::CheckBox)
        .expect("the popover's Pitch envelope switch");
    switch.click();
    lumen.frames(3);
}

fn transposed(lumen: &Lumen, id: Id) -> i32 {
    clip(lumen, id).warp.expect("warped").transpose_cents
}

fn mode(lumen: &Lumen, id: Id) -> WarpMode {
    clip(lumen, id).warp.expect("warped").mode
}

/// F1 (the owner's report): the chip changed width and the popover shook
/// horizontally while Transpose was dragged.
#[test]
fn the_chip_and_its_popover_stay_still_while_the_transpose_is_dragged() {
    let (mut lumen, id) = warped_in("pitch-still", "Melodic");
    open_pitch(&mut lumen);
    lumen.frames(30);
    let chip = chip_rect(&lumen);
    let field = transpose_rect(&lumen);
    assert_eq!(transposed(&lumen, id), 0);

    type_into(&mut lumen, "Transpose: ", "-5.24");
    lumen.frames(30);
    assert_eq!(transposed(&lumen, id), -524);
    assert_eq!(chip_rect(&lumen), chip, "the chip did not move or resize");
    assert_eq!(
        transpose_rect(&lumen).left(),
        field.left(),
        "nor did the popover"
    );

    type_into(&mut lumen, "Transpose: ", "2");
    lumen.frames(30);
    assert_eq!(transposed(&lumen, id), 200);
    assert_eq!(chip_rect(&lumen), chip, "nor at another transpose");
    assert_eq!(transpose_rect(&lumen).left(), field.left());
    // Only the words it says change.
    assert!(
        shows(&lumen, "Pitch: transposed +2 st"),
        "the chip says what is set"
    );
}

/// F2 (the owner's report): with warp off a clip kept a pitch envelope point,
/// and the popover said nothing about it.
#[test]
fn an_envelope_is_shown_and_removed_in_one_step_even_when_it_is_not_heard() {
    let (mut lumen, id) = opened_click_track("pitch-envelope-row");
    assert!(clip(&lumen, id).warp.is_none(), "the clip is not warped");
    open_pitch(&mut lumen);
    press_envelope_switch(&mut lumen);
    close_chip(&mut lumen);

    // A click high on the wave adds a point an octave or so up.
    let body = shown(&lumen).body;
    let at = Pos2::new(x_of(&lumen, 2.0), body.top() + body.height() * 0.25);
    lumen.harness.hover_at(at);
    lumen.frames(2);
    lumen.click_at(at, Modifiers::NONE);
    let lane = clip(&lumen, id).envelopes.pitch.expect("a point was added");
    assert_eq!(lane.len(), 1, "{lane:?}");

    open_pitch(&mut lumen);
    lumen.frames(30);
    assert!(shows(&lumen, "Envelope"), "the row says there is one");
    assert!(
        shows(
            &lumen,
            "Not heard until the clip is warped in a mode that changes pitch"
        ),
        "and that it is not heard yet"
    );
    lumen.click("Remove envelope");
    lumen.frames(30);
    assert!(
        clip(&lumen, id).envelopes.pitch.is_none(),
        "Remove envelope took it away"
    );
    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(
        clip(&lumen, id).envelopes.pitch.is_some(),
        "one undo brings it back"
    );
}

/// F3 (the owner's report): in Repitch the popover did nothing and did not
/// say why.
#[test]
fn repitch_says_why_and_offers_melodic_in_one_step() {
    let (mut lumen, id) = warped_in("pitch-repitch", "Repitch");
    assert_eq!(mode(&lumen, id), WarpMode::Repitch);
    open_pitch(&mut lumen);
    lumen.frames(30);
    assert!(
        shows(&lumen, "Repitch changes speed and pitch together"),
        "the notice comes first"
    );
    assert!(shows(&lumen, "Use Melodic"), "and the way out");
    assert!(transpose_disabled(&lumen), "Transpose waits");

    lumen.click("Use Melodic");
    lumen.frames(30);
    assert_eq!(mode(&lumen, id), WarpMode::Melodic);
    assert!(!transpose_disabled(&lumen), "Transpose is live now");
    assert!(
        !shows(&lumen, "Repitch changes speed and pitch together"),
        "the notice went"
    );

    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        mode(&lumen, id),
        WarpMode::Repitch,
        "one undo takes it back"
    );
}
