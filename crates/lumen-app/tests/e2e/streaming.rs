//! Playing audio from disk (M9 slice R6): with "Play audio from disk" on, a
//! long file plays from its stream file and is heard as it was from memory;
//! a short one stays in memory; turning the switch off loads them back.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::ClipContent;

use super::clip_commands::command;
use super::{Lumen, TempDir, wav, wav_at, wav_rated};

/// The loudest left sample of a stretch of playback.
fn loudest(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(100), 4_800)
        .iter()
        .fold(0.0f32, |peak, f| peak.max(f[0].abs()))
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| {
        app.is_playing() && app.playhead() > Ticks::quarters(1)
    });
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
}

#[test]
fn a_long_file_plays_from_disk_and_a_short_one_from_memory() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming"), |launch| {
        launch.playing_from_disk()
    });
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    let short = wav(&lumen.dir().join("short.wav"), 2.0);
    lumen.drop_files(&[long, short]);
    let mut from_disk = lumen.app().media_from_disk();
    from_disk.sort();
    assert_eq!(from_disk, [("long".into(), true), ("short".into(), false)]);

    play(&mut lumen);
    let peak = loudest(&mut lumen);
    // Both tones at 3,000 of 32,767, on two tracks at unity.
    assert!(peak > 0.15, "the streamed tone is heard: {peak}");
    let reading = lumen.app().disk_reading().expect("the disk thread runs");
    assert_eq!(reading.failed, 0);
    stop(&mut lumen);
}

#[test]
fn the_switch_loads_files_again_in_memory_or_from_disk() {
    let mut lumen = Lumen::start("streaming-switch");
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    lumen.drop_files(&[long]);
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), false)]);
    assert_eq!(lumen.app().disk_reading(), None, "no disk thread while off");

    lumen.app_mut().set_playing_from_disk(true);
    lumen.wait_for("the file to play from disk", |app| {
        app.media_from_disk() == [("long".into(), true)]
    });
    play(&mut lumen);
    let streamed = loudest(&mut lumen);
    stop(&mut lumen);

    lumen.app_mut().set_playing_from_disk(false);
    lumen.wait_for("the file to be in memory again", |app| {
        app.media_from_disk() == [("long".into(), false)] && app.disk_reading().is_none()
    });
    play(&mut lumen);
    let in_memory = loudest(&mut lumen);
    stop(&mut lumen);
    assert!(streamed > 0.05, "heard from disk: {streamed}");
    assert!(
        (streamed - in_memory).abs() < 1e-3,
        "the same from disk ({streamed}) as from memory ({in_memory})"
    );
}

/// The waveform peaks kept in the test's cache folder.
fn kept_peaks(lumen: &Lumen) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(lumen.dir().join("Cache"))
        .map(|files| {
            files
                .flatten()
                .map(|f| f.path())
                .filter(|p| p.extension().is_some_and(|e| e == "peaks"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_file_opened_again_shows_its_waveform_from_the_cache() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming-peaks"), |launch| {
        launch.playing_from_disk()
    });
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    lumen.drop_files(std::slice::from_ref(&long));
    let kept = kept_peaks(&lumen);
    assert_eq!(kept.len(), 1, "the long file's peaks are kept: {kept:?}");
    // Aged, so a hit is seen as the entry being used now.
    let aged = std::time::UNIX_EPOCH + Duration::from_secs(1_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&kept[0])
        .unwrap()
        .set_modified(aged)
        .unwrap();

    // A new session opens the same file.
    let dir = lumen.crash();
    let mut lumen = Lumen::start_with(dir, |launch| launch.playing_from_disk());
    lumen.drop_files(&[long]);
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), true)]);
    assert_eq!(kept_peaks(&lumen), kept, "read, not made again");
    let used = std::fs::metadata(&kept[0]).unwrap().modified().unwrap();
    assert!(used > aged, "the waveform came from the cache");
}

