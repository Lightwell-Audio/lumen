use eframe::egui::{Key, Modifiers};

use super::{Lumen, wav};
use lumen_app::TimeView;
use lumen_app::testing::PROJECT_EXTENSION;

#[test]
fn zoom_keys_fit_and_step_back() {
    let mut lumen = Lumen::start("zoom");
    lumen.key(Modifiers::COMMAND, Key::T);
    let start = lumen.app().view();

    lumen.key(Modifiers::NONE, Key::Equals);
    lumen.frames(30);
    let closer = lumen.app().view();
    assert!(closer.zoom > start.zoom * 1.5, "+ zooms in");

    lumen.key(Modifiers::NONE, Key::Minus);
    lumen.key(Modifiers::NONE, Key::Minus);
    lumen.frames(30);
    assert!(lumen.app().view().zoom < closer.zoom, "- zooms out");

    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::X);
        lumen.frames(30);
    }
    let back = lumen.app().view();
    assert!(
        (back.zoom - start.zoom).abs() < 1e-6,
        "{back:?} vs {start:?}"
    );

    lumen.key(Modifiers::NONE, Key::W);
    lumen.frames(30);
    assert!(lumen.app().view().zoom > start.zoom);
}

#[test]
fn zooming_out_stops_at_the_song() {
    let mut lumen = Lumen::start("zoom-out");
    for _ in 0..20 {
        lumen.key(Modifiers::NONE, Key::Minus);
    }
    lumen.frames(30);
    let view = lumen.app().view();
    assert!(view.zoom > 5.0, "{view:?}");
    assert_eq!(view.left, 0.0);
}

#[test]
fn height_keys_fit_and_restore_the_tracks() {
    let mut lumen = Lumen::start("heights");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(30);
    let ctx = lumen.harness.ctx.clone();
    let height = |lumen: &Lumen| lumen.app().tracks_height(&ctx);
    let before = height(&lumen);
    lumen.key(Modifiers::NONE, Key::H);
    lumen.frames(30);
    assert!(height(&lumen) > before * 2.0, "H fills the height");
    lumen.key(Modifiers::NONE, Key::H);
    lumen.frames(30);
    assert!((height(&lumen) - before).abs() < 1.0, "H again restores");
}

#[test]
fn follow_turns_the_page_while_playing() {
    let mut lumen = Lumen::start("follow");
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 2_000.0,
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the view to turn a page", |app| app.view().left > 0.0);
    assert_eq!(lumen.app().view().zoom, 2_000.0);
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_project_saved_scrolled_past_its_tracks_opens_showing_them() {
    let dir = lumen_project::testing::TempDir::new("e2e-scroll-clamp");
    let folder = dir.path().join("Golden");
    std::fs::create_dir_all(folder.join("Samples")).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumen-project/tests/fixtures/v10/golden.lumen");
    let path = folder.join(format!("Golden.{PROJECT_EXTENSION}"));
    std::fs::copy(fixture, &path).unwrap();
    wav(&folder.join("Samples/kick.wav"), 1.0);

    let mut lumen = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    lumen.wait_for("the project to open", |app| !app.is_busy());
    lumen.frames(5);
    assert!(lumen.shows("Mute Drums"), "the first track is in view");
    assert!(lumen.shows("Mute Bass"));
}
