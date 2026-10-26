//! Recording audio (M9 slice R2, ADR-0102), end to end on the input test
//! device, which plays a ramp (every frame holds its own number) so a file
//! shows whether any frame was lost, repeated or moved.
//!
//! Record (F9) on an armed audio track writes one file as the input arrives;
//! when recording stops the file is one clip from where recording began, as
//! one undo step. Looping keeps one long file, saving copies the file into
//! the project, and a crash in the middle leaves a file that the next start
//! repairs and puts back on its track. The device runs below real time, so
//! these wait for frames, never for time.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_app::testing::InputDeviceTurn;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Color, Id, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, project_of};

use super::Lumen;

const VOX: Id = Id(21);
const GTR: Id = Id(22);
const RATE: u32 = 48_000;

fn project() -> Project {
    project_of(
        "Take",
        vec![
            Track::new(VOX, "Vox", TrackKind::Audio, Color(0x4F81BD)),
            Track::new(GTR, "Gtr", TrackKind::Audio, Color(0xC0504D)),
        ],
    )
}

/// Starts on the input device playing the ramp, with `project` opened.
fn launch(
    name: &str,
    project: &Project,
    adjust: impl FnOnce(lumen_app::Launch) -> lumen_app::Launch,
) -> (Lumen, InputDeviceTurn) {
    let turn = lumen_app::testing::input_device_turn();
    turn.set(|d| d.input_ramp = true);
    let dir = TempDir::new(&format!("e2e-audio-record-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| adjust(l.with_input_device().opening(path)));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.frames(30);
    (lumen, turn)
}

fn clips_of(lumen: &Lumen, id: Id) -> Vec<std::sync::Arc<Clip>> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == id)
        .expect("the track")
        .clips
        .clone()
}

/// Arms Vox, records until `frames` have been written, and stops.
fn record(lumen: &mut Lumen, frames: u64) {
    lumen.click("Arm Vox");
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the recording to begin", |app| {
        app.recording() && app.is_playing()
    });
    lumen.wait_for("frames to be written", move |app| {
        app.audio_take_frames(VOX).is_some_and(|f| f >= frames)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the clip", |app| {
        !app.recording()
            && app
                .project()
                .tracks
                .iter()
                .find(|t| t.id == VOX)
                .is_some_and(|t| !t.clips.is_empty())
    });
}

/// Loads `path` and checks that its left channel is one unbroken stretch of
/// the ramp. Returns its length.
#[track_caller]
fn assert_unbroken(path: &Path) -> usize {
    let audio = lumen_media::load(path, RATE, &AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|e| panic!("{} does not load: {e}", path.display()));
    let left = audio.channel(0);
    assert!(!left.is_empty(), "the file holds audio");
    for (n, pair) in left.windows(2).enumerate() {
        let step = f64::from(pair[1]) - f64::from(pair[0]);
        // A millionth a frame, or the ramp starting over.
        assert!(
            (step - 1e-6).abs() < 2e-7 || step < -0.99,
            "frame {n} to {}: the ramp moved by {step}",
            n + 1
        );
    }
    left.len()
}

