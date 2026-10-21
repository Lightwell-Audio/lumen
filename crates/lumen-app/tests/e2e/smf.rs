//! Standard MIDI Files (M5 slice 7, ADR-0062): a file dropped between
//! tracks makes a MIDI track per part, one dropped on a MIDI track or
//! imported onto one is a clip there; the file's tempo and meter are only
//! offered (F2); a damaged file says why; Export MIDI file… and Export MIDI
//! clip… write what plays, read back note for note.

use std::path::PathBuf;

use eframe::egui::{Event, Key, Modifiers, pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Note, Project, TrackKind};
use lumen_project::smf::{self, MidiFile};
use lumen_project::testing::TempDir;

use super::Lumen;
use super::properties::select;

/// A hand-made file from `tests/smf/` (see `make.py`).
fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/smf")
        .join(name)
}

fn read(path: &PathBuf) -> MidiFile {
    smf::read(&std::fs::read(path).expect("the file exists")).expect("the file reads")
}

fn notes(project: &Project, track: usize) -> Vec<Note> {
    project.tracks[track]
        .clips
        .iter()
        .flat_map(|c| match &c.content {
            ClipContent::Midi { notes, .. } => notes
                .iter()
                .map(|n| Note::new(c.start + n.start(), n.length(), n.pitch(), n.velocity()))
                .collect(),
            ClipContent::Audio { .. } => Vec::new(),
        })
        .collect()
}

/// Runs a command by its name from the palette.
fn palette(lumen: &mut Lumen, name: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
}

#[test]
fn a_file_dropped_on_empty_lanes_makes_a_track_per_part_and_offers_its_tempo() {
    let mut lumen = Lumen::start("smf-drop-tracks");
    let groove = sample("groove-480.mid");
    lumen.drop_files(std::slice::from_ref(&groove));
    lumen.wait_for("the tracks", |app| app.project().tracks.len() == 2);
    lumen.frames(30);
    assert_eq!(lumen.track_names(), ["Groove ch 1", "Groove ch 10"]);
    let source = read(&groove);
    let project = lumen.app().project().clone();
    assert!(project.tracks.iter().all(|t| t.kind == TrackKind::Midi));
    let at = project.tracks[0].clips[0].start;
    for (i, part) in source.tracks.iter().enumerate() {
        let shifted: Vec<Note> = part
            .notes
            .iter()
            .map(|n| Note::new(at + n.start(), n.length(), n.pitch(), n.velocity()))
            .collect();
        assert_eq!(notes(&project, i), shifted, "part {i} as in the file");
    }
    assert_eq!(
        lumen.app().undo_label(),
        Some("Import groove-480 (2 tracks)")
    );
    assert_eq!(project.tempo.len(), 1, "the project's tempo is untouched");

    // The banner offers the file's tempo and meter; one click takes them.
    assert!(
        lumen
            .harness
            .query_by_label_contains("groove-480.mid is at 100 BPM, with one tempo change, in 3/4")
            .is_some()
    );
    lumen.click("Use its tempo and meter");
    let project = lumen.app().project();
    assert_eq!(project.tempo, source.tempo);
    assert_eq!(project.meter, source.meter);
    assert_eq!(lumen.app().undo_label(), Some("Use the file's tempo"));
    assert!(
        !lumen.shows("Use its tempo and meter"),
        "the banner has gone"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tempo.len(), 1, "one undo step");
}

#[test]
fn a_file_dropped_or_imported_onto_a_midi_track_is_one_clip_there() {
    let mut lumen = Lumen::start("smf-onto-track");
    let groove = sample("groove-480.mid");
    // Ctrl+Shift+T adds a MIDI track and selects it; Import puts the file
    // on it at the playhead, every part in one clip.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    lumen.frames(30);
    lumen.app_mut().answer_next_dialog(vec![groove.clone()]);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    lumen.wait_for("the file", |app| {
        !app.is_busy() && app.project().tracks[0].clips.len() == 1
    });
    let project = lumen.app().project().clone();
    assert_eq!(project.tracks.len(), 1, "no new track");
    let clip = &project.tracks[0].clips[0];
    assert_eq!(clip.start, lumen.app().playhead());
    assert_eq!(clip.name, "groove-480");
    let all: usize = read(&groove).tracks.iter().map(|p| p.notes.len()).sum();
    assert_eq!(notes(&project, 0).len(), all, "every part");
    assert_eq!(lumen.app().undo_label(), Some("Import groove-480"));

    // Dropped on the track's lane further on: a second clip there.
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, clip.id).expect("in view");
    let smpte = sample("smpte-25.mid");
    lumen.drop_files_at(pos2(r.right() + 200.0, r.center().y), &[smpte]);
    lumen.wait_for("the clip", |app| app.project().tracks[0].clips.len() == 2);
    assert_eq!(lumen.app().project().tracks.len(), 1);
}

#[test]
fn a_damaged_or_type_2_file_says_why_and_adds_nothing() {
    let mut lumen = Lumen::start("smf-bad");
    let bad = lumen.dir().join("bad.mid");
    std::fs::write(&bad, b"not a MIDI file at all").unwrap();
    lumen.drop_files(&[bad, sample("sequential.mid")]);
    lumen.frames(3);
    assert!(lumen.app().project().tracks.is_empty());
    assert!(
        lumen
            .harness
            .query_by_label_contains("sequential.mid is a type 2 MIDI file")
            .is_some(),
        "the last file's reason shows"
    );
}

