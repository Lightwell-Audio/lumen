//! Notes outside a clip's window (ADR-0059): the arrangement hides them and
//! does not play them (ADR-0056), but the clip editor shows them, in the
//! dimmed time past the clip, to be picked and edited, as Ableton's clip
//! view does.

use eframe::egui::{Key, Modifiers};
use lumen_app::TimeView;
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Note};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

const CLIP: Id = Id(3);

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

/// A clip from beat 0 to 4 with one note inside it and one past its end
/// (hidden), and the loop brace out to beat 16 so the editor can zoom out
/// that far.
fn launch(name: &str) -> Lumen {
    let notes = vec![
        Note::new(q(1), q(1), 60, 100),
        Note::new(q(6), q(1), 64, 100),
    ];
    let mut project = project_of(
        "Hidden",
        vec![midi_track(1, "Keys", "poly", (0, 4), None, notes, vec![])],
    );
    project.transport.loop_end = q(16);
    project.transport.loop_enabled = true;
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.app_mut().set_detail_size(460.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    lumen
}

fn notes(lumen: &Lumen) -> Vec<Note> {
    match &lumen.app().project().tracks[0].clips[0].content {
        ClipContent::Midi { notes, .. } => notes.clone(),
        ClipContent::Audio { .. } => panic!("not MIDI"),
    }
}

/// Opens the clip and zooms the editor out until beat 7 is in view.
fn open_wide(lumen: &mut Lumen) {
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, CLIP)
        .expect("the clip is in view");
    lumen.double_click(r.center());
    lumen.frames(20);
    assert_eq!(lumen.app().editor_open_clip(), Some(CLIP));
    let body = lumen.app().editor_body().expect("the editor has drawn");
    lumen.harness.hover_at(body.center());
    lumen.frames(2);
    for _ in 0..8 {
        let end = lumen.app().editor_point(q(7), 64).expect("drawn");
        if body.contains(end) {
            break;
        }
        lumen.key(Modifiers::NONE, Key::Minus);
        lumen.frames(3);
    }
    lumen.app_mut().editor_show_pitch(62);
    lumen.frames(5);
}

#[test]
fn a_note_past_the_clips_end_is_shown_picked_and_moved_back_in_the_editor() {
    let mut lumen = launch("hidden-notes-editor");
    open_wide(&mut lumen);
    let body = lumen.app().editor_body().unwrap();
    let hidden = lumen
        .app()
        .editor_point(q(6) + Ticks(q(1).0 / 4), 64)
        .unwrap();
    assert!(body.contains(hidden), "beat 6 is in view: {hidden:?}");

    // A click on it picks it: it is there to be edited.
    lumen.drag(hidden, hidden, Modifiers::NONE);
    lumen.frames(3);
    assert_eq!(lumen.app().editor_picked(), [1]);

    // Dragged back inside the clip, it plays again.
    let to = lumen
        .app()
        .editor_point(q(2) + Ticks(q(1).0 / 4), 64)
        .unwrap();
    lumen.drag(hidden, to, Modifiers::NONE);
    lumen.frames(3);
    let moved = notes(&lumen)[1];
    assert_eq!((moved.start(), moved.pitch()), (q(2), 64));
    let clip = &lumen.app().project().tracks[0].clips[0];
    assert_eq!(lumen_project::edit::shown_notes(clip), [0, 1]);

    // Ctrl+A picks every note the editor shows, outside the window too.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(notes(&lumen)[1].start(), q(6), "undone");
    lumen.harness.hover_at(body.center());
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(lumen.app().editor_picked(), [0, 1]);
}
