//! Hearing and window in the audio editor (M9.5 S2c, LC A, D22, D25): Loop
//! clip repeats one pass of the clip, or the range picked on it, without
//! touching the project's loop or the undo history; it lets go when another
//! clip opens or recording starts; the editor's ruler clicks, scrubs and
//! pauses Follow at the first move, and takes Left and Right from the
//! keyboard; the editor pops out into its own window and docks back. The
//! bench at the end times the editor on a one-hour file.

use eframe::egui::{Key, Modifiers, Pos2};
use lumen_core::time::{PPQ, Ticks};

use super::audio_editor_handles::{clip, open, opened, shown, to_the_detail_panel, x_of};
use super::{Lumen, midi_play, wav};

fn q(beats: f64) -> Ticks {
    Ticks((beats * PPQ as f64).round() as i64)
}

/// The ruler, at beat `q`: just above the loop brace's band.
fn on_ruler(lumen: &Lumen, beats: f64) -> Pos2 {
    Pos2::new(x_of(lumen, beats), shown(lumen).band.top() - 5.0)
}

fn loop_clip(lumen: &mut Lumen) {
    midi_play::palette(lumen, "Loop clip");
    lumen.frames(3);
}

#[test]
fn loop_clip_repeats_one_pass_and_leaves_the_projects_loop_and_the_history_alone() {
    // The clip plays beats 1 to 3.
    let (mut lumen, _id) = opened("editor-loop-clip");
    let project_loop = lumen.app().engine_loop();
    let undo = lumen.app().undo_label().map(str::to_owned);
    assert_eq!(lumen.app().clip_loop_played(), None);

    loop_clip(&mut lumen);
    assert_eq!(lumen.app().clip_loop_played(), Some((q(1.0), q(3.0))));
    assert_eq!(
        lumen.app().engine_loop(),
        project_loop,
        "the project's loop"
    );
    assert_eq!(lumen.app().undo_label().map(str::to_owned), undo, "no step");
    assert_eq!(
        lumen.app().playhead(),
        q(1.0),
        "the playhead goes to the start of the pass"
    );

    loop_clip(&mut lumen);
    assert_eq!(lumen.app().clip_loop_played(), None, "toggled off");
    assert_eq!(lumen.app().engine_loop(), project_loop);
}

#[test]
fn loop_clip_repeats_the_picked_range_when_there_is_one() {
    let (mut lumen, _id) = opened("editor-loop-range");
    let y = shown(&lumen).body.center().y;
    lumen.drag(
        Pos2::new(x_of(&lumen, 1.5), y),
        Pos2::new(x_of(&lumen, 2.5), y),
        Modifiers::NONE,
    );
    loop_clip(&mut lumen);
    assert_eq!(lumen.app().clip_loop_played(), Some((q(1.5), q(2.5))));
    // Letting go of the range goes back to the whole pass.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    assert_eq!(lumen.app().clip_loop_played(), Some((q(1.0), q(3.0))));
}

#[test]
fn loop_clip_lets_go_when_another_clip_opens() {
    let (mut lumen, id) = opened("editor-loop-lets-go");
    loop_clip(&mut lumen);
    assert!(lumen.app().clip_loop_played().is_some());

    // A second file lands as another clip; opening it ends the override.
    let file = wav(&lumen.dir().join("snare.wav"), 1.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    let other = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.id)
        .find(|c| *c != id)
        .expect("a second clip");
    open(&mut lumen, other);
    lumen.frames(3);
    assert_eq!(
        lumen.app().clip_loop_played(),
        None,
        "Loop clip is for the clip it was turned on for"
    );
}

#[test]
fn a_clip_that_loops_repeats_one_turn_of_its_loop() {
    // The clip plays beats 1 to 3; its loop is shortened by half a beat.
    let (mut lumen, id) = opened("editor-loop-turn");
    let y = shown(&lumen).band.center().y;
    lumen.click_at(Pos2::new(x_of(&lumen, 2.0), y), Modifiers::NONE);
    let (_, end) = shown(&lumen).brace.expect("the brace shows its ends");
    let grip = Pos2::new(end - 1.0, y);
    let to = Pos2::new(grip.x - (x_of(&lumen, 2.0) - x_of(&lumen, 1.5)), y);
    lumen.drag(grip, to, Modifiers::NONE);
    let turn = clip(&lumen, id)
        .loop_length
        .filter(|l| *l > Ticks::ZERO)
        .expect("the clip loops");
    loop_clip(&mut lumen);
    let (from, to) = lumen.app().clip_loop_played().expect("looped");
    assert_eq!(from, q(1.0));
    assert!(to - from <= Ticks::quarters(2), "no more than the clip");
    assert!(
        to - from <= turn + Ticks(PPQ / 100),
        "one turn of the loop, not the clip: {from:?} to {to:?} for {turn:?}"
    );
}

