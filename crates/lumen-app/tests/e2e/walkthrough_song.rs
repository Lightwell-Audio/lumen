use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use lumen_app::testing::project_file;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id, Project, TrackKind};
use lumen_project::testing::TempDir;

use super::clip_commands::cut_at;
use super::{Lumen, wav};

const RATE: u32 = 48_000;
const VOX_FRAMES: u64 = 48_000;
const GTR_FRAMES: u64 = 24_000;

fn track_id(lumen: &Lumen, name: &str) -> Id {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("a track named {name}"))
        .id
}

fn clips(lumen: &Lumen, id: Id) -> Vec<Arc<Clip>> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == id)
        .map_or_else(Vec::new, |t| t.clips.clone())
}

fn add_audio_track(lumen: &mut Lumen, name: &str) -> Id {
    let before = lumen.app().project().tracks.len();
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("the new track", move |app| {
        app.project().tracks.len() == before + 1
    });
    lumen.key(Modifiers::NONE, Key::F2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(name.into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(30);
    let id = track_id(lumen, name);
    let kind = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .kind;
    assert_eq!(kind, TrackKind::Audio, "{name} is an audio track");
    id
}

fn unfinished(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("Recordings"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "take"))
                .count()
        })
        .unwrap_or(0)
}

#[track_caller]
fn assert_unbroken(path: &Path) -> u64 {
    let audio = lumen_media::load(path, RATE, &AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|e| panic!("{} does not load: {e}", path.display()));
    let left = audio.channel(0);
    assert!(!left.is_empty(), "{} holds audio", path.display());
    for (n, pair) in left.windows(2).enumerate() {
        let step = f64::from(pair[1]) - f64::from(pair[0]);
        assert!(
            (step - 1e-6).abs() < 2e-7 || step < -0.99,
            "{}: frame {n} to {}: the ramp moved by {step}",
            path.display(),
            n + 1
        );
    }
    left.len() as u64
}

fn decode(path: &Path) -> lumen_media::Audio {
    lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|e| panic!("{} does not decode: {e}", path.display()))
}

fn peak(audio: &lumen_media::Audio) -> f32 {
    (0..audio.channels())
        .flat_map(|c| audio.channel(c).iter())
        .fold(0.0f32, |m, s| m.max(s.abs()))
}

fn begin_take(lumen: &mut Lumen, track: Id, frames: u64) {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the recording to begin", |app| {
        app.recording() && app.is_playing()
    });
    lumen.wait_for("frames to be written", move |app| {
        app.audio_take_frames(track).is_some_and(|f| f >= frames)
    });
}

fn export(lumen: &mut Lumen, path: &Path) -> Vec<PathBuf> {
    lumen.app_mut().answer_next_dialog(vec![path.to_owned()]);
    lumen.click("Export audio…");
    lumen.wait_for("the export", |app| !app.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    let files = lumen.app().audio_export_files();
    assert!(!files.is_empty(), "the export wrote files");
    files
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|e| panic!("{} does not exist: {e}", path.display()))
}

