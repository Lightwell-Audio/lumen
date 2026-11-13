//! The built-in sampler in the model (format v36, M8 slice B5).
//!
//! The format ones: a golden file that loads forever, an older project that
//! opens with no sample, a project with none that writes none, and the checks
//! that refuse a sound a project may not hold. The rules ones: which pad and
//! zone answer a note, that a map never loses a pad, and that the media pool
//! counts what the sampler plays as used.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::drums::presets::Preset;
use lumen_project::edit::media;
use lumen_project::model::{
    Clip, ClipContent, Color, Device, DrumIcon, DrumMap, Face, Id, Media, OneShot, Pad, PluginRef,
    Project, Region, SAMPLER, Sampler, Sound, Track, TrackKind, Zone,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

mod common;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

fn midi_clip(id: u64) -> Arc<Clip> {
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
            notes: Vec::new(),
            events: Vec::new(),
        },
    })
}

/// A media entry of `frames` frames at 48 kHz.
fn file(id: u64, name: &str, frames: u64) -> Media {
    Media {
        id: Id(id),
        path: format!("Samples/Imported/{name}.wav"),
        original_path: None,
        frames,
        channels: 2,
        sample_rate: 48_000,
        size_bytes: frames * 8,
    }
}

/// A MIDI track with the built-in sampler holding `sampler`.
fn sampled(id: u64, sampler: Sampler, media: Vec<Media>) -> Project {
    let mut track = Track::new(Id(id), "Drums", TrackKind::Midi, Color(0x5E8C6A));
    let mut device = Device::new(
        Id(id + 1),
        PluginRef::Builtin {
            name: SAMPLER.to_owned(),
        },
        false,
        Vec::new(),
    );
    device.sampler = Some(sampler);
    track.devices = vec![device];
    track.clips = vec![midi_clip(id + 2)];
    let mut project = project_of("Sampler", vec![track]);
    project.media = media;
    project
}

/// The model the v36 golden file must always load as: a sampler with every
/// field a sound may hold, so the fixture records how each is written. Never
/// change it.
fn golden_v36_expected() -> Project {
    let mut sampler = Sampler {
        face: Face::Pads,
        one_shot: OneShot {
            region: Some(Region {
                media: Id(0x3603),
                start: 480,
                end: Some(9_600),
                root: 60,
            }),
            looping: true,
            reversed: true,
        },
        zones: vec![Zone {
            media: Id(0x3604),
            low: 36,
            high: 47,
            low_velocity: 1,
            high_velocity: 127,
            root: 40,
        }],
        pads: Vec::new(),
    };
    sampler.pads.push(Pad {
        note: 38,
        media: Some(Id(0x3605)),
        level_db: -3.5,
        pan: 0.25,
        tune: -2.0,
        output: 2,
        choke_fade: 0.01,
        root: Some(40),
    });
    sampler.pads.push(Pad::playing(36, Id(0x3605)));
    sampler.pads.sort_by_key(|p| p.note);
    sampled(
        0x3601,
        sampler,
        vec![
            file(0x3603, "one", 48_000),
            file(0x3604, "zones", 24_000),
            file(0x3605, "pads", 12_000),
        ],
    )
}

#[test]
fn golden_v36_loads_and_round_trips() {
    let project = common::golden("v36", golden_v36_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
    // Every field survives the file.
    let sampler = project.tracks[0].devices[0]
        .sampler
        .as_ref()
        .expect("the golden holds a sampler");
    assert_eq!(sampler.face, Face::Pads);
    let region = sampler.one_shot.region.expect("a region");
    assert_eq!(
        (region.start, region.end, region.root),
        (480, Some(9_600), 60)
    );
    assert!(sampler.one_shot.looping && sampler.one_shot.reversed);
    assert_eq!(sampler.zones[0].high, 47);
    let pad = sampler.pad(38).expect("the snare pad");
    assert_eq!(pad.output, 2);
    assert_eq!(pad.root, Some(40));
    // The device is an instrument, so it takes a MIDI track's notes.
    assert!(project.tracks[0].devices[0].is_instrument());
    assert!(project.tracks[0].devices[0].is_sampler());
}

#[test]
fn a_project_before_the_sampler_opens_with_no_sound() {
    let old = load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v35/golden.lumen"),
    )
    .expect("the v35 file loads");
    assert!(
        old.tracks
            .iter()
            .all(|t| t.devices.iter().all(|d| d.sampler.is_none()))
    );
}

