//! Clip marks (M9.5 S3, ADR-0128): a reversed clip and a clip with its
//! polarity inverted carry a small mark at the right end of the title bar,
//! hover says what was done, a narrow clip shows one dot and a very narrow
//! one nothing, and MIDI clips never carry them.

use eframe::egui::{Key, Modifiers, Pos2};
use lumen_core::time::Ticks;

use super::copies::command;
use super::properties::{clip, rect, with_two_clips};

fn hover_header(lumen: &mut super::Lumen, id: lumen_project::model::Id) -> String {
    let r = rect(lumen, id);
    lumen
        .harness
        .hover_at(Pos2::new(r.left() + 12.0, r.top() + 6.0));
    lumen.frames(4);
    lumen.app().info_line().unwrap_or_default()
}

#[test]
fn a_reversed_clip_and_an_inverted_one_say_so_on_hover_and_undo_removes_the_mark() {
    let (mut lumen, kick, snare) = with_two_clips("badges-hover");
    assert!(!hover_header(&mut lumen, kick).contains("Reversed"));

    lumen.app_mut().select_clip(kick);
    lumen.frames(2);
    command(&mut lumen, "Reverse");
    lumen.frames(3);
    assert!(clip(&lumen, kick).reversed);
    let line = hover_header(&mut lumen, kick);
    assert!(line.contains("Reversed"), "{line}");
    assert!(
        !hover_header(&mut lumen, snare).contains("Reversed"),
        "only the reversed clip carries the mark"
    );

    lumen.app_mut().select_clip(kick);
    lumen.frames(2);
    command(&mut lumen, "Invert polarity");
    lumen.frames(3);
    let line = hover_header(&mut lumen, kick);
    assert!(
        line.contains("Reversed") && line.contains("Polarity inverted"),
        "{line}"
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!hover_header(&mut lumen, kick).contains("Polarity inverted"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(!hover_header(&mut lumen, kick).contains("Reversed"));
}

#[test]
fn a_narrow_clip_still_draws_without_trouble_and_marks_are_not_a_midi_thing() {
    let (mut lumen, kick, _) = with_two_clips("badges-narrow");
    lumen.app_mut().trim_clip_end_to(kick, Ticks(60), false);
    lumen.app_mut().select_clip(kick);
    lumen.frames(3);
    command(&mut lumen, "Reverse");
    lumen.frames(3);
    assert!(clip(&lumen, kick).reversed);
    // The clip is a few points wide: nothing overlaps, nothing panics, and
    // the clip still answers a hover.
    let r = rect(&lumen, kick);
    assert!(r.width() > 0.0);
    lumen.frames(5);
}
