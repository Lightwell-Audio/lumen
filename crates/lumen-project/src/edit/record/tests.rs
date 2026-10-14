use super::*;
use crate::edit::shown_notes;
use crate::model::Color;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note::new(q(start), q(length), pitch, 100)
}

fn tn(start: i64, length: i64, pitch: u8) -> TakeNote {
    TakeNote {
        start: q(start),
        length: q(length),
        pitch,
        velocity: 100,
    }
}

fn ev(at: i64, value: u8) -> TakeEvent {
    TakeEvent {
        at: q(at),
        kind: EventKind::Pressure { value },
    }
}

fn ev_at(at: i64, value: u8) -> ClipEvent {
    ClipEvent {
        at: q(at),
        kind: EventKind::Pressure { value },
    }
}

fn take(start: i64, end: i64, notes: Vec<TakeNote>) -> Take {
    Take {
        start: q(start),
        end: q(end),
        notes,
        events: Vec::new(),
    }
}

fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Clip {
    Clip {
        id: Id(id),
        name: format!("c{id}"),
        start: q(start),
        length: q(length),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        content: midi(notes),
    }
}

fn midi(notes: Vec<Note>) -> ClipContent {
    ClipContent::Midi {
        notes,
        events: Vec::new(),
    }
}

/// Track 1 (MIDI, `clips`) and track 2 (MIDI, one clip of its own, so a
/// test can see that it is left alone). The master is replaced so its id
/// cannot collide with the test ids.
fn base(clips: Vec<Clip>) -> Project {
    let mut p = crate::testing::small_project(1);
    p.markers.clear();
    p.media.clear();
    p.master = Arc::new(Track::master(Id(999_999)));
    p.tracks = vec![
        Arc::new(Track {
            clips: clips.into_iter().map(Arc::new).collect(),
            ..Track::new(Id(1), "Keys", TrackKind::Midi, Color(0))
        }),
        Arc::new(Track {
            clips: vec![Arc::new(clip(20, 0, 4, vec![note(0, 1, 36)]))],
            ..Track::new(Id(2), "Bass", TrackKind::Midi, Color(0))
        }),
    ];
    p
}

/// Fresh ids counting from 900, as one call's `fresh` would.
fn ids() -> impl FnMut() -> Id {
    let mut next = 900;
    move || {
        let id = Id(next);
        next += 1;
        id
    }
}

#[test]
fn a_note_played_again_wins_its_pitch() {
    // An old 60 from 0 to 4; the take plays 60 again at 2: the old one
    // is cut at 2, not stacked under the new one.
    assert_eq!(
        settle(vec![note(0, 4, 60)], vec![note(2, 1, 60)]),
        vec![note(0, 2, 60), note(2, 1, 60)]
    );
    // An old note that starts under the new one goes; one that starts
    // after it ends stays; other pitches are left alone.
    assert_eq!(
        settle(
            vec![note(3, 1, 60), note(6, 1, 60), note(3, 2, 64)],
            vec![note(2, 3, 60)],
        ),
        vec![note(2, 3, 60), note(3, 2, 64), note(6, 1, 60)]
    );
    // Played on the same start: the new note replaces the old.
    assert_eq!(
        settle(vec![note(1, 2, 60)], vec![Note::new(q(1), q(1), 60, 30)]),
        vec![Note::new(q(1), q(1), 60, 30)]
    );
}

#[test]
fn new_notes_settle_among_themselves_the_later_winning() {
    // A loop pass over a note of the pass before.
    assert_eq!(
        settle(Vec::new(), vec![note(3, 1, 60), note(1, 4, 60)]),
        vec![note(1, 2, 60), note(3, 1, 60)]
    );
}

