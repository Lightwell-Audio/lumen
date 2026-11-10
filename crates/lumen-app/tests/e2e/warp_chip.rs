//! The audio editor's Warp chip and detection (M9.5 S5a, ADR-0134): the file
//! is listened to once and its transients and tempo are offered; the chip's
//! popover turns warp on from the tempo found, doubles or halves it, sets the
//! project tempo from the clip, and lines the sound at the edit cursor up
//! with a bar. Each is one undo step. The detection itself is tested on
//! synthetic audio in `lumen-media`; here a click track stands in for a loop.

use std::path::{Path, PathBuf};

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks, bpm_at};
use lumen_project::model::{Clip, Id};

use super::audio_editor_handles::{clip, open, shown, x_of};
use super::clip_commands::command;
use super::{Lumen, TempDir};

const RATE: u32 = 48_000;

/// A stereo WAV of `beats` clicks at `bpm`, the first at `lead` seconds, in a
/// file `seconds` long. A click is a short decaying burst of noise, a louder
/// one on every fourth beat.
fn click_track(path: &Path, (bpm, beats): (f64, usize), lead: f64, seconds: f64) -> PathBuf {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut mono = vec![0.0_f32; frames];
    for b in 0..beats {
        let at = ((lead + b as f64 * 60.0 / bpm) * f64::from(RATE)) as usize;
        let gain = if b % 4 == 0 { 0.9 } else { 0.5 };
        let mut seed = 0x1234_5678_u32;
        for i in 0..2_400 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            let decay = (-(i as f32) / 400.0).exp();
            if let Some(slot) = mono.get_mut(at + i) {
                *slot += gain * noise * decay;
            }
        }
    }
    let data: Vec<u8> = mono
        .iter()
        .flat_map(|s| {
            let [a, b] = ((s.clamp(-1.0, 1.0) * 30_000.0) as i16).to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the click track");
    path.to_path_buf()
}

/// Lumen with a click track at 100 BPM (sixteen beats, the first a quarter of
/// a second in) dropped in and opened in the Clip tab, detection done.
pub(super) fn opened_click_track(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let file = click_track(&lumen.dir().join("clicks.wav"), (100.0, 16), 0.25, 9.85);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    let id = lumen.app().project().tracks[0].clips[0].id;
    // Eight beats of it: at half the tempo the clip must still be inside the
    // file.
    lumen
        .app_mut()
        .trim_clip_end_to(id, Ticks::quarters(8), false);
    open(&mut lumen, id);
    lumen.wait_for("detection", |app| app.audio_detection().ready);
    (lumen, id)
}

/// The chip, opened.
pub(super) fn open_chip(lumen: &mut Lumen) {
    let chip = lumen
        .harness
        .query_all_by_label_contains("Warp: ")
        .next()
        .expect("the Warp chip is in the strip");
    chip.click();
    lumen.frames(4);
}

/// The popover closed, so a key reaches the app.
pub(super) fn close_chip(lumen: &mut Lumen) {
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
}

pub(super) fn warped(lumen: &Lumen, id: Id) -> Clip {
    let c = clip(lumen, id);
    assert!(c.warp.is_some(), "the clip is warped");
    c
}

fn original_bpm(c: &Clip) -> f64 {
    c.warp.as_ref().expect("warped").original_bpm
}

#[test]
fn a_file_is_listened_to_once_and_offers_its_transients_and_tempo() {
    let (lumen, _) = opened_click_track("warp-detect");
    let found = lumen.app().audio_detection();
    assert!(found.ready && !found.failed && !found.working);
    let bpm = found.tempo.expect("a tempo");
    assert!((bpm - 100.0).abs() < 1.5, "{bpm} for 100");
    assert_eq!(found.transients, 16, "a click for every beat");
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Warp: off")
            .next()
            .is_some(),
        "the chip says the clip is not warped"
    );
}

