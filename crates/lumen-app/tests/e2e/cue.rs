//! Cue (M4 slice 4, the owner's C1 and O1): a cue pair chosen in the audio
//! settings, the master's Solo/Cue switch, headphone buttons that cue a
//! track before its fader, and the metronome on the cue pair. The test
//! device has four outputs; the owner's interface has two, so this is where
//! cue is checked (MT-MIX-4 notes it).

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::{Lumen, wav};

/// The tone's peak in `wav`.
const TONE: f32 = 3_000.0 / 32_768.0;

/// Lets audio play past any glide, then the loudest main and cue samples
/// (left) of the next stretch.
fn loudest(lumen: &mut Lumen) -> (f32, f32) {
    // Main and cue together.
    let heard = lumen.listen(Duration::from_millis(150), 1_000);
    heard.iter().fold((0.0f32, 0.0f32), |(m, c), f| {
        (m.max(f[0].abs()), c.max(f[2].abs()))
    })
}

/// Chooses the test device's outputs 3 and 4 as the cue pair.
fn choose_cue_pair(lumen: &mut Lumen) {
    lumen.click("Lumen");
    lumen.click("Audio and MIDI settings\u{2026}");
    // The pair is listed under Output and under Cue: the second is the cue.
    let label = "3/4   Out 3 / Out 4";
    // The window lists the device's outputs once its background look-up
    // answers, which on a loaded machine can take a while: wait for both
    // lists, or the last one found would be the Output list's.
    lumen.run_until("the outputs listed under Output and Cue", |l| {
        l.harness.query_all_by_label(label).count() >= 2
    });
    let cue = lumen
        .harness
        .query_all_by_label(label)
        .last()
        .expect("the cue pair is offered");
    cue.click();
    lumen.frames(3);
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

    // Without a cue pair the switch says what is missing.
    lumen.click("Solo/Cue switch: Cue");
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("Audio settings"))
    );
    assert!(lumen.shows(&format!("Solo {track}")));

    choose_cue_pair(&mut lumen);
    lumen.click("Solo/Cue switch: Cue");
    assert!(lumen.is_on("Solo/Cue switch: Cue"));
    // The S buttons are headphone buttons now.
    assert!(!lumen.shows(&format!("Solo {track}")));
    lumen.click(&format!("Cue {track}"));
    assert!(lumen.is_on(&format!("Cue {track}")));

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let (main, cue) = loudest(&mut lumen);
    // A stereo track at the centre keeps its level (the owner's B).
    assert!((main - TONE).abs() < TONE * 0.1, "main {main}");
    assert!(
        (cue - TONE).abs() < TONE * 0.1,
        "cue before the fader: {cue}"
    );
    // Muted: gone from the main pair, still in the headphones.
    lumen.click(&format!("Mute {track}"));
    let (main, cue) = loudest(&mut lumen);
    assert!(main < 1e-4 && cue > TONE * 0.9, "main {main}, cue {cue}");
    // Back to Solo: nothing is cued any more.
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
    // Two seconds hold four clicks at 120 BPM.
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
