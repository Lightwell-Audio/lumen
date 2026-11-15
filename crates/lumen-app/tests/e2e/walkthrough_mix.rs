use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::{Lumen, wav_at};

const KICK: f32 = 2_000.0;
const SNARE: f32 = 3_000.0;
const VOCAL: f32 = 4_000.0;

const REVERB_LATENCY: u32 = 480;

fn centred(peaks: &[f32]) -> f32 {
    peaks.iter().sum::<f32>() / 32_768.0
}

fn loudest(lumen: &mut Lumen) -> f32 {
    let heard = lumen.listen(Duration::from_millis(150), 1_000);
    heard.iter().fold(0.0f32, |m, f| m.max(f[0].abs()))
}

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

fn wait_to_show(lumen: &mut Lumen, label: &str) {
    lumen.run_until(&format!("{label} to show"), |l| {
        l.harness.query_by_label_contains(label).is_some()
    });
}

fn send_at(lumen: &mut Lumen, label: &str, level: &str) {
    wait_to_show(lumen, label);
    lumen.harness.get_by_label_contains(label).click();
    lumen.frames(3);
    lumen.harness.event(Event::Text(level.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

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
    let mut lumen = Lumen::start("walkthrough-mix");
    let files = [("kick", KICK), ("snare", SNARE), ("vocal", VOCAL)]
        .map(|(name, peak)| wav_at(&lumen.dir().join(format!("{name}.wav")), 20.0, peak));
    lumen.drop_files(&files);
    lumen.frames(20);
    assert_eq!(lumen.track_names(), ["kick", "snare", "vocal"]);

    lumen
        .harness
        .get_by_label("kick: double-click to rename")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::SHIFT, Key::ArrowDown);
    lumen.key(Modifiers::COMMAND, Key::G);
    assert_eq!(lumen.track_names(), ["Group 1", "kick", "snare", "vocal"]);

    lumen.click("Add track");
    lumen.click("Add return track");
    let project = lumen.app().project();
    let ret = project
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Return)
        .expect("a return")
        .id;

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

    lumen.app_mut().set_test_latency(ret, REVERB_LATENCY);
    lumen.wait_for("the compensation", |app| {
        app.compensation() == REVERB_LATENCY
    });

    let everything = 2.0 * centred(&[KICK, SNARE, VOCAL]);
    play(&mut lumen);
    hears(
        &mut lumen,
        everything,
        "the whole mix, dry and wet in phase",
    );

    lumen.click("Solo Group 1");
    assert!(lumen.shows("Solo kick (through its group)"));
    assert!(lumen.shows("Mute vocal (silenced)"));
    hears(
        &mut lumen,
        2.0 * centred(&[KICK, SNARE]),
        "the soloed drums",
    );

    lumen.click("Mute kick");
    hears(&mut lumen, 2.0 * centred(&[SNARE]), "the snare alone");

    ctrl_click(&mut lumen, "Solo vocal");
    hears(
        &mut lumen,
        2.0 * centred(&[SNARE, VOCAL]),
        "snare and vocal",
    );

    lumen.click("Solo Return 1");
    hears(&mut lumen, centred(&[SNARE, VOCAL]), "the return, wet only");

    lumen.click("Clear all solos");
    lumen.click("Mute kick");
    hears(&mut lumen, everything, "the whole mix again");
    lumen.click("Mute Return 1");
    hears(&mut lumen, centred(&[KICK, SNARE, VOCAL]), "dry only");
    lumen.click("Mute Return 1");
    lumen.key(Modifiers::NONE, Key::Space);

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
