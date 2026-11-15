use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;

use super::{Lumen, wav};

const TONE: f32 = 3_000.0 / 32_768.0;

fn loudest(lumen: &mut Lumen) -> (f32, f32) {
    let heard = lumen.listen(Duration::from_millis(150), 1_000);
    heard.iter().fold((0.0f32, 0.0f32), |(m, c), f| {
        (m.max(f[0].abs()), c.max(f[2].abs()))
    })
}

pub(super) fn choose_cue_pair(lumen: &mut Lumen) {
    lumen.open_audio_settings();
    let label = "3/4   Out 3 / Out 4";
    lumen.run_until("the outputs listed under Output and Cue", |l| {
        l.harness.query_all_by_label(label).count() >= 2
    });
    let started = std::time::Instant::now();
    loop {
        let cue = lumen
            .harness
            .query_all_by_label(label)
            .last()
            .expect("the cue pair is offered");
        cue.scroll_to_me();
        lumen.frames(2);
        let cue = lumen
            .harness
            .query_all_by_label(label)
            .last()
            .expect("the cue pair is offered");
        cue.click();
        lumen.frames(3);
        let chosen = lumen
            .harness
            .query_all_by_label(label)
            .last()
            .is_some_and(|cue| {
                cue.accesskit_node().toggled() == Some(eframe::egui::accesskit::Toggled::True)
            });
        if chosen {
            break;
        }
        assert!(
            started.elapsed() < super::patience(),
            "the cue pair selection was not accepted"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.wait_for("the cue pair to open", |app| app.cue_ready());
}

#[test]
fn a_cued_track_plays_on_the_cue_pair_before_its_fader_and_its_mute() {
    let mut lumen = Lumen::start("cue-track");
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.track_names()[0].clone();

    assert!(!lumen.shows("Solo/Cue switch: Cue"));
    assert!(lumen.shows(&format!("Solo {track}")));

    choose_cue_pair(&mut lumen);
    lumen.click("Solo/Cue switch: Cue");
    assert!(lumen.is_on("Solo/Cue switch: Cue"));
    assert!(!lumen.shows(&format!("Solo {track}")));
    lumen.click(&format!("Cue {track}"));
    assert!(lumen.is_on(&format!("Cue {track}")));

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let (main, cue) = loudest(&mut lumen);
    assert!((main - TONE).abs() < TONE * 0.1, "main {main}");
    assert!(
        (cue - TONE).abs() < TONE * 0.1,
        "cue before the fader: {cue}"
    );
    lumen.click(&format!("Mute {track}"));
    let (main, cue) = loudest(&mut lumen);
    assert!(main < 1e-4 && cue > TONE * 0.9, "main {main}, cue {cue}");
    lumen.click("Solo/Cue switch: Cue");
    let (_, cue) = loudest(&mut lumen);
    assert!(cue < 1e-4, "cue {cue}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_metronome_plays_on_the_cue_pair_when_set_to() {
    let mut lumen = Lumen::start("cue-metronome");
    choose_cue_pair(&mut lumen);
    let metronome = lumen.harness.get_by_label("Metronome").rect();
    lumen.right_click(metronome.center());
    lumen.click("Cue");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.click("Metronome");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let mut heard = Vec::new();
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(250));
        lumen.frames(1);
        heard.extend(lumen.app_mut().heard_all());
    }
    let peak = |c: usize| heard.iter().fold(0.0f32, |m, f| m.max(f[c].abs()));
    assert!(peak(2) > 0.05, "clicks on the cue pair: {}", peak(2));
    assert!(peak(0) < 1e-4, "none on the main pair: {}", peak(0));
    assert_eq!(lumen.app().saved_metronome_output(), "cue");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn solo_or_cue_runs_from_the_palette_once_a_cue_pair_is_chosen() {
    let mut lumen = Lumen::start("cue-command");
    let ctx = lumen.harness.ctx.clone();
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Solo or Cue".into()));
    lumen.frames(2);
    let shown = lumen.app().palette_shows(&ctx);
    assert!(
        shown.iter().any(|(name, on)| name == "Solo or Cue" && !on),
        "greyed without a cue pair: {shown:?}"
    );
    lumen.key(Modifiers::NONE, Key::Escape);

    choose_cue_pair(&mut lumen);
    assert!(!lumen.is_on("Solo/Cue switch: Cue"));
    super::lane_view::run_command(&mut lumen, "Solo or Cue");
    assert!(lumen.is_on("Solo/Cue switch: Cue"));
    super::lane_view::run_command(&mut lumen, "Solo or Cue");
    assert!(!lumen.is_on("Solo/Cue switch: Cue"));
}
