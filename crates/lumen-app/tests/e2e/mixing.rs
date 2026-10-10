//! The mixer (M4): what a track's controls do to what is heard.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::{Lumen, wav};

/// The test tone's peak (`wav`): 3,000 of 32,768.
const TONE: f32 = 3_000.0 / 32_768.0;

/// A playing project with one track holding a long tone; returns the
/// track's name.
fn playing(name: &str) -> (Lumen, String) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.track_names()[0].clone();
    // Unfolded, the header shows volume and pan once the row has grown.
    lumen.click(&format!("Fold {track}"));
    for _ in 0..100 {
        if lumen
            .harness
            .query_by_label_contains(&format!("Pan {track}:"))
            .is_some()
        {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    (lumen, track)
}

/// Lets `ms` of audio play (past any glide), then returns the loudest
/// left and right samples of the next `ms`.
fn levels(lumen: &mut Lumen, ms: u64) -> (f32, f32) {
    std::thread::sleep(Duration::from_millis(ms));
    lumen.frames(1);
    lumen.app_mut().heard();
    std::thread::sleep(Duration::from_millis(ms));
    lumen.frames(1);
    let heard = lumen.app_mut().heard();
    assert!(heard.len() > 1_000, "heard {} frames", heard.len());
    heard.iter().fold((0.0f32, 0.0f32), |(l, r), s| {
        (l.max(s.0.abs()), r.max(s.1.abs()))
    })
}

fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= want.max(1e-3) * 0.1
}

#[test]
fn a_tracks_pan_and_mute_are_heard_through_the_mixer() {
    let (mut lumen, track) = playing("mixer-pan-mute");
    // Centred, a stereo track keeps its level on both sides (the owner's
    // B, after M4 slice 7; the pan law is for mono tracks).
    let (left, right) = levels(&mut lumen, 150);
    assert!(near(left, TONE) && near(right, TONE), "{left} {right}");

    // Pan hard left: the left side at full level, the right silent.
    let pan = lumen
        .harness
        .get_by_label_contains(&format!("Pan {track}:"))
        .rect();
    lumen.drag(
        pan.center(),
        pan.center() - vec2(400.0, 0.0),
        Modifiers::NONE,
    );
    assert_eq!(lumen.app().project().tracks[0].pan, -1.0);
    let (left, right) = levels(&mut lumen, 150);
    assert!(near(left, TONE) && right < 1e-4, "{left} {right}");

    // Mute: silence.
    lumen.click(&format!("Mute {track}"));
    let (left, right) = levels(&mut lumen, 150);
    assert!(left < 1e-4 && right < 1e-4, "{left} {right}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn mixer_changes_while_playing_never_click() {
    let (mut lumen, track) = playing("mixer-no-click");
    lumen.app_mut().heard();
    // Volume and pan dragged back and forth, mute and solo toggled, a step
    // per screen frame.
    let volume = lumen
        .harness
        .get_by_label_contains(&format!("Volume {track}:"))
        .rect();
    let pan = lumen
        .harness
        .get_by_label_contains(&format!("Pan {track}:"))
        .rect();
    for (slider, reach) in [(volume, 60.0), (pan, 30.0)] {
        lumen.harness.hover_at(slider.center());
        lumen.harness.drag_at(slider.center());
        for step in 0..16 {
            let x = [-1.0, 0.0, 1.0, 0.0][step % 4] * reach;
            lumen.harness.hover_at(slider.center() + vec2(x, 0.0));
            lumen.frames(1);
            std::thread::sleep(Duration::from_millis(16));
        }
        lumen.harness.drop_at(slider.center());
        lumen.frames(2);
    }
    for control in ["Mute", "Mute", "Solo", "Solo"] {
        lumen.click(&format!("{control} {track}"));
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(50));
    lumen.frames(1);
    let heard = lumen.app_mut().heard();
    assert!(lumen.app().is_playing());
    assert!(heard.len() > 2_000, "heard {} frames", heard.len());
    // The tone moves by at most TONE × 0.05 a frame; a stepped level change
    // would jump by far more.
    let steepest = heard
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).abs().max((w[1].1 - w[0].1).abs()))
        .fold(0.0, f32::max);
    assert!(steepest < TONE * 0.065, "a click: a step of {steepest}");
}

/// A mono WAV of the test tone (as `wav`, one channel).
fn mono_wav(path: &std::path::Path, seconds: f32) -> std::path::PathBuf {
    let frames = (seconds * 48_000.0) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| (((i as f32 * 0.05).sin() * 3_000.0) as i16).to_le_bytes())
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(48_000_u32.to_le_bytes());
    out.extend((48_000_u32 * 2).to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the mono WAV");
    path.to_path_buf()
}

/// The owner's B (after M4 slice 7): the pan law is for mono material. A
/// mono file centred plays 3 dB down per side by default, and follows the
/// project's law; a stereo file beside it keeps its level.
#[test]
fn a_mono_track_follows_the_pan_law_and_a_stereo_one_keeps_its_level() {
    let mut lumen = Lumen::start("mixer-mono-law");
    let mono = mono_wav(&lumen.dir().join("mono.wav"), 20.0);
    lumen.drop_files(&[mono]);
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let centre = TONE * std::f32::consts::FRAC_1_SQRT_2;
    let (left, right) = levels(&mut lumen, 150);
    assert!(near(left, centre) && near(right, centre), "{left} {right}");

    // At 0 dB, balance: both sides at the file's level.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Pan law: 0 dB".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    let (left, right) = levels(&mut lumen, 150);
    assert!(near(left, TONE) && near(right, TONE), "{left} {right}");
    lumen.key(Modifiers::NONE, Key::Space);
}
