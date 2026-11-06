//! The audio clip editor (M9.5 S1): an opened audio clip shows its whole
//! file, the part it plays bright; Whole file and Zoom to clip frame the
//! view (buttons and palette commands); the zoom keys zoom it down to single
//! samples; a click on its ruler moves the playhead; and the note editor and
//! the audio editor draw the same header (the owner's TL C and KC C).

use eframe::egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::Id;

use super::{Lumen, TempDir, midi_play, wav};

/// Lumen with a two-second file dropped in (four beats at 120 BPM), its
/// clip trimmed to beats 1 to 3 and opened in the Clip tab.
fn with_trimmed_clip(name: &str) -> (Lumen, Id) {
    let lumen = Lumen::start(name);
    open_trimmed(lumen, 2.0)
}

/// Drops in a file `seconds` long, trims its clip to beats 1 to 3 and opens
/// it in the Clip tab.
fn open_trimmed(mut lumen: Lumen, seconds: f32) -> (Lumen, Id) {
    let kick = wav(&lumen.dir().join("kick.wav"), seconds);
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
    // The playhead is the engine's answer to the locate: wait for it.
    lumen.wait_for("the playhead to reach beat 2", |app| {
        (app.playhead().0 - Ticks::quarters(2).0).abs() < PPQ / 4
    });
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
    // KC C: the Draw key is in Draw's tooltip (widgets::toggle's unit tests
    // check that no key is painted inside the piece, and that Pointer is
    // grey while Draw is lit).
    lumen.harness.hover_at(draw.center());
    lumen.frames(40);
    assert!(
        lumen.shows("Click to add a note, drag to set its length, click a note to remove it"),
        "hovering Draw shows its tooltip"
    );
}

fn wheel(lumen: &mut Lumen, at: Pos2, delta: f32, modifiers: Modifiers) {
    lumen.harness.hover_at(at);
    lumen.frames(1);
    lumen.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, delta),
        modifiers,
        phase: TouchPhase::Move,
    });
    // egui spreads a wheel step over the frames after it: let it settle.
    lumen.frames(90);
}

#[test]
fn the_wheel_scrolls_the_editor_and_ctrl_wheel_zooms_it() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-wheel");
    let before = lumen.app().audio_editor_shown().expect("drawn");
    let arrangement = lumen.app().view();
    wheel(&mut lumen, before.body.center(), 120.0, Modifiers::COMMAND);
    let zoomed = lumen.app().audio_editor_shown().expect("drawn");
    assert!(zoomed.view.zoom > before.view.zoom, "Ctrl+wheel zooms in");
    wheel(&mut lumen, zoomed.body.center(), -200.0, Modifiers::NONE);
    let scrolled = lumen.app().audio_editor_shown().expect("drawn");
    assert!(
        scrolled.view.left > zoomed.view.left,
        "the wheel scrolls later"
    );
    assert_eq!(scrolled.view.zoom, zoomed.view.zoom);
    assert_eq!(lumen.app().view(), arrangement, "the arrangement stays put");
}

#[test]
fn the_zoom_keys_zoom_the_arrangement_when_the_pointer_is_there() {
    let (mut lumen, clip) = with_trimmed_clip("audio-editor-keys-away");
    let before = lumen.app().audio_editor_shown().expect("drawn");
    let ctx = lumen.harness.ctx.clone();
    let lane = lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view");
    lumen.harness.hover_at(lane.center());
    lumen.frames(2);
    // A click in the arrangement gives it the keyboard.
    let arrangement = lumen.app().view();
    click_at(&mut lumen, lane.center());
    lumen.key(Modifiers::NONE, Key::Plus);
    lumen.frames(3);
    assert_ne!(lumen.app().view(), arrangement, "the arrangement zoomed");
    let after = lumen.app().audio_editor_shown().expect("drawn");
    assert_eq!(after.view, before.view, "the editor did not");
}

