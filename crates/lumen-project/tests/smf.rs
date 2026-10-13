//! Standard MIDI Files (M5 slice 7, ADR-0062): the hand-made files in
//! `tests/smf/` (written by `make.py`, not by Lumen) and a golden file
//! Lumen wrote, read and written back.

use std::fs;
use std::path::PathBuf;

use lumen_core::time::{PPQ, TempoPoint, Ticks};
use lumen_project::model::{ClipEvent, EventKind, Note, TimeSignature};
use lumen_project::smf::{self, FileTrack, Layout, MidiFile, ReadError};

fn file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/smf")
        .join(name)
}

fn read(name: &str) -> MidiFile {
    let bytes = fs::read(file(name)).expect("the test file exists");
    smf::read(&bytes).expect("the test file reads")
}

/// `n / d` quarter notes.
fn q(n: i64, d: i64) -> Ticks {
    Ticks(PPQ * n / d)
}

fn note(start: Ticks, length: Ticks, pitch: u8, velocity: u8) -> Note {
    Note::new(start, length, pitch, velocity)
}

fn event(at: Ticks, kind: EventKind) -> ClipEvent {
    ClipEvent { at, kind }
}

fn bpm_near(point: &TempoPoint, at: Ticks, bpm: f64) {
    assert_eq!(point.at, at);
    assert!(
        (point.bpm - bpm).abs() < 0.001,
        "{} BPM, not {bpm}",
        point.bpm
    );
}

#[test]
fn a_type_0_file_splits_by_channel_with_running_status_and_odd_note_endings() {
    let file = read("groove-480.mid");
    assert_eq!(file.tempo.len(), 2);
    bpm_near(&file.tempo[0], Ticks::ZERO, 100.0);
    bpm_near(&file.tempo[1], q(8, 1), 140.0);
    assert_eq!(
        file.meter,
        Some(TimeSignature {
            numerator: 3,
            denominator: 4
        })
    );
    assert_eq!(file.tracks.len(), 2, "channels 1 and 10");
    let keys = &file.tracks[0];
    assert_eq!((keys.channel, keys.source), (0, 0));
    assert_eq!(keys.name.as_deref(), Some("Groove"));
    assert_eq!(
        keys.notes,
        vec![
            note(Ticks::ZERO, q(1, 1), 60, 100),
            // Ended by a note-on with velocity 0, under running status.
            note(q(1, 1), q(1, 1), 64, 90),
            // Struck again before its note-off: the second strike ends it.
            note(q(2, 1), q(1, 2), 62, 80),
            note(q(5, 2), q(1, 2), 62, 70),
        ]
    );
    assert_eq!(
        keys.events,
        vec![
            // The bank select at the program change's tick is its bank.
            event(
                Ticks::ZERO,
                EventKind::Program {
                    program: 5,
                    bank: Some(1 << 7 | 2)
                }
            ),
            event(q(3, 1), EventKind::PitchBend { value: 4096 }),
            event(q(3, 1), EventKind::Pressure { value: 90 }),
        ],
        "polyphonic aftertouch and sysex are dropped"
    );
    let drums = &file.tracks[1];
    assert_eq!((drums.channel, drums.source), (9, 0));
    assert_eq!(drums.name.as_deref(), Some("Groove"));
    assert_eq!(
        drums.notes,
        vec![
            note(Ticks::ZERO, q(1, 2), 36, 120),
            note(q(1, 1), q(1, 2), 36, 110),
            // Never released: it ends with its track.
            note(q(3, 1), q(5, 1), 46, 100),
        ]
    );
    assert!(drums.events.is_empty());
    assert_eq!(file.end(), q(8, 1));
}

#[test]
fn an_smpte_file_is_placed_through_its_own_tempo() {
    let file = read("smpte-25.mid");
    assert_eq!(file.tempo.len(), 2);
    bpm_near(&file.tempo[0], Ticks::ZERO, 120.0);
    bpm_near(&file.tempo[1], q(2, 1), 60.0);
    assert_eq!(file.meter, None);
    assert_eq!(file.tracks.len(), 1, "the conductor track has no notes");
    let lead = &file.tracks[0];
    assert_eq!((lead.channel, lead.source), (0, 1));
    assert_eq!(lead.name.as_deref(), Some("Lead"));
    assert_eq!(
        lead.notes,
        vec![
            note(Ticks::ZERO, q(1, 2), 72, 100),
            note(q(1, 1), q(1, 2), 74, 100),
            note(q(3, 1), q(1, 4), 76, 100),
        ]
    );
}

