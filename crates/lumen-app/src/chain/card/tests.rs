//! The card's tests.

use super::body::kilobytes;
use super::*;

static CLOSED: strip::Editor = strip::Editor::Closed;

fn look(face: Face<'_>, bypass: bool, instrument: bool) -> Look<'_> {
    Look {
        name: "Tape Delay",
        vendor: "Vendor D",
        identity: "x",
        instrument,
        bypass,
        selected: false,
        face,
        latency_ms: 2.7,
        reports_latency: true,
        track_color: Color32::WHITE,
        band: 96.0,
        folded: false,
        picture: None,
        slots: &[],
        parameters: 0,
        field: None,
        editor: &CLOSED,
        learning: false,
        sidechain: None,
        outputs: None,
    }
}

#[test]
fn the_footer_says_the_signal_or_what_was_kept() {
    assert_eq!(
        footer_words(&look(Face::Ready, false, true)),
        ("MIDI \u{2192} audio".into(), "2.7 ms".into())
    );
    assert_eq!(
        footer_words(&look(Face::Ready, false, false)),
        ("audio \u{2192} audio".into(), "2.7 ms".into())
    );
    assert_eq!(
        footer_words(&look(Face::Ready, true, false)),
        ("bypassed".into(), "2.7 ms kept".into())
    );
    assert_eq!(
        footer_words(&look(
            Face::Failed(Failure {
                words: "x",
                from: "y",
                keeps: false
            }),
            false,
            false
        ))
        .0,
        "bypassed"
    );
    assert_eq!(
        footer_words(&look(Face::Loading, false, false)).0,
        "starting\u{2026}"
    );
}

#[test]
fn a_device_with_no_latency_to_report_says_nothing_of_one() {
    let mut quiet = look(Face::Ready, false, true);
    quiet.reports_latency = false;
    assert_eq!(
        footer_words(&quiet),
        ("MIDI \u{2192} audio".into(), String::new())
    );
    let mut bypassed = look(Face::Ready, true, true);
    bypassed.reports_latency = false;
    assert_eq!(footer_words(&bypassed), ("bypassed".into(), String::new()));
}

#[test]
fn saved_settings_are_counted_in_whole_kilobytes_rounded_up() {
    assert_eq!(kilobytes(0), "0 KB");
    assert_eq!(kilobytes(1), "1 KB");
    assert_eq!(kilobytes(14 * 1024), "14 KB");
    assert_eq!(kilobytes(14 * 1024 + 1), "15 KB");
}
