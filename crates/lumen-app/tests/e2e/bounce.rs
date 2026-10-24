//! M7b E4: Bounce renders a track with its own sound into a new file and puts
//! it on the track or on a new one, as one undo step, with a popover that
//! remembers its choices.

use std::sync::atomic::AtomicBool;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Media, Note, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::clip_commands::{audio, command};
use super::hosting::{PLUGIN, instrument_device, launch, timing_sensitive, wait_for_state};

fn bounce_with_popover(lumen: &mut Lumen) {
    command(lumen, "Bounce");
    lumen.frames(3);
    assert!(lumen.shows("Include effects"), "the popover is open");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));
}

fn media_of(lumen: &Lumen, track: usize, clip: usize) -> Media {
    let ClipContent::Audio { media, .. } = lumen.app().project().tracks[track].clips[clip].content
    else {
        panic!("an audio clip")
    };
    lumen
        .app()
        .project()
        .media
        .iter()
        .find(|m| m.id == media)
        .expect("the clip's media is in the project")
        .clone()
}

fn decode(media: &Media) -> lumen_media::Audio {
    lumen_media::decode(
        std::path::Path::new(&media.path),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap()
}

#[test]
fn bouncing_a_clip_makes_a_new_track_after_it_and_undo_gives_everything_back() {
    let mut lumen = audio("bounce-new");
    let before = lumen.app().project().clone();
    let source = before.tracks[0].clips[0].clone();
    lumen.app_mut().select_clip(source.id);
    bounce_with_popover(&mut lumen);

    let after = lumen.app().project();
    assert_eq!(after.tracks.len(), 2);
    assert_eq!(
        after.tracks[0], before.tracks[0],
        "the source is left alone"
    );
    assert_eq!(after.tracks[1].kind, TrackKind::Audio);
    let clip = &after.tracks[1].clips[0];
    assert_eq!(clip.start, source.start);
    assert!(clip.length >= source.length);
    assert!(clip.name.ends_with("(bounced)"), "{}", clip.name);
    assert_eq!(lumen.app().selected_clips(), [clip.id]);

    let media = media_of(&lumen, 1, 0);
    let path = std::path::PathBuf::from(&media.path);
    assert!(path.starts_with(lumen.dir().join("Renders")), "{path:?}");
    let audio = decode(&media);
    assert_eq!(audio.frames() as u64, media.frames);
    assert!(
        audio.channel(0).iter().any(|v| v.abs() > 0.01),
        "the file has the sound in it"
    );

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), Some("Import take"));
}

#[test]
fn replace_puts_the_file_on_the_track_and_the_choice_is_remembered() {
    let mut lumen = audio("bounce-replace");
    let before = lumen.app().project().clone();
    let source = before.tracks[0].clips[0].clone();
    lumen.app_mut().select_clip(source.id);
    command(&mut lumen, "Bounce");
    lumen.frames(3);
    lumen.click("Replace the track's audio with the file");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));

    let after = lumen.app().project();
    assert_eq!(after.tracks.len(), 1);
    assert_eq!(after.tracks[0].clips.len(), 1);
    assert_ne!(after.tracks[0].clips[0].id, source.id);
    assert_eq!(after.tracks[0].clips[0].start, source.start);
    assert!(after.media.len() > before.media.len(), "the original stays");

    // The next bounce opens with Replace chosen.
    let clip = after.tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    command(&mut lumen, "Bounce");
    lumen.frames(3);
    assert!(lumen.is_on("Replace the track's audio with the file"));
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert!(!lumen.shows("Include effects"), "Esc closes the popover");
}

#[test]
fn with_nothing_selected_there_is_nothing_to_bounce_and_no_popover() {
    let mut lumen = audio("bounce-nothing");
    let before = lumen.app().project().clone();
    // The command is not available: the palette keeps it, and Esc leaves.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Bounce".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(10);
    assert!(!lumen.shows("Include effects"));
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().undo_label(), Some("Import take"));
}

/// A MIDI track's notes go through its synth into a file on a new audio
/// track, and the notes keep playing (the default is a new track).
#[test]
fn a_midi_track_bounces_to_an_audio_track_and_its_notes_are_left_playing() {
    let dir = TempDir::new("e2e-bounce-midi");
    let path = dir.path().join("song.lumen");
    let notes = vec![Note::new(Ticks::ZERO, Ticks::quarters(2), 60, 100)];
    let project = project_of(
        "Keys",
        vec![midi_track(1, "Keys", "poly", (0, 4), None, notes, vec![])],
    );
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    let before = lumen.app().project().clone();
    lumen.app_mut().select_tracks(&[before.tracks[0].id]);
    bounce_with_popover(&mut lumen);

    let after = lumen.app().project();
    assert_eq!(after.tracks.len(), 2);
    assert_eq!(after.tracks[0], before.tracks[0], "the notes still play");
    assert_eq!(after.tracks[1].kind, TrackKind::Audio);
    let audio = decode(&media_of(&lumen, 1, 0));
    assert!(
        audio.channel(0).iter().any(|v| v.abs() > 0.001),
        "it sounds"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project(), &before);
}

/// The test plugin's instrument, started in a host of the bounce's own: its
/// note rings on for seconds after the clip ends, and the tail switch decides
/// whether that is in the file.
#[test]
fn a_plugin_instrument_bounces_in_its_own_host_with_its_ring_as_the_tail() {
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
    let project = project_of("Rings", vec![track]);
    let mut lumen = launch("bounce-rings", &project, &["Rings"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let track: Id = lumen.app().project().tracks[0].id;

    lumen.app_mut().select_tracks(&[track]);
    bounce_with_popover(&mut lumen);
    let with_tail = media_of(&lumen, 1, 0);
    let rate = u64::from(with_tail.sample_rate);
    // The clip is two quarters, a second at 120 BPM; the release is four
    // seconds from the note's end at half a second.
    assert!(
        with_tail.frames > rate + rate,
        "the ring is in the file: {} frames",
        with_tail.frames
    );
    let audio = decode(&with_tail);
    assert!(audio.channel(0).iter().any(|v| v.abs() > 0.01), "it sounds");
    assert!(
        lumen
            .app()
            .device_state(PLUGIN)
            .is_some_and(|s| s.starts_with("ready")),
        "the track's own plugin is untouched"
    );

    // Tail off: the file ends with the clip.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.app_mut().select_tracks(&[track]);
    command(&mut lumen, "Bounce");
    lumen.frames(3);
    lumen.click("Include tail");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));
    let without = media_of(&lumen, 1, 0);
    assert_eq!(without.frames, rate, "exactly the clip's second");
}
