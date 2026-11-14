//! The drum editor's frame time beside the piano roll's, on the same clip of
//! 100,000 hits (M8 D2's acceptance): an ignored bench that prints both and
//! fails only if the lanes are far slower. It drives the real window and GPU,
//! as the arrangement's bench does (ADR-0011).

use std::time::Instant;

use lumen_core::time::Ticks;
use lumen_project::drums::presets::Preset;
use lumen_project::model::{Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

const HITS: usize = 100_000;

/// A mapped track whose clip holds `HITS` hits across 400 beats, on the
/// 808-style kit's sounds.
fn dense_kit() -> Project {
    let map = Preset::EIGHT_OH_EIGHT.map(Id(500));
    let sounds: Vec<u8> = map.sounds.iter().map(|s| s.input).collect();
    let beats = 400_i64;
    let step = beats * lumen_core::time::PPQ / HITS as i64;
    let notes: Vec<Note> = (0..HITS as i64)
        .map(|i| {
            Note::new(
                Ticks(i * step),
                Ticks(step * 2),
                sounds[(i % sounds.len() as i64) as usize],
                20 + (i * 7 % 100) as u8,
            )
        })
        .collect();
    let mut project = project_of(
        "Dense",
        vec![midi_track(
            1,
            "Drums",
            "pluck",
            (0, beats),
            None,
            notes,
            vec![],
        )],
    );
    project.drum_maps = vec![map.clone()];
    let track = std::sync::Arc::make_mut(&mut project.tracks[0]);
    track.drum_map = Some(map.id);
    project
}

/// Milliseconds a frame took, over `frames` frames after a warm-up.
fn measure(lumen: &mut Lumen, frames: usize) -> (f64, f64, f64) {
    lumen.frames(20);
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        let start = Instant::now();
        lumen.frames(1);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    (at(0.5), at(0.95), at(1.0))
}

#[test]
#[ignore = "a bench: prints frame times for a 100,000-hit clip in the lanes and the roll"]
fn a_hundred_thousand_hits_in_the_lanes_beside_the_piano_roll() {
    let project = dense_kit();
    let dir = TempDir::new("e2e-drum-bench");
    let path = dir.path().join("dense.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save");
    let clip = project.tracks[0].clips[0].id;
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    lumen.app_mut().set_detail_size(560.0);
    lumen.app_mut().open_clip_editor(clip);
    lumen.frames(60);
    assert_eq!(
        lumen.app().drum_view_shown(),
        Some("lanes"),
        "the lanes first"
    );
    let lanes = measure(&mut lumen, 120);

    lumen.app_mut().show_the_keys_view();
    lumen.frames(60);
    assert_eq!(lumen.app().drum_view_shown(), Some("keys"));
    let roll = measure(&mut lumen, 120);

    println!(
        "100,000 hits, 1280x820 at 100%:\n\
         drum lanes   p50 {:6.2} ms  p95 {:6.2}  worst {:6.2}\n\
         piano roll   p50 {:6.2} ms  p95 {:6.2}  worst {:6.2}",
        lanes.0, lanes.1, lanes.2, roll.0, roll.1, roll.2
    );
    assert!(
        lanes.1 <= roll.1 * 1.15 + 1.0,
        "the lanes' p95 ({:.2} ms) within 15% of the roll's ({:.2} ms)",
        lanes.1,
        roll.1
    );
}
