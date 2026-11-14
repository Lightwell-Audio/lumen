//! Drum maps in the model (format v35, M8 slice D0).
//!
//! The format ones: a golden file that loads forever, an older project that
//! opens with no maps, a project with none that writes none, and the checks
//! that refuse a map a project may not hold. The rules ones: translation
//! (mute, solo, channel, length override, unknown note), the note a recording
//! keeps, and conversion between maps.

use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::grid::{Feel, Size};
use lumen_core::time::Ticks;
use lumen_project::drums::{self, convert_notes, presets::Preset};
use lumen_project::model::{
    Clip, ClipContent, Color, DrumIcon, DrumMap, Id, Note, Project, Quantize, Route, Sound, Track,
    TrackKind,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

mod common;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn midi_clip(id: u64, notes: Vec<Note>) -> Arc<Clip> {
    Arc::new(Clip {
        id: Id(id),
        name: format!("c{id}"),
        start: Ticks::ZERO,
        length: q(4),
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
    })
}

/// The model the v35 golden file must always load as: a kit track with the
/// 808-style map, one sound carrying every field, and its pattern. Never
/// change it.
fn golden_v35_expected() -> Project {
    let mut map = Preset::EIGHT_OH_EIGHT.map(Id(0x3501));
    // One sound with every field a sound may hold, so the fixture records
    // how each is written.
    let snare = map
        .sounds
        .iter_mut()
        .find(|s| s.name == "Snare")
        .expect("the 808 kit has a snare");
    snare.channel = Some(9);
    snare.route = Route::Output(2);
    snare.mute = true;
    snare.quantize = Some(Quantize {
        size: Size::Note(16),
        feel: Feel::Triplet,
    });
    snare.length = Some(q(1));
    snare.color = Color(0x9E5A6A);
    snare.icon = DrumIcon::Snare;
    let kick = map
        .sounds
        .iter_mut()
        .find(|s| s.name == "Kick")
        .expect("the 808 kit has a kick");
    kick.choke = None;
    let mut track = Track::new(Id(0x3502), "Kit", TrackKind::Midi, Color(0x5E8C6A));
    track.drum_map = Some(map.id);
    track.clips = vec![midi_clip(
        0x3503,
        vec![
            Note::new(Ticks::ZERO, Ticks(120), 36, 100),
            Note::new(q(1), Ticks(120), 38, 90),
        ],
    )];
    let mut project = project_of("Golden v35", vec![track]);
    project.drum_maps = vec![map];
    project
}

#[test]
fn golden_v35_loads_and_round_trips() {
    let project = common::golden("v35", golden_v35_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
    // Every field of a sound survives the file.
    let snare = &project.drum_maps[0].sound(38).expect("the snare").clone();
    assert_eq!(snare.channel, Some(9));
    assert_eq!(snare.route, Route::Output(2));
    assert!(snare.mute);
    assert_eq!(snare.length, Some(q(1)));
    assert_eq!(snare.quantize.map(|q| q.feel), Some(Feel::Triplet));
}

#[test]
fn a_project_before_drum_maps_opens_with_none() {
    let old = load(&fixture("v34/golden.lumen")).expect("v34 file loads");
    assert!(old.drum_maps.is_empty());
    assert!(old.tracks.iter().all(|t| t.drum_map.is_none()));
    assert!(old.master.drum_map.is_none());
}

#[test]
fn a_project_with_no_map_writes_neither() {
    let clip = midi_clip(1, vec![Note::new(Ticks::ZERO, Ticks(120), 36, 100)]);
    let mut track = Track::new(Id(2), "Keys", TrackKind::Midi, Color(0x4C7A9E));
    track.clips = vec![clip];
    let text = String::from_utf8(to_bytes(&project_of("Plain", vec![track])).unwrap()).unwrap();
    assert!(!text.contains("drum_map"), "{text}");
    assert!(!text.contains("drum_maps"), "{text}");
}

#[test]
fn an_icon_is_saved_by_name_and_an_unknown_one_reads_as_percussion() {
    let mut map = DrumMap::new(
        Id(1),
        "Kit",
        vec![Sound::new("Kick", 36, 36, DrumIcon::FloorTom)],
    );
    map.sounds[0].icon = DrumIcon::Cowbell;
    let mut track = Track::new(Id(2), "Kit", TrackKind::Midi, Color(0));
    track.drum_map = Some(map.id);
    let mut project = project_of("Icons", vec![track]);
    project.drum_maps = vec![map];
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    assert!(text.contains("\"cowbell\""), "{text}");

    // A name from a later build reads as percussion rather than refusing the
    // whole project.
    let text = text.replace("\"cowbell\"", "\"tambourine_deep\"");
    let read = from_bytes(text.as_bytes()).expect("an unknown icon still opens");
    assert_eq!(read.drum_maps[0].sounds[0].icon, DrumIcon::Percussion);
}

#[test]
fn a_map_a_project_may_not_hold_is_refused() {
    let base = || {
        let mut map = DrumMap::new(
            Id(7),
            "Kit",
            vec![
                Sound::new("Kick", 36, 36, DrumIcon::Kick),
                Sound::new("Snare", 38, 38, DrumIcon::Snare),
            ],
        );
        map.sounds[1].choke = Some(1);
        let mut track = Track::new(Id(8), "Kit", TrackKind::Midi, Color(0));
        track.drum_map = Some(map.id);
        let mut project = project_of("Refused", vec![track]);
        project.drum_maps = vec![map];
        project
    };
    assert_eq!(validate(&base()), Ok(()));
    let edit = |f: &dyn Fn(&mut Project)| {
        let mut project = base();
        f(&mut project);
        project
    };
    // A sound routed to an instrument track of the project is fine: the
    // kit track itself is id 8.
    assert_eq!(
        validate(&edit(&|p: &mut Project| {
            p.drum_maps[0].sounds[0].route = Route::Track(Id(8));
        })),
        Ok(())
    );
    for operation in [
        // Two maps with one id.
        &(|p: &mut Project| p.drum_maps.push(p.drum_maps[0].clone())) as &dyn Fn(&mut Project),
        // Two sounds answering to one note.
        &|p: &mut Project| p.drum_maps[0].sounds[1].input = 36,
        // A note, a channel or an output pair out of range.
        &|p: &mut Project| p.drum_maps[0].sounds[0].input = 200,
        &|p: &mut Project| p.drum_maps[0].sounds[0].channel = Some(16),
        &|p: &mut Project| p.drum_maps[0].sounds[0].route = Route::Output(0),
        // A length no clip may hold, and choke group 0.
        &|p: &mut Project| p.drum_maps[0].sounds[0].length = Some(Ticks::ZERO),
        &|p: &mut Project| p.drum_maps[0].sounds[0].choke = Some(0),
        // A track naming a map that is not there, or a track that is not
        // MIDI.
        &|p: &mut Project| {
            p.drum_maps.clear();
        },
        &|p: &mut Project| {
            Arc::make_mut(&mut p.tracks[0]).kind = TrackKind::Audio;
        },
        // A map with no name, and a sound with none (a rename refuses one).
        &|p: &mut Project| p.drum_maps[0].name.clear(),
        &|p: &mut Project| p.drum_maps[0].sounds[0].name.clear(),
        &|p: &mut Project| p.drum_maps[0].sounds[0].name = "   ".into(),
        // A sound routed to a track that is not there, or that is not an
        // instrument track.
        &|p: &mut Project| p.drum_maps[0].sounds[0].route = Route::Track(Id(999)),
        &|p: &mut Project| {
            p.tracks.push(Arc::new(Track::new(
                Id(99),
                "Audio",
                TrackKind::Audio,
                Color(0),
            )));
            p.drum_maps[0].sounds[0].route = Route::Track(Id(99));
        },
    ] {
        let project = edit(operation);
        assert!(
            validate(&project).is_err(),
            "a map a project may not hold was accepted"
        );
        assert!(
            to_bytes(&project).map_or(true, |bytes| from_bytes(&bytes).is_err()),
            "it must not round trip either"
        );
    }
}

#[test]
fn translation_reads_the_sound_by_the_input_note() {
    let map = Preset::GENERAL_MIDI.map(Id(1));
    // General MIDI's closed hat is its own note; a note with no sound plays
    // as written.
    assert_eq!(
        drums::played(&map, 42),
        Some(drums::Played {
            key: 42,
            channel: None,
            length: None,
            route: lumen_project::model::Route::Main,
        })
    );
    assert_eq!(
        drums::played(&map, 100),
        Some(drums::Played {
            key: 100,
            channel: None,
            length: None,
            route: lumen_project::model::Route::Main,
        })
    );
    // Swapping a sound's output note changes what plays, never the note.
    let mut edited = map.clone();
    edited.sounds.retain(|s| s.input != 42);
    edited
        .sounds
        .push(Sound::new("Closed Hi-Hat", 42, 60, DrumIcon::ClosedHat));
    assert_eq!(drums::played(&edited, 42).map(|p| p.key), Some(60));
    assert_eq!(
        convert_notes(
            &map,
            &edited,
            &[Note::new(Ticks::ZERO, Ticks(1), 42, 100)],
            false
        )
        .0
        .len(),
        1
    );
}

#[test]
fn every_preset_name_gets_an_icon_and_the_kits_are_different() {
    let icons: Vec<Vec<DrumIcon>> = Preset::ALL
        .iter()
        .map(|p| p.sounds().iter().map(|s| s.icon).collect())
        .collect();
    assert!(icons.iter().all(|list| !list.is_empty()));
    // Each built-in kit has a kick, a snare and an open hat.
    for list in &icons {
        for icon in [DrumIcon::Kick, DrumIcon::Snare, DrumIcon::OpenHat] {
            assert!(list.contains(&icon), "a kit without a {icon:?}");
        }
    }
    // GM2 is GM with the extra effects and percussion, so it is longer.
    assert!(Preset::GENERAL_MIDI_2.sounds().len() > Preset::GENERAL_MIDI.sounds().len());
}
