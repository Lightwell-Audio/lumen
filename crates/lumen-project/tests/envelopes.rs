//! Format v25 (M7 slice A7): an audio clip may carry gain and pan envelopes,
//! measured from the clip's start. Older files hold none; a damaged one is
//! refused with a reason.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::envelope::Kind;
use lumen_project::model::{Clip, ClipContent, Color, Id, Media, Project, Track, TrackKind};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn lane(points: &[(i64, f32)]) -> Lane {
    Lane::new(
        points
            .iter()
            .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
            .collect(),
    )
    .unwrap()
}

/// The model the v25 golden file must always load as. Never change it.
fn golden_v25_expected() -> Project {
    let mut track = Track::new(Id(1000), "Vocal", TrackKind::Audio, Color(0x4F81BD));
    let mut clip = Clip {
        id: Id(1001),
        name: "Take".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(8),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: Some(Ticks::quarters(4)),
        loop_phase: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        takes: None,
        content: ClipContent::Audio {
            media: Id(1002),
            offset: 0,
            gain_db: -2.0,
        },
    };
    // A point before the clip's start is kept, hidden.
    clip.envelopes.set(
        Kind::Gain,
        Lane::new(vec![
            Point::new(Ticks::quarters(-1), -12.0),
            Point::new(Ticks::ZERO, 0.0).with_shape(Shape::Step),
            Point::new(Ticks::quarters(2), -6.0),
        ])
        .unwrap(),
    );
    clip.envelopes.set(Kind::Pan, lane(&[(0, -1.0), (3, 1.0)]));
    track.clips = vec![Arc::new(clip)];
    let mut project = project_of("Golden v25", vec![track]);
    project.auto_crossfade = false;
    project.media = vec![Media {
        id: Id(1002),
        path: "Samples/take.wav".into(),
        original_path: None,
        frames: 480_000,
        sample_rate: 48_000,
        channels: 1,
        size_bytes: 960_000,
    }];
    project
}

fn golden_v26_expected() -> Project {
    let mut project = golden_v25_expected();
    let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
    clip.loop_phase = Ticks::quarters(2);
    clip.fade_in = lumen_core::fade::Fade {
        length: Ticks::quarters(2),
        offset: Ticks::quarters(1),
        span: Ticks::quarters(4),
        curve: lumen_core::curve::Curve::SLOW,
    };
    clip.fade_out = lumen_core::fade::Fade {
        length: Ticks::quarters(1),
        offset: Ticks::quarters(1),
        span: Ticks::quarters(2),
        curve: lumen_core::curve::Curve::FAST,
    };
    clip.audio_phase_seconds = 0.125;
    project.name = "Golden v26".into();
    project
}

#[test]
fn golden_v25_file_loads_forever() {
    let path = fixture("v25/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v25_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v25 golden file must load");
    assert_eq!(project, golden_v25_expected());
}

#[test]
fn golden_v26_file_loads_and_round_trips_phase_and_fade_windows() {
    let path = fixture("v26/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v26_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v26 golden file must load");
    assert_eq!(project, golden_v26_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn invalid_loop_phase_and_fade_windows_are_refused() {
    let mut project = golden_v26_expected();
    {
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        clip.loop_length = None;
        clip.loop_phase = Ticks::quarters(1);
    }
    assert!(
        to_bytes(&project)
            .unwrap_err()
            .to_string()
            .contains("loop phase without a loop")
    );
    let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
    clip.loop_length = Some(Ticks::quarters(4));
    clip.loop_phase = Ticks::quarters(2);
    clip.fade_in.offset = Ticks::quarters(3);
    assert!(
        to_bytes(&project)
            .unwrap_err()
            .to_string()
            .contains("invalid fade in window")
    );
}

#[test]
fn an_explicit_audio_source_phase_must_stay_within_the_media_even_for_loops() {
    let mut project = golden_v26_expected();
    let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
    clip.audio_phase_seconds = 1.0e300;
    assert!(
        to_bytes(&project)
            .unwrap_err()
            .to_string()
            .contains("audio source phase exceeds the file")
    );
}

#[test]
fn an_older_project_has_no_envelopes() {
    for old in ["v1/golden.lumen", "v9/golden.lumen", "v24/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        for track in &project.tracks {
            assert!(track.clips.iter().all(|c| c.envelopes.is_empty()), "{old}");
        }
    }
}

#[test]
fn envelopes_are_written_only_when_there_are_some() {
    let with = String::from_utf8(to_bytes(&golden_v25_expected()).unwrap()).unwrap();
    assert!(with.contains("\"envelopes\""));
    let mut plain = golden_v25_expected();
    Arc::make_mut(&mut Arc::make_mut(&mut plain.tracks[0]).clips[0]).envelopes = Default::default();
    let without = String::from_utf8(to_bytes(&plain).unwrap()).unwrap();
    assert!(!without.contains("envelopes"));
}

#[test]
fn an_envelope_that_cannot_work_is_refused_with_a_reason() {
    let refused = |edit: &dyn Fn(&mut Clip)| {
        let mut project = golden_v25_expected();
        edit(Arc::make_mut(
            &mut Arc::make_mut(&mut project.tracks[0]).clips[0],
        ));
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let why = refused(&|c| c.envelopes.set(Kind::Gain, lane(&[(0, 40.0)])));
    assert!(why.contains("outside"), "{why}");
    let why = refused(&|c| c.envelopes.set(Kind::Pan, lane(&[(0, 2.0)])));
    assert!(why.contains("outside"), "{why}");
    let why = refused(&|c| {
        c.content = ClipContent::midi();
    });
    assert!(why.contains("MIDI clip holds an envelope"), "{why}");
    let why = refused(&|c| {
        c.envelopes.set(
            Kind::Gain,
            Lane::new(vec![Point::new(Ticks(1 << 60), 0.0)]).unwrap(),
        );
    });
    assert!(why.contains("out of range"), "{why}");
}

#[test]
fn a_damaged_envelope_file_is_not_opened() {
    let text = String::from_utf8(to_bytes(&golden_v25_expected()).unwrap()).unwrap();
    // The clip's pan reaches 1.0: make it 100.
    let damaged = text.replacen("\"value\": 1.0", "\"value\": 100.0", 1);
    assert_ne!(text, damaged);
    assert!(from_bytes(damaged.as_bytes()).is_err());
}

#[test]
fn a_saved_project_reads_back_the_same() {
    let project = golden_v25_expected();
    let back = from_bytes(&to_bytes(&project).unwrap()).unwrap();
    assert_eq!(back, project);
}
