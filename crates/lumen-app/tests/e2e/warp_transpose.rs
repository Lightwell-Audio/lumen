//! The Transpose row of the Warp popover (M9.5 S6a, ADR-0137). A warped clip in
//! a stretch mode takes a transpose in semitones, typed or dragged; each change
//! is one undo step. Repitch and Rhythmic say why the row is off. The audio is
//! tested in `lumen-media` (the partials move, the bits repeat); here the
//! project and the screen are checked.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_project::model::{Id, WarpMode};

use super::audio_clip_properties::type_into;
use super::warp_chip::{close_chip, open_chip, opened_click_track, warped};
use super::{Lumen, access};

fn transpose(lumen: &Lumen, id: Id) -> i32 {
    warped(lumen, id).warp.expect("warped").transpose_cents
}

fn shows(lumen: &Lumen, label: &str) -> bool {
    lumen
        .harness
        .query_all_by_label_contains(label)
        .next()
        .is_some()
}

/// The mode dropdown in the popover (named "Warp mode: " and the mode it
/// shows), opened, then the mode in its list.
fn choose_mode(lumen: &mut Lumen, mode: &str) {
    lumen.harness.get_by_label_contains("Warp mode: ").click();
    lumen.frames(3);
    lumen.click(mode);
}

#[test]
fn a_transpose_is_typed_in_semitones_and_each_change_is_one_undo_step() {
    let (mut lumen, id) = opened_click_track("warp-transpose");
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    assert!(
        shows(&lumen, "Set by tempo in Repitch"),
        "Repitch says the row is set by the tempo"
    );

    choose_mode(&mut lumen, "Melodic");
    assert_eq!(warped(&lumen, id).warp.unwrap().mode, WarpMode::Melodic);
    type_into(&mut lumen, "Transpose: ", "2");
    assert_eq!(transpose(&lumen, id), 200, "two semitones up");
    assert!(shows(&lumen, "Transpose: +2 st"), "the field says it");

    type_into(&mut lumen, "Transpose: ", "-1.5");
    assert_eq!(
        transpose(&lumen, id),
        -150,
        "a half semitone is kept in cents"
    );
    assert!(shows(&lumen, "Transpose: \u{2212}1.5 st"));
    assert_eq!(lumen.app().undo_label(), Some("Transpose clips"));

    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        transpose(&lumen, id),
        200,
        "one undo takes back the last typing"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(transpose(&lumen, id), 0, "and the one before it");
}

#[test]
fn rhythmic_says_it_is_not_there_yet_and_repitch_takes_the_transpose_away() {
    let (mut lumen, id) = opened_click_track("warp-transpose-modes");
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    choose_mode(&mut lumen, "Full mix");
    type_into(&mut lumen, "Transpose: ", "5");
    assert_eq!(transpose(&lumen, id), 500);

    choose_mode(&mut lumen, "Rhythmic");
    assert!(
        shows(&lumen, "Not in Rhythmic yet"),
        "Rhythmic says why the row is off"
    );
    assert_eq!(transpose(&lumen, id), 0, "Rhythmic holds no transpose");

    choose_mode(&mut lumen, "Repitch");
    assert!(shows(&lumen, "Set by tempo in Repitch"));
    assert_eq!(transpose(&lumen, id), 0);
    close_chip(&mut lumen);
    assert!(
        access::focused(&lumen).is_none(),
        "closing the popover leaves no field focused"
    );
}

/// The Warp popover with a transpose set, rendered for a look by eye:
/// `SHOTS=<folder> LOOK_SCALE=1.5 cargo test -p lumen-app --test e2e
/// look_at_the_transpose_row -- --ignored`.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn look_at_the_transpose_row() {
    let (mut lumen, _) = opened_click_track("warp-transpose-look");
    let scale: f32 = std::env::var("LOOK_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    lumen.harness.set_pixels_per_point(scale);
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    choose_mode(&mut lumen, "Melodic");
    type_into(&mut lumen, "Transpose: ", "2.5");
    lumen.frames(4);
    let image = lumen.harness.render().expect("render");
    let dir = std::path::PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
    image
        .save(dir.join(format!("transpose-dark-{}.png", (scale * 100.0) as u32)))
        .expect("save");
}