#[test]
fn replace_settles_active_loop_conflicts_but_keeps_hidden_notes_untouched() {
    let base = base(vec![]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(0, 3, vec![tn(0, 3, 60), tn(1, 4, 60), tn(4, 2, 60)]),
        Mode::Replace,
    )
    .unwrap();
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![note(0, 1, 60), note(1, 4, 60), note(4, 2, 60)]),
        "the later active loop note wins while the post-window note is untouched"
    );
}

#[test]
fn an_overdub_over_the_same_pitch_cuts_the_old_note() {
    let base_project = base(vec![clip(10, 0, 8, vec![note(0, 4, 60), note(0, 4, 67)])]);
    let mut p = base_project.clone();
    put(
        &mut p,
        &base_project,
        &take(0, 8, vec![tn(2, 1, 60)]),
        Mode::Overdub,
    )
    .expect("placed");
    let ClipContent::Midi { notes, .. } = &p.tracks[0].clips[0].content else {
        panic!("a MIDI clip");
    };
    assert_eq!(notes, &vec![note(0, 2, 60), note(0, 4, 67), note(2, 1, 60)]);
}

/// Places `take` on track 1 as a Replace or an Overdub with the take clip
/// id 50.
fn put(p: &mut Project, base: &Project, take: &Take, mode: Mode) -> Result<Vec<Id>, Blocked> {
    let placement = Placement {
        mode,
        punch: None,
        take_clip: Id(50),
    };
    place(p, base, Id(1), take, placement, &mut ids())
}

/// `(id, start, length)` of track 1's clips.
fn spans(p: &Project) -> Vec<(Id, Ticks, Ticks)> {
    p.tracks[0]
        .clips
        .iter()
        .map(|c| (c.id, c.start, c.length))
        .collect()
}

fn clip_of(p: &Project, id: u64) -> &Clip {
    p.tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == Id(id))
        .map(|c| c.as_ref())
        .expect("the clip is in the project")
}

