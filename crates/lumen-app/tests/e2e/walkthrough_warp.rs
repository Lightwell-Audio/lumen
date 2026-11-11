//! M9.5 S7: the milestone's exit criterion end to end (`docs/milestones/M9.5.md`,
//! "Exit criterion"). A drum loop at 92 BPM is dropped into a song at 120 BPM
//! and is heard in time with nothing done to it. One late hit is fixed with
//! warp markers, a vocal phrase is transposed up two semitones, both clips are
//! trimmed, faded and slipped in the Clip tab, and the song is exported. The
//! file is checked against what the test device played: the same hits, the
//! same distances between them, and the vocal at the pitch that was set.
//!
//! The loop is a click track (a click on each beat, one of them 80 ms late) and
//! the phrase a steady tone, so each claim is a number that can be measured:
//! where the hits fall and what the frequency is. What it sounds like is for
//! the owner's ears, in MT-M9.5-1.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{Id, WarpMode};
use lumen_project::testing::TempDir;

use super::audio_clip_properties::type_into;
use super::audio_editor_handles::{clip, handles, open, shown, x_of};
use super::warp_chip::{clicks_at, close_chip, open_chip};
use super::{Lumen, wav_at};

const RATE: f64 = 48_000.0;
/// The loop's tempo, and the song's.
const LOOP_BPM: f64 = 92.0;
/// Seconds into the loop of the hit that is played late, and by how much.
const LATE_HIT: usize = 5;
const LATE_BY: f64 = 0.08;
/// Seconds of a beat in the loop.
const LOOP_BEAT: f64 = 60.0 / LOOP_BPM;
/// Frames listened to and exported: the first eight beats of the song.
const LISTEN: usize = 4 * 48_000;

fn track_clip(lumen: &Lumen, name: &str) -> Id {
    let project = lumen.app().project();
    project
        .tracks
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("a track named {name}"))
        .clips[0]
        .id
}

/// Where in the left channel a hit starts: the first frame over 0.1 after at
/// least a quarter of a second of quiet, so a click's tail is one hit.
fn onsets(left: impl IntoIterator<Item = f32>) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (i, s) in left.into_iter().enumerate() {
        if s.abs() > 0.1 && found.last().is_none_or(|last| i - last > 12_000) {
            found.push(i);
        }
    }
    found
}

/// The seconds between each hit and the next, from the second hit on: the test
/// device drops what it played before `listen` began, up to a block, so the
/// first hit heard may be only its tail.
fn gaps(onsets: &[usize]) -> Vec<f64> {
    onsets[1..]
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / RATE)
        .collect()
}

/// Plays from the start for [`LISTEN`] frames and gives the left channel.
fn hear(lumen: &mut Lumen) -> Vec<f32> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.listen(Duration::ZERO, LISTEN);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    heard.iter().map(|f| f[0]).collect()
}

/// Exports the whole project to `path` and gives its left channel.
fn export(lumen: &mut Lumen, path: &Path) -> Vec<f32> {
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    assert!(lumen.app().audio_export_open());
    lumen.app_mut().answer_next_dialog(vec![path.to_owned()]);
    lumen.click("Export audio…");
    lumen.wait_for("the export", |app| !app.audio_export_busy());
    assert_eq!(lumen.app().audio_export_error(), None);
    let audio = lumen_media::decode(path, &AtomicBool::new(false), &mut |_| {})
        .unwrap_or_else(|e| panic!("{} does not decode: {e}", path.display()));
    assert_eq!(audio.sample_rate, 48_000);
    audio.channel(0).to_vec()
}

/// The loop (eight clicks a beat apart at 92 BPM, one of them late) and the
/// phrase (three seconds of a steady tone).
fn drum_files(dir: &Path) -> (PathBuf, PathBuf) {
    let onsets: Vec<f64> = (0..8)
        .map(|k| k as f64 * LOOP_BEAT + if k == LATE_HIT { LATE_BY } else { 0.0 })
        .collect();
    let drums = clicks_at(&dir.join("Drums 92 BPM.wav"), &onsets, 8.0 * LOOP_BEAT);
    let vocal = wav_at(&dir.join("Vocal.wav"), 3.0, 1_500.0);
    (drums, vocal)
}

/// How often a tone crosses zero upwards in `frames`, as a frequency.
fn frequency(frames: &[f32]) -> f64 {
    let crossings = frames
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f64 * RATE / frames.len() as f64
}

/// The strip's middle line, at beat `q`.
fn on_strip(lumen: &Lumen, q: f64) -> Pos2 {
    let (rect, _) = lumen.app().audio_warp_strip().expect("the strip is drawn");
    Pos2::new(x_of(lumen, q), rect.center().y)
}

