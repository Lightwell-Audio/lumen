//! Format v31 (M9 slice R3): a clip may keep the other passes of a loop
//! recording. Earlier files keep none, and a clip without them writes nothing.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{
    Clip, ClipContent, Color, Id, Media, Note, OtherTake, Pass, Project, Takes, Track, TrackKind,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn clip(id: u64, content: ClipContent) -> Clip {
    Clip {
        id: Id(id),
        name: "Take".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(8),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        takes: None,
        content,
    }
}

fn audio_pass(offset: i64) -> Pass {
    Pass::Audio {
        offset,
        audio_phase_seconds: 0.0,
    }
}

fn note(pitch: u8) -> Note {
    Note::new(Ticks::ZERO, Ticks::quarters(1), pitch, 90)
}

/// The model the v31 golden file must always load as. Never change it.
fn golden_v31_expected() -> Project {
    let mut vocal = Track::new(Id(3_100), "Vocal", TrackKind::Audio, Color(0x4F81BD));
    let mut audio = clip(
        3_101,
        ClipContent::Audio {
            media: Id(3_102),
            offset: 192_000,
            gain_db: 0.0,
        },
    );
    audio.takes = Some(Takes::new(
        2,
        3,
        vec![
            OtherTake {
                take: 1,
                pass: audio_pass(96_000),
            },
            OtherTake {
                take: 0,
                pass: Pass::Audio {
                    offset: 0,
                    audio_phase_seconds: 0.5,
                },
            },
        ],
    ));
    vocal.clips = vec![Arc::new(audio)];
    let mut keys = Track::new(Id(3_103), "Keys", TrackKind::Midi, Color(0xC0504D));
    let mut midi = clip(
        3_104,
        ClipContent::Midi {
            notes: vec![note(64)],
            events: vec![],
        },
    );
    midi.takes = Some(Takes::new(
        1,
        2,
        vec![OtherTake {
            take: 0,
            pass: Pass::Midi {
                notes: vec![note(60), note(62)],
                events: vec![],
            },
        }],
    ));
    keys.clips = vec![Arc::new(midi)];
    let mut project = project_of("Golden v31", vec![vocal, keys]);
    project.media = vec![Media {
        id: Id(3_102),
        path: "Samples/Recorded/Vocal 1.wav".into(),
        original_path: None,
        frames: 384_000,
        channels: 1,
        sample_rate: 48_000,
        size_bytes: 0,
    }];
    project
}

#[test]
fn golden_v31_loads_and_round_trips() {
    let path = fixture("v31/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v31_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v31 golden file must load");
    assert_eq!(project, golden_v31_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn older_projects_keep_no_takes() {
    let legacy = load(&fixture("v30/golden.lumen")).expect("v30 file loads");
    assert!(
        legacy
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .all(|c| c.takes.is_none())
    );
}

#[test]
fn a_plain_clip_writes_no_takes() {
    let mut track = Track::new(Id(1), "A", TrackKind::Midi, Color(0));
    track.clips = vec![Arc::new(clip(
        2,
        ClipContent::Midi {
            notes: vec![note(60)],
            events: vec![],
        },
    ))];
    let text = String::from_utf8(to_bytes(&project_of("Plain", vec![track])).unwrap()).unwrap();
    assert!(!text.contains("takes"), "{text}");
}

#[test]
fn takes_that_are_not_a_set_are_refused() {
    let good = golden_v31_expected();
    assert!(validate(&good).is_ok());
    let with = |edit: &dyn Fn(&mut Takes)| {
        let mut project = good.clone();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let clip = Arc::make_mut(&mut track.clips[0]);
        edit(clip.takes.as_mut().unwrap());
        validate(&project)
    };
    // The active pass listed among the others, a number twice, a number
    // past the count, a count that does not add up.
    assert!(with(&|t| t.others[0].take = t.active).is_err());
    assert!(with(&|t| t.others[1].take = t.others[0].take).is_err());
    assert!(with(&|t| t.others[0].take = 9).is_err());
    assert!(with(&|t| t.count = 4).is_err());
    assert!(with(&|t| t.active = 3).is_err());
}

#[test]
fn a_pass_of_the_wrong_kind_or_before_its_file_is_refused() {
    let good = golden_v31_expected();
    let edit_audio = |edit: &dyn Fn(&mut Takes)| {
        let mut project = good.clone();
        let track = Arc::make_mut(&mut project.tracks[0]);
        edit(Arc::make_mut(&mut track.clips[0]).takes.as_mut().unwrap());
        validate(&project)
    };
    assert!(
        edit_audio(&|t| {
            t.others[0].pass = Pass::Midi {
                notes: vec![],
                events: vec![],
            }
        })
        .is_err()
    );
    assert!(edit_audio(&|t| t.others[0].pass = audio_pass(-1)).is_err());
    assert!(
        edit_audio(&|t| {
            t.others[0].pass = Pass::Audio {
                offset: 0,
                audio_phase_seconds: f64::NAN,
            }
        })
        .is_err()
    );
    // A MIDI pass with a note out of range.
    let mut project = good.clone();
    let track = Arc::make_mut(&mut project.tracks[1]);
    let clip = Arc::make_mut(&mut track.clips[0]);
    clip.takes.as_mut().unwrap().others[0].pass = Pass::Midi {
        notes: vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 60, 0)],
        events: vec![],
    };
    assert!(validate(&project).is_err());
}
