use lumen_core::midi::Kind;
use lumen_core::time::PPQ;
use lumen_project::model::{Clip, ClipEvent, Color, Note, Track, TrackKind};

use super::*;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Arc<Clip> {
    Arc::new(Clip {
        id: Id(id),
        name: String::new(),
        start: q(start),
        length: q(length),
        muted: false,
        color: None,
        locked: false,
        crossfade: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        content: ClipContent::Midi {
            notes,
            events: Vec::new(),
        },
    })
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

fn whole(clips: &[Arc<Clip>]) -> Vec<Member<'_>> {
    clips
        .iter()
        .map(|clip| Member {
            clip,
            length: clip.length,
        })
        .collect()
}

/// `(start, length, pitch)` of a run's notes, in quarters, sorted.
fn played(run: &[Member<'_>]) -> Vec<(i64, i64, u8)> {
    let mut out: Vec<_> = run_notes(run)
        .iter()
        .map(|n| (n.start.0 / q(1).0, n.length.0 / q(1).0, n.pitch))
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn a_note_held_across_a_split_plays_whole_as_one_note() {
    // A note from 2 to 10 split at 4 and 6, with a short one on each side.
    let clips = [
        clip(1, 0, 4, vec![note(2, 8, 60), note(0, 1, 62)]),
        clip(2, 4, 2, vec![note(-2, 8, 60), note(1, 1, 64)]),
        clip(3, 6, 6, vec![note(-4, 8, 60), note(-6, 1, 62)]),
    ];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 1, "touching clips play as one");
    assert_eq!(played(&found[0]), vec![(0, 1, 62), (2, 8, 60), (5, 1, 64)]);
    let mut cache = Cache::default();
    let engine = cache.clip_with_suppression(&found[0], None);
    assert_eq!(
        &*engine[0].seams,
        &[q(4), q(6)],
        "the engine knows where each clip begins"
    );
}

#[test]
fn a_clip_that_does_not_hold_the_note_cuts_it_at_the_seam() {
    // Moved up against the first clip, the second holds no such note.
    let clips = [
        clip(1, 0, 4, vec![note(2, 8, 60)]),
        clip(2, 4, 4, vec![note(0, 1, 64)]),
    ];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 1);
    assert_eq!(played(&found[0]), vec![(2, 2, 60), (4, 1, 64)]);
}

#[test]
fn a_hidden_capture_sharing_a_start_does_not_carry_the_old_note_on() {
    // The old note sounds 1 to 3; a punched take from 2 keeps a recorded
    // note of the same pitch hidden from 1 to 5. The old note stops at 2
    // and nothing sounds in the take's window.
    let clips = [
        clip(1, 0, 2, vec![note(1, 2, 60)]),
        clip(2, 2, 2, vec![note(-1, 4, 60)]),
    ];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 1);
    assert_eq!(played(&found[0]), vec![(1, 1, 60)]);
}

#[test]
fn an_identical_or_shorter_hidden_capture_is_not_a_seam_copy() {
    let old_end = q(2) + Ticks(PPQ / 2);
    for captured_end in [old_end, old_end - Ticks(PPQ / 4)] {
        let clips = [
            clip(1, 0, 2, vec![Note::new(q(1), old_end - q(1), 60, 100)]),
            clip(
                2,
                2,
                2,
                vec![Note::new(q(-1), captured_end - q(1), 60, 100).with_carries_on(false)],
            ),
        ];
        let found = runs(whole(&clips));
        assert_eq!(found.len(), 1);
        assert_eq!(played(&found[0]), vec![(1, 1, 60)]);
    }
}

#[test]
fn apart_or_looping_clips_are_runs_of_their_own() {
    let clips = [
        clip(1, 0, 4, vec![note(2, 8, 60)]),
        clip(2, 5, 4, vec![note(-3, 8, 60)]),
    ];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 2);
    assert_eq!(played(&found[0]), vec![(2, 2, 60)]);
    assert_eq!(played(&found[1]), vec![], "hidden before its start");

    let mut looping = clip(3, 4, 8, vec![note(0, 3, 60)]);
    Arc::make_mut(&mut looping).loop_length = Some(q(2));
    let clips = [clip(1, 0, 4, vec![note(2, 8, 60)]), looping];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 2);
    assert_eq!(played(&found[1]), vec![(0, 3, 60)], "the engine cuts loops");
}