#[test]
fn replace_puts_one_clip_over_the_range_with_its_notes_relative_to_it() {
    let base = base(vec![]);
    let mut p = base.clone();
    let mut t = take(
        4,
        8,
        vec![tn(5, 1, 60), tn(4, 2, 62), tn(4, 1, 50), tn(9, 1, 70)],
    );
    t.events = vec![ev(6, 10), ev(4, 20), ev(9, 30)];
    assert_eq!(put(&mut p, &base, &t, Mode::Replace), Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(50), q(4), q(4))]);
    assert_eq!(clip_of(&p, 50).name, "MIDI clip");
    assert_eq!(
        clip_of(&p, 50).content,
        ClipContent::Midi {
            notes: vec![
                note(0, 1, 50),
                note(0, 2, 62),
                note(1, 1, 60),
                note(5, 1, 70)
            ],
            events: vec![ev_at(0, 20), ev_at(2, 10), ev_at(5, 30)],
        }
    );
    assert_eq!(p.tracks[1], base.tracks[1], "the other track is untouched");
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn replace_cuts_the_clip_it_covers_and_keeps_both_outside_parts() {
    let base = base(vec![clip(10, 0, 8, vec![note(1, 1, 60), note(5, 1, 64)])]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(2, 4, vec![tn(3, 1, 62)]),
        Mode::Replace,
    )
    .unwrap();
    assert_eq!(
        spans(&p),
        [
            (Id(10), q(0), q(2)),
            (Id(50), q(2), q(2)),
            (Id(900), q(4), q(4)),
        ]
    );
    // The pieces of the old clip are windows onto all its notes (format
    // v15): each shows its own, and keeps the other's hidden.
    assert_eq!(
        clip_of(&p, 10).content,
        midi(vec![note(1, 1, 60), note(5, 1, 64)])
    );
    assert_eq!(shown_notes(clip_of(&p, 10)), vec![0]);
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 62)]));
    assert_eq!(
        clip_of(&p, 900).content,
        midi(vec![note(-3, 1, 60), note(1, 1, 64)])
    );
    assert_eq!(shown_notes(clip_of(&p, 900)), vec![1]);
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_begun_at_a_clips_start_is_a_new_clip_and_the_rest_stays_its_own() {
    // The owner's case: begun exactly at a clip's start, the take is a
    // new clip carrying what the old one played there, and the rest of
    // the old clip stays a clip of its own after it.
    let base = base(vec![clip(10, 0, 8, vec![note(0, 1, 48), note(5, 1, 50)])]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(0, 4, vec![tn(3, 1, 64)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(50), q(0), q(4)), (Id(10), q(4), q(4))]);
    let new = clip_of(&p, 50);
    assert_eq!(new.content, midi(vec![note(0, 1, 48), note(3, 1, 64)]));
    assert_eq!(new.name, "c10", "named after the clip it began in");
    assert_eq!(shown_notes(clip_of(&p, 10)), vec![1]);
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_begun_inside_a_clip_is_a_new_clip_from_there_to_the_stop() {
    let old = Clip {
        color: Some(Color(3)),
        ..clip(
            10,
            0,
            8,
            vec![note(0, 1, 48), note(2, 1, 50), note(5, 1, 52)],
        )
    };
    let base = base(vec![old]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(2, 4, vec![tn(3, 1, 64)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(
        spans(&p),
        [
            (Id(10), q(0), q(2)),
            (Id(50), q(2), q(2)),
            (Id(900), q(4), q(4)),
        ]
    );
    // The new clip carries what the old one played there, and the take.
    let new = clip_of(&p, 50);
    assert_eq!(new.content, midi(vec![note(0, 1, 50), note(1, 1, 64)]));
    assert_eq!((new.name.as_str(), new.color), ("c10", Some(Color(3))));
    // What it covers is hidden in the pieces, not lost.
    assert_eq!(shown_notes(clip_of(&p, 10)), vec![0]);
    assert_eq!(shown_notes(clip_of(&p, 900)), vec![2]);
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_keeps_a_note_held_past_the_stop() {
    // The visible clip ends with the take, but trimming it later can
    // recover the recorded note's full length.
    let base = base(vec![clip(10, 0, 16, vec![])]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(4, 8, vec![tn(7, 3, 60)]),
        Mode::Overdub,
    )
    .unwrap();
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(3, 3, 60)]));
    assert_eq!(spans(&p)[1], (Id(50), q(4), q(4)));
}

#[test]
fn overdub_begun_over_a_sounding_note_carries_it_on_whole() {
    // A note from 2 to 10 sounds when the take begins at 4 (the owner's
    // boundary case): the new clip keeps it whole, starting before it,
    // so it carries on through both seams instead of stopping at 4.
    let base = base(vec![clip(10, 0, 12, vec![note(2, 8, 60)])]);
    let mut p = base.clone();
    put(&mut p, &base, &take(4, 6, vec![]), Mode::Overdub).unwrap();
    assert_eq!(
        spans(&p),
        [
            (Id(10), q(0), q(4)),
            (Id(50), q(4), q(2)),
            (Id(900), q(6), q(6)),
        ]
    );
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(-2, 8, 60)]));
    assert_eq!(
        crate::edit::seams::carried(&p.tracks[0]),
        vec![vec![], vec![0], vec![0]],
        "the note sounds on through the new clip and the rest"
    );
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_begun_inside_keeps_carried_notes_the_same_as_the_take_grows() {
    // A note that runs past the take's end is not cut at it, so each
    // commit leaves it as it was and the engine does not strike it again.
    let base = base(vec![clip(10, 0, 16, vec![note(2, 8, 60), note(5, 6, 62)])]);
    let carried = |end: i64| {
        let mut p = base.clone();
        put(&mut p, &base, &take(4, end, vec![]), Mode::Overdub).unwrap();
        clip_of(&p, 50).content.clone()
    };
    let first = carried(6);
    assert_eq!(first, midi(vec![note(-2, 8, 60), note(1, 6, 62)]));
    assert_eq!(carried(9), first);
    assert_eq!(carried(12), first);
}

