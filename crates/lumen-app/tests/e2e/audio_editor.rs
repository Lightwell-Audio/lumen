//! The audio clip editor (M9.5 S1): an opened audio clip shows its whole
//! file, the part it plays bright; Whole file and Zoom to clip frame the
//! view (buttons and palette commands); the zoom keys zoom it down to single
//! samples; a click on its ruler moves the playhead; and the note editor and
//! the audio editor draw the same header (the owner's TL C and KC C).

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::Id;

use super::{Lumen, midi_play, wav};

/// Lumen with a two-second file dropped in (four beats at 120 BPM), its
/// clip trimmed to beats 1 to 3 and opened in the Clip tab.
fn with_trimmed_clip(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let kick = wav(&lumen.dir().join("kick.wav"), 2.0);
    lumen.drop_files(&[kick]);
    lumen.frames(40);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().trim_clip_start_to(clip, Ticks::quarters(1));
    lumen
        .app_mut()
        .trim_clip_end_to(clip, Ticks::quarters(3), false);
    lumen.app_mut().select_clip(clip);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the audio editor to draw", |app| {
        app.audio_editor_shown().is_some()
    });
    (lumen, clip)
}

fn click_at(lumen: &mut Lumen, at: Pos2) {
    lumen.harness.hover_at(at);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
}

#[test]
fn an_opened_audio_clip_shows_its_whole_file_with_the_part_it_plays() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-file");
    assert!(lumen.shows("Audio editor: kick"));
    let shown = lumen.app().audio_editor_shown().expect("drawn");
    assert_eq!(shown.window, (1.0, 3.0), "the clip plays beats 1 to 3");
    assert!(shown.file.0.abs() < 1e-6, "the file starts at beat 0");
    assert!((shown.file.1 - 4.0).abs() < 1e-6, "{:?}", shown.file);
    // Opened, it is framed on the part it plays.
    let x = |q: f64| shown.view.x(shown.body.left(), q);
    assert!(x(1.0) >= shown.body.left() && x(3.0) <= shown.body.right());
    assert!(x(3.0) - x(1.0) > shown.body.width() * 0.9);

    // Whole file shows all four beats; Zoom to clip goes back.
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    let whole = lumen.app().audio_editor_shown().expect("drawn");
    let x = |q: f64| whole.view.x(whole.body.left(), q);
    assert!(x(0.0) >= whole.body.left() - 1.0 && x(4.0) <= whole.body.right() + 1.0);
    assert!(whole.view.zoom < shown.view.zoom);
    midi_play::palette(&mut lumen, "Zoom the audio editor to the clip");
    lumen.frames(3);
    let back = lumen.app().audio_editor_shown().expect("drawn");
    assert!((back.view.zoom - shown.view.zoom).abs() < 1e-9);
}

#[test]
fn the_zoom_keys_zoom_the_editor_down_to_single_samples() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-zoom");
    let before = lumen.app().audio_editor_shown().expect("drawn");
    assert!(!before.samples);
    let arrangement = lumen.app().view();
    // The keys are the editor's while the pointer is over it.
    lumen.harness.hover_at(before.body.center());
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Plus);
    lumen.frames(2);
    let zoomed = lumen.app().audio_editor_shown().expect("drawn");
    assert!(zoomed.view.zoom > before.view.zoom);
    assert_eq!(lumen.app().view(), arrangement, "the arrangement stays put");
    for _ in 0..60 {
        lumen.key(Modifiers::NONE, Key::Plus);
    }
    lumen.wait_for("samples to be drawn", |app| {
        app.audio_editor_shown().is_some_and(|s| s.samples)
    });
}

#[test]
fn a_click_on_the_editor_ruler_moves_the_playhead() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-ruler");
    let shown = lumen.app().audio_editor_shown().expect("drawn");
    let ruler = lumen
        .harness
        .get_by_label("Editor ruler: click to move the playhead")
        .rect();
    let x = shown.view.x(shown.body.left(), 2.0);
    click_at(&mut lumen, Pos2::new(x, ruler.center().y));
    let at = lumen.app().playhead();
    assert!(
        (at.0 - Ticks::quarters(2).0).abs() < PPQ / 4,
        "the playhead is at beat {}",
        at.as_quarters()
    );
}

#[test]
fn both_clip_editors_draw_the_same_header() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-header");
    let audio_title = lumen.harness.get_by_label("Clip kick").rect();
    let audio_grid = lumen
        .harness
        .get_all_by(|n| n.label().is_some_and(|l| l.starts_with("Editor grid:")))
        .next()
        .expect("the audio editor's grid chip")
        .rect();
    assert!(audio_title.right() < audio_grid.left());
    assert!((audio_title.center().y - audio_grid.center().y).abs() < 12.0);

    midi_play::palette(&mut lumen, "Add MIDI track");
    lumen.frames(30);
    midi_play::palette(&mut lumen, "Create MIDI clip");
    lumen.frames(10);
    let midi_name = {
        let project = lumen.app().project();
        project.tracks[1].clips[0].name.clone()
    };
    let midi_title = lumen
        .harness
        .get_by_label(&format!("Clip {midi_name}"))
        .rect();
    let midi_grid = lumen
        .harness
        .get_all_by(|n| n.label().is_some_and(|l| l.starts_with("Editor grid:")))
        .next()
        .expect("the note editor's grid chip")
        .rect();
    // Same parts, same order, same heights: the title, then the tools, then
    // the grid chip, on one centre line.
    assert!(midi_title.right() < midi_grid.left());
    assert!((midi_title.height() - audio_title.height()).abs() < 1.0);
    assert!((midi_grid.height() - audio_grid.height()).abs() < 1.0);
    let pointer = lumen.harness.get_by_label("Pointer tool").rect();
    let draw = lumen.harness.get_by_label("Draw tool").rect();
    assert!(midi_title.right() < pointer.left() && draw.right() < midi_grid.left());
    // KC C: the Draw key is not written in the control.
    assert!(
        !lumen
            .harness
            .query_all_by_label("B")
            .any(|n| draw.contains(n.rect().center())),
        "no key inside the Draw piece"
    );
}