#[test]
fn split_loop_pieces_keep_source_phase_for_playback() {
    let source = vec![note(0, 1, 60), note(2, 2, 62)];
    let mut first = clip(31, 0, 1, source.clone());
    Arc::make_mut(&mut first).loop_length = Some(q(4));
    Arc::make_mut(&mut first).loop_phase = q(3);
    Arc::make_mut(&mut first).content = ClipContent::Midi {
        notes: source.clone(),
        events: [0, 1, 3]
            .into_iter()
            .map(|at| ClipEvent {
                at: q(at),
                kind: EventKind::Controller {
                    number: 1,
                    value: at as u8,
                },
            })
            .collect(),
    };
    let mut second = clip(32, 1, 5, source);
    Arc::make_mut(&mut second).loop_length = Some(q(4));
    Arc::make_mut(&mut second).loop_phase = q(4);
    Arc::make_mut(&mut second).content = first.content.clone();

    let clips = [first, second];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 1, "contiguous split pieces form one loop run");
    let mut cache = Cache::default();
    let engine = cache.clip_with_suppression(&found[0], None);
    assert_eq!(engine.len(), 2, "phase fragment then periodic remainder");
    assert_eq!(
        (engine[0].start, engine[0].length, engine[0].period),
        (q(0), q(1), None)
    );
    assert_eq!(
        (engine[1].start, engine[1].length, engine[1].period),
        (q(1), q(5), Some(q(4)))
    );
    assert_eq!(
        engine[0]
            .notes
            .iter()
            .map(|n| (n.start, n.length, n.pitch))
            .collect::<Vec<_>>(),
        [(q(0), q(1), 62)],
        "the note already sounding at the split starts at the clip boundary"
    );
    assert_eq!(
        engine[1]
            .notes
            .iter()
            .map(|n| (n.start, n.length, n.pitch))
            .collect::<Vec<_>>(),
        [(q(0), q(1), 60), (q(2), q(2), 62)]
    );
    assert_eq!(engine[0].controls.len(), 1);
    assert_eq!(engine[0].controls[0].at, q(0));
    assert_eq!(
        engine[0].controls[0].msg.kind(),
        Kind::Controller {
            number: 1,
            value: 3
        }
    );
    assert_eq!(
        engine[1]
            .controls
            .iter()
            .map(|c| (c.at, c.msg.kind()))
            .collect::<Vec<_>>(),
        [
            (
                q(0),
                Kind::Controller {
                    number: 1,
                    value: 0
                }
            ),
            (
                q(1),
                Kind::Controller {
                    number: 1,
                    value: 1
                }
            ),
            (
                q(3),
                Kind::Controller {
                    number: 1,
                    value: 3
                }
            ),
        ]
    );
}

#[test]
fn a_split_loop_seam_does_not_cut_a_sustained_source_note() {
    let source = vec![note(0, 3, 60)];
    let mut first = clip(33, 0, 1, source.clone());
    Arc::make_mut(&mut first).loop_length = Some(q(4));
    Arc::make_mut(&mut first).loop_phase = q(1);
    let mut second = clip(34, 1, 7, source);
    Arc::make_mut(&mut second).loop_length = Some(q(4));
    Arc::make_mut(&mut second).loop_phase = q(2);
    let clips = [first, second];
    let found = runs(whole(&clips));
    assert_eq!(found.len(), 1);
    let mut cache = Cache::default();
    let engine = cache.clip_with_suppression(&found[0], None);
    assert_eq!(engine.len(), 2);
    assert_eq!(engine[0].period, None);
    assert_eq!((engine[0].start, engine[0].length), (q(0), q(3)));
    assert_eq!(engine[0].seams.as_ref(), &[q(1)]);
    assert_eq!(engine[0].notes[0].start, q(0));
    assert_eq!(engine[0].notes[0].length, q(2));
    assert_eq!(engine[1].period, Some(q(4)));
}

#[test]
fn a_run_cut_short_stops_at_its_end() {
    // A Replace take records over the first clip from 3: no seam.
    let clips = [
        clip(1, 0, 4, vec![note(2, 8, 60)]),
        clip(2, 4, 4, vec![note(-2, 8, 60)]),
    ];
    let members = [
        Member {
            clip: &clips[0],
            length: q(3),
        },
        Member {
            clip: &clips[1],
            length: q(4),
        },
    ];
    let found = runs(members);
    assert_eq!(found.len(), 2);
    assert_eq!(played(&found[0]), vec![(2, 1, 60)]);
}