#[test]
fn a_note_played_over_a_carried_one_ends_it_there() {
    let base = base(vec![clip(10, 0, 12, vec![note(2, 8, 60)])]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(4, 6, vec![tn(5, 1, 60)]),
        Mode::Overdub,
    )
    .unwrap();
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![note(-2, 3, 60), note(1, 1, 60)]),
        "the played note wins its pitch from 5"
    );
    // Cut short, the old note no longer reaches the rest of the clip.
    assert_eq!(
        crate::edit::seams::carried(&p.tracks[0]),
        vec![vec![], vec![0], vec![]]
    );
}

#[test]
fn an_empty_overdub_over_empty_lane_shows_its_clip_at_once() {
    // Nothing played yet: the clip is there from the record start.
    let base = base(vec![clip(10, 0, 2, vec![])]);
    for end in [5, 8] {
        let mut p = base.clone();
        let placed = put(&mut p, &base, &take(4, end, vec![]), Mode::Overdub);
        assert_eq!(placed, Ok(vec![Id(50)]));
        assert_eq!(spans(&p)[1], (Id(50), q(4), q(end - 4)));
        assert_eq!(clip_of(&p, 50).content, midi(vec![]));
    }
}

#[test]
fn overdub_inside_a_new_clip_skips_what_a_muted_clip_holds() {
    let muted = Clip {
        muted: true,
        ..clip(10, 0, 8, vec![note(3, 1, 48)])
    };
    let base = base(vec![muted]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(2, 4, vec![tn(2, 1, 64)]),
        Mode::Overdub,
    )
    .unwrap();
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(0, 1, 64)]));
}

#[test]
fn overdub_begun_before_a_clip_and_reaching_it_is_a_new_clip_beside_it() {
    // The owner's case: begun before a clip and running into it, the
    // take is a new clip from the record start, carrying the old notes
    // it reaches; the old clip keeps the rest as a clip of its own and
    // does not move or grow.
    let base = base(vec![clip(10, 4, 4, vec![note(0, 1, 48), note(3, 1, 50)])]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(2, 6, vec![tn(2, 1, 60), tn(5, 1, 64)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(50), q(2), q(4)), (Id(10), q(6), q(2))]);
    let new = clip_of(&p, 50);
    assert_eq!(
        new.content,
        midi(vec![note(0, 1, 60), note(2, 1, 48), note(3, 1, 64)]),
        "the old note still sounds at 4"
    );
    assert_eq!(new.name, "MIDI clip");
    assert_eq!(shown_notes(clip_of(&p, 10)), vec![1]);
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_stopped_before_a_clip_leaves_it_alone() {
    let base = base(vec![clip(10, 4, 4, vec![note(0, 1, 48)])]);
    for end in [3, 4] {
        let mut p = base.clone();
        let t = take(1, end, vec![tn(2, 1, 60)]);
        assert_eq!(put(&mut p, &base, &t, Mode::Overdub), Ok(vec![Id(50)]));
        assert_eq!(
            spans(&p),
            [(Id(50), q(1), q(end - 1)), (Id(10), q(4), q(4))]
        );
        assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
    }
}

#[test]
fn overdub_begun_at_a_clips_end_is_a_new_clip_after_it() {
    let base = base(vec![clip(10, 0, 2, vec![note(0, 1, 48)])]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(2, 4, vec![tn(3, 1, 60)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(10), q(0), q(2)), (Id(50), q(2), q(2))]);
    assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 60)]));
}