#[test]
fn a_click_on_the_ruler_moves_the_playhead_and_a_drag_pauses_follow() {
    let (mut lumen, _id) = opened("editor-ruler-scrub");
    lumen.app_mut().resume_follow();
    lumen.harness.hover_at(on_ruler(&lumen, 2.0));
    lumen.harness.drag_at(on_ruler(&lumen, 2.0));
    lumen.frames(1);
    lumen.harness.drop_at(on_ruler(&lumen, 2.0));
    lumen.wait_for("the playhead at the click", |app| app.playhead() == q(2.0));
    assert!(lumen.app().follow_paused(), "a click pauses Follow");

    lumen.app_mut().resume_follow();
    let (a, b) = (on_ruler(&lumen, 1.0), on_ruler(&lumen, 2.4));
    lumen.harness.hover_at(a);
    lumen.harness.drag_at(a);
    lumen.frames(2);
    for step in 1..=6 {
        lumen.harness.hover_at(a + (b - a) * (step as f32 / 6.0));
        lumen.frames(1);
        assert!(lumen.app().follow_paused(), "paused from the first move");
        assert!(!lumen.app().is_playing(), "scrubbing is not playing");
    }
    lumen.harness.drop_at(b);
    lumen.frames(3);
    lumen.wait_for("the playhead where the step began", |app| {
        let t = app.playhead();
        t >= q(2.0) && t <= q(2.5)
    });
}

#[test]
fn the_ruler_takes_left_and_right_from_the_keyboard() {
    let (mut lumen, _id) = opened("editor-ruler-keys");
    lumen.app_mut().locate_to(q(2.0));
    lumen.frames(3);
    to_the_detail_panel(&mut lumen);
    let ctx = lumen.harness.ctx.clone();
    for _ in 0..90 {
        if lumen.app().audio_stop_focused(&ctx) == Some("ruler") {
            break;
        }
        lumen.key(Modifiers::NONE, Key::Tab);
    }
    assert_eq!(lumen.app().audio_stop_focused(&ctx), Some("ruler"));
    let before = lumen.app().playhead();
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    lumen.wait_for("the playhead to move on", move |app| {
        app.playhead() > before
    });
    let after = lumen.app().playhead();
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    lumen.wait_for("the playhead to move back", move |app| {
        app.playhead() < after
    });
}

#[test]
fn pop_out_puts_the_editor_in_its_own_window_and_dock_brings_it_back() {
    let (mut lumen, _id) = opened("editor-pop-out");
    assert!(!lumen.app().audio_editor_popped());
    midi_play::palette(&mut lumen, "Pop out");
    lumen.frames(5);
    assert!(lumen.app().audio_editor_popped(), "popped out");
    midi_play::palette(&mut lumen, "Pop out");
    lumen.frames(5);
    assert!(!lumen.app().audio_editor_popped(), "docked again");
}

#[test]
fn loop_clip_lets_go_when_recording_starts_and_does_not_come_back() {
    let (mut lumen, _id) = opened("editor-loop-record");
    loop_clip(&mut lumen);
    assert!(lumen.app().clip_loop_played().is_some());
    let name = lumen.app().project().tracks[0].name.clone();
    lumen.click(&format!("Arm {name}"));
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("recording", |app| app.recording());
    // The engine was told the project's loop no later than the take began
    // (the clip's loop must not wrap the recording).
    assert_eq!(
        lumen.app().engine_loop_sent(),
        lumen.app().engine_loop(),
        "the engine plays the project's loop from the first frame of the take"
    );
    lumen.frames(3);
    assert_eq!(lumen.app().clip_loop_played(), None, "recording ends it");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the take to stop", |app| !app.recording());
    lumen.frames(5);
    assert_eq!(
        lumen.app().clip_loop_played(),
        None,
        "it is turned on again by hand"
    );
}