#[test]
fn bounded_replace_suppresses_only_inside_the_punch_window() {
    let clip = clip(
        1,
        0,
        12,
        vec![
            note(1, 1, 60),  // before punch-in
            note(3, 5, 61),  // cut at punch-in
            note(4, 1, 62),  // inside
            note(8, 1, 63),  // at punch-out
            note(11, 1, 64), // after punch-out
        ],
    );
    let suppression = Suppression {
        from: q(4),
        until: q(8),
    };
    let actual = run_notes_masked(&whole(std::slice::from_ref(&clip)), Some(suppression));
    let mut played: Vec<_> = actual
        .iter()
        .map(|n| (n.start.0 / q(1).0, n.length.0 / q(1).0, n.pitch))
        .collect();
    played.sort_unstable();
    assert_eq!(
        played,
        vec![(1, 1, 60), (3, 1, 61), (8, 1, 63), (11, 1, 64)]
    );
}

#[test]
fn bounded_replace_splits_looping_clip_without_expanding_repeats() {
    let mut looping = clip(21, 0, 12, vec![note(0, 1, 60), note(1, 1, 62)]);
    Arc::make_mut(&mut looping).loop_length = Some(q(2));
    let suppression = Suppression {
        from: q(4),
        until: q(8),
    };
    let mut cache = Cache::default();
    let engine = cache
        .clip_with_suppression(&whole(std::slice::from_ref(&looping)), Some(suppression))
        .to_vec();
    assert_eq!(
        engine.len(),
        2,
        "aligned bounds need only prefix and suffix"
    );
    assert_eq!(engine[0].period, Some(q(2)));
    assert_eq!((engine[0].start, engine[0].length), (q(0), q(4)));
    assert_eq!(engine[1].period, Some(q(2)));
    assert_eq!((engine[1].start, engine[1].length), (q(8), q(4)));
    assert!(Arc::ptr_eq(&engine[0].notes, &engine[1].notes));
}

#[test]
fn a_huge_looping_clip_stays_constant_size_during_a_narrow_replace() {
    let huge = Ticks(1 << 52);
    let clip = Arc::new(Clip {
        id: Id(22),
        name: String::new(),
        start: Ticks::ZERO,
        length: huge,
        muted: false,
        color: None,
        locked: false,
        crossfade: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: Some(Ticks(1)),
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        content: ClipContent::Midi {
            notes: vec![Note::new(Ticks::ZERO, Ticks(1), 60, 100)],
            events: Vec::new(),
        },
    });
    let mask_start = Ticks(1 << 51);
    let mut cache = Cache::default();
    let engine = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&clip)),
            Some(Suppression {
                from: mask_start,
                until: mask_start + Ticks(1),
            }),
        )
        .to_vec();
    assert_eq!(engine.len(), 2);
    assert_eq!(engine[0].length, mask_start);
    assert_eq!(engine[1].start, mask_start + Ticks(1));
    assert_eq!(engine[1].length, huge - mask_start - Ticks(1));
    assert!(engine.iter().all(|part| part.notes.len() == 1));
    assert!(Arc::ptr_eq(&engine[0].notes, &engine[1].notes));
}

#[test]
fn nonaligned_loop_bounds_rotate_the_first_suffix_cycle_and_controls() {
    let mut looping = clip(23, 0, 12, vec![note(0, 1, 60), note(1, 1, 62)]);
    Arc::make_mut(&mut looping).loop_length = Some(q(2));
    Arc::make_mut(&mut looping).content = ClipContent::Midi {
        notes: vec![note(0, 1, 60), note(1, 1, 62)],
        events: vec![
            model::ClipEvent {
                at: q(0),
                kind: EventKind::Controller {
                    number: 1,
                    value: 10,
                },
            },
            model::ClipEvent {
                at: q(1),
                kind: EventKind::Controller {
                    number: 1,
                    value: 20,
                },
            },
        ],
    };
    let mut cache = Cache::default();
    let engine = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&looping)),
            Some(Suppression {
                from: q(3),
                until: q(7),
            }),
        )
        .to_vec();
    assert_eq!(engine.len(), 3);
    assert_eq!((engine[0].start, engine[0].length), (q(0), q(3)));
    assert_eq!((engine[1].start, engine[1].length), (q(7), q(1)));
    assert_eq!((engine[2].start, engine[2].length), (q(8), q(4)));
    assert_eq!(engine[1].period, None);
    assert_eq!(engine[2].period, Some(q(2)));
    assert_eq!(engine[1].notes.len(), 1);
    assert_eq!(engine[1].notes[0].pitch, 62);
    assert_eq!(engine[1].controls.len(), 1);
    assert_eq!(engine[1].controls[0].at, Ticks::ZERO);
    assert_eq!(
        engine[1].controls[0].msg.kind(),
        Kind::Controller {
            number: 1,
            value: 20
        }
    );
}