#[test]
fn warp_starts_from_the_tempo_found_and_each_change_is_one_undo_step() {
    let (mut lumen, id) = opened_click_track("warp-chip");
    let found = lumen.app().audio_detection().tempo.expect("a tempo");

    open_chip(&mut lumen);
    lumen.click("Warp clip");
    let on = warped(&lumen, id);
    assert!(
        (original_bpm(&on) - found).abs() < 1e-9,
        "{} from {found}",
        original_bpm(&on)
    );
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Warp: on")
            .next()
            .is_some(),
        "the chip says it is on"
    );

    // Twice and half the tempo the file was made at, each undone in one.
    // How much of the file four beats play.
    let heard = |c: &Clip| {
        let w = c.warp.as_ref().expect("warped");
        w.source_at(Ticks::quarters(4)) - w.source_at(Ticks::ZERO)
    };
    let before = heard(&on);
    lumen.click("\u{00d7}2");
    assert!((original_bpm(&warped(&lumen, id)) - 2.0 * found).abs() < 1e-9);
    assert!(
        (heard(&warped(&lumen, id)) - before / 2.0).abs() < 1e-9,
        "a file made at twice the tempo is stretched to twice the length"
    );
    lumen.click("\u{00f7}2");
    assert!((original_bpm(&warped(&lumen, id)) - found).abs() < 1e-9);
    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!((original_bpm(&warped(&lumen, id)) - 2.0 * found).abs() < 1e-9);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!((original_bpm(&warped(&lumen, id)) - found).abs() < 1e-9);

    // The song takes the clip's tempo.
    open_chip(&mut lumen);
    lumen.click("Set project tempo from clip");
    let project = lumen.app().project();
    assert!((bpm_at(&project.tempo, Ticks::ZERO) - found).abs() < 1e-6);
    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(bpm_at(&lumen.app().project().tempo, Ticks::ZERO), 120.0);

    // Warp off: the clip plays at its own speed again.
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    assert!(clip(&lumen, id).warp.is_none());
    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(clip(&lumen, id).warp.is_some(), "one undo turns it back on");
}

#[test]
fn warp_from_here_puts_the_sound_at_the_cursor_on_the_bar_line() {
    let (mut lumen, id) = opened_click_track("warp-here");
    // The first click is at 0.25 s, half a beat in at 120 BPM. A click on the
    // wave places the cursor on the grid there.
    let s = shown(&lumen);
    let at = Pos2::new(x_of(&lumen, 0.5) + 2.0, s.body.center().y);
    lumen.harness.hover_at(at);
    lumen.frames(1);
    lumen.click_at(at, Modifiers::NONE);
    lumen.frames(3);
    assert_eq!(
        lumen.app().audio_editor_cursor(),
        Some(Ticks(PPQ / 2)),
        "the cursor is half a beat in"
    );

    open_chip(&mut lumen);
    lumen.click("Warp from here");
    let c = warped(&lumen, id);
    let source_at_start = c.warp.as_ref().expect("warped").source_at(Ticks::ZERO);
    assert!(
        (source_at_start - 0.25).abs() < 0.003,
        "the first click now plays at the clip's start: {source_at_start}"
    );
    close_chip(&mut lumen);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert!(
        clip(&lumen, id).warp.is_none(),
        "warping and moving it was one step"
    );
}

#[test]
fn a_locked_clip_cannot_be_warped_from_the_chip() {
    let (mut lumen, id) = opened_click_track("warp-locked");
    lumen.click("Lock clip");
    assert!(clip(&lumen, id).locked);
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    assert!(
        clip(&lumen, id).warp.is_none(),
        "a locked clip stays as it is"
    );
}

#[test]
fn at_the_narrowest_window_warp_is_a_page_of_the_more_menu() {
    let (mut lumen, id) = opened_click_track("warp-folded");
    lumen.harness.set_size(vec2(480.0, 540.0));
    lumen.app_mut().set_browser_size(220.0);
    lumen.frames(20);
    assert_eq!(shown(&lumen).strip, "fold fades", "the narrowest window");
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Warp: ")
            .next()
            .is_none(),
        "the chip is folded away"
    );

    lumen.click("More clip properties");
    lumen.click("Warp\u{2026}");
    lumen.click("Warp clip");
    assert!(clip(&lumen, id).warp.is_some(), "the page turned warp on");
    lumen.click("Back");
    assert!(
        lumen.shows("Warp\u{2026}"),
        "Back returns to the menu's rows"
    );
}

