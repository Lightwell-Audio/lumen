//! Reverse and polarity heard through the engine (M7b slice E1): the clips
//! the project describes are turned into engine clips as the app does, the
//! reversed twin standing in for a reversed clip's file, and rendered offline.
//! A reversed clip must play the same stretch of its file backwards on the
//! same samples; an inverted one must be the plain render with its sign
//! changed, bit for bit.

use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{
    ClipAudio, Command, Destination, NodeSpec, Routing, Timeline, clip_player, engine, mixer,
};
use lumen_project::model::{ClipContent, Id, Project};

use crate::document::{Document, DropInto, Imported};
use crate::lanes::engine_clips;
use crate::media::{self, reversed_key};

const RATE: u32 = 48_000;
/// Frames of the file: ten seconds.
const FRAMES: usize = 480_000;
/// The clip's first source frame.
const START: i64 = 1_000;
/// Frames either side of a clip's edges left out of comparisons: the
/// engine's edge dip and the 20 ms the level glides in over.
const EDGE: usize = 2_000;

/// A file whose frame `i` is distinct, as a ramp with a little noise.
fn file() -> Arc<ClipAudio> {
    let left: Vec<f32> = (0..FRAMES)
        .map(|i| (i % 1_999) as f32 / 4_000.0 - 0.25 + (i as f32 * 1e-6))
        .collect();
    let right = left.clone();
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![left, right],
    })
}

/// A project with one two-second clip (four quarters at 120 BPM) taking the
/// file from `START`.
fn document() -> Document {
    let mut doc = Document::new(crate::empty_project());
    doc.import(vec![Imported {
        media: Id(1000),
        path: PathBuf::from("C:/a/a.wav"),
        name: "A".into(),
        sample_rate: RATE,
        channels: 2,
        frames: FRAMES as u64,
        size_bytes: 0,
        at: Ticks::ZERO,
        into: DropInto::NewTracks {
            before: None,
            group: None,
        },
    }]);
    doc.apply("prepare", None, |project| {
        project.tempo = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }];
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        clip.length = Ticks::quarters(4);
        if let ClipContent::Audio { offset, .. } = &mut clip.content {
            *offset = START;
        }
    });
    doc
}

/// Renders the project from its start for `frames`, with the file and its
/// reversed twin standing for the media.
fn render(project: &Project, frames: usize, block: usize) -> Vec<f32> {
    render_as(project, frames, block, |id| id.0 & reversed_key(0) != 0)
}

/// [`render`] with `twin` saying which media ids get the reversed audio.
fn render_as(
    project: &Project,
    frames: usize,
    block: usize,
    twin: impl Fn(Id) -> bool,
) -> Vec<f32> {
    let forward = file();
    let backward = Arc::new(media::mirror(&forward));
    let timeline = Timeline::new(
        TempoMap::new(project.tempo.clone()).unwrap(),
        project.meter_map(),
    );
    let list = engine_clips(project, RATE, |id| {
        let audio = if twin(id) { &backward } else { &forward };
        Some((audio.clone(), RATE))
    });
    let (mut clips, player) = clip_player();
    clips.set(list).unwrap();
    let (mut routing, source) = mixer(player);
    routing
        .set(&Routing {
            nodes: vec![NodeSpec::unity(
                project.tracks[0].id.0,
                Destination::Hardware(0),
            )],
            cue: None,
            measure: None,
        })
        .unwrap();
    let (mut handle, mut engine) = engine(timeline, source, RATE);
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; block * 2];
    let mut out = Vec::new();
    for _ in 0..frames.div_ceil(block) {
        engine.process(RATE, &mut data, 2, block);
        out.extend_from_slice(&data);
    }
    out.truncate(frames * 2);
    out
}

fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved.chunks(2).map(|f| f[0]).collect()
}

#[test]
fn a_reversed_clip_plays_the_same_stretch_of_its_file_backwards() {
    let mut doc = document();
    let id = doc.project().tracks[0].clips[0].id;
    assert_eq!(doc.reverse_clips(&[id]).changed, 1);
    // The oracle: the file reversed first, then the window that ends where
    // the clip's window began, played forwards.
    let mut oracle = document();
    oracle.apply("oracle", None, |project| {
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        if let ClipContent::Audio { offset, .. } = &mut clip.content {
            *offset = FRAMES as i64 - START - 96_000;
        }
    });
    for block in [64, 128, 333, 4_096] {
        let heard = render(doc.project(), 96_000, block);
        let want = render_as(oracle.project(), 96_000, block, |_| true);
        assert_eq!(heard, want, "block {block}");
    }
}

#[test]
fn reversing_twice_is_the_original_render_bit_for_bit() {
    let mut doc = document();
    let id = doc.project().tracks[0].clips[0].id;
    let before = render(doc.project(), 96_000, 128);
    doc.reverse_clips(&[id]);
    doc.reverse_clips(&[id]);
    assert_eq!(render(doc.project(), 96_000, 128), before);
}

#[test]
fn reversing_one_part_of_a_split_clip_mirrors_only_that_part() {
    let mut doc = document();
    let id = doc.project().tracks[0].clips[0].id;
    let parts = doc.split_clips(&[id], &[Ticks::quarters(2)]);
    assert_eq!(parts.len(), 2);
    let before = left(&render(doc.project(), 96_000, 128));
    doc.reverse_clips(&parts[..1]);
    let after = left(&render(doc.project(), 96_000, 128));
    // The second part is untouched, away from the seam.
    for k in 48_000 + EDGE..96_000 - EDGE {
        assert_eq!(after[k], before[k], "k {k}");
    }
    // The first part plays its own second of the file backwards: the oracle
    // is a one-second clip of the reversed file ending where that stretch
    // began.
    let mut oracle = document();
    oracle.apply("oracle", None, |project| {
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        clip.length = Ticks::quarters(2);
        if let ClipContent::Audio { offset, .. } = &mut clip.content {
            *offset = FRAMES as i64 - START - 48_000;
        }
    });
    let want = left(&render_as(oracle.project(), 96_000, 128, |_| true));
    for k in EDGE..48_000 - EDGE {
        assert_eq!(after[k], want[k], "k {k}");
    }
}

#[test]
fn inverted_polarity_is_the_plain_render_with_its_sign_changed() {
    let mut doc = document();
    let id = doc.project().tracks[0].clips[0].id;
    let plain = render(doc.project(), 96_000, 128);
    assert_eq!(doc.invert_clips(&[id]), 1);
    let inverted = render(doc.project(), 96_000, 128);
    assert_eq!(plain.len(), inverted.len());
    for (i, (p, n)) in plain.iter().zip(&inverted).enumerate() {
        assert_eq!(*n, -p, "sample {i}");
    }
}

#[test]
fn a_reversed_clip_asks_for_the_twin_and_is_silent_until_it_is_there() {
    let mut doc = document();
    let id = doc.project().tracks[0].clips[0].id;
    doc.reverse_clips(&[id]);
    let forward = file();
    let asked = std::cell::RefCell::new(Vec::new());
    let list = engine_clips(doc.project(), RATE, |media| {
        asked.borrow_mut().push(media.0);
        (media.0 == 1000).then(|| (forward.clone(), RATE))
    });
    assert_eq!(*asked.borrow(), [reversed_key(1000)]);
    assert!(list.is_empty(), "no twin yet: nothing to play");
}