#[test]
fn a_loop_at_another_tempo_is_in_time_one_hit_is_fixed_a_phrase_is_transposed_and_the_export_is_what_was_heard()
 {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-walkthrough-warp"), |l| {
        l.playing_from_disk()
    });
    let (drums_file, vocal_file) = drum_files(lumen.dir());
    lumen.drop_files(&[drums_file, vocal_file]);
    lumen.wait_for("both files", |app| {
        app.project().tracks.len() == 2 && app.project().tracks.iter().all(|t| !t.clips.is_empty())
    });
    lumen.frames(30);
    let drums = track_clip(&lumen, "Drums 92 BPM");
    let vocal = track_clip(&lumen, "Vocal");

    // 1. In time, with no extra steps: the file says 92 BPM, so it follows the
    //    song, and its eight beats are eight beats at 120.
    let warp = clip(&lumen, drums).warp.expect("the loop follows the song");
    assert_eq!(
        (warp.mode, warp.original_bpm),
        (WarpMode::FullMix, LOOP_BPM)
    );
    let length = clip(&lumen, drums).length;
    assert!(
        (length.0 - 8 * PPQ).abs() < PPQ / 100,
        "eight beats long, not {length:?}"
    );
    assert!(
        clip(&lumen, vocal).warp.is_none(),
        "a phrase with no tempo plays as it is"
    );
    lumen.frames(30);
    let before = gaps(&onsets(hear(&mut lumen)));
    assert!(
        before.len() >= 4,
        "the hits of the loop are heard: {before:?}"
    );
    for gap in &before[..3] {
        assert!((gap - 0.5).abs() < 0.025, "on the song's beat: {before:?}");
    }
    assert!(
        before[3] > 0.5 + 0.04,
        "the late hit is heard late: {before:?}"
    );

    // 2. Fix the late hit: Fix this hit pins the hits either side of it and
    //    adds a marker on it, one undo step, and the marker is dragged onto
    //    its beat. Nothing else moves: the anchors stay where they were.
    open(&mut lumen, drums);
    lumen.wait_for("detection", |app| app.audio_detection().ready);
    let late_beat = (LATE_HIT as f64 * LOOP_BEAT + LATE_BY) * LOOP_BPM / 60.0;
    lumen.right_click(on_strip(&lumen, late_beat));
    assert!(lumen.shows("Fix this hit"), "the strip offers to fix it");
    lumen.click("Fix this hit");
    let from = on_strip(&lumen, late_beat);
    let to = on_strip(&lumen, LATE_HIT as f64);
    lumen.drag(from, to, Modifiers::NONE);
    let fixed = clip(&lumen, drums);
    let map = fixed.warp.clone().expect("still warped");
    assert_eq!(map.markers.len(), 4, "the first, two anchors and the hit");
    let source = map.source_at(Ticks::quarters(LATE_HIT as i64));
    let late_at = LATE_HIT as f64 * LOOP_BEAT + LATE_BY;
    assert!(
        (source - late_at).abs() < 0.01,
        "the late hit plays on its beat: {source} s for the hit at {late_at} s"
    );
    for k in [4, 6] {
        let source = map.source_at(Ticks::quarters(k));
        assert!(
            (source - k as f64 * LOOP_BEAT).abs() < 0.002,
            "an anchor keeps hit {k} where it was: {source} s"
        );
    }

    // 3. The vocal: warped, Melodic, two semitones up.
    open(&mut lumen, vocal);
    lumen.wait_for("the phrase's detection", |app| {
        let found = app.audio_detection();
        found.ready || found.failed
    });
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    lumen.harness.get_by_label_contains("Warp mode: ").click();
    lumen.frames(3);
    lumen.click("Melodic");
    close_chip(&mut lumen);
    lumen.harness.get_by_label_contains("Pitch: ").click();
    lumen.frames(4);
    type_into(&mut lumen, "Transpose: ", "2");
    close_chip(&mut lumen);
    let tuned = clip(&lumen, vocal).warp.expect("the phrase is warped");
    assert_eq!(
        (tuned.mode, tuned.transpose_cents),
        (WarpMode::Melodic, 200)
    );

    // 4. Trim, fade and slip the vocal, in the Clip tab: its end to beat 5, a
    //    fade in of half a beat, and the audio one step earlier (a clip at the start of its file has nothing before it).
    let mid = shown(&lumen).body.center().y;
    let grip = Pos2::new(handles(&lumen).end - 2.0, mid);
    lumen.drag(
        grip,
        Pos2::new(x_of(&lumen, 5.0) + 3.0, mid),
        Modifiers::NONE,
    );
    let h = handles(&lumen);
    lumen.drag(
        h.fade_in,
        Pos2::new(x_of(&lumen, 0.5), h.fade_in.y),
        Modifiers::NONE,
    );
    lumen.harness.hover_at(shown(&lumen).body.center());
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowLeft);
    let phrase = clip(&lumen, vocal);
    assert_eq!(phrase.start + phrase.length, Ticks::quarters(5));
    assert_eq!(phrase.fade_in.length, Ticks(PPQ / 2));
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
    assert_eq!(phrase.warp.as_ref().map(|w| w.transpose_cents), Some(200));

    // 5. The same for the loop: its end to beat 7.5, a fade out of a quarter
    //    beat and a slip, which is tried and taken back: the hits stay on
    //    the beat. Each is one undo step.
    open(&mut lumen, drums);
    let mid = shown(&lumen).body.center().y;
    let grip = Pos2::new(handles(&lumen).end - 2.0, mid);
    lumen.drag(
        grip,
        Pos2::new(x_of(&lumen, 7.5) + 3.0, mid),
        Modifiers::NONE,
    );
    let h = handles(&lumen);
    lumen.drag(
        h.fade_out,
        Pos2::new(x_of(&lumen, 7.25), h.fade_out.y),
        Modifiers::NONE,
    );
    let trimmed = clip(&lumen, drums);
    assert_eq!(
        trimmed.start + trimmed.length,
        Ticks::quarters(7) + Ticks(PPQ / 2)
    );
    assert_eq!(trimmed.fade_out.length, Ticks(PPQ / 4));
    assert_eq!(trimmed.warp.as_ref().map(|w| w.markers.len()), Some(4));
    lumen.harness.hover_at(shown(&lumen).body.center());
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowLeft);
    assert_ne!(clip(&lumen, drums), trimmed, "the slip moved the audio");
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(clip(&lumen, drums), trimmed, "one undo takes the slip back");

    // 6. What is heard: every hit on the beat, the late one included.
    lumen.frames(30);
    let heard = hear(&mut lumen);
    let heard_hits = onsets(heard.iter().copied());
    assert!(heard_hits.len() >= 7, "hits heard: {heard_hits:?}");
    for gap in gaps(&heard_hits) {
        assert!(
            (gap - 0.5).abs() < 0.025,
            "every hit on the beat once the late one is fixed: {:?}",
            gaps(&heard_hits)
        );
    }

    // 7. The export is what was heard: the same hits at the same distances,
    //    and the phrase at the pitch that was set.
    let song = lumen.dir().join("song.wav");
    let exported = export(&mut lumen, &song);
    let exported_hits = onsets(exported.iter().copied().take(LISTEN));
    // The test device drops what it played before `listen` begins, so the
    // first hit can be heard as a tail, or (on a slow runner) not at all: the
    // heard hits are then the export's from its second on. Nothing else may
    // be missing.
    let missed = exported_hits.len().checked_sub(heard_hits.len());
    assert!(
        matches!(missed, Some(0 | 1)),
        "the same hits: heard {heard_hits:?}, exported {exported_hits:?}"
    );
    let exported_hits = &exported_hits[missed.unwrap_or(0)..];
    for (heard, exported) in gaps(&heard_hits).into_iter().zip(gaps(exported_hits)) {
        assert!(
            (heard - exported).abs() < 0.002,
            "a gap of {heard} s heard and {exported} s exported: heard {heard_hits:?}, exported {exported_hits:?}"
        );
    }
    // The vocal alone, between two hits (the loop's clicks have died away by
    // then and the fade in has ended): its tone is 382 Hz, and two semitones up
    // is 429 Hz.
    let from = (0.30 * RATE) as usize;
    let tone = frequency(&exported[from..from + (0.15 * RATE) as usize]);
    assert!(
        (415.0..445.0).contains(&tone),
        "the phrase is two semitones up, not {tone} Hz"
    );
}