#[test]
fn overdub_over_whole_clips_carries_what_they_played_into_one_clip() {
    // Clips the take covers end to end are recorded over, as in Replace,
    // but what they played is in the new clip.
    let base = base(vec![
        clip(10, 0, 2, vec![note(1, 1, 48)]),
        clip(11, 4, 2, vec![note(0, 1, 50)]),
    ]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(0, 8, vec![tn(3, 1, 60), tn(7, 1, 62)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(50), q(0), q(8))]);
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![
            note(1, 1, 48),
            note(3, 1, 60),
            note(4, 1, 50),
            note(7, 1, 62)
        ])
    );
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_over_empty_lane_keeps_captured_content_outside_its_window() {
    // The take begins after the clip has ended: nothing borders it.
    let base = base(vec![clip(10, 0, 2, vec![])]);
    let mut p = base.clone();
    let mut t = take(3, 6, vec![tn(3, 1, 60), tn(7, 1, 62)]);
    t.events = vec![ev(4, 7)];
    let placed = put(&mut p, &base, &t, Mode::Overdub);
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(10), q(0), q(2)), (Id(50), q(3), q(3))]);
    assert_eq!(
        clip_of(&p, 50).content,
        ClipContent::Midi {
            notes: vec![note(0, 1, 60), note(4, 1, 62)],
            events: vec![ev_at(1, 7)],
        }
    );
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn overdub_before_a_looping_clip_leaves_its_loop_alone() {
    let looped = Clip {
        loop_length: Some(q(2)),
        ..clip(10, 4, 4, vec![note(0, 1, 48)])
    };
    let base = base(vec![looped]);
    let mut p = base.clone();
    put(
        &mut p,
        &base,
        &take(2, 4, vec![tn(2, 1, 60)]),
        Mode::Overdub,
    )
    .unwrap();
    assert_eq!(spans(&p), [(Id(50), q(2), q(2)), (Id(10), q(4), q(4))]);
    assert_eq!(clip_of(&p, 10), &*base.tracks[0].clips[0]);
}

#[test]
fn overdub_in_a_looping_clip_is_a_new_clip_carrying_its_repeats() {
    let looped = Clip {
        loop_length: Some(q(2)),
        ..clip(10, 0, 8, vec![note(1, 1, 48)])
    };
    let base = base(vec![looped]);
    let mut p = base.clone();
    let placed = put(
        &mut p,
        &base,
        &take(4, 6, vec![tn(4, 3, 60), tn(5, 1, 67)]),
        Mode::Overdub,
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    let at_50 = spans(&p).iter().position(|s| s.0 == Id(50)).unwrap();
    assert_eq!(spans(&p)[at_50], (Id(50), q(4), q(2)));
    assert_eq!(spans(&p).len(), 3, "the loop before and after it stays");
    // The repeat at 4 played its note at 5; the captured long note keeps
    // its full length beyond the visible take window.
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![note(0, 3, 60), note(1, 1, 48), note(1, 1, 67)])
    );
    assert_eq!(crate::validate(&p), Ok(()));
}

#[test]
fn punch_limits_the_clip_window_and_keeps_the_full_capture_hidden() {
    let base = base(vec![]);
    let mut p = base.clone();
    let t = take(0, 8, vec![tn(1, 1, 60), tn(2, 4, 62), tn(5, 1, 64)]);
    let placed = place(
        &mut p,
        &base,
        Id(1),
        &t,
        Placement {
            mode: Mode::Replace,
            punch: Some((q(2), q(4))),
            take_clip: Id(50),
        },
        &mut ids(),
    );
    assert_eq!(placed, Ok(vec![Id(50)]));
    assert_eq!(spans(&p), [(Id(50), q(2), q(2))]);
    // Content before and after the punch remains hidden in the clip.
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![note(-1, 1, 60), note(0, 4, 62), note(3, 1, 64)])
    );

    let mut away = base.clone();
    let none = place(
        &mut away,
        &base,
        Id(1),
        &t,
        Placement {
            mode: Mode::Replace,
            punch: Some((q(10), q(12))),
            take_clip: Id(50),
        },
        &mut ids(),
    );
    assert_eq!(none, Ok(vec![Id(50)]));
    assert_eq!(spans(&away), [(Id(50), q(10), Ticks(1))]);
    assert_eq!(
        clip_of(&away, 50).content,
        midi(vec![note(-9, 1, 60), note(-8, 4, 62), note(-5, 1, 64)])
    );
}

