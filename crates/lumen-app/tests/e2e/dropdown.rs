//! Dropdowns (combo boxes) in the premium control look: the audio settings'
//! buffer size is a `widgets::Dropdown`. It is named "<label>: <value>",
//! opens its list of choices as a menu, picks by click, and works from the
//! keyboard alone: Enter opens it, the arrows move between rows, Enter
//! picks, and Esc closes the list and leaves the window open.

use std::time::Instant;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};

use super::Lumen;

/// A window can move after the settings menu opens, so a click on the first
/// layout's combo rectangle may miss. Keep trying until its option is in the
/// accessibility tree; that confirms the menu is actually open.
fn open_by_click(lumen: &mut Lumen, button: &str, option: &str) {
    let started = Instant::now();
    loop {
        if lumen.shows(option) {
            return;
        }
        lumen.click(button);
        if lumen.shows(option) {
            return;
        }
        assert!(
            started.elapsed() < super::PATIENCE,
            "the dropdown option {option:?} never became available"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_buffer_size_is_picked_by_click_and_by_keyboard() {
    let mut lumen = Lumen::start("dropdown");
    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    // The window opens over a few frames; wait for the combo, not for a count.
    for _ in 0..400 {
        lumen.frames(1);
        if lumen.shows("Buffer size: 128 samples") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the combo is named"
    );
    // By click: the list opens as a menu and a row applies its size; the
    // button keeps keyboard focus afterwards.
    open_by_click(&mut lumen, "Buffer size: 128 samples", "64 samples");
    assert!(lumen.shows("64 samples"), "the list offers 64 samples");
    lumen.click("64 samples");
    lumen.run_until("the 64-sample choice to apply", |l| {
        l.shows("Buffer size: 64 samples")
    });
    assert!(lumen.shows("Buffer size: 64 samples"), "the pick applies");
    // By keyboard: Enter opens the list with the current row focused; the
    // arrow goes down one row and Enter picks it.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the buffer-size list to open from the keyboard", |l| {
        l.shows("64 samples")
    });
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.run_until("the next buffer-size row to receive focus", |l| {
        l.harness
            .query_all_by_label("128 samples")
            .any(|row| row.accesskit_node().is_focused())
    });
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the key-selected buffer size to apply", |l| {
        l.shows("Buffer size: 128 samples")
    });
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the key pick applies"
    );
    // Esc closes the list and keeps the window open.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the list to open before Escape", |l| l.shows("64 samples"));
    assert!(lumen.shows("64 samples"), "the list is open");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.run_until("Escape to close the list", |l| !l.shows("64 samples"));
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the window stays open"
    );
}