/// The bench of M9.5 S2c: the editor's frame time on a one-hour file at
/// every zoom, from the whole hour to the deepest view. It prints a table
/// (`cargo test -p lumen-app --test e2e editor_frame_time -- --ignored
/// --nocapture`) and fails only when a frame takes over a second, which no
/// machine should. ADR-0127 records the numbers.
#[test]
#[ignore = "a bench: prints frame times"]
fn editor_frame_time_on_a_one_hour_file_at_every_zoom() {
    use std::time::Instant;
    let mut lumen = Lumen::start("editor-bench");
    // One hour of mono at the project's rate: the length is what the editor
    // sizes itself by, and a file of another rate would be converted in
    // memory.
    let file = hour_of_mono(&lumen.dir().join("hour.wav"));
    lumen.drop_files(&[file]);
    lumen.wait_for("the clip", |app| !app.project().tracks[0].clips.is_empty());
    lumen.frames(40);
    let id = lumen.app().project().tracks[0].clips[0].id;
    open(&mut lumen, id);
    lumen.frames(20);
    let p = shown(&lumen).body.center();
    lumen.harness.hover_at(p);
    lumen.frames(5);
    let s = shown(&lumen);
    println!(
        "file {:?} frames, drawn from samples: {}",
        s.file, s.samples
    );
    println!("zoom (points per beat)  ms per frame");
    let mut worst = 0.0_f64;
    for _ in 0..14 {
        let zoom = shown(&lumen).view.zoom;
        let started = Instant::now();
        let n = 20;
        lumen.frames(n);
        let ms = started.elapsed().as_secs_f64() * 1000.0 / n as f64;
        worst = worst.max(ms);
        println!("{zoom:>22.2}  {ms:>9.2}");
        for _ in 0..4 {
            lumen.key(Modifiers::NONE, Key::Plus);
        }
        lumen.harness.hover_at(p + eframe::egui::vec2(1.0, 0.0));
        lumen.frames(3);
    }
    println!("worst: {worst:.2} ms");
    assert!(worst < 1000.0, "a frame took {worst} ms");
}

/// A one-hour mono 16-bit file at 48 kHz, written in chunks.
fn hour_of_mono(path: &std::path::Path) -> std::path::PathBuf {
    use std::io::Write;
    let frames: u64 = 3_600 * 48_000;
    let bytes = (frames * 2) as u32;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).expect("create"));
    let mut head = Vec::new();
    head.extend(b"RIFF");
    head.extend((36 + bytes).to_le_bytes());
    head.extend(b"WAVEfmt ");
    head.extend(16_u32.to_le_bytes());
    head.extend(1_u16.to_le_bytes());
    head.extend(1_u16.to_le_bytes());
    head.extend(48_000_u32.to_le_bytes());
    head.extend(96_000_u32.to_le_bytes());
    head.extend(2_u16.to_le_bytes());
    head.extend(16_u16.to_le_bytes());
    head.extend(b"data");
    head.extend(bytes.to_le_bytes());
    out.write_all(&head).expect("head");
    let mut chunk = Vec::with_capacity(96_000);
    for start in (0..frames).step_by(48_000) {
        chunk.clear();
        for i in start..start + 48_000 {
            let v = ((i as f32 * 0.05).sin() * 3_000.0) as i16;
            chunk.extend(v.to_le_bytes());
        }
        out.write_all(&chunk).expect("chunk");
    }
    out.flush().expect("flush");
    path.to_path_buf()
}

#[test]
fn hiding_the_detail_panel_lets_go_of_loop_clip_and_of_a_scrub_in_progress() {
    let (mut lumen, _id) = opened("editor-hidden");
    loop_clip(&mut lumen);
    assert!(lumen.app().clip_loop_played().is_some());

    // A scrub begins; the panel is hidden under the held pointer.
    let (a, b) = (on_ruler(&lumen, 1.0), on_ruler(&lumen, 1.6));
    lumen.harness.hover_at(a);
    lumen.harness.drag_at(a);
    lumen.frames(2);
    lumen.harness.hover_at(b);
    lumen.frames(2);
    assert!(lumen.app().audio_editor_scrubbing(), "scrubbing");
    midi_play::palette(&mut lumen, "hide detail panel");
    lumen.frames(6);
    assert!(!lumen.app().detail_shown(), "the panel is hidden");
    lumen.wait_for("the scrub to be let go of", |app| {
        !app.audio_editor_scrubbing()
    });
    assert_eq!(lumen.app().clip_loop_played(), None, "Loop clip is off");
    lumen.harness.drop_at(b);
    lumen.frames(3);

    // Showing the panel again does not bring the loop back.
    midi_play::palette(&mut lumen, "hide detail panel");
    lumen.frames(6);
    assert_eq!(lumen.app().clip_loop_played(), None);
}