#[test]
fn punch_keeps_full_capture_in_both_modes_and_trim_recovers_it() {
    let take = Take {
        start: q(0),
        end: q(8),
        notes: vec![tn(1, 4, 60), tn(2, 5, 62), tn(3, 1, 64), tn(5, 2, 65)],
        events: vec![ev(1, 11), ev(3, 33), ev(5, 55)],
    };
    for mode in [Mode::Replace, Mode::Overdub] {
        let old = if mode == Mode::Overdub {
            vec![clip(10, 0, 8, vec![note(1, 6, 60), note(2, 5, 62)])]
        } else {
            vec![]
        };
        let base = base(old);
        let mut p = base.clone();
        place(
            &mut p,
            &base,
            Id(1),
            &take,
            Placement {
                mode,
                punch: Some((q(2), q(4))),
                take_clip: Id(50),
            },
            &mut ids(),
        )
        .unwrap();

        let recorded = clip_of(&p, 50);
        assert_eq!((recorded.start, recorded.length), (q(2), q(2)));
        let ClipContent::Midi { notes, events } = &recorded.content else {
            panic!("recording creates a MIDI clip");
        };
        assert_eq!(
            events,
            &vec![ev_at(-1, 11), ev_at(1, 33), ev_at(3, 55)],
            "events before and after the visible range stay in {mode:?}"
        );
        if mode == Mode::Replace {
            assert_eq!(
                notes,
                &vec![
                    note(-1, 4, 60),
                    note(0, 5, 62),
                    note(1, 1, 64),
                    note(3, 2, 65)
                ]
            );
            assert_eq!(shown_notes(recorded), vec![1, 2]);
        } else {
            // The pre-window same-pitch note remains hidden and does not
            // shorten the old note carried into the visible window.
            assert_eq!(
                notes,
                &vec![
                    note(-1, 6, 60),
                    note(-1, 4, 60),
                    note(0, 5, 62),
                    note(1, 1, 64),
                    note(3, 2, 65),
                ]
            );
            assert_eq!(shown_notes(recorded), vec![2, 3]);
        }
        assert_eq!(crate::validate(&p), Ok(()));
        let bytes = crate::to_bytes(&p).expect("hidden captured content serializes");
        let loaded = crate::from_bytes(&bytes).expect("hidden captured content loads");
        assert_eq!(loaded, p, "{mode:?} round trip retains hidden content");

        // Widen both clip edges using the normal trim operation. The
        // content and its absolute timing are restored without rewriting
        // the captured notes or events.
        let tempo = lumen_core::time::TempoMap::new(p.tempo.clone()).unwrap();
        let end = crate::edit::trim(&p, &tempo, recorded, crate::edit::Edge::End, q(6), false);
        let widened = crate::edit::trim(&p, &tempo, &end, crate::edit::Edge::Start, q(1), false);
        let ClipContent::Midi {
            notes: recovered,
            events: recovered_events,
        } = widened.content
        else {
            panic!("trim preserves MIDI content");
        };
        if mode == Mode::Replace {
            assert_eq!(
                recovered,
                vec![
                    note(0, 4, 60),
                    note(1, 5, 62),
                    note(2, 1, 64),
                    note(4, 2, 65)
                ]
            );
        } else {
            assert_eq!(recovered[0], note(0, 6, 60));
            assert_eq!(recovered[1], note(0, 4, 60));
            assert_eq!(recovered[4], note(4, 2, 65));
        }
        assert_eq!(
            recovered_events,
            vec![ev_at(0, 11), ev_at(2, 33), ev_at(4, 55)]
        );
    }
}

