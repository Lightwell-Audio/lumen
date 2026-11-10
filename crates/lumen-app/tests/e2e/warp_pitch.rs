//! Pitch (M9.5 S6, ADR-0137). The audio editor's Pitch chip sets a warped
//! clip's transpose in semitones and cents and says what is set; the Pitch pill
//! shows the pitch envelope over the wave, where a click adds a point; Transpose
//! clip up and down move the selected clips by a semitone; the arrangement's
//! clip carries the transpose as a small "+2" and says so on hover; and the
//! chip is a page of the ⋯ menu when the strip is too narrow. Repitch and an
//! unwarped clip say why the pitch cannot change. What is heard is tested
//! where the audio is made (`lumen-media`, `media::warped`).

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_project::envelope::Kind;
use lumen_project::model::{Id, WarpMode};

use super::Lumen;
use super::audio_clip_properties::type_into;
use super::audio_editor_handles::{clip, shown, x_of};
use super::clip_commands::command;
use super::properties::rect;
use super::warp_chip::{close_chip, open_chip, opened_click_track, warped};

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

/// A warped clip in `mode`, opened in the Clip tab, with every popover closed.
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

/// Beat `q` on the wave, at a quarter of its height from the top.
fn high(lumen: &Lumen, q: f64) -> Pos2 {
    let body = shown(lumen).body;
    Pos2::new(x_of(lumen, q), body.top() + body.height() * 0.25)
}

fn hover_header(lumen: &mut Lumen, id: Id) -> String {
    let r = rect(lumen, id);
    lumen
        .harness
        .hover_at(Pos2::new(r.left() + 12.0, r.top() + 6.0));
    lumen.frames(4);
    lumen.app().info_line().unwrap_or_default()
}