#[test]
fn a_reversed_clip_is_listened_to_as_it_plays() {
    // Review round 1: the transients were those of the forward file while the
    // editor drew and played the reversed twin.
    let (mut lumen, id) = opened_click_track("warp-reversed");
    let forward = lumen.app().audio_detection();
    let first = forward.first.expect("a transient");
    assert!((first - 0.25).abs() < 0.03, "{first}: the first click");
    assert_eq!(forward.file, Some(media_of(&lumen, id)));

    lumen.click("Reverse");
    assert!(clip(&lumen, id).reversed);
    lumen.wait_for("the reversed file's detection", |app| {
        let d = app.audio_detection();
        d.ready && d.file != forward.file
    });
    let backward = lumen.app().audio_detection();
    assert_eq!(
        backward.file,
        Some(lumen_app::testing::reversed_key(media_of(&lumen, id)))
    );
    let after = backward.first.expect("a transient in the twin");
    assert!(
        (after - first).abs() > 0.1,
        "the same place ({after}) in a file played backwards"
    );
}

#[test]
fn warp_from_here_uses_the_cursor_only_for_the_clip_the_editor_has_open() {
    // Review round 1: the command used the open clip's cursor for whichever
    // clip was selected.
    let (mut lumen, opened) = opened_click_track("warp-here-other");
    let s = shown(&lumen);
    let at = Pos2::new(x_of(&lumen, 0.5) + 2.0, s.body.center().y);
    lumen.harness.hover_at(at);
    lumen.frames(1);
    lumen.click_at(at, Modifiers::NONE);
    lumen.frames(3);
    assert_eq!(lumen.app().audio_editor_cursor(), Some(Ticks(PPQ / 2)));

    let other = click_track(&lumen.dir().join("other.wav"), (100.0, 16), 0.25, 9.85);
    lumen.drop_files(&[other]);
    lumen.frames(40);
    let second = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.id)
        .find(|id| *id != opened)
        .expect("the second file became a clip");
    assert_eq!(clip(&lumen, second).start, Ticks::ZERO);
    lumen.app_mut().select_clip(second);
    lumen.frames(3);

    command(&mut lumen, "Warp from here");
    let c = warped(&lumen, second);
    let w = c.warp.as_ref().expect("warped");
    assert!(
        w.source_at(Ticks::ZERO).abs() < 0.01,
        "the playhead, not the other clip's cursor, was here: {}",
        w.source_at(Ticks::ZERO)
    );
    assert!(
        clip(&lumen, opened).warp.is_none(),
        "the opened clip is as it was"
    );
}

/// The media a clip plays.
fn media_of(lumen: &Lumen, id: Id) -> u64 {
    match clip(lumen, id).content {
        lumen_project::model::ClipContent::Audio { media, .. } => media.0,
        lumen_project::model::ClipContent::Midi { .. } => panic!("an audio clip"),
    }
}

/// A project with a click track of whole bars (sixteen beats at 100 BPM) and
/// no tempo in its name or tags, dropped in.
fn dropped_loop(lumen: &mut Lumen) -> Id {
    let file = click_track(&lumen.dir().join("clicks.wav"), (100.0, 16), 0.0, 9.6);
    lumen.drop_files(&[file]);
    lumen.wait_for("the clip", |app| {
        app.project()
            .tracks
            .first()
            .is_some_and(|t| !t.clips.is_empty())
    });
    lumen.app().project().tracks[0].clips[0].id
}

