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
    // The collect finishes its copy before it says it has: wait for its own
    // word, not only for a file to appear.
    lumen.run_until("the collect to finish", |lumen| {
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Collected 1 audio file"))
    });
    assert!(
        std::fs::read_dir(&copied)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false),
        "the sample is in the project folder"
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

/// H1: a pad routed to an output pair is heard on the track made for that
/// pair, and on the instrument's own track when no track plays it. The two
/// tracks are panned apart, so the side it is heard on says which track
/// played it.
#[test]
fn a_pad_on_an_output_pair_is_heard_on_the_track_that_plays_it() {
    let dir = TempDir::new("e2e-sampler-pair");
    let frames = 48_000;
    let file = constant(&dir.path().join("pair.wav"), frames, 16_000);
    std::fs::create_dir_all(dir.path().join("Samples/Imported")).unwrap();
    std::fs::copy(&file, dir.path().join("Samples/Imported/pair.wav")).unwrap();
    // A pad on output 1, one note on 36.
    let mut pad = Sampler {
        face: Face::Pads,
        ..Sampler::default()
    };
    pad.keep_pads(&Sampler::pads_for(None));
    pad.pad_mut(36).media = Some(Id(5));
    pad.pad_mut(36).output = 1;
    let mut project = sampled(
        "Pair",
        vec![Note::new(Ticks::ZERO, Ticks::quarters(4), 36, 127)],
        pad,
        vec![media_of(5, "Samples/Imported/pair.wav", frames as u64)],
    );
    // The sampler's own track is hard right.
    std::sync::Arc::make_mut(&mut project.tracks[0]).pan = 1.0;
    let device = project.tracks[0].devices[0].id;
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("the test project saves");
    let mut lumen = open("sampler-pair-own", dir, &path);
    wait_loaded(&mut lumen, device);

    // With no track playing pair 1, the hit stays on the instrument's track.
    play(&mut lumen);
    let (left, right) = sides(&mut lumen);
    stop(&mut lumen);
    assert!(right > 0.2, "heard on its own track, hard right: {right}");
    assert!(left < 0.01, "and not on the left: {left}");

    // With an audio track made for pair 1, panned hard left, the hit is heard
    // there, not on the sampler's track.
    let dir = lumen.crash();
    let path = dir.path().join("song.lumen");
    let mut routed = Track::new(Id(77), "Pair 1", TrackKind::Audio, Color(0x4C7A9E));
    routed.pan = -1.0;
    routed.plugin_output = Some(lumen_project::model::PluginOutput { device, bus: 1 });
    project.tracks.push(std::sync::Arc::new(routed));
    lumen_project::save(&project, &path, &Default::default()).expect("it saves");
    let mut lumen = open("sampler-pair-routed", dir, &path);
    wait_loaded(&mut lumen, device);
    play(&mut lumen);
    let (left, right) = sides(&mut lumen);
    stop(&mut lumen);
    assert!(left > 0.2, "heard on the track that plays pair 1: {left}");
    assert!(right < 0.01, "and not on the sampler's track: {right}");
}

/// The loudest frame of each side over a moment of playback.
fn sides(lumen: &mut Lumen) -> (f32, f32) {
    lumen
        .listen(Duration::from_millis(20), 2_000)
        .iter()
        .fold((0.0f32, 0.0f32), |(left, right), frame| {
            (left.max(frame[0].abs()), right.max(frame[1].abs()))
        })
}

/// H2: only the files a sampler plays are decoded. A project of audio clips
/// and no sampler decodes nothing into the sampler's library.
#[test]
fn a_project_without_a_sampler_decodes_no_samples() {
    let mut lumen = Lumen::start("sampler-nothing");
    let file = wav(&lumen.dir().join("tone.wav"), 1.0);
    lumen.drop_files(&[file]);
    lumen.wait_for("the clip", |app| app.project().tracks.len() == 1);
    lumen.frames(20);
    assert_eq!(
        lumen.app().sample_memory(),
        0,
        "the clip is the media library's, not the sampler's"
    );
}

