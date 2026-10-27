//! Resizing a track by the bottom edge of its header (the owner, after the
//! interface audit's C7: the edge was easy to miss). The edge is a band
//! each side of the row's line, and its lower half is not taken by the
//! header below, which drags tracks.

use eframe::egui::{Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_project::model::{Color, Id, Track, TrackKind};
use lumen_project::testing::{TempDir, project_of};

use super::Lumen;

fn two_tracks(name: &str) -> Lumen {
    let tracks = vec![
        Track::new(Id(1000), "upper", TrackKind::Audio, Color(0x4F81BD)),
        Track::new(Id(1001), "lower", TrackKind::Audio, Color(0x4F81BD)),
    ];
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project_of(name, tracks), &path, &Default::default())
        .expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 2);
    lumen.frames(30);
    lumen
}

/// Drags from `below` points under the upper track's line, 50 pt down, and
/// returns how much taller the tracks grew.
fn drag_from_below_the_line(lumen: &mut Lumen, below: f32) -> f32 {
    let ctx = lumen.harness.ctx.clone();
    let before = lumen.app().tracks_height(&ctx);
    let edge = lumen.harness.get_by_label("Resize upper").rect();
    let from = Pos2::new(edge.center().x, edge.center().y + below);
    lumen.drag(from, from + vec2(0.0, 50.0), Modifiers::NONE);
    lumen.frames(40);
    lumen.app().tracks_height(&ctx) - before
}

#[test]
fn the_resize_edge_reaches_below_the_line_over_the_next_header() {
    let mut lumen = two_tracks("resize-below");
    // 5 pt below the line is over the lower track's header, and still the
    // upper track's edge: 44 pt dragged 50 pt down unfolds it to 88.
    let grew = drag_from_below_the_line(&mut lumen, 5.0);
    assert!(
        (grew - 44.0).abs() < 0.5,
        "the upper track unfolded: {grew}"
    );
    let names: Vec<_> = lumen
        .app()
        .project()
        .tracks
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["upper", "lower"], "no track was moved");
}

#[test]
fn a_drag_further_into_the_next_header_moves_tracks_not_the_edge() {
    let mut lumen = two_tracks("resize-clear");
    // 10 pt below the line is the lower header's own: no resize.
    let grew = drag_from_below_the_line(&mut lumen, 10.0);
    assert!(grew.abs() < 0.5, "no track changed height: {grew}");
}