#[test]
fn the_pitch_chip_sets_the_transpose_and_says_what_is_set() {
    let (mut lumen, id) = warped_in("pitch-chip", "Melodic");
    assert_eq!(warped(&lumen, id).warp.unwrap().mode, WarpMode::Melodic);
    assert!(shows(&lumen, "Pitch: not changed"), "nothing is set yet");

    open_pitch(&mut lumen);
    type_into(&mut lumen, "Transpose: ", "2");
    assert_eq!(transpose(&lumen, id), 200, "two semitones up");
    assert!(shows(&lumen, "Transpose: +2 st"), "the field says it");
    type_into(&mut lumen, "Transpose: ", "-1.5");
    assert_eq!(
        transpose(&lumen, id),
        -150,
        "a half semitone is kept in cents"
    );
    assert_eq!(lumen.app().undo_label(), Some("Transpose clips"));
    close_chip(&mut lumen);
    assert!(
        shows(&lumen, "Pitch: transposed \u{2212}1.5 st"),
        "the chip says what is set"
    );

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
fn repitch_and_an_unwarped_clip_say_why_the_pitch_does_not_change() {
    let (mut lumen, id) = opened_click_track("pitch-silent");
    open_pitch(&mut lumen);
    assert!(
        shows(&lumen, "Warp the clip first"),
        "an unwarped clip says to warp it"
    );
    close_chip(&mut lumen);

    open_chip(&mut lumen);
    lumen.click("Warp clip");
    close_chip(&mut lumen);
    open_pitch(&mut lumen);
    assert!(shows(&lumen, "Set by tempo in Repitch"), "Repitch says why");
    close_chip(&mut lumen);

    // A transpose set in Melodic goes when the clip is set to Repitch.
    open_chip(&mut lumen);
    lumen.harness.get_by_label_contains("Warp mode: ").click();
    lumen.frames(3);
    lumen.click("Full mix");
    close_chip(&mut lumen);
    open_pitch(&mut lumen);
    type_into(&mut lumen, "Transpose: ", "5");
    assert_eq!(transpose(&lumen, id), 500);
    close_chip(&mut lumen);
    open_chip(&mut lumen);
    lumen.harness.get_by_label_contains("Warp mode: ").click();
    lumen.frames(3);
    lumen.click("Repitch");
    assert_eq!(transpose(&lumen, id), 0, "Repitch holds no transpose");
}

#[test]
fn the_pitch_pill_shows_the_envelope_and_a_click_adds_a_point_in_semitones() {
    let (mut lumen, id) = warped_in("pitch-envelope", "Full mix");
    assert_eq!(lumen.app().audio_editor_envelope(), None);
    lumen.click("Pitch envelope");
    assert_eq!(lumen.app().audio_editor_envelope(), Some("pitch"));
    lumen.harness.hover_at(high(&lumen, 2.0));
    lumen.frames(2);
    lumen.click_at(high(&lumen, 2.0), Modifiers::NONE);
    let lane = clip(&lumen, id)
        .envelopes
        .get(Kind::Pitch)
        .cloned()
        .expect("a point was added");
    assert_eq!(lane.len(), 1, "{lane:?}");
    // The lane shows two octaves either way, so a click high on the wave is an
    // octave or so up, and inside the range.
    let value = lane.points()[0].value;
    assert!((9.0..=20.0).contains(&value), "{value} semitones");
    assert!(
        shows(&lumen, "Pitch: with a pitch envelope"),
        "the chip says there is an envelope"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        clip(&lumen, id).envelopes.get(Kind::Pitch).is_none(),
        "one undo step"
    );
}

#[test]
fn transpose_clip_up_and_down_move_the_selected_warped_clip_by_a_semitone() {
    let (mut lumen, id) = warped_in("pitch-commands", "Melodic");
    lumen.app_mut().select_clip(id);
    lumen.frames(3);
    command(&mut lumen, "Transpose clip up a semitone");
    command(&mut lumen, "Transpose clip up a semitone");
    assert_eq!(transpose(&lumen, id), 200);
    command(&mut lumen, "Transpose clip down a semitone");
    assert_eq!(transpose(&lumen, id), 100);
    assert_eq!(lumen.app().undo_label(), Some("Transpose clips"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(transpose(&lumen, id), 200, "each press is one undo step");
}

#[test]
fn the_arrangements_clip_says_it_is_transposed_on_hover() {
    let (mut lumen, id) = warped_in("pitch-mark", "Melodic");
    assert!(!hover_header(&mut lumen, id).contains("Transposed"));
    open_pitch(&mut lumen);
    type_into(&mut lumen, "Transpose: ", "2");
    close_chip(&mut lumen);
    let line = hover_header(&mut lumen, id);
    assert!(line.contains("Transposed +2 st"), "{line}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!hover_header(&mut lumen, id).contains("Transposed"));
}

#[test]
fn at_the_narrowest_window_pitch_is_a_page_of_the_more_menu() {
    let (mut lumen, id) = warped_in("pitch-folded", "Melodic");
    lumen.harness.set_size(vec2(480.0, 540.0));
    lumen.app_mut().set_browser_size(220.0);
    lumen.frames(20);
    assert_eq!(shown(&lumen).strip, "fold fades", "the narrowest window");
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Pitch: ")
            .next()
            .is_none(),
        "the chip is folded away"
    );
    lumen.click("More clip properties");
    lumen.click("Pitch\u{2026}");
    type_into(&mut lumen, "Transpose: ", "3");
    assert_eq!(transpose(&lumen, id), 300, "the page edits the clip");
    lumen.click("Back");
    assert!(shows(&lumen, "Fades\u{2026}"), "back at the menu");
}

#[test]
fn the_strip_folds_pitch_after_warp_and_before_the_fades() {
    const ORDER: [&str; 7] = [
        "full",
        "icons",
        "dot",
        "fold processing",
        "fold warp",
        "fold pitch",
        "fold fades",
    ];
    let (mut lumen, _) = warped_in("pitch-fold-order", "Melodic");
    let mut seen: Vec<&'static str> = Vec::new();
    for width in (480..=1800).rev().step_by(10) {
        lumen.harness.set_size(vec2(width as f32, 540.0));
        lumen.app_mut().set_browser_size(220.0);
        lumen.frames(4);
        let layout = shown(&lumen).strip;
        if seen.last() != Some(&layout) {
            seen.push(layout);
        }
    }
    let rank = |name: &str| ORDER.iter().position(|l| *l == name).expect(name);
    assert!(
        seen.windows(2).all(|pair| rank(pair[0]) < rank(pair[1])),
        "narrowing steps through the layouts in order: {seen:?}"
    );
    assert_eq!(seen.last(), Some(&"fold fades"), "{seen:?}");
    assert!(
        seen.contains(&"fold pitch"),
        "Pitch folds in a layout of its own: {seen:?}"
    );
}

fn save_shot(lumen: &mut Lumen, name: &str, scale: f32) {
    let image = lumen.harness.render().expect("render");
    let dir = std::path::PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
    image
        .save(dir.join(format!("{name}-{}.png", (scale * 100.0) as u32)))
        .expect("save");
}

/// The Pitch chip, its popover, the envelope and the transpose mark, rendered
/// for a look by eye: `SHOTS=<folder> LOOK_SCALE=1.5 cargo test -p lumen-app
/// --test e2e look_at_the_pitch -- --ignored` (the theme is the process's).
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn look_at_the_pitch() {
    let (mut lumen, _) = warped_in("pitch-look", "Melodic");
    let scale: f32 = std::env::var("LOOK_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    lumen.harness.set_pixels_per_point(scale);
    lumen.frames(4);
    open_pitch(&mut lumen);
    type_into(&mut lumen, "Transpose: ", "2.5");
    close_chip(&mut lumen);
    lumen.click("Pitch envelope");
    lumen.click_at(high(&lumen, 1.0), Modifiers::NONE);
    lumen.click_at(high(&lumen, 3.0), Modifiers::NONE);
    lumen.frames(4);
    save_shot(&mut lumen, "pitch-strip", scale);
    open_pitch(&mut lumen);
    lumen.frames(4);
    save_shot(&mut lumen, "pitch-popover", scale);
}
