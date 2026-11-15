use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

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
const LOOP_BPM: f64 = 92.0;
const LATE_HIT: usize = 5;
const LATE_BY: f64 = 0.08;
const LOOP_BEAT: f64 = 60.0 / LOOP_BPM;
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

fn onsets(left: impl IntoIterator<Item = f32>) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (i, s) in left.into_iter().enumerate() {
        if s.abs() > 0.1 && found.last().is_none_or(|last| i - last > 12_000) {
            found.push(i);
        }
    }
    found
}

fn gaps(onsets: &[usize]) -> Vec<f64> {
    onsets
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / RATE)
        .collect()
}

fn hear(lumen: &mut Lumen) -> Vec<f32> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    let heard = lumen.listen_from(|l| l.key(Modifiers::NONE, Key::Space), LISTEN);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    heard.iter().map(|f| f[0]).collect()
}

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

fn drum_files(dir: &Path) -> (PathBuf, PathBuf) {
    let onsets: Vec<f64> = (0..8)
        .map(|k| k as f64 * LOOP_BEAT + if k == LATE_HIT { LATE_BY } else { 0.0 })
        .collect();
    let drums = clicks_at(&dir.join("Drums 92 BPM.wav"), &onsets, 8.0 * LOOP_BEAT);
    let vocal = wav_at(&dir.join("Vocal.wav"), 3.0, 1_500.0);
    (drums, vocal)
}

fn frequency(frames: &[f32]) -> f64 {
    let crossings = frames
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f64 * RATE / frames.len() as f64
}

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
    let late = LATE_HIT - 1;
    assert!(
        before.len() > late,
        "the hits of the loop are heard: {before:?}"
    );
    for gap in &before[..late] {
        assert!((gap - 0.5).abs() < 0.025, "on the song's beat: {before:?}");
    }
    assert!(
        before[late] > 0.5 + 0.04,
        "the late hit is heard late: {before:?}"
    );

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

    let song = lumen.dir().join("song.wav");
    let exported = export(&mut lumen, &song);
    let exported_hits = onsets(exported.iter().copied().take(LISTEN));
    assert_eq!(
        exported_hits.len(),
        heard_hits.len(),
        "the same hits: heard {heard_hits:?}, exported {exported_hits:?}"
    );
    for (heard, exported) in gaps(&heard_hits).into_iter().zip(gaps(&exported_hits)) {
        assert!(
            (heard - exported).abs() < 0.002,
            "a gap of {heard} s heard and {exported} s exported: heard {heard_hits:?}, exported {exported_hits:?}"
        );
    }
    let from = (0.30 * RATE) as usize;
    let tone = frequency(&exported[from..from + (0.15 * RATE) as usize]);
    assert!(
        (415.0..445.0).contains(&tone),
        "the phrase is two semitones up, not {tone} Hz"
    );
}

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
    let past = map.source_at(Ticks::quarters(6)) - 6.0 * LOOP_BEAT;
    assert!(
        past > 0.05,
        "hit 6 sounds early: the audio is {past} s past it"
    );
}
