//! Warp in the model (format v33, M9.5 slice S4a; the pitch envelope is v34, S6).
//!
//! Two kinds of test. The format ones: a golden file that loads forever, older
//! projects that open unwarped, a project without warp that writes none, and
//! the checks that refuse a warp no clip may hold. The property ones: every
//! audio-clip edit run on random warped clips, checking that the second of the
//! file heard at each tick of the song is what the edit promises, that the
//! project stays valid, and that split followed by Join returns the original.
//! Nothing here listens: until S4b, warp is stored and kept true, not played.

use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::fade::Fade;
use lumen_core::time::{PPQ, Ticks};
use lumen_core::warp::{self, Marker};
use lumen_project::edit::clip_loop::{self, LoopEdit};
use lumen_project::edit::{self, Edge, clip_ops, reverse, split, warp as warp_edit};
use lumen_project::model::{
    Clip, ClipContent, Color, Id, Media, OtherTake, Pass, Project, Takes, Track, TrackKind, Warp,
    WarpMode,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

const RATE: u32 = 48_000;
const FILE_SECONDS: f64 = 120.0;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

mod common;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// ---------------------------------------------------------------- the format

fn audio_clip(id: u64, start: Ticks, length: Ticks, offset: i64) -> Clip {
    Clip {
        id: Id(id),
        name: format!("c{id}"),
        start,
        length,
        muted: false,
        color: None,
        locked: false,
        fade_in: Fade::default(),
        crossfade: false,
        fade_out: Fade::default(),
        loop_length: None,
        loop_phase: Ticks::ZERO,
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        takes: None,
        warp: None,
        content: ClipContent::Audio {
            media: Id(900),
            offset,
            gain_db: 0.0,
        },
    }
}

fn media() -> Media {
    Media {
        id: Id(900),
        path: "loop.wav".into(),
        original_path: None,
        frames: (FILE_SECONDS * f64::from(RATE)) as u64,
        channels: 2,
        sample_rate: RATE,
        size_bytes: 0,
    }
}

fn project_with(clip: Clip) -> Project {
    let mut track = Track::new(Id(3_300), "Loop", TrackKind::Audio, Color(0x4F81BD));
    track.clips = vec![Arc::new(clip)];
    let mut project = project_of("Warp", vec![track]);
    project.media = vec![media()];
    project
}

/// The model the v33 golden file must always load as. Never change it.
fn golden_v33_expected() -> Project {
    let mut clip = audio_clip(3_301, q(4), q(8), 96_000);
    clip.warp = Some(Warp {
        mode: WarpMode::Melodic,
        original_bpm: 92.0,
        markers: vec![
            Marker::new(2.0, Ticks::ZERO),
            Marker::new(4.5, q(4)),
            Marker::new(5.0, q(5)),
        ],
        transpose_cents: -200,
    });
    clip.audio_phase_seconds = 0.0;
    let mut project = project_with(clip);
    project.name = "Golden v33".into();
    project
}

#[test]
fn golden_v33_loads_and_round_trips() {
    let project = common::golden("v33", golden_v33_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

/// The model the v34 golden file must always load as. Never change it.
fn golden_v34_expected() -> Project {
    use lumen_core::automation::{Lane, Point, Shape};
    use lumen_core::curve::Curve;
    use lumen_project::envelope::Kind;
    let mut clip = audio_clip(3_401, q(4), q(8), 96_000);
    clip.warp = Some(Warp {
        mode: WarpMode::FullMix,
        original_bpm: 92.0,
        markers: vec![Marker::new(2.0, Ticks::ZERO), Marker::new(4.5, q(4))],
        transpose_cents: 200,
    });
    clip.audio_phase_seconds = 0.0;
    clip.envelopes.set(
        Kind::Pitch,
        Lane::new(vec![
            Point::new(q(0), 0.0),
            Point::new(q(2), 7.0).with_shape(Shape::Curve(Curve::default())),
            Point::new(q(4), -12.5).with_shape(Shape::Step),
            Point::new(q(6), 0.0),
        ])
        .unwrap(),
    );
    let mut project = project_with(clip);
    project.name = "Golden v34".into();
    project
}

#[test]
fn golden_v34_loads_and_round_trips() {
    let project = common::golden("v34", golden_v34_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn a_project_before_pitch_envelopes_opens_with_none() {
    let old = load(&fixture("v33/golden.lumen")).expect("v33 file loads");
    assert!(
        old.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .all(|c| c.envelopes.pitch.is_none())
    );
}

#[test]
fn older_projects_open_with_every_clip_unwarped() {
    let legacy = load(&fixture("v32/golden.lumen")).expect("v32 file loads");
    assert!(
        legacy
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .all(|c| c.warp.is_none())
    );
}

#[test]
fn a_clip_without_warp_writes_none() {
    let text =
        String::from_utf8(to_bytes(&project_with(audio_clip(1, q(0), q(4), 0))).unwrap()).unwrap();
    assert!(!text.contains("warp"), "{text}");
    assert!(!text.contains("transpose"), "{text}");
}

#[test]
fn a_warp_no_clip_may_hold_is_refused() {
    let good = golden_v33_expected();
    assert_eq!(validate(&good), Ok(()));
    let edit = |change: &dyn Fn(&mut Warp)| {
        let mut project = good.clone();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let clip = Arc::make_mut(&mut track.clips[0]);
        change(clip.warp.as_mut().unwrap());
        validate(&project)
    };
    assert!(
        edit(&|w| w.markers.clear()).is_err(),
        "a warp needs a marker"
    );
    assert!(
        edit(&|w| w.markers.reverse()).is_err(),
        "markers in time order"
    );
    assert!(
        edit(&|w| w.markers[1].source = 1.0).is_err(),
        "the file moves forward between markers"
    );
    assert!(edit(&|w| w.markers[0].source = f64::NAN).is_err());
    assert!(edit(&|w| w.original_bpm = 5.0).is_err());
    assert!(edit(&|w| w.original_bpm = f64::NAN).is_err());
    assert!(edit(&|w| w.transpose_cents = 10_000).is_err());
    assert!(
        edit(&|w| w.markers = (0..5_000)
            .map(|n| Marker::new(f64::from(n), Ticks(i64::from(n))))
            .collect())
        .is_err(),
        "too many markers"
    );
    // A MIDI clip is not warped.
    let mut project = good.clone();
    let track = Arc::make_mut(&mut project.tracks[0]);
    let clip = Arc::make_mut(&mut track.clips[0]);
    clip.content = ClipContent::midi();
    clip.audio_phase_seconds = 0.0;
    assert!(validate(&project).is_err());
}

#[test]
fn a_warped_take_keeps_its_own_map_through_a_swap_and_a_save() {
    let mut project = project_with(audio_clip(1, q(0), q(8), 48_000));
    let tempo = edit::tempo_of(&project);
    let clip = (*project.tracks[0].clips[0]).clone();
    let mut clip = clip;
    clip.takes = Some(Takes::new(
        0,
        2,
        vec![OtherTake {
            take: 1,
            pass: Pass::Audio {
                offset: 240_000,
                audio_phase_seconds: 0.0,
                warp: None,
            },
        }],
    ));
    let mut clip = warp_edit::enable(&project, &clip, 120.0).unwrap();
    // Trim the start: both passes' maps follow the new start.
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(clip.clone());
    clip = edit::trim(&project, &tempo, &clip, Edge::Start, q(2), false);
    for pass in [Pass::of(&clip).unwrap(), {
        let mut other = clip.clone();
        assert!(edit::takes::swap(&mut other, 1));
        Pass::of(&other).unwrap()
    }] {
        let Pass::Audio {
            warp: Some(warp), ..
        } = pass
        else {
            panic!("each pass is warped")
        };
        // 120 BPM, the file made at 120: the passes start at seconds 1 and 5;
        // the start moved two beats (one second) on in both.
        let at_new_start = warp.source_at(Ticks::ZERO);
        assert!(
            (at_new_start - 2.0).abs() < 1e-9 || (at_new_start - 6.0).abs() < 1e-9,
            "{at_new_start}"
        );
    }
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(clip);
    assert_eq!(validate(&project), Ok(()));
    let back = from_bytes(&to_bytes(&project).unwrap()).unwrap();
    assert_eq!(back, project);
}

// ----------------------------------------------------------------- the model

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A number in `lo..hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

/// A random warped clip on an otherwise empty project: from one to four
/// markers, a few ticks off the beat, the file made at 92, 120 or 140 BPM; a
/// loop on half of them. The whole window sits well inside the file.
fn random_clip(rng: &mut Rng, looping: bool) -> Project {
    random_clip_with(rng, looping, false)
}

/// [`random_clip`], with a second take whose pass sits near the start of the
/// file (often the one that limits a trim) when `takes` is set.
fn random_clip_with(rng: &mut Rng, looping: bool, takes: bool) -> Project {
    let start = Ticks(q(rng.range(1, 12)).0 + rng.range(0, PPQ));
    let length = Ticks(q(rng.range(4, 20)).0 + rng.range(0, PPQ));
    let offset = rng.range(8, 30) * i64::from(RATE);
    let mut clip = audio_clip(1, start, length, offset);
    if rng.coin() {
        clip.fade_in.length = q(1);
        clip.fade_out.length = Ticks(q(1).0 / 2);
    }
    if looping {
        clip.loop_length = Some(Ticks(q(rng.range(3, 9)).0 + rng.range(0, PPQ)));
        clip.loop_phase = Ticks(rng.range(0, 3 * PPQ));
    }
    if takes {
        let pass = Pass::Audio {
            offset: rng.range(0, 3) * i64::from(RATE),
            audio_phase_seconds: 0.0,
            warp: None,
        };
        clip.takes = Some(Takes::new(0, 2, vec![OtherTake { take: 1, pass }]));
    }
    let mut project = project_with(clip);
    let bpm = [92.0, 120.0, 140.0][rng.range(0, 3) as usize];
    let clip = (*project.tracks[0].clips[0]).clone();
    let warped = warp_edit::enable(&project, &clip, bpm).unwrap();
    let first = warped.warp.as_ref().unwrap().markers[0].source;
    let mut at = Ticks(-rng.range(0, 2 * PPQ));
    let mut source = first - 0.5 * rng.unit();
    let mut markers = Vec::new();
    for _ in 0..rng.range(1, 5) {
        markers.push(Marker::new(source, at));
        let step = rng.range(PPQ / 2, 5 * PPQ);
        at = at + Ticks(step);
        source += step as f64 * warp::seconds_per_tick(bpm) * (0.6 + 1.2 * rng.unit());
    }
    let warped = warp_edit::set_markers(&project, &warped, markers).unwrap();
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(warped);
    assert_eq!(validate(&project), Ok(()));
    project
}

fn only(project: &Project) -> Clip {
    assert_eq!(project.tracks[0].clips.len(), 1);
    (*project.tracks[0].clips[0]).clone()
}

/// The second of the file, read forwards, that `clip` plays at `abs` on the
/// song's timeline. Positions of a looping clip wrap as the engine's do.
fn heard(clip: &Clip, abs: Ticks) -> f64 {
    let warp = clip.warp.as_ref().expect("a warped clip");
    let mut at = abs - clip.start + clip.loop_phase;
    if let Some(period) = clip.loop_length {
        at = Ticks(at.0.rem_euclid(period.0));
    }
    let seconds = warp.source_at(at);
    if clip.reversed {
        FILE_SECONDS - seconds
    } else {
        seconds
    }
}

/// Ticks inside `clip`'s window, never on a turn of its loop.
fn inside(rng: &mut Rng, clip: &Clip, count: usize) -> Vec<Ticks> {
    let mut out = Vec::new();
    while out.len() < count {
        let abs = Ticks(clip.start.0 + rng.range(1, clip.length.0));
        let on_turn = clip
            .loop_length
            .is_some_and(|p| (abs - clip.start + clip.loop_phase).0.rem_euclid(p.0) == 0);
        if !on_turn {
            out.push(abs);
        }
    }
    out
}

fn same_second(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-9, "{what}: {a} against {b}");
}

/// The first place a looping clip's content repeats, or the end of the clip.
fn first_repeat(clip: &Clip) -> Ticks {
    match clip.loop_length {
        Some(p) => {
            let turn = clip.start - Ticks(clip.loop_phase.0.rem_euclid(p.0));
            (turn + p).min(clip.start + clip.length)
        }
        None => clip.start + clip.length,
    }
}

fn fresh() -> impl FnMut() -> Id {
    let mut next = 1_000;
    move || {
        next += 1;
        Id(next)
    }
}

const CASES: usize = 300;

#[test]
fn splitting_keeps_every_second_and_joining_returns_the_original() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, case % 2 == 1);
        let original = only(&project);
        let cuts: Vec<Ticks> = (0..rng.range(1, 4))
            .map(|_| Ticks(original.start.0 + rng.range(PPQ / 4, original.length.0 - PPQ / 4)))
            .collect();
        let parts = split::clips(&mut project, &[original.id], &cuts, &mut fresh());
        assert_eq!(validate(&project), Ok(()), "case {case}");
        for id in &parts {
            let part = project.tracks[0]
                .clips
                .iter()
                .find(|c| c.id == *id)
                .unwrap()
                .clone();
            assert_anchored(&project, &part);
            for abs in inside(&mut rng, &part, 8) {
                same_second(
                    heard(&part, abs),
                    heard(&original, abs),
                    &format!("case {case}, part {id:?}"),
                );
            }
        }
        let joined = clip_ops::join(&mut project, &parts);
        assert!(!joined.apart, "case {case}: the pieces meet");
        assert_eq!(only(&project), original, "case {case}: split then join");
    }
}

#[test]
fn trimming_reveals_the_same_audio_and_stays_in_the_file() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, case % 4 == 3);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        let edge = if rng.coin() { Edge::Start } else { Edge::End };
        let to = Ticks(original.start.0 + rng.range(-2 * PPQ, original.length.0 + 2 * PPQ));
        let trimmed = edit::trim(&project, &tempo, &original, edge, to, false);
        Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(trimmed.clone());
        assert_eq!(validate(&project), Ok(()), "case {case}");
        assert_anchored(&project, &trimmed);
        // The new window plays what the map played there before, the part
        // the trim revealed included. A looping clip's start cannot go before
        // its loop's origin, so its revealed part is the same turn.
        let mut extended = original.clone();
        if original.loop_length.is_none() {
            extended.length = Ticks(i64::MAX / 8);
        }
        for abs in inside(&mut rng, &trimmed, 10) {
            if original.loop_length.is_none() || abs >= original.start - original.loop_phase {
                same_second(
                    heard(&trimmed, abs),
                    heard(&extended, abs),
                    &format!("case {case}, {edge:?} to {to:?}"),
                );
            }
        }
        // The first frame heard is never before the file's.
        let first = trimmed.warp.as_ref().unwrap().source_at(Ticks::ZERO);
        assert!(first > -1e-6, "case {case}: starts at {first}");
    }
}

#[test]
fn slipping_slides_the_audio_under_the_window() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, case % 4 == 3);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        let by = Ticks(rng.range(-3 * PPQ, 3 * PPQ));
        let slid = edit::slip(&project, &tempo, &original, by);
        Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(slid.clone());
        assert_eq!(validate(&project), Ok(()), "case {case}");
        assert_anchored(&project, &slid);
        let applied = slid.warp.as_ref().unwrap().markers[0].at
            - original.warp.as_ref().unwrap().markers[0].at;
        // Within what the file allows, the whole slip is made.
        assert_eq!(applied, by, "case {case}: {by:?} is well inside the file");
        for abs in inside(&mut rng, &slid, 10) {
            // The position in the content (wrapped by a loop) moves back by
            // the slip before the old map reads it.
            let mut at = abs - slid.start + slid.loop_phase;
            if let Some(period) = slid.loop_length {
                at = Ticks(at.0.rem_euclid(period.0));
            }
            same_second(
                heard(&slid, abs),
                original.warp.as_ref().unwrap().source_at(at - applied),
                &format!("case {case}"),
            );
        }
        assert_eq!(
            (slid.start, slid.length, slid.loop_phase),
            (original.start, original.length, original.loop_phase),
            "the window stays put"
        );
    }
}

#[test]
fn slip_stops_at_the_files_edges() {
    // Looping and not, with a phase: whatever the slip asked for, the part of
    // the map the window plays (one turn of a loop) lies inside the file, to
    // the last bit: no tolerance.
    let mut rng = Rng(7);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, case % 2 == 1);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        for by in [q(1_000), q(-1_000), Ticks(rng.range(-30 * PPQ, 30 * PPQ))] {
            let slid = edit::slip(&project, &tempo, &original, by);
            Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(slid.clone());
            assert_eq!(validate(&project), Ok(()), "case {case}: {by:?}");
            assert_anchored(&project, &slid);
            let warp = slid.warp.as_ref().unwrap();
            let span = slid.loop_length.unwrap_or(slid.length);
            let (first, end) = (warp.source_at(Ticks::ZERO), warp.source_at(span));
            assert!(
                first >= 0.0 && end <= FILE_SECONDS,
                "case {case}, {by:?}: {first} to {end}"
            );
        }
    }
}

/// `offset` and `audio_phase_seconds` hold the second the map puts at tick 0
/// (never before the file's first frame), as every edit promises.
fn assert_anchored(project: &Project, clip: &Clip) {
    let ClipContent::Audio { offset, .. } = clip.content else {
        unreachable!()
    };
    let rate = f64::from(project.media[0].sample_rate);
    let held = offset as f64 / rate + clip.audio_phase_seconds;
    let want = clip.warp.as_ref().unwrap().source_at(Ticks::ZERO).max(0.0);
    assert!((held - want).abs() < 1e-9, "anchor: {held} against {want}");
}

#[test]
fn a_transpose_that_overflows_is_refused_not_a_panic() {
    let mut project = golden_v33_expected();
    for cents in [i32::MIN, i32::MAX, -4_801, 4_801] {
        let track = Arc::make_mut(&mut project.tracks[0]);
        Arc::make_mut(&mut track.clips[0])
            .warp
            .as_mut()
            .unwrap()
            .transpose_cents = cents;
        assert!(validate(&project).is_err(), "{cents}");
    }
}

#[test]
fn changing_the_original_tempo_moves_the_anchor_of_the_clip_and_every_take() {
    let mut project = project_with(audio_clip(1, q(0), q(8), 48_000));
    let mut clip = (*project.tracks[0].clips[0]).clone();
    clip.takes = Some(Takes::new(
        0,
        2,
        vec![OtherTake {
            take: 1,
            pass: Pass::Audio {
                offset: 240_000,
                audio_phase_seconds: 0.0,
                warp: None,
            },
        }],
    ));
    let clip = warp_edit::enable(&project, &clip, 120.0).unwrap();
    // One marker well after the clock, so tick 0 reads through the original
    // tempo: changing it moves the second at tick 0.
    let clip = warp_edit::set_markers(&project, &clip, vec![Marker::new(3.0, q(2))]).unwrap();
    let slow = warp_edit::set(&project, &clip, &warp_edit::Change::OriginalBpm(60.0)).unwrap();
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(slow.clone());
    assert_eq!(validate(&project), Ok(()));
    assert_anchored(&project, &slow);
    // The stored pass followed too.
    let Some(Pass::Audio {
        offset,
        audio_phase_seconds,
        warp: Some(other),
    }) = slow.takes.as_ref().and_then(|t| t.other(1)).cloned()
    else {
        panic!("the other pass has a warp")
    };
    assert_eq!(other.original_bpm, 60.0);
    let held = offset as f64 / f64::from(RATE) + audio_phase_seconds;
    assert!((held - other.source_at(Ticks::ZERO).max(0.0)).abs() < 1e-9);
    // And warp off keeps the anchor.
    let off = warp_edit::disable(&slow).unwrap();
    let ClipContent::Audio { offset, .. } = off.content else {
        unreachable!()
    };
    let held = offset as f64 / f64::from(RATE) + off.audio_phase_seconds;
    assert!((held - slow.warp.as_ref().unwrap().source_at(Ticks::ZERO).max(0.0)).abs() < 1e-9);
}

#[test]
fn markers_that_leave_the_file_are_refused() {
    let project = random_clip(&mut Rng(3), false);
    let clip = only(&project);
    let before = clip.warp.clone();
    for markers in [
        vec![Marker::new(FILE_SECONDS + 5e-6, Ticks::ZERO)],
        vec![Marker::new(-1.0, Ticks::ZERO)],
        vec![Marker::new(FILE_SECONDS - 0.5, Ticks::ZERO)],
    ] {
        assert_eq!(
            warp_edit::set_markers(&project, &clip, markers.clone()),
            Err(warp_edit::Refused::Setting),
            "{markers:?}"
        );
    }
    assert_eq!(clip.warp, before);
    // A tempo change that pushes the end out of the file cuts the clip back
    // to where the file ends (MT-WARP-2): never longer, and never refused
    // for that alone.
    let mut project = project_with(audio_clip(1, q(0), q(8), 0));
    let clip = (*project.tracks[0].clips[0]).clone();
    let tail = Marker::new(FILE_SECONDS - 4.0, Ticks::ZERO);
    let clip = warp_edit::enable(&project, &clip, 120.0).unwrap();
    let clip = warp_edit::set_markers(&project, &clip, vec![tail]).unwrap();
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(clip.clone());
    // Four seconds of file for a clip that plays four; at 60 BPM it needs eight.
    let fitted = warp_edit::set(&project, &clip, &warp_edit::Change::OriginalBpm(60.0))
        .expect("the clip is cut back, not refused");
    assert!(fitted.length < clip.length, "cut back to the file's end");
    assert_eq!(fitted.start, clip.start);
    every_pass_fits(&fitted);
    let mut after = project.clone();
    Arc::make_mut(&mut after.tracks[0]).clips[0] = Arc::new(fitted);
    assert_eq!(validate(&after), Ok(()));
}

#[test]
fn warp_off_keeps_the_anchor_of_a_phased_loop() {
    let mut rng = Rng(0x600d_f00d);
    for case in 0..CASES / 2 {
        let project = random_clip(&mut rng, true);
        let on = only(&project);
        let off = warp_edit::disable(&on).unwrap();
        // The offset holds the second at the clock, which is where the loop's
        // content begins; turning warp off does not move it.
        let ClipContent::Audio { offset, .. } = off.content else {
            unreachable!()
        };
        let held = offset as f64 / f64::from(RATE) + off.audio_phase_seconds;
        let want = on.warp.as_ref().unwrap().source_at(Ticks::ZERO).max(0.0);
        assert!(
            (held - want).abs() < 1e-9,
            "case {case}: {held} against {want}"
        );
    }
}

#[test]
fn reversing_plays_the_same_stretch_the_other_way_and_twice_returns_the_clip() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, case % 2 == 1);
        let original = only(&project);
        let done = reverse::reverse(&mut project, &[original.id]);
        assert_eq!(done.changed, 1, "case {case}: well inside the file");
        assert_eq!(validate(&project), Ok(()), "case {case}");
        assert_anchored(&project, &only(&project));
        let flipped = only(&project);
        assert!(flipped.reversed);
        let end = original.start + original.length;
        for abs in inside(&mut rng, &flipped, 10) {
            // The reversed clip plays the old one's audio from the other
            // end of its window, backwards.
            let mirror = original.start + end - abs;
            let on_turn = original.loop_length.is_some_and(|p| {
                (mirror - original.start + original.loop_phase)
                    .0
                    .rem_euclid(p.0)
                    == 0
            });
            if on_turn {
                continue;
            }
            same_second(
                heard(&flipped, abs),
                heard(&original, mirror),
                &format!("case {case}"),
            );
        }
        reverse::reverse(&mut project, &[original.id]);
        let back = only(&project);
        // Mirroring twice returns the map to within the last bit of a second.
        let (a, b) = (back.warp.as_ref().unwrap(), original.warp.as_ref().unwrap());
        assert_eq!(a.markers.len(), b.markers.len(), "case {case}");
        for (x, y) in a.markers.iter().zip(&b.markers) {
            assert_eq!(x.at, y.at, "case {case}: reverse twice");
            same_second(x.source, y.source, "reverse twice");
        }
        let mut back = back;
        same_second(
            back.audio_phase_seconds,
            original.audio_phase_seconds,
            "the phase after reverse twice",
        );
        back.warp = original.warp.clone();
        back.audio_phase_seconds = original.audio_phase_seconds;
        assert_eq!(back, original, "case {case}: reverse twice");
    }
}

#[test]
fn unlooping_makes_clips_that_play_the_same_seconds() {
    let mut rng = Rng(0x0bad_cafe_1234_5678);
    for case in 0..CASES / 2 {
        let mut project = random_clip(&mut rng, true);
        let original = only(&project);
        let (ids, too_long) = clip_ops::unloop(&mut project, &[original.id], &mut fresh());
        assert!(!too_long);
        assert_eq!(validate(&project), Ok(()), "case {case}");
        assert!(!ids.is_empty());
        for piece in project.tracks[0].clips.iter() {
            assert_anchored(&project, piece);
            assert!(piece.loop_length.is_none() && piece.loop_phase == Ticks::ZERO);
            for abs in inside(&mut rng, piece, 6) {
                same_second(
                    heard(piece, abs),
                    heard(&original, abs),
                    &format!("case {case}"),
                );
            }
            // A piece starts on the second the map puts there.
            let anchor = piece.warp.as_ref().unwrap().source_at(Ticks::ZERO);
            let ClipContent::Audio { offset, .. } = piece.content else {
                unreachable!()
            };
            same_second(
                offset as f64 / f64::from(RATE) + piece.audio_phase_seconds,
                anchor,
                "the offset follows the map",
            );
        }
    }
}

#[test]
fn the_brace_keeps_what_plays_up_to_the_first_repeat() {
    let mut rng = Rng(0x7777_1111_aaaa_5555);
    let (mut made, mut skipped, mut total_checked) = (0, 0, 0);
    for case in 0..CASES {
        let project = random_clip(&mut rng, case % 3 != 0);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        let looping = original.loop_length.is_some();
        let turn = original
            .loop_length
            .map(|p| original.start - Ticks(original.loop_phase.0.rem_euclid(p.0)));
        let change = match (looping, rng.range(0, 3)) {
            (false, _) => LoopEdit::On,
            (true, 0) => LoopEdit::End(Ticks(original.start.0 + rng.range(PPQ, 6 * PPQ))),
            (true, 1) => LoopEdit::Start(Ticks(original.start.0 - rng.range(0, 2 * PPQ))),
            (true, _) => LoopEdit::Off,
        };
        let Some(changed) = clip_loop::set_clip_loop(&project, &tempo, &original, change) else {
            skipped += 1;
            continue;
        };
        made += 1;
        let mut after = project.clone();
        Arc::make_mut(&mut after.tracks[0]).clips[0] = Arc::new(changed.clone());
        assert_eq!(validate(&after), Ok(()), "case {case}: {change:?}");
        assert_anchored(&after, &changed);
        let until = first_repeat(&original).min(first_repeat(&changed));
        let _ = turn;
        let mut checked = 0;
        for abs in (0..40).map(|_| Ticks(original.start.0 + rng.range(1, original.length.0))) {
            let on_turn = |c: &Clip| {
                c.loop_length
                    .is_some_and(|p| (abs - c.start + c.loop_phase).0.rem_euclid(p.0) == 0)
            };
            if abs >= until
                || abs >= changed.start + changed.length
                || on_turn(&original)
                || on_turn(&changed)
            {
                continue;
            }
            same_second(
                heard(&changed, abs),
                heard(&original, abs),
                &format!("case {case}: {change:?}"),
            );
            checked += 1;
        }
        total_checked += checked;
        // Same clip start, on the same tick.
        assert_eq!(changed.start, original.start);
    }
    assert!(made > CASES / 2, "{made} edits made, {skipped} refused");
    assert!(
        total_checked > 2_000,
        "only {total_checked} positions compared"
    );
}

#[test]
fn warp_off_leaves_the_clip_where_it_stands_in_its_file() {
    let mut rng = Rng(0x5151_5151_a5a5_a5a5);
    for case in 0..CASES {
        let mut project = random_clip(&mut rng, false);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        let cut = Ticks(original.start.0 + rng.range(PPQ, original.length.0 - PPQ));
        let trimmed = edit::trim(&project, &tempo, &original, Edge::Start, cut, false);
        let off = warp_edit::disable(&trimmed).unwrap();
        Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(off.clone());
        assert_eq!(validate(&project), Ok(()), "case {case}");
        let ClipContent::Audio { offset, .. } = off.content else {
            unreachable!()
        };
        same_second(
            offset as f64 / f64::from(RATE) + off.audio_phase_seconds,
            heard(&trimmed, trimmed.start),
            &format!("case {case}: the second under the start"),
        );
    }
}

/// Whether every pass `clip` keeps, the playing one and each stored, reads
/// inside the file over the window it plays: from the clock to the end of the
/// clip, or of one turn of its loop. No tolerance.
fn every_pass_fits(clip: &Clip) {
    let span = clip.loop_length.unwrap_or(clip.length);
    let mut warps = vec![clip.warp.clone().unwrap()];
    if let Some(takes) = &clip.takes {
        for other in &takes.others {
            if let Pass::Audio {
                warp: Some(warp), ..
            } = &other.pass
            {
                warps.push(warp.clone());
            }
        }
    }
    for (n, warp) in warps.iter().enumerate() {
        let (first, end) = (warp.source_at(Ticks::ZERO), warp.source_at(span));
        assert!(
            first >= 0.0 && end <= FILE_SECONDS,
            "pass {n} reads {first} to {end} of a {FILE_SECONDS} s file"
        );
    }
}

#[test]
fn a_trim_or_a_brace_edit_keeps_every_take_inside_its_file() {
    let mut rng = Rng(0x7a4e_5eed_0001);
    let mut stored = 0;
    for case in 0..CASES {
        let mut project = random_clip_with(&mut rng, case % 3 == 0, true);
        let tempo = edit::tempo_of(&project);
        let original = only(&project);
        stored += usize::from(original.takes.is_some());
        let edge = if rng.coin() { Edge::Start } else { Edge::End };
        let to = Ticks(original.start.0 + rng.range(-12 * PPQ, original.length.0 + 12 * PPQ));
        let trimmed = edit::trim(&project, &tempo, &original, edge, to, false);
        Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(trimmed.clone());
        assert_eq!(validate(&project), Ok(()), "case {case}");
        every_pass_fits(&trimmed);
        // The brace too, on the trimmed clip.
        let change = match rng.range(0, 4) {
            0 => LoopEdit::On,
            1 => LoopEdit::End(Ticks(trimmed.start.0 + rng.range(PPQ, 9 * PPQ))),
            2 => LoopEdit::Start(Ticks(trimmed.start.0 - rng.range(0, 6 * PPQ))),
            _ => LoopEdit::Off,
        };
        if let Some(changed) = clip_loop::set_clip_loop(&project, &tempo, &trimmed, change) {
            Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(changed.clone());
            assert_eq!(validate(&project), Ok(()), "case {case}: {change:?}");
            every_pass_fits(&changed);
        }
    }
    assert_eq!(stored, CASES, "every case carries a stored take");
}

#[test]
fn a_new_original_tempo_cuts_the_clip_back_when_a_stored_take_would_leave_the_file() {
    let mut project = project_with(audio_clip(1, q(0), q(4), 0));
    let mut clip = (*project.tracks[0].clips[0]).clone();
    // The playing pass is pinned by two markers (it reads the same whatever
    // the original tempo is); the stored one starts two seconds from the end
    // of the file and plays four beats, which fill two seconds at 120 BPM and
    // four at 60.
    clip.takes = Some(Takes::new(
        0,
        2,
        vec![OtherTake {
            take: 1,
            pass: Pass::Audio {
                offset: (FILE_SECONDS as i64 - 2) * i64::from(RATE),
                audio_phase_seconds: 0.0,
                warp: None,
            },
        }],
    ));
    let clip = warp_edit::enable(&project, &clip, 120.0).unwrap();
    let pinned = vec![Marker::new(0.0, Ticks::ZERO), Marker::new(2.0, q(4))];
    let clip = warp_edit::set_markers(&project, &clip, pinned).unwrap();
    Arc::make_mut(&mut project.tracks[0]).clips[0] = Arc::new(clip.clone());
    assert_eq!(validate(&project), Ok(()));
    every_pass_fits(&clip);
    // The clip is cut to where the tightest pass's file ends, so every pass
    // still reads inside it.
    let fitted = warp_edit::set(&project, &clip, &warp_edit::Change::OriginalBpm(60.0))
        .expect("the clip is cut back, not refused");
    assert!(fitted.length < clip.length);
    every_pass_fits(&fitted);
}

#[test]
fn markers_within_a_microsecond_of_the_file_edges_are_still_refused() {
    let project = random_clip(&mut Rng(11), false);
    let clip = only(&project);
    let length = clip.length;
    for markers in [
        vec![Marker::new(-5e-7, Ticks::ZERO)],
        vec![
            Marker::new(FILE_SECONDS - 4.0, Ticks::ZERO),
            Marker::new(FILE_SECONDS + 5e-7, length),
        ],
    ] {
        assert_eq!(
            warp_edit::set_markers(&project, &clip, markers.clone()),
            Err(warp_edit::Refused::Setting),
            "{markers:?}"
        );
    }
}