#[test]
fn full_loop_mask_has_no_output_and_unchanged_windows_reuse_cache() {
    let mut looping = clip(24, 0, 8, vec![note(0, 1, 60)]);
    Arc::make_mut(&mut looping).loop_length = Some(q(2));
    let mut cache = Cache::default();
    let full = cache.clip_with_suppression(
        &whole(std::slice::from_ref(&looping)),
        Some(Suppression {
            from: q(0),
            until: q(8),
        }),
    );
    assert!(full.is_empty());
    let edge = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&looping)),
            Some(Suppression {
                from: q(2),
                until: q(4),
            }),
        )
        .to_vec();
    let same = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&looping)),
            Some(Suppression {
                from: q(2),
                until: q(4),
            }),
        )
        .to_vec();
    assert_eq!(edge.len(), 2);
    assert!(Arc::ptr_eq(&edge[0].notes, &same[0].notes));
    assert!(Arc::ptr_eq(&edge[1].notes, &same[1].notes));
}

#[test]
fn moving_loop_window_reuses_unchanged_periodic_pattern() {
    let mut looping = clip(25, 0, 12, vec![note(0, 1, 60)]);
    Arc::make_mut(&mut looping).loop_length = Some(q(2));
    let mut cache = Cache::default();
    let first = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&looping)),
            Some(Suppression {
                from: q(3),
                until: q(7),
            }),
        )
        .to_vec();
    let next = cache
        .clip_with_suppression(
            &whole(std::slice::from_ref(&looping)),
            Some(Suppression {
                from: q(5),
                until: q(9),
            }),
        )
        .to_vec();
    assert_eq!(first.len(), 3);
    assert_eq!(next.len(), 3);
    assert!(Arc::ptr_eq(&first[0].notes, &next[0].notes));
    assert!(Arc::ptr_eq(&first[2].notes, &next[2].notes));
    assert_ne!(first[1].start, next[1].start);
}

#[test]
fn carried_note_does_not_resume_after_a_bounded_replace_window() {
    let clips = [
        clip(1, 0, 4, vec![note(2, 8, 60)]),
        clip(2, 4, 8, vec![note(-2, 8, 60)]),
    ];
    let suppression = Suppression {
        from: q(3),
        until: q(6),
    };
    let actual = run_notes_masked(&whole(&clips), Some(suppression));
    assert_eq!(actual.len(), 1);
    assert_eq!((actual[0].start, actual[0].length), (q(2), q(1)));
}

#[test]
fn a_run_is_known_by_its_first_clip_while_the_seams_after_it_move() {
    // An Overdub begun at 4 inside a clip, committed twice: the notes keep
    // their places and lengths in the run, so the engine keeps them.
    let commit = |end: i64| {
        [
            clip(1, 0, 4, vec![note(2, 8, 60)]),
            clip(50, 4, end - 4, vec![note(-2, 8, 60), note(3, 4, 64)]),
            clip(
                900,
                end,
                16 - end,
                vec![note(2 - end, 8, 60), note(7 - end, 4, 64)],
            ),
        ]
    };
    let (a, b) = (commit(6), commit(9));
    let mut cache = Cache::default();
    let first = cache
        .clip_with_suppression(&runs(whole(&a))[0], None)
        .to_vec();
    let second = cache
        .clip_with_suppression(&runs(whole(&b))[0], None)
        .to_vec();
    assert_eq!(
        (first[0].id, first[0].start),
        (second[0].id, second[0].start)
    );
    assert_eq!(first[0].notes, second[0].notes);
    assert_eq!(played(&runs(whole(&b))[0]), vec![(2, 8, 60), (7, 4, 64)]);
}

#[test]
fn sounding_keys_follow_a_note_across_a_seam() {
    let track = Track {
        clips: vec![
            clip(1, 0, 4, vec![note(2, 8, 60)]),
            clip(2, 4, 8, vec![note(-2, 8, 60)]),
        ],
        ..Track::new(Id(9), "t", TrackKind::Midi, Color(0))
    };
    assert_eq!(sounding_keys(&track, q(3)), vec![60]);
    assert_eq!(sounding_keys(&track, q(9)), vec![60]);
    assert!(sounding_keys(&track, q(10)).is_empty());
}