#[test]
fn a_project_with_no_sampler_writes_none() {
    let project = sampled(1, Sampler::default(), Vec::new());
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    // An empty sound is written (its face and no samples), and nothing else.
    assert!(text.contains("\"sampler\""), "{text}");
    assert!(!text.contains("\"zones\""), "{text}");
    assert!(!text.contains("\"pads\""), "{text}");
    assert!(!text.contains("\"region\""), "{text}");

    // A device that is not the sampler writes no sound at all.
    let mut project = sampled(1, Sampler::default(), Vec::new());
    let track = Arc::make_mut(&mut project.tracks[0]);
    track.devices[0].plugin = PluginRef::Builtin {
        name: "utility".to_owned(),
    };
    track.devices[0].sampler = None;
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    assert!(!text.contains("\"sampler\""), "{text}");
}

#[test]
fn a_sampler_a_project_may_not_hold_is_refused() {
    let base = || {
        sampled(
            10,
            Sampler {
                one_shot: OneShot {
                    region: Some(Region::whole(Id(20), 60)),
                    looping: false,
                    reversed: false,
                },
                ..Sampler::default()
            },
            vec![file(20, "one", 1_000)],
        )
    };
    assert_eq!(validate(&base()), Ok(()));
    let edit = |f: &dyn Fn(&mut Project)| {
        let mut project = base();
        f(&mut project);
        project
    };
    fn device(p: &mut Project) -> &mut Sampler {
        Arc::make_mut(&mut p.tracks[0]).devices[0]
            .sampler
            .as_mut()
            .expect("the sound")
    }
    for operation in [
        // A region past the end of its file, and one that ends before it
        // starts.
        &(|p: &mut Project| {
            device(p).one_shot.region = Some(Region {
                start: 900,
                end: Some(1_100),
                ..Region::whole(Id(20), 60)
            });
        }) as &dyn Fn(&mut Project),
        &|p: &mut Project| {
            device(p).one_shot.region = Some(Region {
                start: 600,
                end: Some(400),
                ..Region::whole(Id(20), 60)
            });
        },
        // A sample that is not in the media, a zone that is not, and a pad
        // that names one.
        &|p: &mut Project| {
            device(p).one_shot.region = Some(Region::whole(Id(99), 60));
        },
        &|p: &mut Project| device(p).zones.push(Zone::single(Id(99), 36, 36)),
        &|p: &mut Project| device(p).pads.push(Pad::playing(36, Id(99))),
        // Ranges out of order, outside the note range, and with no velocity.
        &|p: &mut Project| {
            device(p).zones.push(Zone {
                low: 60,
                high: 40,
                ..Zone::single(Id(20), 60, 60)
            });
        },
        &|p: &mut Project| {
            device(p).zones.push(Zone {
                high: 200,
                ..Zone::single(Id(20), 60, 60)
            });
        },
        &|p: &mut Project| {
            device(p).zones.push(Zone {
                low_velocity: 0,
                ..Zone::single(Id(20), 60, 60)
            });
        },
        // A pad at a level, a pan, a tune, an output pair or a fade no person
        // could have set.
        &|p: &mut Project| device(p).pad_mut(36).level_db = 40.0,
        &|p: &mut Project| device(p).pad_mut(36).pan = 2.0,
        &|p: &mut Project| device(p).pad_mut(36).tune = 40.0,
        &|p: &mut Project| device(p).pad_mut(36).output = 9,
        &|p: &mut Project| device(p).pad_mut(36).choke_fade = 0.0,
        // A sound on a device that is not a sampler.
        &|p: &mut Project| {
            Arc::make_mut(&mut p.tracks[0]).devices[0].plugin = PluginRef::Builtin {
                name: "utility".to_owned(),
            };
        },
    ] {
        let project = edit(operation);
        assert!(
            validate(&project).is_err(),
            "a sampler a project may not hold was accepted"
        );
        assert!(
            to_bytes(&project).map_or(true, |bytes| from_bytes(&bytes).is_err()),
            "it must not round trip either"
        );
    }
}

#[test]
fn the_pads_are_the_maps_sounds_or_sixteen_of_its_own() {
    let own = Sampler::pads_for(None);
    assert_eq!(own.len(), 16);
    assert_eq!(own[0], 36);
    assert_eq!(own[15], 51);

    let map = Preset::EIGHT_OH_EIGHT.map(Id(1));
    let notes = Sampler::pads_for(Some(&map));
    assert_eq!(notes.len(), map.sounds.len());
    assert!(notes.contains(&36) && notes.contains(&46), "{notes:?}");
}