#[test]
fn an_untagged_loop_of_whole_bars_follows_the_song_once_it_is_listened_to() {
    let mut lumen = Lumen::start("follow-detected");
    let id = dropped_loop(&mut lumen);
    lumen.wait_for("the loop to follow the song", |app| {
        app.project().tracks[0].clips[0].warp.is_some()
    });
    let c = warped(&lumen, id);
    assert!(
        (original_bpm(&c) - 100.0).abs() < 1e-6,
        "{}: the tempo of sixteen beats in 9.6 s",
        original_bpm(&c)
    );
    assert_eq!(
        c.length,
        Ticks::quarters(16),
        "as long in beats as the file"
    );
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.starts_with("Warped to follow the song")),
        "it says so: {:?}",
        lumen.app().note()
    );

    // One undo step lets it play at its own speed; the import stays.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    let plain = clip(&lumen, id);
    assert!(plain.warp.is_none());
    assert_eq!(
        plain.length,
        Ticks::quarters(19) + Ticks(PPQ / 5),
        "9.6 s at 120"
    );
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.frames(3);
    assert!(clip(&lumen, id).warp.is_some(), "redo follows again");
}

#[test]
fn a_loop_is_not_listened_to_when_the_setting_is_never() {
    let mut lumen = Lumen::start("follow-detected-never");
    lumen.open_preferences("Editing");
    lumen.click("New audio clips: never follow the tempo");
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    let id = dropped_loop(&mut lumen);
    lumen.frames(30);
    assert_eq!(lumen.app().follow_listening(), 0, "nothing to listen to");
    assert!(clip(&lumen, id).warp.is_none());
}

/// Plays four seconds from the start and gives the seconds between the clicks
/// heard.
fn click_gaps(lumen: &mut Lumen) -> Vec<f64> {
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.listen(std::time::Duration::from_millis(0), 4 * 48_000);
    lumen.key(Modifiers::NONE, Key::Space);
    let mut onsets: Vec<usize> = Vec::new();
    for (i, f) in heard.iter().enumerate() {
        if f[0].abs() > 0.1 && onsets.last().is_none_or(|l| i - l > 12_000) {
            onsets.push(i);
        }
    }
    assert!(onsets.len() >= 5, "clicks heard: {onsets:?}");
    onsets
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / 48_000.0)
        .collect()
}

#[test]
fn a_warped_loop_is_heard_on_the_songs_beat() {
    // Sixteen clicks 0.6 s apart (100 BPM) in a song at 120 BPM: warped, they
    // must be heard every 0.5 s.
    // Warped clips play from the disk thread's streams, as the app does by
    // default (the test harness turns "Play audio from disk" off otherwise).
    let mut lumen = Lumen::start_with(TempDir::new("e2e-warp-heard"), |launch| {
        launch.playing_from_disk()
    });
    let id = dropped_loop(&mut lumen);
    lumen.wait_for("the loop to follow the song", |app| {
        app.project().tracks[0].clips[0].warp.is_some()
    });
    assert!((original_bpm(&clip(&lumen, id)) - 100.0).abs() < 1e-6);
    lumen.frames(10);
    for gap in click_gaps(&mut lumen) {
        assert!((gap - 0.5).abs() < 0.02, "{gap} s between clicks");
    }
}

/// The owner's report (2026-11-08): the wave was drawn stretched and the clip
/// was heard as recorded, because their settings kept "Play audio from disk"
/// off. The popover says so, and one click makes the clip follow the song.
#[test]
fn without_disk_playback_a_warped_clip_is_heard_unwarped_until_it_is_turned_on() {
    let mut lumen = Lumen::start("warp-heard-without-disk");
    let id = dropped_loop(&mut lumen);
    lumen.wait_for("the loop to follow the song", |app| {
        app.project().tracks[0].clips[0].warp.is_some()
    });
    lumen.frames(10);
    for gap in click_gaps(&mut lumen) {
        assert!((gap - 0.6).abs() < 0.02, "{gap} s: heard as recorded");
    }

    open(&mut lumen, id);
    open_chip(&mut lumen);
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Play audio from disk is off")
            .next()
            .is_some(),
        "the popover says why"
    );
    lumen.click("Turn on Play audio from disk");
    lumen.wait_for("the setting", |app| app.plays_from_disk());
    close_chip(&mut lumen);
    lumen.frames(30);
    for gap in click_gaps(&mut lumen) {
        assert!((gap - 0.5).abs() < 0.02, "{gap} s: on the song's beat");
    }
}