/// H3: a knob turned while the transport is stopped reaches the instrument
/// with the next block, so a note played live on it is heard at the new level.
#[test]
fn the_level_set_while_stopped_is_heard_on_a_live_note() {
    let dir = TempDir::new("e2e-sampler-level");
    let frames = 48_000;
    let file = constant(&dir.path().join("dc.wav"), frames, 16_000);
    std::fs::create_dir_all(dir.path().join("Samples/Imported")).unwrap();
    std::fs::copy(&file, dir.path().join("Samples/Imported/dc.wav")).unwrap();
    let sound = Sampler {
        one_shot: lumen_project::model::OneShot {
            // The region's root is C4; the keyboard's A is a pitch below it,
            // which leaves a constant sample at its own level.
            region: Some(Region::whole(Id(5), 60)),
            looping: false,
            reversed: false,
        },
        ..Sampler::default()
    };
    let project = sampled(
        "Level",
        Vec::new(),
        sound,
        vec![media_of(5, "Samples/Imported/dc.wav", frames as u64)],
    );
    let (mut lumen, _path) = started("sampler-level", &project, dir);
    let track = lumen.app().project().tracks[0].id;
    let device = lumen.app().project().tracks[0].devices[0].id;
    wait_loaded(&mut lumen, device);
    // The selected MIDI track is the one the computer keyboard plays.
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(5);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    lumen.frames(3);
    assert!(lumen.app().computer_keyboard_on());

    held(&mut lumen);
    let loud = heard_level(&mut lumen);
    released(&mut lumen);
    assert!(loud > 0.4, "the note is heard at the level it has: {loud}");

    // Level to âˆ’20 dB while stopped; the next note is ten times quieter.
    let level = lumen_devices::params::find(lumen_devices::sampler::TABLE, LEVEL)
        .expect("the level is in the table")
        .normalised(-20.0);
    lumen
        .app_mut()
        .set_device_param_at_rest(device, LEVEL, f64::from(level));
    lumen.frames(5);
    held(&mut lumen);
    // The first block a silent device is given starts the knob's glide (a
    // value the interface moved is smoothed over 20 ms): the note is heard at
    // the new level once it has arrived, which is what a person hears.
    lumen.listen(Duration::from_millis(40), 400);
    let quiet = heard_level(&mut lumen);
    released(&mut lumen);
    assert!(
        (quiet / loud - 0.1).abs() < 0.02,
        "−20 dB is a tenth: {quiet} against {loud}"
    );
    assert!(
        (quiet / loud - 0.1).abs() < 0.02,
        "âˆ’20 dB is a tenth: {quiet} against {loud}"
    );
}

/// Holds the keyboard's A, as a person holding a key.
fn held(lumen: &mut Lumen) {
    lumen.harness.event(eframe::egui::Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(3);
}

/// Lets go of it.
fn released(lumen: &mut Lumen) {
    lumen.harness.event(eframe::egui::Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(3);
}

/// The loudest frame of a held note.
fn heard_level(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(5), 2_000)
        .iter()
        .fold(0.0f32, |most, frame| {
            most.max(frame[0].abs().max(frame[1].abs()))
        })
}

/// L4: the Zones face through the commands: the files land across the
/// keyboard in order, and a note in each zone plays its own file. The three
/// files are constants of 0.125, 0.25 and 0.49, so the level heard says which
/// one played.
#[test]
fn zones_across_the_keyboard_play_their_own_files() {
    let dir = TempDir::new("e2e-sampler-zones");
    std::fs::create_dir_all(dir.path().join("Samples/Imported")).unwrap();
    let frames = 48_000;
    for (name, value) in [("low", 4_096i16), ("mid", 8_192), ("high", 16_000)] {
        let file = constant(&dir.path().join(format!("{name}.wav")), frames, value);
        std::fs::copy(
            &file,
            dir.path().join(format!("Samples/Imported/{name}.wav")),
        )
        .unwrap();
    }
    let project = sampled(
        "Zones",
        vec![
            // One note in each zone: 10 is the low file's, 60 the mid's and
            // 120 the high one's. Each is a quarter, inside the four-quarter
            // clip (the high file, read at its zone's root, lasts less than
            // that).
            Note::new(Ticks::ZERO, Ticks::quarters(1), 10, 127),
            Note::new(Ticks::quarters(1), Ticks::quarters(1), 60, 127),
            Note::new(Ticks::quarters(2), Ticks::quarters(1), 120, 127),
        ],
        Sampler {
            face: Face::Zones,
            ..Sampler::default()
        },
        Vec::new(),
    );
    let (mut lumen, _path) = started("sampler-zones", &project, dir);
    let track = lumen.app().project().tracks[0].id;
    let device = lumen.app().project().tracks[0].devices[0].id;
    lumen.app_mut().select_tracks(&[track]);
    lumen.app_mut().choose_card(device);
    lumen.frames(2);
    // The three files are chosen in order and become the three zones.
    let names = ["low", "mid", "high"];
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| lumen.dir().join(format!("Samples/Imported/{name}.wav")))
        .collect();
    lumen.app_mut().answer_next_dialog(paths);
    command(&mut lumen, "Load samples into Shutter");
    wait_loaded(&mut lumen, device);
    lumen.frames(5);
    let held = lumen.app().sampler_content(device).expect("a sound");
    assert_eq!(held.zones.len(), 3, "three zones, in the order chosen");
    let ids: Vec<Id> = lumen.app().project().media.iter().map(|m| m.id).collect();
    for (zone, id) in held.zones.iter().zip(&ids) {
        assert_eq!(zone.media, *id, "each zone plays the file it was given");
    }

    play(&mut lumen);
    let heard = lumen.listen_from(|_| {}, 150_000);
    stop(&mut lumen);
    // The three plateaus: each zone played the file it was given.
    let levels: Vec<f32> = heard
        .iter()
        .map(|frame| frame[0].abs())
        .filter(|level| *level > 0.005)
        .collect();
    let distinct = |wanted: f32| levels.iter().any(|level| (level - wanted).abs() < 0.02);
    assert!(
        distinct(4_096.0 / 32_767.0),
        "the low file played: {levels:?}"
    );
    assert!(distinct(8_192.0 / 32_767.0), "the mid file played");
    assert!(distinct(16_000.0 / 32_767.0), "the high file played");
}
