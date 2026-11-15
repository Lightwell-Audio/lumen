use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::{Lumen, wav, wav_at};

fn tone_db() -> f32 {
    20.0 * (3_000.0f32 / 32_768.0).log10()
}

fn unfold(lumen: &mut Lumen, track: &str) {
    lumen.click(&format!("Fold {track}"));
    lumen.run_until(&format!("{track} to unfold"), |l| {
        l.harness
            .query_by_label_contains(&format!("Volume {track}:"))
            .is_some()
    });
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
}

fn listen(lumen: &mut Lumen, ms: u64) {
    for _ in 0..ms / 50 {
        lumen.frames(2);
        std::thread::sleep(Duration::from_millis(50));
        lumen.frames(1);
    }
}

fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn a_playing_tracks_level_shows_and_comes_to_rest_when_stopped() {
    let mut lumen = Lumen::start("meters-level");
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    let master = lumen.app().project().master.id;
    play(&mut lumen);
    listen(&mut lumen, 400);
    let want = tone_db();
    lumen.wait_for("the track and the master at the tone's level", |app| {
        (app.meter(track).peak_db - want).abs() < 0.3
            && (app.meter(master).peak_db - want).abs() < 0.3
    });
    listen(&mut lumen, 50);
    let shown = lumen.app().meter(track);
    assert!((shown.peak_db - want).abs() < 0.3, "{shown:?}, want {want}");
    assert!(shown.rms_db < shown.peak_db && shown.rms_db > want - 4.0);
    assert!(!shown.clipped);
    let on_master = lumen.app().meter(master);
    assert!((on_master.peak_db - want).abs() < 0.3, "{on_master:?}");
    assert!(lumen.app().meters_moving());

    lumen.key(Modifiers::NONE, Key::Space);
    listen(&mut lumen, 3_500);
    let rested = lumen.app().meter(track);
    assert!(rested.peak_db.is_infinite(), "{rested:?}");
    assert!(rested.hold_db.is_infinite(), "{rested:?}");
    assert!(
        rested.rms_db < shown.rms_db - 6.0,
        "{rested:?} after {shown:?}"
    );
}

#[test]
fn a_clip_stays_lit_until_clicked_and_the_palette_clears_every_one() {
    let mut lumen = Lumen::start("meters-clip");
    let files = [
        wav_at(&lumen.dir().join("loud.wav"), 20.0, 32_000.0),
        wav_at(&lumen.dir().join("loud2.wav"), 20.0, 32_000.0),
    ];
    lumen.drop_files(&files);
    lumen.frames(5);
    let names = lumen.track_names();
    let ids: Vec<_> = lumen.app().project().tracks.iter().map(|t| t.id).collect();
    for name in &names {
        unfold(&mut lumen, name);
        let volume = lumen
            .harness
            .get_by_label_contains(&format!("Volume {name}:"))
            .rect();
        lumen.drag(
            volume.center(),
            volume.center() + vec2(400.0, 0.0),
            Modifiers::NONE,
        );
    }
    play(&mut lumen);
    listen(&mut lumen, 300);
    lumen.key(Modifiers::NONE, Key::Space);
    for &id in &ids {
        assert!(lumen.app().meter(id).clipped, "{:?}", lumen.app().meter(id));
    }
    listen(&mut lumen, 600);
    assert!(lumen.app().meter(ids[0]).clipped);

    lumen.click(&format!("Clear the clip light of {}", names[0]));
    assert!(!lumen.app().meter(ids[0]).clipped);
    assert!(lumen.app().meter(ids[1]).clipped);
    assert!(!lumen.shows(&format!("Clear the clip light of {}", names[0])));

    palette(&mut lumen, "Clear clip lights");
    assert!(!lumen.app().meter(ids[1]).clipped);
}

#[test]
fn integrated_loudness_starts_again_on_play_from_stop() {
    let mut lumen = Lumen::start("meters-loudness");
    let file = wav(&lumen.dir().join("tone.wav"), 30.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    play(&mut lumen);
    listen(&mut lumen, 1_000);
    let (momentary, _, integrated, true_peak) = lumen.app().loudness();
    assert!(
        momentary.is_finite() && integrated.is_finite(),
        "{momentary} {integrated}"
    );
    assert!(true_peak < tone_db() + 1.0, "{true_peak}");

    lumen.key(Modifiers::SHIFT, Key::Space);
    lumen.wait_for("the pause", |app| !app.is_playing());
    lumen.key(Modifiers::SHIFT, Key::Space);
    lumen.wait_for("playback to continue", |app| app.is_playing());
    assert!(lumen.app().loudness().2.is_finite());

    lumen.key(Modifiers::NONE, Key::Space);
    listen(&mut lumen, 200);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(2);
    assert!(
        lumen.app().loudness().2.is_infinite(),
        "{:?}",
        lumen.app().loudness()
    );
}

fn reference_tone(path: &std::path::Path, dbfs: f64, seconds: f64) -> std::path::PathBuf {
    let amplitude = 10f64.powf(dbfs / 20.0) * 32_767.0;
    let frames = (seconds * 48_000.0) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let t = i as f64 / 48_000.0;
            let v = (amplitude * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin()).round() as i16;
            let [a, b] = v.to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(48_000_u32.to_le_bytes());
    out.extend((48_000_u32 * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the reference tone");
    path.to_path_buf()
}

#[test]
fn the_loudness_column_reads_the_reference_tone_at_minus_23() {
    let mut lumen = Lumen::start("meters-reference");
    let file = reference_tone(&lumen.dir().join("ref.wav"), -23.0, 30.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    play(&mut lumen);
    listen(&mut lumen, 3_000);
    let (momentary, _, integrated, true_peak) = lumen.app().loudness();
    let want = -23.0;
    assert!((momentary - want).abs() < 0.1, "momentary {momentary}");
    assert!((integrated - want).abs() < 0.1, "integrated {integrated}");
    assert!(
        (f64::from(true_peak) - want).abs() < 0.2,
        "true peak {true_peak}"
    );
}

fn sine_file(
    path: &std::path::Path,
    rate: u32,
    frequency: f64,
    phase: f64,
    peak: f64,
) -> std::path::PathBuf {
    let frames = rate as usize * 20;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let t = i as f64 / f64::from(rate);
            let v =
                (peak * (2.0 * std::f64::consts::PI * frequency * t + phase).sin()).round() as i16;
            let [a, b] = v.to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the sine");
    path.to_path_buf()
}

#[test]
fn full_scale_reads_zero_and_a_converted_file_shows_its_true_peak() {
    let cases = [
        (48_000, 1_000.0, 0.0, 32_767.0, 0.0),
        (
            44_100,
            11_025.0,
            std::f64::consts::FRAC_PI_4,
            32_767.0 * std::f64::consts::SQRT_2,
            3.01,
        ),
    ];
    for (rate, frequency, phase, amplitude, want) in cases {
        let mut lumen = Lumen::start("meters-full-scale");
        let file = sine_file(
            &lumen.dir().join("full.wav"),
            rate,
            frequency,
            phase,
            amplitude,
        );
        lumen.drop_files(&[file]);
        lumen.frames(5);
        let track = lumen.app().project().tracks[0].id;
        let master = lumen.app().project().master.id;
        play(&mut lumen);
        listen(&mut lumen, 600);
        lumen.wait_for(&format!("{rate} Hz read at {want} dB"), |app| {
            [track, master]
                .iter()
                .all(|id| (app.meter(*id).peak_db - want).abs() < 0.05)
        });
    }
}
