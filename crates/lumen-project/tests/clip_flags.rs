//! Format v27 (M7b slice E1): an audio clip may be reversed and may have its
//! polarity inverted. Older files hold neither; a flag on a MIDI clip is
//! refused with a reason.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, Color, Id, Media, Project, Track, TrackKind};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn clip(content: ClipContent) -> Clip {
    Clip {
        id: Id(2001),
        name: "Take".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(8),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        audio_phase_seconds: 0.25,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        content,
    }
}

/// The model the v27 golden file must always load as. Never change it.
fn golden_v27_expected() -> Project {
    let mut track = Track::new(Id(2000), "Reversed", TrackKind::Audio, Color(0x4F81BD));
    let mut flagged = clip(ClipContent::Audio {
        media: Id(2002),
        offset: 4_800,
        gain_db: -3.0,
    });
    flagged.reversed = true;
    flagged.inverted = true;
    let mut plain = clip(ClipContent::Audio {
        media: Id(2002),
        offset: 0,
        gain_db: 0.0,
    });
    plain.id = Id(2003);
    plain.name = "Plain".into();
    plain.start = Ticks::quarters(16);
    plain.audio_phase_seconds = 0.0;
    track.clips = vec![Arc::new(flagged), Arc::new(plain)];
    let mut project = project_of("Golden v27", vec![track]);
    project.media = vec![Media {
        id: Id(2002),
        path: "Samples/take.wav".into(),
        original_path: None,
        frames: 480_000,
        sample_rate: 48_000,
        channels: 1,
        size_bytes: 960_000,
    }];
    project
}

#[test]
fn golden_v27_file_loads_forever_and_round_trips() {
    let path = fixture("v27/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v27_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v27 golden file must load");
    assert_eq!(project, golden_v27_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn a_plain_clip_writes_neither_flag() {
    let project = golden_v27_expected();
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    assert_eq!(text.matches("\"reversed\"").count(), 1);
    assert_eq!(text.matches("\"inverted\"").count(), 1);
}

#[test]
fn files_from_before_v27_load_with_neither_flag() {
    let project = load(&fixture("v26/golden.lumen")).expect("v26 golden file must load");
    for track in &project.tracks {
        for clip in &track.clips {
            assert!(!clip.reversed && !clip.inverted);
        }
    }
}

#[test]
fn reverse_or_polarity_on_a_midi_clip_is_refused() {
    for (reversed, inverted) in [(true, false), (false, true)] {
        let mut track = Track::new(Id(2000), "Keys", TrackKind::Midi, Color(0x4F81BD));
        let mut midi = clip(ClipContent::midi());
        midi.audio_phase_seconds = 0.0;
        midi.reversed = reversed;
        midi.inverted = inverted;
        track.clips = vec![Arc::new(midi)];
        let project = project_of("Keys", vec![track]);
        let message = to_bytes(&project).expect_err("a MIDI clip cannot be reversed");
        assert!(message.to_string().contains("MIDI"), "{message}");
    }
}