/// The quietest stretch of 10 ms in `heard`: zero if any stretch was silent.
fn quietest(heard: &[[f32; 4]]) -> f32 {
    heard
        .chunks(480)
        .map(|stretch| stretch.iter().fold(0.0f32, |peak, f| peak.max(f[0].abs())))
        .fold(f32::MAX, f32::min)
}

#[test]
fn a_loop_and_a_jump_play_from_disk_without_a_gap() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming-loop"), |launch| {
        launch.playing_from_disk()
    });
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    lumen.drop_files(&[long]);
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), true)]);

    // A loop over the clip's second and third beats, once its row settled.
    lumen.frames(30);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    let rect = lumen.app().clip_rect(&lumen.harness.ctx, clip).unwrap();
    let zoom = lumen.app().view().zoom as f32;
    let y = rect.bottom() - 6.0;
    let from = eframe::egui::pos2(rect.left() + zoom, y);
    let to = eframe::egui::pos2(rect.left() + zoom * 3.0, y);
    lumen.drag(from, to, Modifiers::NONE);
    assert!(lumen.app().time_range().is_some(), "a time selection");
    lumen.key(Modifiers::COMMAND, Key::L);
    let (start, end, on) = lumen.app().engine_loop();
    assert!(on && end > start, "the loop plays");

    play(&mut lumen);
    // Two seconds: the one-second loop goes round twice.
    let looped = lumen.listen(Duration::from_millis(100), 96_000);
    assert!(quietest(&looped) > 0.04, "no gap: {}", quietest(&looped));

    // Back to the start while playing, then on into the loop again.
    lumen.key(Modifiers::NONE, Key::Home);
    let jumped = lumen.listen(Duration::from_millis(100), 48_000);
    assert!(quietest(&jumped) > 0.04, "no gap: {}", quietest(&jumped));
    stop(&mut lumen);
    let reading = lumen.app().disk_reading().expect("the disk thread runs");
    assert_eq!((reading.underruns, reading.failed), (0, 0));
}

#[test]
fn a_disk_too_slow_to_keep_up_is_reported_as_late() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming-late"), |launch| {
        launch
            .playing_from_disk()
            .with_slow_disk(Duration::from_millis(250))
    });
    let long = wav(&lumen.dir().join("long.wav"), 40.0);
    lumen.drop_files(&[long]);
    assert!(!lumen.shows("Audio: Disk late"));

    // Play waits at most 150 ms for the first pages (the owner's L A), then
    // starts without them.
    play(&mut lumen);
    lumen.wait_for("a late read", |app| {
        app.disk_reading().is_some_and(|r| r.underruns > 0)
    });
    // The chip says "Disk late", or "Dropouts" when the test device also
    // fell behind (dropouts are the worse news, so they come first); either
    // opens the audio panel, which counts the late reads.
    lumen.frames(3);
    lumen
        .harness
        .query_by_label_contains("Audio: Disk late")
        .or_else(|| lumen.harness.query_by_label_contains("Audio: Dropout"))
        .expect("the warning chip")
        .click();
    lumen.run_until("the panel's count of late reads", |l| {
        l.harness
            .query_by_label_contains("late disk read")
            .is_some()
    });
    stop(&mut lumen);
}

fn decode(path: &std::path::Path) -> lumen_media::Audio {
    lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

/// Exports the whole project to `path` from the export window.
fn export(lumen: &mut Lumen, path: &std::path::Path) {
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    lumen.app_mut().answer_next_dialog(vec![path.to_owned()]);
    lumen.click("Export audio…");
    lumen.wait_for("the export", |a| !a.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
}

/// Bounces the selected clip with the popover's choices, and returns the
/// bounced file's audio.
fn bounce(lumen: &mut Lumen) -> lumen_media::Audio {
    command(lumen, "Bounce");
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));
    let project = lumen.app().project();
    let ClipContent::Audio { media, .. } = project.tracks.last().unwrap().clips[0].content else {
        panic!("an audio clip")
    };
    let media = project.media.iter().find(|m| m.id == media).unwrap();
    decode(std::path::Path::new(&media.path))
}

