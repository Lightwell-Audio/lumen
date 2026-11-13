//! Shutter, the built-in sampler (M8 slice B5): adding it from the palette,
//! loading a sample into it, hearing it play, a pad choking its own earlier
//! hit, and the project keeping its sample through a save, a Collect and save
//! and a reopen.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_devices::sampler::{LEVEL, PITCH};
use lumen_project::model::{
    Clip, ClipContent, Color, Device, Face, Id, Media, Note, PluginRef, Project, Region, SAMPLER,
    Sampler, Track, TrackKind,
};
use lumen_project::testing::{TempDir, project_of};
use lumen_project::{from_bytes, to_bytes};

use super::copies::command;
use super::{Lumen, wav};

/// A stereo WAV of `frames` frames of one constant value, at 48 kHz: every
/// frame is the same, so what is heard says which notes are sounding.
fn constant(path: &Path, frames: usize, value: i16) -> PathBuf {
    let mut data: Vec<u8> = Vec::with_capacity(frames * 4);
    let [a, b] = value.to_le_bytes();
    for _ in 0..frames {
        data.extend([a, b, a, b]);
    }
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
    std::fs::write(path, out).expect("write the test WAV");
    path.to_path_buf()
}

/// A project with one MIDI track carrying Shutter, its clip holding `notes`,
/// and the media entries `media` (with `sampler` playing them).
fn sampled(name: &str, notes: Vec<Note>, sampler: Sampler, media: Vec<Media>) -> Project {
    let mut clip = Clip {
        id: Id(9),
        name: "Part".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(4),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Ticks::ZERO,
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        takes: None,
        warp: None,
        content: ClipContent::Midi {
            notes,
            events: Vec::new(),
        },
    };
    clip.name = "Part".into();
    let mut track = Track::new(Id(1), "Drums", TrackKind::Midi, Color(0x5E8C6A));
    let mut device = Device::new(
        Id(2),
        PluginRef::Builtin {
            name: SAMPLER.to_owned(),
        },
        false,
        Vec::new(),
    );
    device.sampler = Some(sampler);
    track.devices = vec![device];
    track.clips = vec![std::sync::Arc::new(clip)];
    let mut project = project_of(name, vec![track]);
    project.media = media;
    project
}

fn media_of(id: u64, path: &str, frames: u64) -> Media {
    Media {
        id: Id(id),
        path: path.to_owned(),
        original_path: None,
        frames,
        channels: 2,
        sample_rate: 48_000,
        size_bytes: frames * 4,
    }
}

/// Opens `project` at `path`, in `dir`, as a person's project would open.
fn open(_name: &str, dir: TempDir, path: &Path) -> Lumen {
    let path = path.to_path_buf();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path.clone()));
    lumen.wait_for("the project", |app| !app.is_busy());
    lumen.frames(10);
    lumen
}

/// Starts with a project of `tracks` saved inside `dir`, and opens it.
fn started(name: &str, project: &Project, dir: TempDir) -> (Lumen, PathBuf) {
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("the test project saves");
    let lumen = open(name, dir, &path);
    (lumen, path)
}

fn play(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
}

fn stop(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
}

/// The loudest frame heard over a moment of playback.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(150), 1_000)
        .iter()
        .fold(0.0f32, |m, frame| m.max(frame[0].abs().max(frame[1].abs())))
}

/// Waits for every sample the device names to be decoded.
fn wait_loaded(lumen: &mut Lumen, device: Id) {
    lumen.wait_for("the samples to decode", move |app| {
        let media = app.sampler_media(device);
        !media.is_empty() && media.iter().all(|id| app.sample_loaded(*id))
    });
}

#[test]
fn shutter_arrives_from_the_palette_with_its_controls() {
    let mut lumen = Lumen::start("sampler-add");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("a track", |app| !app.project().tracks.is_empty());
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);

    command(&mut lumen, "Add Shutter (sampler)");
    lumen.wait_for("Shutter's card", |app| {
        app.project()
            .tracks
            .iter()
            .any(|t| t.devices.iter().any(|d| d.is_sampler()))
    });
    lumen.frames(20);
    let device = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.devices)
        .find(|d| d.is_sampler())
        .expect("the device is on a track")
        .id;
    assert!(
        lumen
            .app()
            .undo_label()
            .is_some_and(|label| label.starts_with("Add Shutter")),
        "{:?}",
        lumen.app().undo_label()
    );
    // The card says the device's own name, and the parameters a person can
    // reach are its table's, in its order.
    println!(
        "card {:?}, strip {:?}",
        lumen.app().chosen_card(),
        lumen.app().strip_parameters(device)
    );
    assert_eq!(
        lumen.app().chosen_card(),
        Some(device),
        "its card is chosen"
    );
    assert!(lumen.shows("Power Shutter"), "the card is drawn");
    assert_eq!(
        lumen.app().strip_parameters(device),
        [LEVEL, PITCH, 3, 4, 5, 6, 7, 8],
        "the eight the card shows"
    );
    assert!(
        lumen
            .harness
            .query_by_label("All parameters (11)")
            .is_some(),
        "every parameter is one button away"
    );
    // It arrives with its sixteen pads and its One shot face.
    let sound = lumen.app().sampler_content(device).expect("a sound");
    assert_eq!(sound.face, Face::OneShot);
    assert_eq!(sound.pads.len(), 16);
    assert!(!lumen.app().is_builtin(device), "it is not an effect");
}