/// What the walkthrough works around, kept as a record of how the strip behaves
/// (M9.5 S7 review, finding M2): one marker moves the audio on both sides of
/// it. Dragging the late hit onto its beat with nothing pinned shifts every
/// later hit 61 ms early, because the audio after the last marker keeps its
/// speed from that marker on. "Fix this hit" is the way to fix one hit: it
/// pins the hits either side first, as the walkthrough does with it. If plain
/// dragging ever pins them itself, this test is the one to change.
#[test]
fn a_lone_marker_moves_the_hits_after_it_so_the_walkthrough_anchors_first() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-warp-lone-marker"), |l| {
        l.playing_from_disk()
    });
    let (drums_file, _) = drum_files(lumen.dir());
    lumen.drop_files(&[drums_file]);
    lumen.wait_for("the loop", |app| {
        app.project().tracks.iter().all(|t| !t.clips.is_empty()) && !app.project().tracks.is_empty()
    });
    lumen.frames(30);
    let drums = track_clip(&lumen, "Drums 92 BPM");
    open(&mut lumen, drums);
    lumen.wait_for("detection", |app| app.audio_detection().ready);
    let late_beat = (LATE_HIT as f64 * LOOP_BEAT + LATE_BY) * LOOP_BPM / 60.0;
    let from = on_strip(&lumen, late_beat);
    let to = on_strip(&lumen, LATE_HIT as f64);
    lumen.drag(from, to, Modifiers::NONE);
    let map = clip(&lumen, drums).warp.expect("warped");
    assert_eq!(map.markers.len(), 2, "the first marker and the one made");
    let late_at = LATE_HIT as f64 * LOOP_BEAT + LATE_BY;
    assert!((map.source_at(Ticks::quarters(LATE_HIT as i64)) - late_at).abs() < 0.01);
    // At beat 6 the audio is now 80 ms past hit 6: the hit sounds early.
    let past = map.source_at(Ticks::quarters(6)) - 6.0 * LOOP_BEAT;
    assert!(
        past > 0.05,
        "hit 6 sounds early: the audio is {past} s past it"
    );
}