#[test]
fn recording_an_armed_audio_track_is_one_clip_with_every_frame_and_one_undo() {
    let (mut lumen, _turn) = launch("basic", &project(), |l| l);
    record(&mut lumen, 9_600);

    let clips = clips_of(&lumen, VOX);
    assert_eq!(clips.len(), 1);
    assert!(
        clips_of(&lumen, GTR).is_empty(),
        "an unarmed track records nothing"
    );
    // It begins where recording began, and runs as long as the file.
    assert_eq!(clips[0].start, Ticks::ZERO);
    let project = lumen.app().project();
    assert_eq!(project.media.len(), 1);
    let media = &project.media[0];
    assert!(media.frames >= 9_600, "{} frames", media.frames);
    // 120 BPM at 48 kHz: a beat is 24,000 frames.
    let beats = media.frames as f64 / 24_000.0;
    let length = clips[0].length.0 as f64 / Ticks::quarters(1).0 as f64;
    assert!((length - beats).abs() < 0.001, "{length} beats for {beats}");
    let path = lumen.app().media_file(media.id).expect("the file is known");
    assert_eq!(assert_unbroken(&path) as u64, media.frames);

    assert_eq!(lumen.app().undo_label(), Some("Record"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        clips_of(&lumen, VOX).is_empty(),
        "one undo takes the take away"
    );
    assert!(lumen.app().project().media.is_empty());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(clips_of(&lumen, VOX).len(), 1, "and redo gives it back");
}

/// A clip of four beats that already sits on Vox, from an earlier take.
fn project_with_old_clip() -> Project {
    use lumen_project::model::{ClipContent, Media};
    let mut p = project();
    p.media.push(Media {
        id: Id(900),
        path: "Old.wav".into(),
        original_path: None,
        frames: 96_000,
        channels: 1,
        sample_rate: RATE,
        size_bytes: 0,
    });
    let clip = Clip {
        id: Id(901),
        name: "Old".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(8),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        content: ClipContent::Audio {
            media: Id(900),
            offset: 0,
            gain_db: 0.0,
        },
    };
    std::sync::Arc::make_mut(&mut p.tracks[0])
        .clips
        .push(std::sync::Arc::new(clip));
    p
}

#[test]
fn the_old_clip_is_silenced_under_a_take_and_cut_when_it_ends() {
    let (mut lumen, _turn) = launch("over", &project_with_old_clip(), |l| l);
    assert!(lumen.app().audio_tracks_recorded_over().is_empty());
    lumen.click("Arm Vox");
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the old clip to be silenced", |app| {
        app.recording() && app.audio_tracks_recorded_over() == vec![VOX]
    });
    lumen.wait_for("frames to be written", |app| {
        app.audio_take_frames(VOX).is_some_and(|f| f >= 9_600)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the take to be placed", |app| {
        !app.recording()
            && app
                .project()
                .tracks
                .iter()
                .find(|t| t.id == VOX)
                .is_some_and(|t| t.clips.iter().any(|c| c.name != "Old"))
    });
    lumen.wait_for(
        "the old clip to play again where it is not covered",
        |app| app.audio_tracks_recorded_over().is_empty(),
    );
    // The take began where the old clip did, so the old clip is cut to what
    // the take does not cover (or gone), never under it.
    let take = clips_of(&lumen, VOX)
        .into_iter()
        .find(|c| c.name != "Old")
        .expect("the take");
    for old in clips_of(&lumen, VOX).iter().filter(|c| c.name == "Old") {
        assert!(
            old.start >= take.start + take.length,
            "the old clip starts under the take"
        );
    }
}

#[test]
fn looping_while_recording_keeps_one_long_file() {
    let mut p = project();
    p.transport.loop_start = Ticks::ZERO;
    p.transport.loop_end = Ticks::quarters(1);
    p.transport.loop_enabled = true;
    let (mut lumen, _turn) = launch("loop", &p, |l| l);
    // A beat is 24,000 frames: two passes and a bit.
    record(&mut lumen, 52_000);
    let clips = clips_of(&lumen, VOX);
    assert_eq!(clips.len(), 1, "the passes are one clip until they are cut");
    let media = lumen.app().project().media[0].clone();
    assert!(media.frames >= 52_000);
    let path = lumen.app().media_file(media.id).expect("the file");
    assert_eq!(assert_unbroken(&path) as u64, media.frames);
    assert!(clips[0].length > Ticks::quarters(2), "longer than the loop");
}

