use std::time::Instant;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};

use super::Lumen;

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
            started.elapsed() < super::patience(),
            "the dropdown option {option:?} never became available"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_buffer_size_is_picked_by_click_and_by_keyboard() {
    let mut lumen = Lumen::start("dropdown");
    lumen.open_audio_settings();
    lumen.run_until("the combo, named", |l| l.shows("Buffer size: 128 samples"));
    open_by_click(&mut lumen, "Buffer size: 128 samples", "64 samples");
    assert!(lumen.shows("64 samples"), "the list offers 64 samples");
    lumen.click("64 samples");
    lumen.run_until("the 64-sample choice to apply", |l| {
        l.harness
            .query_by_label("Buffer size: 64 samples")
            .is_some_and(|b| !b.accesskit_node().is_disabled() && b.accesskit_node().is_focused())
    });
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
        l.harness
            .query_by_label("Buffer size: 128 samples")
            .is_some_and(|b| !b.accesskit_node().is_disabled() && b.accesskit_node().is_focused())
    });
    assert!(
        lumen.shows("Buffer size: 128 samples"),
        "the key pick applies"
    );
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