#[test]
fn export_and_bounce_are_the_same_from_disk_and_from_memory() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming-render"), |launch| {
        launch.playing_from_disk()
    });
    let long = wav_at(&lumen.dir().join("long.wav"), 35.0, 5_000.0);
    lumen.drop_files(&[long]);
    lumen.wait_for("import", |a| !a.is_busy() && !a.project().tracks.is_empty());
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), true)]);
    let clip = lumen.app().project().tracks[0].clips[0].id;

    let streamed = lumen.dir().join("streamed.wav");
    export(&mut lumen, &streamed);
    lumen.app_mut().select_clip(clip);
    let bounced_streamed = bounce(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);

    lumen.app_mut().set_playing_from_disk(false);
    lumen.wait_for("the files in memory", |app| {
        let media = app.media_from_disk();
        media.iter().any(|(name, _)| name == "long")
            && media.iter().all(|(_, from_disk)| !from_disk)
    });
    let in_memory = lumen.dir().join("in-memory.wav");
    export(&mut lumen, &in_memory);
    lumen.app_mut().select_clip(clip);
    let bounced_in_memory = bounce(&mut lumen);

    let (a, b) = (decode(&streamed), decode(&in_memory));
    assert!(
        a.channel(0).iter().any(|s| s.abs() > 0.1),
        "the export is heard"
    );
    assert_eq!(a, b, "the export is the same");
    assert_eq!(
        bounced_streamed, bounced_in_memory,
        "the bounce is the same"
    );
}

#[test]
fn a_file_changed_on_disk_is_converted_again() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-streaming-changed"), |launch| {
        launch.playing_from_disk()
    });
    let path = lumen.dir().join("long.wav");
    let long = wav(&path, 40.0);
    lumen.drop_files(std::slice::from_ref(&long));
    play(&mut lumen);
    let before = loudest(&mut lumen);
    stop(&mut lumen);

    // The file is recorded over, louder, and opened again in a new session.
    let dir = lumen.crash();
    wav_at(&path, 40.0, 9_000.0);
    let mut lumen = Lumen::start_with(dir, |launch| launch.playing_from_disk());
    lumen.drop_files(&[long]);
    assert_eq!(lumen.app().media_from_disk(), [("long".into(), true)]);
    play(&mut lumen);
    let after = loudest(&mut lumen);
    stop(&mut lumen);
    assert!(
        after > before * 2.5,
        "the new recording is heard, not the old copy: {before} then {after}"
    );
}

/// This process's private memory, in MB, as the system counts it.
fn private_mb() -> f64 {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-Process -Id {}).PrivateMemorySize64",
                std::process::id()
            ),
        ])
        .output()
        .expect("powershell runs");
    let bytes: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    bytes / 1e6
}