#[test]
fn a_type_2_file_and_a_damaged_one_are_refused() {
    let bytes = fs::read(file("sequential.mid")).unwrap();
    assert_eq!(smf::read(&bytes), Err(ReadError::Sequential));
    assert!(matches!(
        smf::read(b"RIFF not a midi file"),
        Err(ReadError::Malformed(_))
    ));
    // Cut off inside its header.
    assert!(matches!(
        smf::read(&bytes[..10]),
        Err(ReadError::Malformed(_))
    ));
}

/// What the golden file holds: every kind of event, at positions exact at
/// 960 ticks a quarter.
fn golden() -> MidiFile {
    let keys = FileTrack {
        name: Some("Keys".into()),
        channel: 2,
        source: 1,
        notes: vec![
            note(Ticks::ZERO, q(1, 1), 60, 100),
            note(Ticks::ZERO, q(1, 1), 64, 90),
            // Touching the next note of the same pitch.
            note(q(1, 1), q(1, 2), 67, 1),
            note(q(3, 2), q(1, 960), 67, 127),
            note(q(7, 2), q(3, 1), 72, 64),
        ],
        events: vec![
            event(
                Ticks::ZERO,
                EventKind::Program {
                    program: 0,
                    bank: Some(16_383),
                },
            ),
            event(
                Ticks::ZERO,
                EventKind::Controller {
                    number: 64,
                    value: 127,
                },
            ),
            event(q(1, 3), EventKind::PitchBend { value: -8192 }),
            event(q(2, 3), EventKind::PitchBend { value: 8191 }),
            event(q(1, 1), EventKind::PitchBend { value: 0 }),
            event(q(2, 1), EventKind::Pressure { value: 33 }),
            event(
                q(4, 1),
                EventKind::Controller {
                    number: 64,
                    value: 0,
                },
            ),
            event(
                q(5, 1),
                EventKind::Program {
                    program: 127,
                    bank: None,
                },
            ),
        ],
    };
    let drums = FileTrack {
        name: Some("Drums".into()),
        channel: 9,
        source: 2,
        notes: (0..8)
            .map(|i| note(q(i, 2), q(1, 8), if i % 2 == 0 { 36 } else { 42 }, 100))
            .collect(),
        events: vec![],
    };
    MidiFile {
        tracks: vec![keys, drums],
        tempo: vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 125.0,
            },
            TempoPoint {
                at: q(4, 1),
                bpm: 80.0,
            },
        ],
        meter: Some(TimeSignature {
            numerator: 7,
            denominator: 8,
        }),
    }
}

#[test]
fn the_golden_file_is_written_as_before_and_reads_back_exactly() {
    let path = file("lumen-960.mid");
    let bytes = smf::write(&golden(), Layout::Parallel);
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when the writer was introduced; never rewritten.
        fs::write(&path, &bytes).unwrap();
    }
    assert_eq!(bytes, fs::read(&path).expect("the golden file exists"));
    assert_eq!(smf::read(&bytes), Ok(golden()));
}

#[test]
fn a_type_0_file_round_trips_its_parts_by_channel() {
    let bytes = smf::write(&golden(), Layout::Single);
    let mut read = smf::read(&bytes).unwrap();
    let mut expected = golden();
    for part in &mut expected.tracks {
        part.source = 0;
        // One track holds both parts, under the first part's name.
        part.name = Some("Keys".into());
    }
    for part in &mut read.tracks {
        assert_eq!(part.source, 0);
    }
    read.tracks.iter_mut().for_each(|p| p.source = 0);
    assert_eq!(read, expected);
}

#[test]
fn positions_finer_than_the_division_round_to_the_nearest_step() {
    let step = PPQ / i64::from(smf::DIVISION);
    let file = MidiFile {
        tracks: vec![FileTrack {
            notes: vec![
                // Just under half a step late, and a length under one step.
                note(Ticks(step * 4 + step / 2 - 1), Ticks(step / 3), 60, 100),
                // Just over half a step: the next step.
                note(Ticks(step * 8 + step / 2 + 1), Ticks(step * 2), 62, 100),
            ],
            ..FileTrack::default()
        }],
        ..MidiFile::default()
    };
    let read = smf::read(&smf::write(&file, Layout::Single)).unwrap();
    assert_eq!(
        read.tracks[0].notes,
        vec![
            // A note never shrinks to nothing: one step at least.
            note(Ticks(step * 4), Ticks(step), 60, 100),
            note(Ticks(step * 9), Ticks(step * 2), 62, 100),
        ]
    );
    assert!(read.tempo.is_empty(), "no tempo given, none written");
}