#[test]
fn capture_wholly_outside_punch_gets_a_hidden_anchor_clip() {
    for (take, punch, anchor) in [
        (
            Take {
                start: q(0),
                end: q(2),
                notes: vec![tn(1, 3, 60)],
                events: vec![ev(1, 11)],
            },
            (q(4), q(8)),
            q(4),
        ),
        (
            Take {
                start: q(9),
                end: q(11),
                notes: vec![tn(9, 3, 62)],
                events: vec![ev(10, 22)],
            },
            (q(4), q(8)),
            q(8) - Ticks(1),
        ),
    ] {
        for mode in [Mode::Replace, Mode::Overdub] {
            let base = base(vec![]);
            let mut p = base.clone();
            place(
                &mut p,
                &base,
                Id(1),
                &take,
                Placement {
                    mode,
                    punch: Some(punch),
                    take_clip: Id(50),
                },
                &mut ids(),
            )
            .unwrap();
            let recorded = clip_of(&p, 50);
            assert_eq!((recorded.start, recorded.length), (anchor, Ticks(1)));
            assert!(shown_notes(recorded).is_empty(), "{mode:?}");
            assert_eq!(
                recorded.content,
                ClipContent::Midi {
                    notes: vec![Note::new(
                        take.notes[0].start - anchor,
                        take.notes[0].length,
                        take.notes[0].pitch,
                        100
                    )],
                    events: vec![ClipEvent {
                        at: take.events[0].at - anchor,
                        kind: take.events[0].kind,
                    }],
                },
                "the whole capture is hidden at the anchor in {mode:?}"
            );
            assert_eq!(crate::validate(&p), Ok(()));
        }
    }

    let empty = Take {
        start: q(0),
        end: q(2),
        notes: vec![],
        events: vec![],
    };
    assert_eq!(range_of(&empty, Some((q(4), q(8)))), None);

    // Before the loop head reaches its start, keep the capture hidden at
    // that start until a later commit has a visible intersection.
    let loop_capture = Take {
        start: q(4),
        end: q(2),
        notes: vec![tn(2, 2, 64)],
        events: vec![ev(2, 44)],
    };
    assert_eq!(range_of(&loop_capture, None), Some((q(4), q(4) + Ticks(1))));
}

#[test]
fn lengths_are_at_least_the_shortest_note_and_keep_the_captured_length() {
    let base = base(vec![]);
    let mut p = base.clone();
    let short = TakeNote {
        start: q(1),
        length: Ticks(1),
        pitch: 60,
        velocity: 100,
    };
    let at_end = TakeNote {
        start: q(4) - Ticks(1),
        length: q(1),
        pitch: 61,
        velocity: 100,
    };
    put(
        &mut p,
        &base,
        &take(0, 4, vec![short, at_end]),
        Mode::Replace,
    )
    .unwrap();
    assert_eq!(
        clip_of(&p, 50).content,
        midi(vec![
            Note::new(q(1), MIN_NOTE, 60, 100),
            Note::new(q(4) - Ticks(1), q(1), 61, 100),
        ])
    );
}

#[test]
fn calls_with_a_growing_take_match_one_call_with_the_final_take() {
    // Begun inside a clip, and (Overdub) before one.
    let cases = [(Mode::Replace, 0), (Mode::Overdub, 0), (Mode::Overdub, 4)];
    for (mode, at) in cases {
        let base = base(vec![clip(10, at, 8, vec![note(0, 1, 48)])]);
        let takes = [
            take(2, 3, vec![tn(2, 1, 60)]),
            take(2, 5, vec![tn(2, 1, 60), tn(3, 2, 62)]),
            take(2, 6, vec![tn(2, 1, 60), tn(3, 2, 62), tn(5, 1, 64)]),
        ];
        let mut p = base.clone();
        for t in &takes {
            put(&mut p, &base, t, mode).unwrap();
        }
        let mut once = base.clone();
        put(&mut once, &base, &takes[2], mode).unwrap();
        assert_eq!(p, once, "{mode:?} over a clip at {at}");
    }
}

