//! The transport bar fits its room (`transport/fit.rs`): at the smallest
//! window, 960 x 540, the position (bars and time) is whole and clear of the
//! audio dot, the MIDI chip and the panel toggles, with the computer
//! keyboard's mode on and off; wider windows keep what gave way.

use eframe::egui::{Key, Modifiers, Rect, vec2};
use egui_kittest::kittest::Queryable;

use super::Lumen;

const POSITION: &str = "Position 1.1.1";
const TIME: &str = "0:00.000";
const DOT: &str = "Audio: ";
const CHIP: &str = "MIDI: ";
const READOUT: &str = "Computer keyboard: ";
/// The narrowest window that shows the keyboard readout again. Record and
/// Overdub (M5 slice 6d) are essentials, so the essentials take 1280 to 1320
/// pt; the readout gives way below that.
const READOUT_ROOM: f32 = 1320.0;

/// The node's rect; of several (the Browser's toggle and its pane), the
/// one in the bar.
fn rect(lumen: &Lumen, label: &str) -> Rect {
    lumen
        .harness
        .query_all_by_label_contains(label)
        .map(|node| node.rect())
        .find(|r| r.top() < 40.0)
        .unwrap_or_else(|| panic!("{label} is not in the bar"))
}

fn resize(lumen: &mut Lumen, size: (f32, f32)) {
    lumen.harness.set_size(vec2(size.0, size.1));
    // Rows settle and the bar measures itself over a few frames.
    lumen.frames(20);
}

/// The bar's pieces in reading order are inside the window and none touches
/// the next: the position, its time, the dot, the chip, the three toggles.
fn assert_clear(lumen: &Lumen, window: f32) {
    let pieces = [
        ("position", rect(lumen, POSITION)),
        ("time", rect(lumen, TIME)),
        ("audio dot", rect(lumen, DOT)),
        ("MIDI chip", rect(lumen, CHIP)),
        ("palette", rect(lumen, "Command palette")),
        ("browser", rect(lumen, "Browser")),
        ("detail", rect(lumen, "Detail")),
    ];
    for (name, r) in &pieces {
        assert!(
            r.left() >= 0.0 && r.right() <= window && r.width() > 4.0,
            "{name} {r:?} is not whole in a window {window} wide"
        );
    }
    for pair in pieces.windows(2) {
        let ((a_name, a), (b_name, b)) = (&pair[0], &pair[1]);
        assert!(
            a.right() <= b.left() + 0.5,
            "{a_name} {a:?} reaches {b_name} {b:?} at {window}"
        );
    }
    // The position is as wide as its text: not cut to a stub.
    let position = rect(lumen, POSITION);
    let time = rect(lumen, TIME);
    assert!(position.width() > 40.0 && time.width() > 40.0);
}

#[test]
fn the_position_is_whole_and_clear_in_the_smallest_window() {
    let mut lumen = Lumen::start("barfit-960");
    resize(&mut lumen, (960.0, 540.0));
    assert_clear(&lumen, 960.0);
    // With the computer keyboard on, its readout is what gives way.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    lumen.frames(20);
    assert!(lumen.app().computer_keyboard_on());
    assert_clear(&lumen, 960.0);
    assert!(lumen.harness.query_by_label_contains(READOUT).is_none());
    // Every control stays reachable.
    for name in [
        "Go to start",
        "Record",
        "Overdub",
        "Metronome",
        "Follow playhead",
        "Computer keyboard plays notes",
        "Clear all solos",
        "Tempo",
        "Time signature",
    ] {
        let r = rect(&lumen, name);
        assert!(r.width() > 4.0, "{name} {r:?}");
        assert!(r.right() <= rect(&lumen, POSITION).left(), "{name} {r:?}");
    }
}

#[test]
fn what_gave_way_comes_back_with_room_in_the_order_it_left() {
    let mut lumen = Lumen::start("barfit-wide");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    for width in [1600.0, 1280.0, 960.0, 1280.0, 1600.0] {
        resize(&mut lumen, (width, 760.0));
        assert_clear(&lumen, width);
        let readout = lumen.harness.query_by_label_contains(READOUT).is_some();
        assert_eq!(
            readout,
            width >= READOUT_ROOM,
            "the keyboard readout at {width}"
        );
    }
}