#[test]
fn a_loaded_sample_plays_and_a_reopened_project_still_plays_it() {
    let dir = TempDir::new("e2e-sampler-play");
    let file = wav(&dir.path().join("one.wav"), 1.0);
    // A MIDI track with Shutter and a note in its clip, and no sample yet.
    let project = sampled(
        "Sampler",
        vec![Note::new(Ticks::ZERO, Ticks::quarters(4), 60, 100)],
        Sampler::default(),
        Vec::new(),
    );
    let (mut lumen, path) = started("sampler-play", &project, dir);
    let track = lumen.app().project().tracks[0].id;
    let device = lumen.app().project().tracks[0].devices[0].id;
    lumen.app_mut().select_tracks(&[track]);
    // The card is chosen, as clicking it does: the sampler's own commands act
    // on the chosen card.
    lumen.app_mut().choose_card(device);
    lumen.frames(2);

    // The file is chosen as the dialog would answer, and the command loads it.
    lumen.app_mut().answer_next_dialog(vec![file.clone()]);
    command(&mut lumen, "Load samples into Shutter");
    wait_loaded(&mut lumen, device);
    lumen.frames(5);
    assert_eq!(lumen.app().undo_label(), Some("Load one"), "one undo step");
    let sound = lumen.app().sampler_content(device).expect("a sound");
    let region = sound.one_shot.region.expect("the region");
    assert_eq!(region.root, 60);
    assert_eq!(
        lumen.app().project().media.len(),
        1,
        "the file is in the project"
    );

    // It plays: the track is heard, and its level is the table's.
    play(&mut lumen);
    let level = heard(&mut lumen);
    assert!(level > 0.005, "the sample is heard: {level}");
    stop(&mut lumen);

    // Saved, the project keeps the file and what it plays of it.
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.media.len(), 1);
    assert_eq!(
        saved.tracks[0].devices[0]
            .sampler
            .as_ref()
            .unwrap()
            .one_shot
            .region,
        Some(region)
    );

    // And a reopen plays it again.
    let dir = lumen.crash();
    let mut again = open("sampler-reopen", dir, &path);
    let device = again.app().project().tracks[0].devices[0].id;
    wait_loaded(&mut again, device);
    again.frames(5);
    play(&mut again);
    let after = heard(&mut again);
    assert!(after > 0.005, "the reopened project plays it: {after}");
    stop(&mut again);
    assert_eq!(
        to_bytes(again.app().project()).unwrap(),
        to_bytes(&saved).unwrap()
    );
}

#[test]
fn a_pad_struck_again_chokes_its_own_first_hit() {
    let dir = TempDir::new("e2e-sampler-choke");
    // One constant sample, two seconds long, on pad 36.
    let frames = 48_000;
    let file = constant(&dir.path().join("hat.wav"), 2 * frames, 16_000);
    let mut sampler = Sampler {
        face: Face::Pads,
        ..Sampler::default()
    };
    sampler.keep_pads(&Sampler::pads_for(None));
    sampler.pad_mut(36).media = Some(Id(5));
    let project = sampled(
        "Choke",
        vec![
            // Struck at 0 and again a sixteenth later; both held long enough
            // to overlap.
            Note::new(Ticks::ZERO, Ticks::quarters(4), 36, 127),
            Note::new(Ticks(120), Ticks::quarters(4), 36, 127),
        ],
        sampler,
        vec![media_of(5, "Samples/Imported/hat.wav", frames as u64)],
    );
    std::fs::create_dir_all(dir.path().join("Samples/Imported")).unwrap();
    std::fs::copy(&file, dir.path().join("Samples/Imported/hat.wav")).unwrap();
    let (mut lumen, _path) = started("sampler-choke", &project, dir);
    let device = lumen.app().project().tracks[0].devices[0].id;
    wait_loaded(&mut lumen, device);
    lumen.frames(5);

    // The pad's level is unity and the sample's is 16000/32767: one hit is
    // about 0.49. A pad struck again takes its own earlier hit with it, so
    // the two never add up: every frame after the second strike stays at one
    // hit's level, and the first hit's fade is smooth rather than a click.
    let heard = lumen.listen_from(
        |lumen| {
            lumen.key(Modifiers::NONE, Key::Space);
        },
        60_000,
    );
    stop(&mut lumen);
    let peak = heard.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
    let last = heard
        .iter()
        .rev()
        .take(200)
        .fold(0.0f32, |m, f| m.max(f[0].abs()));
    assert!(
        peak > 0.4 && peak < 0.9,
        "one hit at a time, never two: {peak}"
    );
    assert!(
        (last - 0.488).abs() < 0.05,
        "and the second hit sounds alone: {last}"
    );
}

