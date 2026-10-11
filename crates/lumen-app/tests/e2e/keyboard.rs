//! The computer keyboard as a MIDI keyboard (M5 slice 2, the owner's K A):
//! Ctrl+Shift+K turns it on, the letters play notes and are taken before
//! the commands, Esc leaves, and nothing it plays is left sounding. These
//! look at the messages the mode sent (`testing`), not at what is heard:
//! which tracks listen is another part's.

use eframe::egui::{Event, Key, Modifiers};
use lumen_core::midi::Msg;

use super::Lumen;

const BUTTON: &str = "Computer keyboard plays notes";

fn on(lumen: &mut Lumen) {
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
}

/// A key goes down and stays down (`repeat` marks an auto-repeat).
fn down(lumen: &mut Lumen, key: Key, repeat: bool) {
    lumen.harness.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(2);
}

fn up(lumen: &mut Lumen, key: Key) {
    lumen.harness.event(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(2);
}

fn readout(lumen: &Lumen, text: &str) -> bool {
    lumen.shows(&format!("Computer keyboard: {text}"))
}

#[test]
fn the_shortcut_turns_the_mode_on_and_the_button_lights() {
    let mut lumen = Lumen::start("keyboard-on");
    assert!(!lumen.is_on(BUTTON), "off at the start");
    assert!(!lumen.app().computer_keyboard_on());
    on(&mut lumen);
    assert!(lumen.app().computer_keyboard_on());
    assert!(lumen.is_on(BUTTON), "the button reads lit");
    assert!(readout(&lumen, "C3\u{2013}C4 \u{00B7} vel 100"));
    lumen.frames(3);
    assert!(
        lumen.shows(
            "Info: A\u{2013}K notes \u{00B7} Z X octave \u{00B7} C V velocity \u{00B7} Esc leave"
        ),
        "the info line teaches the letters"
    );
    // The button toggles it too.
    lumen.click(BUTTON);
    assert!(!lumen.app().computer_keyboard_on());
    assert!(!lumen.is_on(BUTTON));
    assert!(!readout(&lumen, "C3\u{2013}C4 \u{00B7} vel 100"));
    lumen.click(BUTTON);
    assert!(lumen.app().computer_keyboard_on());
}

#[test]
fn a_letter_sends_a_note_on_and_its_release_the_note_off_and_repeats_are_ignored() {
    let mut lumen = Lumen::start("keyboard-notes");
    on(&mut lumen);
    lumen.app_mut().computer_keyboard_sent();
    down(&mut lumen, Key::A, false);
    down(&mut lumen, Key::A, true);
    down(&mut lumen, Key::A, true);
    assert_eq!(lumen.app().computer_keyboard_held(), [60]);
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_on(0, 60, 100)],
        "the auto-repeats sent nothing"
    );
    down(&mut lumen, Key::U, false);
    assert_eq!(lumen.app().computer_keyboard_held(), [60, 70]);
    up(&mut lumen, Key::A);
    up(&mut lumen, Key::U);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [
            Msg::note_on(0, 70, 100),
            Msg::note_off(0, 60),
            Msg::note_off(0, 70)
        ]
    );
}

#[test]
fn z_x_c_v_change_the_octave_and_velocity_and_a_held_note_ends_where_it_began() {
    let mut lumen = Lumen::start("keyboard-octave");
    on(&mut lumen);
    down(&mut lumen, Key::A, false);
    lumen.key(Modifiers::NONE, Key::X);
    assert!(readout(&lumen, "C4\u{2013}C5 \u{00B7} vel 100"));
    lumen.key(Modifiers::NONE, Key::Z);
    lumen.key(Modifiers::NONE, Key::Z);
    lumen.key(Modifiers::NONE, Key::Z);
    assert!(readout(&lumen, "C1\u{2013}C2 \u{00B7} vel 100"));
    lumen.key(Modifiers::NONE, Key::V);
    assert!(readout(&lumen, "C1\u{2013}C2 \u{00B7} vel 120"));
    lumen.key(Modifiers::NONE, Key::C);
    lumen.key(Modifiers::NONE, Key::C);
    assert!(readout(&lumen, "C1\u{2013}C2 \u{00B7} vel 80"));
    lumen.app_mut().computer_keyboard_sent();
    up(&mut lumen, Key::A);
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_off(0, 60)],
        "A began C3 and ends it, whatever the octave is now"
    );
    down(&mut lumen, Key::A, false);
    assert_eq!(lumen.app().computer_keyboard_held(), [36]);
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_on(0, 36, 80)]
    );
}