#[test]
fn record_edit_save_crash_recover_reopen_and_export_a_song() {
    let turn = lumen_app::testing::input_device_turn();
    turn.set(|d| d.input_ramp = true);
    let mut lumen = Lumen::start_with(TempDir::new("e2e-walkthrough-song"), |l| {
        l.with_input_device()
            .autosaving_after(Duration::from_millis(50))
    });
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.frames(30);

    let vox = add_audio_track(&mut lumen, "Vox");
    let gtr = add_audio_track(&mut lumen, "Gtr");

    let drums = wav(&lumen.dir().join("drums.wav"), 2.0);
    lumen.drop_files(&[drums]);
    lumen.wait_for("the import", |app| {
        !app.is_busy() && app.project().tracks.len() == 3
    });
    lumen.frames(30);

    lumen.click("Arm Vox");
    begin_take(&mut lumen, vox, VOX_FRAMES);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the take to be placed", move |app| {
        !app.recording()
            && !app.audio_takes_placing()
            && app
                .project()
                .tracks
                .iter()
                .any(|t| t.id == vox && !t.clips.is_empty())
    });
    let take = clips(&lumen, vox);
    assert_eq!(take.len(), 1);
    assert_eq!(lumen.app().undo_label(), Some("Record"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(clips(&lumen, vox).is_empty(), "Undo takes the take away");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.frames(3);
    assert_eq!(clips(&lumen, vox), take, "Redo puts the same clip back");
    lumen.click("Arm Vox");

    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::Equals);
    }
    lumen.frames(15);
    assert_eq!(lumen.app().project().tracks[0].id, vox);
    cut_at(&mut lumen, 2);
    assert_eq!(lumen.app().undo_label(), Some("Split clips"));
    let edited = clips(&lumen, vox);
    assert_eq!(edited.len(), 2);

    let target = project_file(lumen.dir(), "Song");
    lumen.app_mut().answer_next_dialog(vec![target]);
    lumen.key(Modifiers::COMMAND, Key::S);
    lumen.wait_for("the save", |app| {
        !app.is_dirty() && !app.is_busy() && app.project_path().is_some()
    });
    let song = lumen.app().project_path().unwrap().to_owned();
    let folder = song.parent().unwrap().to_owned();
    assert!(
        folder.join("Samples/Recorded/Vox 1.wav").is_file(),
        "the take was copied into the project: {:?}",
        lumen.app().file_notice()
    );

    lumen.click("Arm Gtr");
    begin_take(&mut lumen, gtr, GTR_FRAMES);
    lumen.wait_for("an autosave", |app| app.autosave_finished());
    let written = lumen.app().audio_take_frames(gtr).unwrap();
    assert_eq!(unfinished(lumen.dir()), 1, "the take is marked unfinished");
    let dir = lumen.crash();

    let mut lumen = Lumen::start_with(dir, |l| l.with_input_device());
    lumen.recover();
    lumen.wait_for("the recovered project and its takes", |app| {
        !app.is_busy() && !app.audio_takes_placing() && app.project().tracks.len() == 3
    });
    lumen.frames(10);
    assert!(
        lumen
            .app()
            .file_notice()
            .is_some_and(|n| n.contains("stopped while recording")),
        "{:?}",
        lumen.app().file_notice()
    );
    assert_eq!(clips(&lumen, vox), edited, "the split is as it was saved");
    let recovered = clips(&lumen, gtr);
    assert_eq!(recovered.len(), 1, "the take is back on its track");
    assert_eq!(recovered[0].start, Ticks::ZERO, "where recording began");
    assert_eq!(
        lumen.app().project_path().map(canonical),
        Some(canonical(&song)),
        "recovered into the song"
    );
    assert!(lumen.app().is_dirty(), "recovered work is unsaved");
    assert_eq!(unfinished(lumen.dir()), 1, "the note waits for a save");
    let gtr_media = {
        let project = lumen.app().project();
        let lumen_project::model::ClipContent::Audio { media, .. } = recovered[0].content else {
            panic!("the take is audio");
        };
        project
            .media
            .iter()
            .find(|m| m.id == media)
            .unwrap()
            .clone()
    };
    assert!(
        gtr_media.frames >= GTR_FRAMES,
        "the recovered take holds {} frames; {GTR_FRAMES} were written before the crash",
        gtr_media.frames
    );
    assert!(
        gtr_media.frames >= written,
        "{written} frames were written before the crash; {} came back",
        gtr_media.frames
    );
    let lumen_project::model::ClipContent::Audio { offset, .. } = recovered[0].content else {
        unreachable!("checked above");
    };
    assert_eq!(offset, 0, "the clip starts at the file's first frame");
    let tempo = &lumen.app().project().tempo;
    assert!(
        tempo.len() == 1 && tempo[0].bpm == 120.0,
        "the song is at 120 BPM: {tempo:?}"
    );
    let whole = Ticks(gtr_media.frames as i64 * lumen_core::time::PPQ / 24_000);
    assert!(
        (recovered[0].length.0 - whole.0).abs() <= lumen_core::time::PPQ / 24_000 + 1,
        "the clip lasts {:?}; the file lasts {whole:?}",
        recovered[0].length
    );

    lumen.key(Modifiers::COMMAND, Key::S);
    lumen.wait_for("the save", |app| !app.is_dirty() && !app.is_busy());
    lumen.run_until("the take's note to go", |l| unfinished(l.dir()) == 0);
    assert!(
        folder.join("Samples/Recorded/Gtr 1.wav").is_file(),
        "the recovered take was copied into the project: {:?}",
        lumen.app().file_notice()
    );
    let saved = lumen.app().project_as_saved();
    for media in &saved.media {
        let file = lumen.app().media_file(media.id).expect("the file");
        assert!(
            canonical(&file).starts_with(canonical(&folder)),
            "{} plays from outside the project",
            file.display()
        );
    }
    let dir = lumen.crash();

    let mut lumen = Lumen::start_with(dir, {
        let song = song.clone();
        move |l| l.with_input_device().opening(song)
    });
    lumen.wait_for("the song to open", |app| {
        !app.is_busy() && app.project().tracks.len() == 3
    });
    lumen.frames(10);
    assert_eq!(lumen.app().file_notice(), None, "nothing is missing");
    assert!(!lumen.app().is_dirty());
    let opened = lumen.app().project_as_saved();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(clips(&lumen, vox), edited, "the split survives the reopen");
    assert_eq!(clips(&lumen, gtr), recovered, "so does the whole take");
    let what = |p: &Project| -> Vec<_> {
        p.media
            .iter()
            .map(|m| (m.id, m.frames, m.channels, m.sample_rate))
            .collect()
    };
    assert_eq!(what(&opened), what(&saved));
    for (name, at_least) in [("Vox 1", VOX_FRAMES), ("Gtr 1", GTR_FRAMES)] {
        let media = opened
            .media
            .iter()
            .find(|m| m.path == format!("Samples/Recorded/{name}.wav"))
            .unwrap_or_else(|| panic!("{name} is in the project's media"));
        let file = lumen.app().media_file(media.id).expect("the file");
        assert!(
            canonical(&file).starts_with(canonical(&folder)),
            "{}",
            file.display()
        );
        let frames = assert_unbroken(&file);
        assert_eq!(frames, media.frames, "{name}");
        assert!(frames >= at_least, "{name} holds {frames} frames");
    }

    let before = lumen.app().project().clone();
    let undo = lumen.app().undo_label().map(str::to_owned);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    assert!(lumen.app().audio_export_open());
    let out = lumen.dir().join("Song.wav");
    let mix = export(&mut lumen, &out);
    assert_eq!(mix.len(), 1);
    let audio = decode(&mix[0]);
    assert_eq!(audio.sample_rate, RATE);
    assert!(audio.channel(0).len() >= 2 * RATE as usize - 1);
    assert!(peak(&audio) > 0.01, "the mix is heard: {}", peak(&audio));

    lumen.click("Output: Master mix");
    lumen.click("Track stems");
    let out = lumen.dir().join("Stems");
    let stems = export(&mut lumen, &out);
    assert_eq!(stems.len(), 3, "one stem per track: {stems:?}");
    for stem in &stems {
        let loudest = peak(&decode(stem));
        assert!(
            loudest > 0.01,
            "{} holds the track: {loudest}",
            stem.display()
        );
    }
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), undo.as_deref());
}
