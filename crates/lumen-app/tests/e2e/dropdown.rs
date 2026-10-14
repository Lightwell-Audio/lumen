//! Dropdowns (combo boxes) in the premium control look: the audio settings'
//! buffer size is a `widgets::Dropdown`. It is named "<label>: <value>",
//! opens its list of choices as a menu, picks by click, and works from the
//! keyboard alone: Enter opens it, the arrows move between rows, Enter
//! picks, and Esc closes the list and leaves the window open.

use eframe::egui::{Key, Modifiers};

use super::Lumen;

#[test]
fn a_buffer_size_is_picked_by_click_and_by_keyboard() {
    let mut lumen = Lumen::start("dropdown");
    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    lumen.frames(5);
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the combo is named"
    );
    // By click: the list opens as a menu and a row applies its size; the
    // button keeps keyboard focus afterwards.
    lumen.click("Buffer size: 128 samples");
    lumen.frames(3);
    assert!(lumen.shows("64 samples"), "the list offers 64 samples");
    lumen.click("64 samples");
    lumen.frames(3);
    assert!(lumen.shows("Buffer size: 64 samples"), "the pick applies");
    // By keyboard: Enter opens the list with the current row focused; the
    // arrow goes down one row and Enter picks it.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the key pick applies"
    );
    // Esc closes the list and keeps the window open.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(lumen.shows("64 samples"), "the list is open");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the window stays open"
    );
}