#[test]
fn collect_and_save_copies_the_sample_and_it_reopens_without_the_original() {
    let dir = TempDir::new("e2e-sampler-collect");
    // The sample lives outside the project folder: in another one.
    let source = TempDir::new("e2e-sampler-outside");
    let outside = wav(&source.path().join("outside.wav"), 1.0);
    let project = sampled(
        "Collect",
        vec![Note::new(Ticks::ZERO, Ticks::quarters(4), 60, 100)],
        Sampler {
            one_shot: lumen_project::model::OneShot {
                region: Some(Region::whole(Id(7), 60)),
                looping: false,
                reversed: false,
            },
            ..Sampler::default()
        },
        vec![media_of(7, &outside.to_string_lossy(), 48_000)],
    );
    let (mut lumen, path) = started("sampler-collect", &project, dir);
    let device = lumen.app().project().tracks[0].devices[0].id;
    wait_loaded(&mut lumen, device);
    lumen.app_mut().set_copy_imported(false);
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    command(&mut lumen, "Collect and save");
    let copied = Path::new(&path).parent().unwrap().join("Samples/Imported");
    lumen.run_until("the sample in the project folder", |_| {
        std::fs::read_dir(&copied)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false)
    });
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Collected 1 audio file")),
        "{:?}",
        lumen.app().note()
    );

    // The original goes: the reopened project plays its own copy.
    std::fs::remove_file(&outside).unwrap();
    let dir = lumen.crash();
    let mut again = open("sampler-collect-reopen", dir, &path);
    let device = again.app().project().tracks[0].devices[0].id;
    wait_loaded(&mut again, device);
    play(&mut again);
    let level = heard(&mut again);
    assert!(level > 0.005, "the copied sample plays: {level}");
    stop(&mut again);
}

#[test]
fn a_missing_sample_shows_in_the_media_pool_and_relinking_it_makes_it_play() {
    let dir = TempDir::new("e2e-sampler-missing");
    // The sample goes missing from its place in the project.
    let project = sampled(
        "Missing",
        vec![Note::new(Ticks::ZERO, Ticks::quarters(4), 60, 100)],
        Sampler {
            one_shot: lumen_project::model::OneShot {
                region: Some(Region::whole(Id(7), 60)),
                looping: false,
                reversed: false,
            },
            ..Sampler::default()
        },
        vec![media_of(7, "Samples/Imported/gone.wav", 48_000)],
    );
    // Where the file is now: outside the project, named as it was.
    let _found = wav(&dir.path().join("gone.wav"), 1.0);
    let (mut lumen, _path) = started("sampler-missing", &project, dir);

    // The media pool knows the file is missing, and nothing sounds.
    assert_eq!(lumen.app().missing_media_count(), Some(1));
    lumen.click("Relink\u{2026}");
    assert!(lumen.app().media_tab_shown());
    let rows = lumen.app().media_rows();
    assert_eq!(rows, [("gone".to_owned(), true, 1)], "the sampler plays it");
    play(&mut lumen);
    let level = heard(&mut lumen);
    stop(&mut lumen);
    assert!(level < 0.001, "a missing sample plays nothing: {level}");

    // Searching the folder it is in finds it, and relinking it makes it play.
    let folder = lumen.dir().to_path_buf();
    lumen.app_mut().answer_next_dialog(vec![folder]);
    lumen.click("Search a folder\u{2026}");
    lumen.wait_for("the search", |app| {
        !app.media_searching() && app.media_found().is_some()
    });
    lumen.frames(3);
    assert_eq!(lumen.app().media_found(), Some(1));
    lumen.click("Relink 1");
    assert!(!lumen.app().media_rows()[0].1, "found");
    lumen.wait_for("the sample to decode", |app| app.sample_loaded(Id(7)));
    lumen.frames(5);
    play(&mut lumen);
    let level = heard(&mut lumen);
    stop(&mut lumen);
    assert!(level > 0.005, "the relinked sample plays: {level}");
}