/// Export writes what plays, and reading the file back gives the same
/// notes; the demo's looping clips come out as real repeats.
#[test]
fn exported_files_read_back_as_they_play() {
    let dir = TempDir::new("e2e-smf-export");
    let path = dir.path().join("song.lumen");
    let demo = lumen_project::testing::midi_demo();
    lumen_project::save(&demo, &path, &Default::default()).unwrap();
    let tracks = demo.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);

    let out = lumen.dir().join("song.mid");
    lumen.app_mut().answer_next_dialog(vec![out.clone()]);
    palette(&mut lumen, "Export MIDI file");
    let written = out.clone();
    lumen.wait_for("the file", move |app| !app.is_busy() && written.exists());
    assert!(
        lumen
            .harness
            .query_by_label_contains("Exported song.mid")
            .is_some()
    );
    let file = read(&out);
    let names: Vec<Option<&str>> = file.tracks.iter().map(|t| t.name.as_deref()).collect();
    assert_eq!(
        names,
        [
            Some("Chords"),
            Some("Pad"),
            Some("Bass"),
            Some("Lead"),
            Some("Hardware")
        ]
    );
    assert_eq!(file.tempo, demo.tempo);
    // The Pad's one clip does not loop: its two notes, as they are.
    assert_eq!(notes(&demo, 1), file.tracks[1].notes);
    // The chords loop every four bars over eight: twice the notes.
    let chords = match &demo.tracks[0].clips[0].content {
        ClipContent::Midi { notes, .. } => notes.len(),
        ClipContent::Audio { .. } => 0,
    };
    assert_eq!(file.tracks[0].notes.len(), chords * 2);

    // One clip, from its menu's command: a type 0 file starting at zero.
    let pad = lumen.app().project().tracks[1].clips[0].id;
    select(&mut lumen, pad);
    let clip_out = lumen.dir().join("pad");
    lumen.app_mut().answer_next_dialog(vec![clip_out]);
    palette(&mut lumen, "Export MIDI clip");
    let written = lumen.dir().join("pad.mid");
    let file = written.clone();
    lumen.wait_for("the file", move |app| !app.is_busy() && file.exists());
    let file = read(&written);
    assert_eq!(file.tracks.len(), 1);
    assert_eq!(file.tracks[0].name.as_deref(), Some("Pad"));
    let first = file.tracks[0].notes[0];
    assert_eq!(first.start(), Ticks::ZERO);
}

/// Two same-pitch notes overlap, and the short second note ends after the
/// long first note. Their note-offs therefore need FIFO pairing on import.
#[test]
fn exporting_and_reimporting_overlapping_same_pitch_notes_keeps_durations() {
    let dir = TempDir::new("e2e-smf-overlapping");
    let path = dir.path().join("overlapping.lumen");
    let expected = vec![
        Note::new(Ticks::ZERO, Ticks::quarters(3), 36, 91),
        Note::new(Ticks::quarters(2), Ticks::quarters(2), 36, 73),
    ];
    let project = lumen_project::testing::project_of(
        "Overlapping",
        vec![lumen_project::testing::midi_track(
            1,
            "Bass",
            "poly",
            (0, 8),
            None,
            expected.clone(),
            vec![],
        )],
    );
    lumen_project::save(&project, &path, &Default::default()).unwrap();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);

    for (name, command) in [
        ("overlapping-file", "Export MIDI file"),
        ("overlapping-clip", "Export MIDI clip"),
    ] {
        let out = lumen.dir().join(format!("{name}.mid"));
        if command == "Export MIDI clip" {
            select(&mut lumen, lumen_project::model::Id(3));
        }
        lumen.app_mut().answer_next_dialog(vec![out.clone()]);
        palette(&mut lumen, command);
        let written = out.clone();
        lumen.wait_for("the exported file", move |app| {
            !app.is_busy() && written.exists()
        });
        assert_eq!(read(&out).tracks[0].notes, expected, "{command}");

        // Reimport the actual exported bytes through the dropped-file flow.
        let mut imported = Lumen::start(&format!("smf-{name}-reimport"));
        imported.drop_files(std::slice::from_ref(&out));
        imported.wait_for("the imported MIDI track", |app| {
            app.project().tracks.len() == 1
        });
        imported.frames(30);
        assert_eq!(notes(imported.app().project(), 0), expected, "{command}");
    }
}

/// Owner's imported-ocarina report: sample rounding must not hide a note
/// when Play begins exactly at its onset, including the second E5.
#[test]
fn imported_notes_sound_when_play_starts_exactly_at_their_onsets() {
    use std::time::Duration;

    let mut lumen = Lumen::start("smf-rounded-onset");
    lumen.drop_files(&[sample("rounded-onset-192.mid")]);
    lumen.wait_for("the imported track", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    lumen.click("Use its tempo");
    assert_eq!(notes(lumen.app().project(), 0).len(), 2);

    for (position, beat) in [("17.2.1", 65), ("17.3.1", 66)] {
        lumen.harness.get_by_label_contains("Position ").click();
        lumen.frames(3);
        lumen.key(Modifiers::COMMAND, Key::A);
        lumen.harness.event(Event::Text(position.into()));
        lumen.key(Modifiers::NONE, Key::Enter);
        // The engine reports the tick nearest the located sample.
        lumen.wait_for("the note onset", |app| {
            (app.playhead().0 - Ticks::quarters(beat).0).abs() < 100
        });
        lumen.key(Modifiers::NONE, Key::Space);
        lumen.wait_for("playback", |app| app.is_playing());
        let heard = lumen.listen(Duration::from_millis(30), 2_000);
        let peak = heard
            .iter()
            .fold(0.0_f32, |peak, f| peak.max(f[0].abs()).max(f[1].abs()));
        assert!(peak > 0.01, "the note at {position} was silent: {peak}");
        lumen.key(Modifiers::NONE, Key::Space);
        lumen.wait_for("stop", |app| !app.is_playing());
        lumen.listen(Duration::from_millis(1_500), 128);
    }
}
