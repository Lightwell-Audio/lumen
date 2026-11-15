use std::time::Duration;

use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;

use super::{Lumen, wav};

const FRAMES: u32 = 595;

#[test]
fn the_transport_shows_compensation_only_while_there_is_some() {
    let mut lumen = Lumen::start("compensation-readout");
    lumen.wait_for("audio", |app| app.audio_running());
    let file = wav(&lumen.dir().join("kick.wav"), 2.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].clone();
    let readout = "12.4 ms compensation";
    assert!(
        lumen.harness.query_by_label_contains(readout).is_none(),
        "nothing while no device adds latency"
    );

    lumen.app_mut().set_test_latency(track.id, FRAMES);
    lumen.wait_for("the routing", |app| app.compensation() == FRAMES);
    lumen.frames(2);
    let chip = lumen.harness.get_by_label_contains("Audio: ");
    assert!(
        chip.accesskit_node()
            .label()
            .unwrap_or_default()
            .ends_with(readout)
    );
    let status = chip.rect();

    lumen.harness.hover_at(status.center());
    lumen.frames(3);
    let said = format!(
        "Info: Delay compensation 12.4 ms · longest path: {}",
        track.name
    );
    assert!(
        lumen.harness.query_by_label_contains(&said).is_some(),
        "{said}"
    );

    lumen.harness.get_by_label_contains("Audio: ").click();
    lumen.frames(3);
    assert!(lumen.shows("12.4 ms · 595 samples"));
    assert!(lumen.shows(&format!("Longest path: {}", track.name)));

    lumen.app_mut().set_test_latency(track.id, 0);
    lumen.wait_for("the routing", |app| app.compensation() == 0);
    lumen.frames(2);
    assert!(lumen.harness.query_by_label_contains(readout).is_none());
}

#[test]
fn a_latent_track_stays_in_time_with_the_others() {
    let mut lumen = Lumen::start("compensation-in-time");
    lumen.wait_for("audio", |app| app.audio_running());
    let files = [
        wav(&lumen.dir().join("a.wav"), 20.0),
        wav(&lumen.dir().join("b.wav"), 20.0),
    ];
    lumen.drop_files(&files);
    lumen.wait_for("both tracks", |app| app.project().tracks.len() == 2);
    let ids: Vec<_> = lumen.app().project().tracks.iter().map(|t| t.id).collect();
    lumen.app_mut().set_detail_size(480.0);
    lumen.frames(3);
    let second = lumen.app().project().tracks[1].name.clone();
    lumen.click(&format!("Invert polarity of {second} in the mixer"));
    lumen.app_mut().set_test_latency(ids[0], FRAMES);
    lumen.wait_for("the routing", |app| app.compensation() == FRAMES);

    lumen.key(eframe::egui::Modifiers::NONE, eframe::egui::Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    let loudest = heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    assert!(loudest < 1e-4, "the tracks drifted apart: {loudest}");

    lumen.app_mut().set_test_latency(ids[0], 0);
    lumen.wait_for("the routing", |app| app.compensation() == 0);
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    let loudest = heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    assert!(loudest < 1e-4, "after the change: {loudest}");
}