#[test]
fn the_palette_shows_the_whole_file() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-palette");
    let before = lumen.app().audio_editor_shown().expect("drawn");
    midi_play::palette(&mut lumen, "Show the whole file in the audio editor");
    lumen.frames(3);
    let whole = lumen.app().audio_editor_shown().expect("drawn");
    assert!(whole.view.zoom < before.view.zoom);
    let x = |q: f64| whole.view.x(whole.body.left(), q);
    assert!(x(0.0) >= whole.body.left() - 1.0 && x(4.0) <= whole.body.right() + 1.0);
}

#[test]
fn the_editor_follows_the_playhead_until_the_view_is_moved() {
    // Ten seconds (twenty beats); framed on beats 1 to 3, playing runs off
    // the right edge, and Follow (Page) turns the page.
    let lumen = Lumen::start("audio-editor-follow");
    let (mut lumen, clip) = open_trimmed(lumen, 10.0);
    lumen
        .app_mut()
        .trim_clip_end_to(clip, Ticks::quarters(20), false);
    lumen.frames(5);
    let framed = lumen.app().audio_editor_shown().expect("drawn");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the editor to turn a page", |app| {
        app.audio_editor_shown()
            .is_some_and(|s| s.view.left > framed.view.left + 1.0)
    });
    // Scrolling back pauses Follow: the view stays where it was put.
    let paged = lumen.app().audio_editor_shown().expect("drawn");
    wheel(&mut lumen, paged.body.center(), 400.0, Modifiers::NONE);
    let put = lumen.app().audio_editor_shown().expect("drawn");
    lumen.frames(60);
    let later = lumen.app().audio_editor_shown().expect("drawn");
    assert_eq!(later.view, put.view, "the view stays where it was scrolled");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
}

#[test]
fn a_file_played_from_disk_shows_its_samples_at_depth() {
    let lumen = Lumen::start_with(TempDir::new("e2e-audio-editor-disk"), |launch| {
        launch.playing_from_disk()
    });
    let (mut lumen, _) = open_trimmed(lumen, 40.0);
    assert_eq!(lumen.app().media_from_disk(), [("kick".into(), true)]);
    let before = lumen.app().audio_editor_shown().expect("drawn");
    lumen.harness.hover_at(before.body.center());
    lumen.frames(2);
    for _ in 0..60 {
        lumen.key(Modifiers::NONE, Key::Plus);
    }
    lumen.wait_for("samples read from disk", |app| {
        app.audio_editor_shown().is_some_and(|s| s.samples)
    });
    // They stay drawn from frame to frame while the view holds still.
    for _ in 0..20 {
        lumen.frames(1);
        assert!(lumen.app().audio_editor_shown().expect("drawn").samples);
    }
}

#[test]
fn z_zooms_the_editor_to_the_clip_and_x_goes_back() {
    let (mut lumen, _) = with_trimmed_clip("audio-editor-z-x");
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    let whole = lumen.app().audio_editor_shown().expect("drawn");
    let arrangement = lumen.app().view();
    lumen.harness.hover_at(whole.body.center());
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Z);
    lumen.frames(3);
    let clip = lumen.app().audio_editor_shown().expect("drawn");
    assert!(clip.view.zoom > whole.view.zoom, "Z zooms to the clip");
    lumen.key(Modifiers::NONE, Key::X);
    lumen.frames(3);
    let back = lumen.app().audio_editor_shown().expect("drawn");
    assert_eq!(back.view, whole.view, "X goes back to the whole file");
    assert_eq!(lumen.app().view(), arrangement, "the arrangement stays put");
}

#[test]
fn x_right_after_opening_a_clip_keeps_it_framed() {
    let (mut lumen, clip) = with_trimmed_clip("audio-editor-x-opened");
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    // Opened again, the clip is framed with nothing to go back to: the
    // whole file was the view of an earlier opening.
    lumen.app_mut().select_clip(clip);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    let framed = lumen.app().audio_editor_shown().expect("drawn");
    lumen.harness.hover_at(framed.body.center());
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::X);
    lumen.frames(3);
    let after = lumen.app().audio_editor_shown().expect("drawn");
    assert_eq!(
        after.view, framed.view,
        "X has no earlier view of this opening"
    );
}
