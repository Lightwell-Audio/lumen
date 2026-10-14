//! Trimming a MIDI clip by its edges (owner: the edges jittered and landed
//! off the pointer). The edge keeps the distance it was picked up at, so it
//! never jumps to the pointer; a trimmed start lands at once instead of
//! gliding (which shook the other edge and the notes inside); the track's
//! note layout stays put while notes leave and come back into the window.

use eframe::egui::{Modifiers, Pos2, vec2};
use lumen_app::TimeView;
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

/// One MIDI clip from beat 0 to 8: a low note at 1 and a high one at 6.
fn project() -> Project {
    let notes = vec![
        Note::new(q(1), q(1), 48, 100),
        Note::new(q(6), q(1), 84, 100),
    ];
    project_of(
        "Trim",
        vec![midi_track(1, "Keys", "poly", (0, 8), None, notes, vec![])],
    )
}

const CLIP: Id = Id(3);
/// Points a beat.
const ZOOM: f32 = 60.0;

fn launch(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: f64::from(ZOOM),
    });
    lumen.frames(40);
    lumen
}

fn clip(lumen: &Lumen) -> (Ticks, Ticks) {
    let c = &lumen.app().project().tracks[0].clips[0];
    (c.start, c.start + c.length)
}

/// Where the clip's notes sound, at project time.
fn sounding(lumen: &Lumen) -> Vec<Ticks> {
    let c = &lumen.app().project().tracks[0].clips[0];
    let ClipContent::Midi { notes, .. } = &c.content else {
        panic!("a MIDI clip");
    };
    notes.iter().map(|n| c.start + n.start()).collect()
}

/// A tick's worth of slack per point of pixel rounding.
#[track_caller]
fn near(actual: Ticks, expected: Ticks) {
    let slack = q(1).0 / ZOOM as i64 + 1;
    assert!(
        (actual.0 - expected.0).abs() <= slack,
        "{actual:?} is not at {expected:?}"
    );
}

#[test]
fn a_start_edge_picked_up_inside_keeps_its_distance_from_the_pointer() {
    let mut lumen = launch("midi-trim-start");
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, CLIP)
        .expect("the clip is in view");
    // A few points inside the edge, with Alt so the grid does not round
    // the offset away.
    let at = Pos2::new(r.left() + 4.0, r.center().y);
    lumen.drag(at, at + vec2(2.0 * ZOOM, 0.0), Modifiers::ALT);
    let (start, end) = clip(&lumen);
    near(start, q(2));
    assert_eq!(end, q(8), "the other edge stays put");
    assert_eq!(lumen.app().undo_label(), Some("Trim clip"));
    assert_eq!(
        sounding(&lumen),
        [q(1), q(6)],
        "the notes stay where they sound"
    );
    assert_eq!(
        lumen.app().drawn_clip_start(&ctx, CLIP),
        Some(start),
        "a trimmed start lands at once"
    );
}

#[test]
fn an_end_edge_picked_up_inside_keeps_its_distance_from_the_pointer() {
    let mut lumen = launch("midi-trim-end");
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, CLIP)
        .expect("the clip is in view");
    let at = Pos2::new(r.right() - 4.0, r.center().y);
    lumen.drag(at, at - vec2(3.0 * ZOOM, 0.0), Modifiers::ALT);
    let (start, end) = clip(&lumen);
    assert_eq!(start, q(0));
    near(end, q(5));
}

#[test]
fn the_note_layout_holds_while_an_edge_hides_notes() {
    let mut lumen = launch("midi-trim-layout");
    assert_eq!(lumen.app().track_pitch_range(0), Some((48, 84)));
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, CLIP)
        .expect("the clip is in view");
    // The end in to beat 4: the high note at 6 is hidden, not gone.
    let at = Pos2::new(r.right() - 2.0, r.center().y);
    lumen.drag(at, at - vec2(4.0 * ZOOM, 0.0), Modifiers::NONE);
    assert_eq!(clip(&lumen).1, q(4));
    assert_eq!(
        lumen.app().track_pitch_range(0),
        Some((48, 84)),
        "the layout does not jump as the note leaves the window"
    );
}