#[test]
fn the_engine_plays_the_pass_on_repeat_and_the_projects_loop_is_still_off() {
    use std::cell::Cell;
    let (mut lumen, _id) = opened("editor-loop-engine");
    let (_, _, enabled) = lumen.app().engine_loop();
    assert!(!enabled, "the project's own loop is off");
    loop_clip(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());

    // The playhead goes round inside beats 1 to 3 and never past.
    let high = Cell::new(Ticks::ZERO);
    let wrapped = Cell::new(false);
    lumen.wait_for("the playhead to wrap", |app| {
        let t = app.playhead();
        if t > high.get() {
            high.set(t);
        } else if t + q(1.0) < high.get() {
            wrapped.set(true);
        }
        wrapped.get()
    });
    assert!(
        high.get() <= q(3.5),
        "the engine looped the pass, not the song: reached {:?}",
        high.get()
    );
    assert!(lumen.app().is_playing(), "and kept playing");
    let (_, _, enabled) = lumen.app().engine_loop();
    assert!(!enabled, "the project's loop was not turned on");
}

#[test]
fn the_clip_loop_command_on_a_locked_clip_changes_nothing_and_says_why() {
    let (mut lumen, id) = opened("editor-clip-loop-locked");
    lumen.right_click(Pos2::new(x_of(&lumen, 2.0), shown(&lumen).body.center().y));
    lumen.click("Lock");
    assert!(clip(&lumen, id).locked);
    let before = clip(&lumen, id);
    let undo = lumen.app().undo_label().map(str::to_owned);
    midi_play::palette(&mut lumen, "Loop audio in clip");
    lumen.frames(3);
    assert_eq!(clip(&lumen, id), before, "the locked clip is as it was");
    assert_eq!(lumen.app().undo_label().map(str::to_owned), undo, "no step");
    let note = lumen.app().note().unwrap_or_default().to_owned();
    assert!(note.contains("locked"), "{note}");
}

#[test]
fn a_range_on_another_track_does_not_narrow_loop_clip() {
    let (mut lumen, id) = opened("editor-loop-other-track");
    midi_play::palette(&mut lumen, "Add audio track");
    lumen.frames(30);
    let own = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == id))
        .map(|t| t.id)
        .expect("the clip's track");
    let other = lumen
        .app()
        .project()
        .tracks
        .iter()
        .map(|t| t.id)
        .find(|t| *t != own)
        .expect("another track");
    lumen.app_mut().pick_time_range(q(1.5), q(2.5), &[other]);
    loop_clip(&mut lumen);
    assert!(
        lumen.app().time_range().is_some(),
        "the range on the other track stands, so the check below is not vacuous"
    );
    assert_eq!(
        lumen.app().clip_loop_played(),
        Some((q(1.0), q(3.0))),
        "a range picked elsewhere is not the clip's"
    );
}

#[test]
fn opening_another_clip_under_a_held_scrub_ends_the_scrub() {
    let (mut lumen, id) = opened("editor-scrub-switch");
    let file = wav(&lumen.dir().join("snare.wav"), 1.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    let other = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.id)
        .find(|c| *c != id)
        .expect("a second clip");
    open(&mut lumen, id);
    let (a, b) = (on_ruler(&lumen, 1.0), on_ruler(&lumen, 1.6));
    lumen.harness.hover_at(a);
    lumen.harness.drag_at(a);
    lumen.frames(2);
    lumen.harness.hover_at(b);
    lumen.frames(2);
    assert!(lumen.app().audio_editor_scrubbing(), "scrubbing");
    open(&mut lumen, other);
    lumen.wait_for("the scrub to end with the clip", |app| {
        !app.audio_editor_scrubbing()
    });
    lumen.harness.drop_at(b);
    lumen.frames(3);
}
