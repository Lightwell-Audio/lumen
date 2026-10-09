//! The info line (M4 slice 0, the owner's choice from the UX pass on
//! hints): what is under the pointer and what can be done there, along the
//! bottom of the window, with shortcuts from the keymap.

use eframe::egui::{Event, Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;

use super::Lumen;
use super::properties::with_two_clips;

/// Hovers the top half of clip `index` on track `track`.
fn hover_clip(lumen: &mut Lumen, track: usize) {
    let id = lumen.app().project().tracks[track].clips[0].id;
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, id).expect("in view");
    lumen
        .harness
        .hover_at(Pos2::new(r.center().x, r.top() + r.height() * 0.3));
    lumen.frames(3);
}

#[test]
fn the_info_line_names_what_is_under_the_pointer_with_its_keys() {
    let (mut lumen, _, _) = with_two_clips("info-clip");
    hover_clip(&mut lumen, 0);
    let info = lumen.info();
    assert!(
        info.starts_with("Clip \u{201C}kick\u{201D}: drag to move"),
        "{info}"
    );
    assert!(info.contains("Ctrl+D duplicate"), "{info}");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Info: Clip")
            .is_some(),
        "the strip is drawn and named for screen readers"
    );

    // A changed shortcut shows in the info line too: it reads the keymap.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(Event::Text("keyboard shortcuts".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.click("Search shortcuts");
    lumen.harness.event(Event::Text("duplicate".into()));
    lumen.frames(2);
    lumen.click("Change Duplicate shortcut Ctrl+D");
    lumen.key(Modifiers::ALT, Key::D);
    lumen.key(Modifiers::NONE, Key::Escape);
    hover_clip(&mut lumen, 0);
    assert!(lumen.info().contains("Alt+D duplicate"), "{}", lumen.info());
}

#[test]
fn the_empty_track_area_says_a_double_click_adds_a_track() {
    let (mut lumen, kick, _) = with_two_clips("info-headers");
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, kick).expect("in view");
    // In the header column, below both tracks.
    lumen
        .harness
        .hover_at(Pos2::new(r.left() - 100.0, r.bottom() + 300.0));
    lumen.frames(3);
    assert!(
        lumen
            .info()
            .starts_with("Empty track area: double-click to add an audio track"),
        "{}",
        lumen.info()
    );
}

#[test]
fn show_info_line_hides_and_shows_the_strip() {
    let (mut lumen, _, _) = with_two_clips("info-toggle");
    hover_clip(&mut lumen, 0);
    let run = |lumen: &mut Lumen| {
        lumen.key(Modifiers::COMMAND, Key::K);
        lumen.harness.event(Event::Text("show info line".into()));
        lumen.frames(2);
        lumen.key(Modifiers::NONE, Key::Enter);
        hover_clip(lumen, 0);
    };
    run(&mut lumen);
    assert!(
        lumen.harness.query_by_label_contains("Info:").is_none(),
        "hidden"
    );
    run(&mut lumen);
    assert!(
        lumen
            .harness
            .query_by_label_contains("Info: Clip")
            .is_some(),
        "back"
    );
}