/// Attaching, swapping and detaching a map never loses a pad's sample or its
/// settings: a pad that the new map also offers keeps everything, and one it
/// does not is dropped with the sound it belonged to.
#[test]
fn a_pad_keeps_its_sample_when_the_map_changes() {
    let mut sampler = Sampler {
        face: Face::Pads,
        ..Sampler::default()
    };
    let eight_oh_eight = Preset::EIGHT_OH_EIGHT.map(Id(1));
    let general = Preset::GENERAL_MIDI.map(Id(2));
    sampler.keep_pads(&Sampler::pads_for(Some(&eight_oh_eight)));
    sampler.pad_mut(36).media = Some(Id(7));
    sampler.pad_mut(36).tune = 5.0;
    sampler.pad_mut(99).media = Some(Id(8));
    sampler.keep_pads(&Sampler::pads_for(Some(&general)));
    // The kick is in both kits and keeps its sample and its tune.
    let kick = sampler.pad(36).expect("the kick");
    assert_eq!((kick.media, kick.tune), (Some(Id(7)), 5.0));
    // The pad the new map does not offer is gone; the map's own sounds are
    // all there.
    assert!(sampler.pad(99).is_none());
    assert_eq!(sampler.pads.len(), general.sounds.len());
    // And the sixteen of its own come back when no map is named.
    sampler.keep_pads(&Sampler::pads_for(None));
    assert_eq!(sampler.pads.len(), 16);
    assert_eq!(sampler.pad(36).and_then(|p| p.media), Some(Id(7)));
}

#[test]
fn a_zone_answers_the_keys_and_velocities_it_covers() {
    let mut sampler = Sampler::default();
    sampler.zones.push(Zone {
        low: 36,
        high: 47,
        low_velocity: 1,
        high_velocity: 63,
        ..Zone::single(Id(1), 40, 40)
    });
    sampler.zones.push(Zone {
        low: 36,
        high: 47,
        low_velocity: 64,
        high_velocity: 127,
        ..Zone::single(Id(2), 40, 40)
    });
    assert_eq!(sampler.zone(36, 100).map(|z| z.media), Some(Id(2)));
    assert_eq!(sampler.zone(36, 60).map(|z| z.media), Some(Id(1)));
    assert_eq!(sampler.zone(47, 1).map(|z| z.media), Some(Id(1)));
    assert!(sampler.zone(50, 100).is_none());
}

#[test]
fn the_media_pool_counts_what_the_sampler_plays_as_used() {
    let project = golden_v36_expected();
    let counts = media::counts(&project);
    for id in [0x3603, 0x3604, 0x3605] {
        assert!(counts.contains_key(&Id(id)), "{id:x} is played");
    }
    // A file no face plays is unused, and a purge keeps the played ones.
    let mut project = project;
    project.media.push(file(0x3609, "spare", 100));
    let unused = media::unused(&project);
    assert_eq!(unused, [Id(0x3609)]);
    assert_eq!(
        media::remove_unused(&mut project, &unused),
        [Id(0x3609)],
        "only the spare goes"
    );
    assert_eq!(project.media.len(), 3);
}

#[test]
fn the_faces_cycle_and_say_what_they_are() {
    assert_eq!(Face::default(), Face::OneShot);
    assert_eq!(Face::OneShot.next(), Face::Zones);
    assert_eq!(Face::Zones.next(), Face::Pads);
    assert_eq!(Face::Pads.next(), Face::OneShot);
    for face in Face::ALL {
        assert!(!face.name().is_empty());
    }
}

/// A drum map's sounds name the notes a clip stores; a pad answers the note,
/// so a pad's sample follows the note rather than the sound's name.
#[test]
fn a_pad_answers_the_maps_input_note() {
    let mut map: DrumMap = Preset::EIGHT_OH_EIGHT.map(Id(1));
    map.sounds
        .push(Sound::new("Cowbell", 99, 99, DrumIcon::Cowbell));
    let mut sampler = Sampler::default();
    sampler.keep_pads(&Sampler::pads_for(Some(&map)));
    sampler.pad_mut(99).media = Some(Id(4));
    assert_eq!(sampler.pad(99).map(|p| p.media), Some(Some(Id(4))));
    // Attaching a map with the same sounds keeps the sample.
    let mut again = map.clone();
    again.name = "Copy".to_owned();
    sampler.keep_pads(&Sampler::pads_for(Some(&again)));
    assert_eq!(sampler.pad(99).map(|p| p.media), Some(Some(Id(4))));
}