#[test]
fn punch_keeps_only_the_punched_range_in_the_file() {
    let mut p = project();
    // 240 BPM at 48 kHz: a beat is 12,000 frames.
    p.tempo = vec![lumen_core::time::TempoPoint {
        at: Ticks::ZERO,
        bpm: 240.0,
        ramp: false,
    }];
    p.transport.loop_start = Ticks::quarters(1);
    p.transport.loop_end = Ticks::quarters(3);
    p.transport.punch_in = true;
    p.transport.punch_out = true;
    let (mut lumen, _turn) = launch("punch", &p, |l| l);
    lumen.click("Arm Vox");
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the recording to begin", |app| {
        app.recording() && app.is_playing()
    });
    // Input is heard a device latency after it is played: go well past it.
    lumen.wait_for("past the punch out", |app| {
        app.playhead() > Ticks::quarters(3) + Ticks(Ticks::quarters(1).0 / 4)
    });
    assert!(
        lumen.app().recording(),
        "recording goes on after the punch out"
    );
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the clip", |app| {
        !app.recording()
            && app
                .project()
                .tracks
                .iter()
                .find(|t| t.id == VOX)
                .is_some_and(|t| !t.clips.is_empty())
    });
    let clips = clips_of(&lumen, VOX);
    assert_eq!(clips.len(), 1);
    let slack = 3;
    assert!(
        (clips[0].start.0 - Ticks::quarters(1).0).abs() <= slack,
        "{:?}",
        clips[0].start
    );
    assert!(
        (clips[0].length.0 - Ticks::quarters(2).0).abs() <= slack,
        "{:?}",
        clips[0].length
    );
    let media = lumen.app().project().media[0].clone();
    assert_eq!(media.frames, 24_000, "two beats, nothing outside them");
    let path = lumen.app().media_file(media.id).expect("the file");
    assert_eq!(assert_unbroken(&path), 24_000);
}

#[test]
fn saving_copies_the_take_into_the_project_and_it_opens_again() {
    let (mut lumen, _turn) = launch("save", &project(), |l| l);
    record(&mut lumen, 9_600);
    // Saved as a new project, in a folder of its own.
    let out = lumen.dir().join("Out").join("Out.lumen");
    lumen.app_mut().save_to(out.clone());
    lumen.wait_for("the save", |app| !app.is_dirty() && !app.is_busy());
    assert!(
        out.parent()
            .unwrap()
            .join("Samples/Recorded/Vox 1.wav")
            .is_file(),
        "the take was copied into the project: {:?}",
        lumen.app().file_notice()
    );
    let frames = lumen.app().project().media[0].frames;

    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, move |l| l.with_input_device().opening(out));
    again.wait_for("the project", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    again.frames(10);
    assert_eq!(clips_of(&again, VOX).len(), 1);
    assert_eq!(again.app().file_notice(), None, "no file is missing");
    let reopened = again.app().project().media[0].clone();
    assert_eq!(reopened.frames, frames);
    assert_eq!(reopened.path, "Samples/Recorded/Vox 1.wav");
    let file = again.app().media_file(reopened.id).expect("the file");
    assert!(file.starts_with(again.dir()), "{}", file.display());
    assert_eq!(assert_unbroken(&file) as u64, frames);
}

#[test]
fn a_crash_while_recording_leaves_a_take_the_next_start_puts_back() {
    let (mut lumen, _turn) = launch("crash", &project(), |l| {
        l.autosaving_after(Duration::from_millis(50))
    });
    lumen.click("Arm Vox");
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("frames to be written", |app| {
        app.audio_take_frames(VOX).is_some_and(|f| f >= 12_000)
    });
    let autosaves = lumen.dir().join("autosave");
    lumen.wait_for("an autosave", |_| {
        std::fs::read_dir(&autosaves).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "lumen"))
        })
    });
    // The file is on disk, beside a note that it is unfinished.
    let recordings = lumen.dir().join("Recordings");
    let sidecars = || {
        std::fs::read_dir(&recordings)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "take"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(sidecars(), 1);

    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |l| l.with_input_device());
    again.click("Recover");
    again.wait_for("the recovered project", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    again.frames(10);
    let clips = clips_of(&again, VOX);
    assert_eq!(clips.len(), 1, "the take is back on its track");
    assert!(
        again
            .app()
            .file_notice()
            .is_some_and(|n| n.contains("stopped while recording")),
        "{:?}",
        again.app().file_notice()
    );
    let media = again.app().project().media[0].clone();
    assert!(media.frames > 0);
    let path = again.app().media_file(media.id).expect("the file");
    assert_eq!(assert_unbroken(&path) as u64, media.frames);
    let left = std::fs::read_dir(again.dir().join("Recordings"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "take"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(left, 0, "it is no longer unfinished");
    // One undo takes it away again.
    assert_eq!(again.app().undo_label(), Some("Record"));
}
