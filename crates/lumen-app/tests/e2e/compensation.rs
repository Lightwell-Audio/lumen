//! Delay compensation (M4 slice 6, ADR-0041): latent tracks stay in time
//! with the rest, and the transport shows the compensation only while
//! there is some (the owner's D1). Latency comes from the engine's
//! synthetic device until devices arrive (M6).

use std::time::Duration;

use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;

use super::{Lumen, wav};

/// 12.4 ms at the test device's 48 kHz.
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
    // The status dot is named for screen readers with it, and the device
    // line ends with it.
    let dot = lumen.harness.get_by_label_contains("Audio: ");
    assert!(
        dot.accesskit_node()
            .label()
            .unwrap_or_default()
            .ends_with(readout)
    );
    let status = lumen
        .harness
        .get_all_by_label_contains(readout)
        .find(|node| {
            !node
                .accesskit_node()
                .label()
                .unwrap_or_default()
                .starts_with("Audio: ")
        })
        .expect("the device line")
        .rect();

    // Hovering it names the longest path in the info line (the bar is
    // drawn before the line, so this reads the line as drawn).
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

    // The audio panel always has it, with the path.
    lumen.harness.get_by_label_contains("Audio: ").click();
    lumen.frames(3);
    assert!(lumen.shows("12.4 ms · 595 samples"));
    assert!(lumen.shows(&format!("Longest path: {}", track.name)));

    // Without the device it goes again (the panel shows 0).
    lumen.app_mut().set_test_latency(track.id, 0);
    lumen.wait_for("the routing", |app| app.compensation() == 0);
    lumen.frames(2);
    assert!(lumen.harness.query_by_label_contains(readout).is_none());
}

#[test]
fn a_latent_track_stays_in_time_with_the_others() {
    // Two tracks play the same tone, the second upside down: in time,
    // they cancel. A device on the first delays it by 595 frames; delay
    // compensation delays the second as much, so they still cancel.
    let mut lumen = Lumen::start("compensation-in-time");
    lumen.wait_for("audio", |app| app.audio_running());
    let files = [
        wav(&lumen.dir().join("a.wav"), 20.0),
        wav(&lumen.dir().join("b.wav"), 20.0),
    ];
    lumen.drop_files(&files);
    lumen.wait_for("both tracks", |app| app.project().tracks.len() == 2);
    let ids: Vec<_> = lumen.app().project().tracks.iter().map(|t| t.id).collect();
    // A tall Mixer tab shows every strip control.
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

    // Taken off while playing, the device's latency goes and the tracks
    // still cancel once the change has passed.
    lumen.app_mut().set_test_latency(ids[0], 0);
    lumen.wait_for("the routing", |app| app.compensation() == 0);
    let heard = lumen.listen(Duration::from_millis(100), 2_000);
    let loudest = heard
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    assert!(loudest < 1e-4, "after the change: {loudest}");
}