#[test]
fn a_locked_clip_in_the_range_blocks_the_take_and_the_track_is_as_base() {
    let locked = Clip {
        locked: true,
        ..clip(10, 0, 2, vec![note(0, 1, 48)])
    };
    let base = base(vec![locked, clip(11, 4, 2, vec![])]);
    for mode in [Mode::Replace, Mode::Overdub] {
        let mut p = base.clone();
        assert_eq!(
            put(&mut p, &base, &take(1, 3, vec![tn(1, 1, 60)]), mode),
            Err(Blocked)
        );
        assert_eq!(p, base, "{mode:?}");
    }
    // A take that placed something first is put back to base when blocked.
    let mut p = base.clone();
    assert_eq!(
        put(
            &mut p,
            &base,
            &take(2, 4, vec![tn(2, 1, 60)]),
            Mode::Replace
        ),
        Ok(vec![Id(50)])
    );
    assert_eq!(
        put(
            &mut p,
            &base,
            &take(1, 3, vec![tn(1, 1, 60)]),
            Mode::Replace
        ),
        Err(Blocked)
    );
    assert_eq!(p.tracks[0], base.tracks[0]);
}

#[test]
fn empty_takes_and_ranges_change_only_what_they_must() {
    let base = base(vec![clip(10, 0, 8, vec![note(1, 1, 48)])]);
    // An empty take from the clip's start is a new clip there, holding
    // what the old one played.
    let mut p = base.clone();
    assert_eq!(
        put(&mut p, &base, &take(0, 4, vec![]), Mode::Overdub),
        Ok(vec![Id(50)])
    );
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(1, 1, 48)]));
    // Begun inside the clip too.
    let mut p = base.clone();
    assert_eq!(
        put(&mut p, &base, &take(1, 4, vec![]), Mode::Overdub),
        Ok(vec![Id(50)])
    );
    assert_eq!(clip_of(&p, 50).content, midi(vec![note(0, 1, 48)]));
    // An empty take still replaces what is under a non-empty range.
    let mut p = base.clone();
    assert_eq!(
        put(&mut p, &base, &take(2, 4, vec![]), Mode::Replace),
        Ok(vec![Id(50)])
    );
    assert_eq!(clip_of(&p, 50).content, midi(vec![]));
    assert_eq!(clip_of(&p, 50).length, q(2));

    // An empty range puts the track back as base has it.
    let mut dirty = base.clone();
    dirty.tracks[0] = Arc::new(Track::new(Id(1), "x", TrackKind::Midi, Color(0)));
    let t = take(3, 3, vec![]);
    assert_eq!(put(&mut dirty, &base, &t, Mode::Replace), Ok(vec![]));
    assert_eq!(dirty.tracks[0], base.tracks[0]);

    // An audio track takes nothing and is put back as base has it.
    let mut audio_base = base.clone();
    Arc::make_mut(&mut audio_base.tracks[0]).kind = TrackKind::Audio;
    let mut p = audio_base.clone();
    p.tracks[0] = Arc::new(Track::new(Id(1), "x", TrackKind::Audio, Color(0)));
    let t = take(0, 4, vec![tn(0, 1, 60)]);
    assert_eq!(put(&mut p, &audio_base, &t, Mode::Replace), Ok(vec![]));
    assert_eq!(p.tracks[0], audio_base.tracks[0]);

    // A track base does not have changes nothing.
    let mut p = base.clone();
    let placed = place(
        &mut p,
        &base,
        Id(77),
        &t,
        Placement {
            mode: Mode::Replace,
            punch: None,
            take_clip: Id(50),
        },
        &mut ids(),
    );
    assert_eq!(placed, Ok(vec![]));
    assert_eq!(p, base);
}
