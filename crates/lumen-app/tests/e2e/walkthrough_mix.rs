//! The M4 exit flow (`docs/milestones/M4.md`, slice 7): route a drum group
//! and a vocal through a return's send, then solo and mute across them,
//! and hear each step. The return carries the synthetic latent device,
//! standing in for the reverb (a plugin from M6, the built-in one in M8):
//! the dry and wet paths only add up exactly when delay compensation lines
//! them up. Then save, reopen, and hear the same mix.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::{Lumen, wav_at};

/// The tones' peaks (of 32,768): each file plays the same tone in phase,
/// so levels add, and each source is told apart by its size.
const KICK: f32 = 2_000.0;
const SNARE: f32 = 3_000.0;
const VOCAL: f32 = 4_000.0;

/// The synthetic reverb's latency: 10 ms.
const REVERB_LATENCY: u32 = 480;

/// What centred tracks play of tones at these peaks, dry or through a
/// send: stereo tracks, buses, returns and the master at the centre pass
/// at unity.
fn centred(peaks: &[f32]) -> f32 {
    peaks.iter().sum::<f32>() / 32_768.0
}

/// The loudest left sample over a stretch, after letting changes glide.
fn loudest(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(150), 1_000);
    heard.iter().fold(0.0f32, |m, f| m.max(f[0].abs()))
}

/// Checks the level heard once it has settled: two stretches in a row
/// agree. (How changes glide is tested in the engine; this is what they
/// arrive at, whatever the threads' timing.)
#[track_caller]
fn hears(lumen: &mut Lumen, want: f32, what: &str) {
    let mut got = loudest(lumen);
    for _ in 0..20 {
        let again = loudest(lumen);
        let settled = (again - got).abs() <= got.max(1e-3) * 0.005;
        got = again;
        if settled {
            break;
        }
    }
    assert!(
        (got - want).abs() <= want.max(1e-3) * 0.05,
        "{what}: heard {got}, want {want}"
    );
}

/// Waits until the control named `label` shows (rows grow over a few
/// frames when unfolded).
fn wait_to_show(lumen: &mut Lumen, label: &str) {
    for _ in 0..100 {
        if lumen.harness.query_by_label_contains(label).is_some() {
            return;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{label} never showed");
}

/// Types `level` into the send knob named `label`.
fn send_at(lumen: &mut Lumen, label: &str, level: &str) {
    wait_to_show(lumen, label);
    lumen.harness.get_by_label_contains(label).click();
    lumen.frames(3);
    lumen.harness.event(Event::Text(level.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

/// Clicks `label` holding Ctrl.
fn ctrl_click(lumen: &mut Lumen, label: &str) {
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::COMMAND));
    lumen.frames(1);
    lumen.click(label);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(1);
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
}

#[test]
fn a_drum_group_and_a_vocal_share_a_return_and_solo_and_mute_across_them() {
    // Three recordings, dropped together: a track each, in phase.
    let mut lumen = Lumen::start("walkthrough-mix");
    let files = [("kick", KICK), ("snare", SNARE), ("vocal", VOCAL)]
        .map(|(name, peak)| wav_at(&lumen.dir().join(format!("{name}.wav")), 20.0, peak));
    lumen.drop_files(&files);
    lumen.frames(20);
    assert_eq!(lumen.track_names(), ["kick", "snare", "vocal"]);

    // Group the drums: select kick and snare, Ctrl+G.
    lumen
        .harness
        .get_by_label("kick: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::SHIFT, Key::ArrowDown);
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(lumen.track_names(), ["Group 1", "kick", "snare", "vocal"]);

    // A return.
    lumen.click("Add track");
    lumen.click("Add return track");
    let project = lumen.app().project();
    let ret = project
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Return)
        .expect("a return")
        .id;

    // Send the group to it at 0 dB from its mixer strip, and the vocal
    // from its unfolded header.
    // (440 pt, not 480: the 44 pt transport bar of the design system takes
    // room from the 900 pt window, and with 480 the unfolded vocal's header
    // slid under the pinned returns, so the click on its fold arrow landed
    // on the "Returns" caption and folded the returns away.)
    lumen.app_mut().set_detail_size(440.0);
    lumen.frames(3);
    send_at(&mut lumen, "Send Group 1 to A Return 1 in the mixer", "0");
    lumen.click("Fold vocal");
    send_at(&mut lumen, "Send vocal to A Return 1", "0");
    let sends = |lumen: &Lumen| {
        let tracks = &lumen.app().project().tracks;
        tracks.iter().map(|t| t.sends.len()).sum::<usize>()
    };
    assert_eq!(sends(&lumen), 2);

    // The synthetic reverb on the return: the paths through it are late by
    // its latency, so everything else waits for them.
    lumen.app_mut().set_test_latency(ret, REVERB_LATENCY);
    lumen.wait_for("the compensation", |app| {
        app.compensation() == REVERB_LATENCY
    });

    // Everything: dry, and as much again through the return, lined up.
    let everything = 2.0 * centred(&[KICK, SNARE, VOCAL]);
    play(&mut lumen);
    hears(
        &mut lumen,
        everything,
        "the whole mix, dry and wet in phase",
    );

    // Solo the drums: their path and the return they send to (returns are
    // solo-safe); the vocal goes, its send too.
    lumen.click("Solo Group 1");
    assert!(lumen.shows("Solo kick (through its group)"));
    assert!(lumen.shows("Mute vocal (silenced)"));
    hears(
        &mut lumen,
        2.0 * centred(&[KICK, SNARE]),
        "the soloed drums",
    );

    // Mute the kick inside the soloed group: mute beats solo, and its send
    // goes with it.
    lumen.click("Mute kick");
    hears(&mut lumen, 2.0 * centred(&[SNARE]), "the snare alone");

    // Ctrl+click adds the vocal's solo.
    ctrl_click(&mut lumen, "Solo vocal");
    hears(
        &mut lumen,
        2.0 * centred(&[SNARE, VOCAL]),
        "snare and vocal",
    );

    // A plain click solos the return alone: wet only.
    lumen.click("Solo Return 1");
    hears(&mut lumen, centred(&[SNARE, VOCAL]), "the return, wet only");

    // Clear the solos and unmute the kick: the whole mix again. Muting the
    // return leaves it dry.
    lumen.click("Clear all solos");
    lumen.click("Mute kick");
    hears(&mut lumen, everything, "the whole mix again");
    lumen.click("Mute Return 1");
    hears(&mut lumen, centred(&[KICK, SNARE, VOCAL]), "dry only");
    lumen.click("Mute Return 1");
    lumen.key(Modifiers::NONE, Key::Space);

    // Save, close, reopen: the same routing, the same mix. (The synthetic
    // reverb is not part of the project: reopened, nothing is late.)
    let path = project_file(lumen.dir(), "Mix");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    let opened = again.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(opened.master, saved.master);
    play(&mut again);
    hears(&mut again, everything, "the reopened mix");
    again.key(Modifiers::NONE, Key::Space);
}
