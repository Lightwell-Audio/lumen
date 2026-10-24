//! M7b E1: join, nudge, crop, unloop and the selection variants, driven by
//! keys, the palette and the clip menu, each one undo step.

use eframe::egui::{Event, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};

use super::{Lumen, wav};

fn command(lumen: &mut Lumen, name: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(!lumen.app().palette_open());
}

fn audio(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("take.wav"), 4.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    lumen
}

fn clip_rect(lumen: &Lumen) -> Rect {
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app().clip_rect(&lumen.harness.ctx, id).unwrap()
}

fn spot(lumen: &Lumen, quarter: f64) -> Pos2 {
    let rect = clip_rect(lumen);
    Pos2::new(
        rect.left() + quarter as f32 * lumen.app().view().zoom as f32,
        rect.bottom() - 6.0,
    )
}

fn click_at(lumen: &mut Lumen, at: Pos2) {
    lumen.harness.hover_at(at);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(3);
}

/// Cuts the clip under `quarter` there: a click puts the playhead and the
/// selection there, Ctrl+E cuts.
fn cut_at(lumen: &mut Lumen, quarter: i64) {
    let clips = lumen.app().project().tracks[0].clips.clone();
    let zoom = lumen.app().view().zoom as f32;
    let clip = clips
        .iter()
        .find(|c| {
            c.start < Ticks::quarters(quarter) && c.start + c.length > Ticks::quarters(quarter)
        })
        .expect("a clip under the cut");
    let rect = lumen.app().clip_rect(&lumen.harness.ctx, clip.id).unwrap();
    let from_start = (Ticks::quarters(quarter) - clip.start).0 as f32 / PPQ as f32;
    let at = Pos2::new(rect.left() + from_start * zoom, rect.bottom() - 6.0);
    click_at(lumen, at);
    lumen.wait_for("the edit position", |app| {
        app.playhead() == Ticks::quarters(quarter)
    });
    lumen.key(Modifiers::COMMAND, Key::E);
}

/// An audio clip cut at two and four quarters, all three selected.
fn three_parts(name: &str) -> Lumen {
    let mut lumen = audio(name);
    cut_at(&mut lumen, 2);
    cut_at(&mut lumen, 4);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 3);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen
}

#[test]
fn ctrl_j_joins_what_a_split_made_and_undo_gives_the_parts_back() {
    let mut lumen = audio("join-roundtrip");
    let original = lumen.app().project().tracks[0].clips.clone();
    let id = original[0].id;
    cut_at(&mut lumen, 2);
    let parts = lumen.app().project().tracks[0].clips.clone();
    assert_eq!(parts.len(), 2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.key(Modifiers::COMMAND, Key::J);
    assert_eq!(lumen.app().project().tracks[0].clips, original);
    assert_eq!(lumen.app().selected_clips(), [id]);
    assert_eq!(lumen.app().undo_label(), Some("Join clips"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips, parts);
}

#[test]
fn join_needs_two_clips_and_apart_audio_is_pointed_to_consolidate() {
    let mut lumen = audio("join-apart");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    let before = lumen.app().project().clone();
    lumen.key(Modifiers::COMMAND, Key::J);
    assert_eq!(lumen.app().project(), &before, "one clip: nothing to join");

    cut_at(&mut lumen, 2);
    // Slide the second part away: the parts no longer meet.
    let second = lumen.app().project().tracks[0].clips[1].id;
    lumen.app_mut().select_clip(second);
    lumen.key(Modifiers::ALT, Key::ArrowRight);
    lumen.key(Modifiers::COMMAND, Key::A);
    let apart = lumen.app().project().clone();
    lumen.key(Modifiers::COMMAND, Key::J);
    assert_eq!(lumen.app().project(), &apart);
    assert_eq!(
        lumen.app().note(),
        Some("These clips do not continue one another: use Consolidate")
    );
}

#[test]
fn alt_arrows_nudge_selected_clips_by_a_grid_step_and_alt_shift_finely() {
    let mut lumen = audio("nudge");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    let start = lumen.app().project().tracks[0].clips[0].start;
    lumen.key(Modifiers::ALT, Key::ArrowRight);
    let after = lumen.app().project().tracks[0].clips[0].start;
    assert!(after > start, "moved later by the grid step");
    let step = after - start;
    lumen.key(Modifiers::ALT, Key::ArrowLeft);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, start);
    assert_eq!(lumen.app().undo_label(), Some("Nudge clip"));

    let fine = Modifiers::ALT | Modifiers::SHIFT;
    lumen.key(fine, Key::ArrowRight);
    let moved = lumen.app().project().tracks[0].clips[0].start - start;
    assert_eq!(moved, Ticks(PPQ / 32));
    assert!(moved < step);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, start);
}