/// Opens `project` in `lumen` and waits until every file of it is ready:
/// the time that took, and the memory it added.
fn open_and_measure(lumen: &mut Lumen, project: &std::path::Path, files: usize) -> (f64, f64) {
    lumen.frames(30);
    let before = private_mb();
    let started = std::time::Instant::now();
    lumen.app_mut().open_project_file(project.to_owned());
    loop {
        lumen.frames(1);
        if !lumen.app().is_busy() && lumen.app().media_from_disk().len() == files {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "the project opens"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let seconds = started.elapsed().as_secs_f64();
    lumen.frames(30);
    (seconds, private_mb() - before)
}

/// One measurement of [`measure_streaming_memory_and_open_time`]: 64 files
/// at `rate` (a rate other than the device's is converted on a cold cache).
fn measure(rate: u32) {
    const FILES: usize = 64;
    let media = TempDir::new("measure-streaming-media");
    let files: Vec<_> = (0..FILES)
        .map(|i| {
            wav_rated(
                &media.path().join(format!("t{i:02}.wav")),
                120.0,
                3_000.0,
                rate,
            )
        })
        .collect();
    let project = media.path().join("song.lumen");
    {
        let mut lumen = Lumen::start("measure-streaming-save");
        lumen.drop_files(&files);
        lumen.app_mut().save_to(project.clone());
        lumen.wait_for("the save", |app| !app.is_dirty() && !app.is_busy());
    }

    let mut cold = Lumen::start_with(TempDir::new("measure-streaming-disk"), |launch| {
        launch.playing_from_disk()
    });
    let (cold_s, cold_mb) = open_and_measure(&mut cold, &project, FILES);
    let dir = cold.crash();
    let mut warm = Lumen::start_with(dir, |launch| launch.playing_from_disk());
    let (warm_s, warm_mb) = open_and_measure(&mut warm, &project, FILES);
    assert!(warm.app().media_from_disk().iter().all(|(_, d)| *d));
    drop(warm);

    let mut memory = Lumen::start("measure-streaming-memory");
    let (memory_s, memory_mb) = open_and_measure(&mut memory, &project, FILES);
    drop(memory);
    eprintln!(
        "measure: {FILES} files of 120 s at {rate} Hz · in memory {memory_s:.2} s, \
         {memory_mb:.0} MB · from disk, cold cache {cold_s:.2} s, {cold_mb:.0} MB · warm \
         cache {warm_s:.2} s, {warm_mb:.0} MB"
    );
}

/// R6 step 7's measurement: 64 stereo files of two minutes, opened with
/// audio in memory, then from disk with a cold cache and a warm one, at the
/// device's rate (played as they are) and at 44.1 kHz (converted). Windows
/// only (it asks PowerShell for the memory). Run in release, with the
/// temporary folder on a drive with 8 GB free:
/// `cargo test --release -p lumen-app --test e2e measure_streaming -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement: writes 3 GB of files and takes minutes"]
fn measure_streaming_memory_and_open_time() {
    measure(48_000);
    measure(44_100);
}

/// Writes the project MT-STREAM-1 plays: 64 tracks, each a three-minute
/// stereo file of its own (a quiet chord of two tones, different on every
/// track, so a gap in one is heard), into the folder named by
/// `LUMEN_STREAM_TEST_DIR`, with `Streaming test.lumen` beside them:
/// `$env:LUMEN_STREAM_TEST_DIR = "D:\LumenStreamTest"; cargo test --release -p lumen-app --test e2e make_the_streaming_test_project -- --ignored`.
#[test]
#[ignore = "writes 2.2 GB for a manual test"]
fn make_the_streaming_test_project() {
    use lumen_media::wav::{Sample, Spec, Writer};

    let Some(dir) = std::env::var_os("LUMEN_STREAM_TEST_DIR").map(std::path::PathBuf::from) else {
        panic!("set LUMEN_STREAM_TEST_DIR to the folder to write the files into");
    };
    std::fs::create_dir_all(&dir).unwrap();
    let rate = 48_000.0f32;
    let frames = 180 * 48_000;
    let files: Vec<_> = (0..64)
        .map(|track| {
            let path = dir.join(format!("Track {:02}.wav", track + 1));
            let (low, high) = (110.0 * (1.0 + track as f32 / 16.0), 1.5);
            let mut writer = Writer::create(
                &path,
                Spec {
                    sample_rate: 48_000,
                    channels: 2,
                    sample: Sample::I16,
                },
            )
            .unwrap();
            let mut left = vec![0.0f32; 48_000];
            let mut right = vec![0.0f32; 48_000];
            for second in 0..frames / 48_000 {
                for i in 0..48_000 {
                    let t = (second * 48_000 + i) as f32 / rate;
                    let phase = std::f32::consts::TAU * low * t;
                    left[i] = 0.01 * (phase.sin() + (phase * high).sin());
                    right[i] = 0.01 * (phase.sin() - (phase * high).sin());
                }
                writer.write(&[&left, &right]).unwrap();
            }
            writer.finish().unwrap();
            path
        })
        .collect();
    let mut lumen = Lumen::start("make-streaming-test");
    lumen.drop_files(&files);
    lumen.wait_for("the files", |app| app.project().tracks.len() == 64);
    lumen.app_mut().save_to(dir.join("Streaming test.lumen"));
    lumen.wait_for("the save", |app| !app.is_dirty() && !app.is_busy());
}