#[test]
fn the_letters_belong_to_the_mode_while_it_is_on_and_to_commands_after_esc() {
    let mut lumen = Lumen::start("keyboard-letters");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(30);
    let ctx = lumen.harness.ctx.clone();
    let before = lumen.app().tracks_height(&ctx);
    on(&mut lumen);
    // H is "Fit tracks to height"; with the mode on it is the note A.
    lumen.key(Modifiers::NONE, Key::H);
    lumen.frames(30);
    assert!((lumen.app().tracks_height(&ctx) - before).abs() < 1.0);
    // Ctrl chords still run commands (Ctrl+T adds a track).
    lumen.key(Modifiers::COMMAND, Key::T);
    assert_eq!(lumen.track_names().len(), 3);
    // Esc leaves the mode, and H is its command again.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().computer_keyboard_on());
    assert!(!lumen.is_on(BUTTON));
    lumen.frames(30);
    let before = lumen.app().tracks_height(&ctx);
    lumen.key(Modifiers::NONE, Key::H);
    lumen.frames(30);
    assert!(lumen.app().tracks_height(&ctx) > before * 1.2, "H fits");
}

#[test]
fn typing_in_a_field_plays_no_notes() {
    let mut lumen = Lumen::start("keyboard-typing");
    lumen.key(Modifiers::COMMAND, Key::T);
    on(&mut lumen);
    lumen.app_mut().computer_keyboard_sent();
    lumen.key(Modifiers::COMMAND, Key::R);
    down(&mut lumen, Key::A, false);
    lumen.harness.event(Event::Text("a".into()));
    up(&mut lumen, Key::A);
    lumen.key(Modifiers::NONE, Key::S);
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert!(lumen.app_mut().computer_keyboard_sent().is_empty());
    assert!(
        lumen.app().computer_keyboard_on(),
        "Esc after typing is the field's"
    );
}

#[test]
fn turning_it_off_panic_and_losing_focus_let_go_of_held_notes() {
    let mut lumen = Lumen::start("keyboard-release");
    on(&mut lumen);
    down(&mut lumen, Key::A, false);
    down(&mut lumen, Key::D, false);
    assert_eq!(lumen.app().computer_keyboard_held(), [60, 64]);
    lumen.app_mut().computer_keyboard_sent();
    on(&mut lumen);
    assert!(!lumen.app().computer_keyboard_on());
    assert!(lumen.app().computer_keyboard_held().is_empty());
    let sent = lumen.app_mut().computer_keyboard_sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(sent.contains(&Msg::note_off(0, 60)) && sent.contains(&Msg::note_off(0, 64)));
    // The late key-up of a released key sends nothing more.
    up(&mut lumen, Key::A);
    assert!(lumen.app_mut().computer_keyboard_sent().is_empty());

    // Panic, from the palette.
    on(&mut lumen);
    down(&mut lumen, Key::G, false);
    lumen.app_mut().computer_keyboard_sent();
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("panic".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_off(0, 67)]
    );

    // The window losing focus.
    down(&mut lumen, Key::J, false);
    lumen.app_mut().computer_keyboard_sent();
    lumen.harness.event(Event::WindowFocused(false));
    lumen.frames(2);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_off(0, 71)]
    );
    assert!(lumen.app().computer_keyboard_on(), "it stays on");
}

#[test]
fn the_readout_gives_way_in_the_smallest_window_and_the_info_line_carries_it() {
    use egui_kittest::kittest::Queryable;
    let mut lumen = Lumen::start("keyboard-narrow");
    lumen.harness.set_size(eframe::egui::vec2(960.0, 540.0));
    lumen.frames(10);
    let position_before = lumen.harness.get_by_label_contains("Position").rect();
    on(&mut lumen);
    lumen.frames(10);
    let detail = lumen.harness.get_by_label("Detail").rect();
    let position = lumen.harness.get_by_label_contains("Position").rect();
    assert!(
        position.right() < detail.left(),
        "the position {position:?} reaches the panel toggles {detail:?}"
    );
    assert!(
        position.left() <= position_before.left() + 40.0,
        "only the button moved the bar's controls: {position:?} was {position_before:?}"
    );
    assert!(
        !readout(&lumen, "C3\u{2013}C4 \u{00B7} vel 100"),
        "no room for it in the bar"
    );
    assert!(
        lumen
            .harness
            .query_by_label_contains("Info: C3\u{2013}C4 \u{00B7} vel 100")
            .is_some(),
        "the info line leads with it"
    );
    // With room it is in the bar.
    lumen.harness.set_size(eframe::egui::vec2(1600.0, 900.0));
    lumen.frames(10);
    assert!(readout(&lumen, "C3\u{2013}C4 \u{00B7} vel 100"));
}
