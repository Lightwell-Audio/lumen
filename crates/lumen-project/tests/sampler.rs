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

fn golden_v37_expected() -> Project {
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
fn golden_v37_loads_and_round_trips() {
    let project = common::golden("v37", golden_v37_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);

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

    assert!(text.contains("\"sampler\""), "{text}");
    assert!(!text.contains("\"zones\""), "{text}");
    assert!(!text.contains("\"pads\""), "{text}");
    assert!(!text.contains("\"region\""), "{text}");

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
        &|p: &mut Project| {
            device(p).one_shot.region = Some(Region::whole(Id(99), 60));
        },
        &|p: &mut Project| device(p).zones.push(Zone::single(Id(99), 36, 36)),
        &|p: &mut Project| device(p).pads.push(Pad::playing(36, Id(99))),
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
        &|p: &mut Project| device(p).pad_mut(36).level_db = 40.0,
        &|p: &mut Project| device(p).pad_mut(36).pan = 2.0,
        &|p: &mut Project| device(p).pad_mut(36).tune = 40.0,
        &|p: &mut Project| device(p).pad_mut(36).output = 9,
        &|p: &mut Project| device(p).pad_mut(36).choke_fade = 0.0,
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

    let kick = sampler.pad(36).expect("the kick");
    assert_eq!((kick.media, kick.tune), (Some(Id(7)), 5.0));

    assert!(sampler.pad(99).is_none());
    assert_eq!(sampler.pads.len(), general.sounds.len());

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
    let project = golden_v37_expected();
    let counts = media::counts(&project);
    for id in [0x3603, 0x3604, 0x3605] {
        assert!(counts.contains_key(&Id(id)), "{id:x} is played");
    }

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

#[test]
fn a_pad_answers_the_maps_input_note() {
    let mut map: DrumMap = Preset::EIGHT_OH_EIGHT.map(Id(1));
    map.sounds
        .push(Sound::new("Cowbell", 99, 99, DrumIcon::Cowbell));
    let mut sampler = Sampler::default();
    sampler.keep_pads(&Sampler::pads_for(Some(&map)));
    sampler.pad_mut(99).media = Some(Id(4));
    assert_eq!(sampler.pad(99).map(|p| p.media), Some(Some(Id(4))));

    let mut again = map.clone();
    again.name = "Copy".to_owned();
    sampler.keep_pads(&Sampler::pads_for(Some(&again)));
    assert_eq!(sampler.pad(99).map(|p| p.media), Some(Some(Id(4))));
}