#[test]
fn a_clip_at_the_start_does_not_nudge_earlier_and_adds_no_undo_step() {
    let mut lumen = audio("nudge-start");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    let label = lumen.app().undo_label().map(str::to_owned);
    lumen.key(Modifiers::ALT, Key::ArrowLeft);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, Ticks::ZERO);
    assert_eq!(lumen.app().undo_label().map(str::to_owned), label);
}

#[test]
fn crop_to_selection_keeps_the_range_and_undo_restores_the_clip() {
    let mut lumen = audio("crop");
    let original = lumen.app().project().tracks[0].clips.clone();
    let from = spot(&lumen, 1.0);
    let to = spot(&lumen, 3.0);
    lumen.drag(from, to, Modifiers::NONE);
    let (a, b, _) = lumen.app().time_range().expect("a time range");
    command(&mut lumen, "Crop to selection");
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].start, clips[0].start + clips[0].length), (a, b));
    assert_eq!(lumen.app().undo_label(), Some("Crop to selection"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips, original);
}

#[test]
fn unloop_makes_real_clips_from_a_loop_and_undo_gives_the_loop_back() {
    let mut lumen = audio("unloop");
    lumen.key(Modifiers::COMMAND, Key::Num1);
    let zoom = lumen.app().view().zoom as f32;
    let r = clip_rect(&lumen);
    let end = Pos2::new(r.right() - 2.0, r.center().y);
    lumen.drag(end, end + vec2(4.0 * zoom, 0.0), Modifiers::SHIFT);
    let looped = lumen.app().project().tracks[0].clips[0].clone();
    assert!(
        looped.loop_length.is_some(),
        "the end was dragged as a loop"
    );
    let id = looped.id;
    lumen.app_mut().select_clip(id);
    command(&mut lumen, "Unloop");
    let clips = &lumen.app().project().tracks[0].clips;
    assert!(clips.len() >= 2, "one clip for each repeat");
    assert!(clips.iter().all(|c| c.loop_length.is_none()));
    assert_eq!(clips[0].id, id);
    let end_of = |c: &lumen_project::model::Clip| c.start + c.length;
    assert_eq!(end_of(clips.last().unwrap()), looped.start + looped.length);
    assert_eq!(lumen.app().undo_label(), Some("Unloop clips"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips[0], looped);
}

#[test]
fn the_selection_commands_pick_to_the_start_to_the_end_in_track_and_the_rest() {
    let mut lumen = three_parts("select-variants");
    let clips = lumen.app().project().tracks[0].clips.clone();
    let ids: Vec<_> = clips.iter().map(|c| c.id).collect();
    lumen.app_mut().select_clip(ids[1]);

    command(&mut lumen, "Select to end");
    assert_eq!(lumen.app().selected_clips(), [ids[1], ids[2]]);
    lumen.app_mut().select_clip(ids[1]);
    command(&mut lumen, "Select to start");
    assert_eq!(lumen.app().selected_clips(), [ids[0], ids[1]]);
    lumen.app_mut().select_clip(ids[0]);
    command(&mut lumen, "Select all in track");
    assert_eq!(lumen.app().selected_clips(), ids);
    lumen.app_mut().select_clip(ids[2]);
    command(&mut lumen, "Invert selection");
    assert_eq!(lumen.app().selected_clips(), [ids[0], ids[1]]);
}

#[test]
fn every_new_clip_command_is_in_the_palette() {
    let mut lumen = three_parts("clip-palette");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    for name in [
        "Nudge clips earlier",
        "Nudge clips later, finely",
        "Select to end",
        "Invert selection",
        "Unloop",
        "Crop to selection",
    ] {
        lumen.key(Modifiers::COMMAND, Key::K);
        lumen.harness.event(Event::Text(name.into()));
        lumen.frames(2);
        assert!(lumen.app().palette_open());
        lumen.key(Modifiers::NONE, Key::Escape);
    }
}

/// A WAV that rises from silence to a loud end over `seconds`.
fn ramp_wav(path: &std::path::Path, seconds: f32) -> std::path::PathBuf {
    let rate = 48_000_u32;
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let rise = i as f32 / frames as f32;
            let v = ((i as f32 * 0.05).sin() * 20_000.0 * rise) as i16;
            let [a, b] = v.to_le_bytes();
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
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the ramp WAV");
    path.to_path_buf()
}

fn ramp(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    let file = ramp_wav(&lumen.dir().join("ramp.wav"), 4.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    lumen
}

/// The loudest sample heard in the next 50 ms, after 50 ms of settling.
fn start_level(lumen: &mut Lumen) -> f32 {
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.listen(std::time::Duration::from_millis(50), 2_400);
    lumen.key(Modifiers::NONE, Key::Space);
    heard
        .iter()
        .flat_map(|f| [f[0].abs(), f[1].abs()])
        .fold(0.0, f32::max)
}

#[test]
fn a_file_that_rises_starts_quiet_and_a_reversed_clip_of_it_starts_loud() {
    let mut lumen = ramp("reverse-hear");
    let clip = lumen.app().project().tracks[0].clips[0].id;
    let media = match lumen.app().project().tracks[0].clips[0].content {
        lumen_project::model::ClipContent::Audio { media, .. } => media,
        _ => panic!("an audio clip"),
    };
    lumen.app_mut().select_clip(clip);
    let forward = start_level(&mut lumen);
    assert!(forward < 0.1, "the ramp begins quiet: {forward}");

    command(&mut lumen, "Reverse");
    assert!(lumen.app().project().tracks[0].clips[0].reversed);
    assert_eq!(lumen.app().undo_label(), Some("Reverse clips"));
    lumen.wait_for("the reversed copy", |app| app.has_reversed_waveform(media));
    let backward = start_level(&mut lumen);
    assert!(backward > 0.5, "the reversed ramp begins loud: {backward}");

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!lumen.app().project().tracks[0].clips[0].reversed);
    let again = start_level(&mut lumen);
    assert!(again < 0.1, "undo plays it forwards again: {again}");
}

#[test]
fn reverse_and_polarity_are_one_undo_step_each_and_need_an_audio_clip() {
    let mut lumen = ramp("flags-undo");
    let clip = lumen.app().project().tracks[0].clips[0].id;
    // Nothing selected: the commands are not available and change nothing.
    let before = lumen.app().project().clone();
    for name in ["Reverse", "Invert polarity"] {
        lumen.key(Modifiers::COMMAND, Key::K);
        lumen.harness.event(Event::Text(name.into()));
        lumen.frames(2);
        lumen.key(Modifiers::NONE, Key::Enter);
        lumen.key(Modifiers::NONE, Key::Escape);
    }
    assert_eq!(lumen.app().project(), &before);
    assert!(!lumen.app().palette_open());

    lumen.app_mut().select_clip(clip);
    command(&mut lumen, "Invert polarity");
    assert!(lumen.app().project().tracks[0].clips[0].inverted);
    assert_eq!(lumen.app().undo_label(), Some("Invert polarity"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!lumen.app().project().tracks[0].clips[0].inverted);

    command(&mut lumen, "Reverse");
    assert!(lumen.app().project().tracks[0].clips[0].reversed);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project(), &before);
}

fn gain_db(lumen: &Lumen) -> f32 {
    let lumen_project::model::ClipContent::Audio { gain_db, .. } =
        lumen.app().project().tracks[0].clips[0].content
    else {
        panic!("an audio clip")
    };
    gain_db
}

#[test]
fn normalize_raises_a_quiet_clip_to_full_scale_in_one_undo_step() {
    let mut lumen = ramp("normalize");
    let clip = lumen.app().project().tracks[0].clips[0].id;
    // Nothing selected: not available, nothing happens.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Normalize".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(gain_db(&lumen), 0.0);

    lumen.app_mut().select_clip(clip);
    command(&mut lumen, "Normalize");
    lumen.wait_for("the scan to apply its gain", |app| {
        app.undo_label() == Some("Normalize")
    });
    // The file's loudest sample is about 0.6 of full scale (20000 of
    // 32768, at the end of the rise): +4.3 dB.
    let db = gain_db(&lumen);
    assert!((db - 4.3).abs() < 0.3, "normalized to {db} dB");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(gain_db(&lumen), 0.0);
}

#[test]
fn a_midi_clips_menu_lists_no_audio_only_commands_and_an_audio_clips_does() {
    use lumen_project::testing::{TempDir, midi_track, project_of};
    // Audio: the rows are there.
    let mut lumen = ramp("menu-audio");
    let at = spot(&lumen, 2.0);
    lumen.right_click(at);
    lumen.frames(3);
    lumen.click("Edit");
    for row in ["Reverse", "Invert polarity", "Normalize"] {
        assert!(lumen.harness.query_by_label(row).is_some(), "audio: {row}");
    }

    // MIDI: left out, while the rest of the menu is whole.
    let dir = TempDir::new("e2e-menu-midi");
    let path = dir.path().join("song.lumen");
    let project = project_of(
        "Menu",
        vec![midi_track(1, "Keys", "poly", (0, 8), None, vec![], vec![])],
    );
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut midi = Lumen::start_with(dir, move |l| l.opening(path));
    midi.wait_for("the project", |app| app.project().tracks.len() == 1);
    midi.app_mut().set_view(lumen_app::TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    midi.frames(40);
    let clip = midi.app().project().tracks[0].clips[0].id;
    let r = midi
        .app()
        .clip_rect(&midi.harness.ctx.clone(), clip)
        .expect("the clip is in view");
    midi.right_click(r.center());
    midi.frames(3);
    midi.click("Edit");
    assert!(
        midi.harness.query_by_label_contains("Split").is_some(),
        "the menu is open"
    );
    for row in ["Reverse", "Invert polarity", "Normalize"] {
        assert!(midi.harness.query_by_label(row).is_none(), "MIDI: {row}");
    }
}

#[test]
fn a_note_is_read_in_full_by_wrapping_never_cut_short() {
    let mut lumen = Lumen::start("note-wraps");
    let area = |lumen: &Lumen| {
        lumen
            .harness
            .ctx
            .memory(|m| m.area_rect(eframe::egui::Id::new("toast area")))
            .expect("the note is shown")
    };
    lumen.app_mut().say_note("Saved");
    lumen.frames(20);
    let short = area(&lumen);
    let long = "These clips do not continue one another: use Consolidate, which closes \
                the gap with silence and writes one new clip";
    lumen.app_mut().say_note(long);
    lumen.frames(20);
    let wrapped = area(&lumen);
    assert!(
        wrapped.width() < 400.0,
        "the note keeps its width: {wrapped:?}"
    );
    assert!(
        wrapped.height() > short.height() * 1.8,
        "a long note takes more lines instead of an ellipsis: {short:?} against {wrapped:?}"
    );
}
