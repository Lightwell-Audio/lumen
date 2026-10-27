//! M9 R5: actual export workflows through names, keys and file-dialog answers.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use eframe::egui::{Key, Modifiers};

use super::copies::command;
use super::{Lumen, wav};

fn audio(name: &str, seconds: f32) -> Lumen {
    let mut lumen = Lumen::start(name);
    let input = wav(&lumen.dir().join("take.wav"), seconds);
    lumen.drop_files(&[input]);
    lumen.wait_for("import", |a| !a.is_busy() && !a.project().tracks.is_empty());
    lumen
}

fn open(lumen: &mut Lumen) {
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    assert!(lumen.app().audio_export_open());
}

fn export(lumen: &mut Lumen, path: &Path) {
    lumen.app_mut().answer_next_dialog(vec![path.to_owned()]);
    lumen.click("Export audio…");
    lumen.wait_for("export completion", |a| !a.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    assert!(!lumen.app().audio_export_files().is_empty());
}

fn decode(path: &Path) -> lumen_media::Audio {
    lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

#[test]
fn export_opens_by_key_and_palette_and_leaves_history_untouched() {
    let mut lumen = audio("export-open", 0.2);
    let before = lumen.app().project().clone();
    let undo = lumen.app().undo_label().map(str::to_owned);
    open(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(!lumen.app().audio_export_open());
    command(&mut lumen, "Export audio");
    lumen.frames(4);
    assert!(lumen.app().audio_export_open());
    let output = lumen.dir().join("mix.wav");
    export(&mut lumen, &output);
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), undo.as_deref());
    assert!(lumen.shows("Export complete"));
    let audio = decode(&output);
    assert_eq!(audio.sample_rate, 48_000);
    assert!(audio.channel(0).iter().any(|s| s.abs() > 0.01));
}

#[test]
fn flac_rate_dither_and_normalization_are_real_decodable_audio() {
    let mut lumen = audio("export-flac", 0.3);
    open(&mut lumen);
    lumen.click("File format: WAV · 32-bit float");
    lumen.click("FLAC · 24-bit");
    lumen.click("Export sample rate: 48.0 kHz");
    lumen.click("44.1 kHz");
    lumen.click("Normalize peak to −1 dBFS");
    lumen.click("TPDF dither");
    let output = lumen.dir().join("mix.flac");
    export(&mut lumen, &output);
    let audio = decode(&output);
    assert_eq!(audio.sample_rate, 44_100);
    let peak = audio
        .channel(0)
        .iter()
        .chain(audio.channel(1))
        .fold(0f32, |p, x| p.max(x.abs()));
    assert!((peak - 10f32.powf(-1.0 / 20.0)).abs() < 0.0001);
}

#[test]
fn device_driven_and_offline_exports_match_sample_for_sample() {
    let mut lumen = audio("export-real-time", 0.2);
    open(&mut lumen);
    lumen.click("Include effect tails");
    let offline = lumen.dir().join("offline.wav");
    export(&mut lumen, &offline);
    lumen.click("Render mode: Offline");
    lumen.click("Real-time");
    let realtime = lumen.dir().join("realtime.wav");
    export(&mut lumen, &realtime);
    assert_eq!(decode(&offline), decode(&realtime));
}

#[test]
fn track_stems_write_to_a_folder_and_existing_stems_are_preserved() {
    let mut lumen = audio("export-stems", 0.2);
    open(&mut lumen);
    lumen.click("Output: Master mix");
    lumen.click("Track stems");
    let output = lumen.dir().join("Stems");
    export(&mut lumen, &output);
    let files = lumen.app().audio_export_files();
    assert_eq!(files.len(), 1);
    let bytes = std::fs::read(&files[0]).unwrap();
    assert!(decode(&files[0]).channel(0).iter().any(|s| s.abs() > 0.01));
    lumen.app_mut().answer_next_dialog(vec![output]);
    lumen.click("Export audio…");
    lumen.wait_for("collision", |a| !a.audio_export_busy());
    assert!(
        lumen
            .app()
            .audio_export_error()
            .unwrap()
            .contains("already exists")
    );
    assert_eq!(std::fs::read(&files[0]).unwrap(), bytes);
}

#[test]
fn a_time_selection_is_required_when_that_range_is_chosen() {
    let mut lumen = audio("export-no-selection", 0.2);
    open(&mut lumen);
    lumen.click("Range: Whole project");
    lumen.click("Time selection");
    lumen.frames(4);
    assert!(lumen.shows("Choose a non-empty project, loop or time selection"));
    assert!(!lumen.app().audio_export_busy());
}

#[test]
fn cancelling_a_real_time_export_keeps_the_existing_destination() {
    let mut lumen = audio("export-cancel", 5.0);
    open(&mut lumen);
    lumen.click("Render mode: Offline");
    lumen.click("Real-time");
    let output = lumen.dir().join("keep.wav");
    std::fs::write(&output, b"previous export").unwrap();
    lumen.app_mut().answer_next_dialog(vec![output.clone()]);
    lumen.click("Export audio…");
    lumen.wait_for("running export", |a| a.audio_export_busy());
    lumen.run_until("cancel control", |l| l.shows("Cancel export"));
    lumen.click("Cancel export");
    lumen.wait_for("stopped export", |a| !a.audio_export_busy());
    assert_eq!(std::fs::read(output).unwrap(), b"previous export");
}

#[test]
fn loop_and_time_selection_export_only_the_requested_samples() {
    let mut lumen = audio("export-range", 4.0);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    lumen.key(Modifiers::COMMAND, Key::L);
    open(&mut lumen);
    lumen.click("Include effect tails");
    lumen.click("Range: Whole project");
    lumen.click("Loop region");
    let loop_file = lumen.dir().join("loop.wav");
    export(&mut lumen, &loop_file);
    assert_eq!(decode(&loop_file).frames(), 192_000);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    let rect = lumen.app().clip_rect(&lumen.harness.ctx, clip).unwrap();
    let zoom = lumen.app().view().zoom as f32;
    let start = eframe::egui::pos2(rect.left() + zoom, rect.bottom() - 6.0);
    let end = eframe::egui::pos2(rect.left() + zoom * 3.0, rect.bottom() - 6.0);
    lumen.drag(start, end, Modifiers::NONE);
    let (a, b, _) = lumen.app().time_range().expect("time selection");
    let expected = ((b.0 - a.0) as f64 / lumen_core::time::PPQ as f64 * 24_000.0).round() as usize;
    open(&mut lumen);
    lumen.click("Range: Loop region");
    lumen.click("Time selection");
    let selection = lumen.dir().join("selection.wav");
    export(&mut lumen, &selection);
    assert_eq!(decode(&selection).frames(), expected);
}

#[test]
fn groups_and_returns_export_from_the_window() {
    let mut lumen = audio("export-group-return", 0.2);
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.click("Add track");
    lumen.click("Add return track");
    open(&mut lumen);
    lumen.click("Output: Master mix");
    lumen.click("Group stems");
    let folder = lumen.dir().join("Groups");
    export(&mut lumen, &folder);
    assert!(
        decode(&lumen.app().audio_export_files()[0])
            .channel(0)
            .iter()
            .any(|s| s.abs() > 0.01)
    );
    lumen.click("Output: Group stems");
    lumen.click("Return stems");
    let folder = lumen.dir().join("Returns");
    export(&mut lumen, &folder);
    assert_eq!(lumen.app().audio_export_files().len(), 1);
    // The return has no sends yet: it is correctly silent.
    assert!(
        decode(&lumen.app().audio_export_files()[0])
            .channel(0)
            .iter()
            .all(|s| *s == 0.0)
    );
}

#[test]
fn a_plugin_instrument_exports_in_its_own_host_with_a_tail() {
    use super::hosting::{PLUGIN, instrument_device, launch, timing_sensitive, wait_for_state};
    use lumen_core::time::Ticks;
    use lumen_project::model::Note;
    use lumen_project::testing::{midi_track, project_of};
    let _turn = timing_sensitive();
    let mut track = midi_track(
        1,
        "Keys",
        "poly",
        (0, 2),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 69, 100)],
        vec![],
    );
    track.devices = vec![instrument_device(PLUGIN)];
    let project = project_of("Export instrument", vec![track]);
    let mut lumen = launch("export-instrument", &project, &["Rings"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    open(&mut lumen);
    let file = lumen.dir().join("instrument.wav");
    export(&mut lumen, &file);
    let audio = decode(&file);
    assert!(audio.frames() > 96_000, "the plugin's release is included");
    assert!(audio.channel(0).iter().any(|s| s.abs() > 0.01));
    assert!(
        lumen
            .app()
            .device_state(PLUGIN)
            .is_some_and(|s| s.starts_with("ready"))
    );
}
