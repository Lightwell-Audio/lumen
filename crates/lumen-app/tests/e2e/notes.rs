//! The clip editor (M5 slice 3c): MIDI clips made and opened from the
//! arrangement, notes drawn, selected, moved, copied, resized and edited,
//! each one undo step; Live A (the document changes once, on release) and
//! Escape; keyboard-only use; the pop-out state. A real second window is
//! checked by hand (MT-MIDI-4): the headless harness embeds viewports.

use eframe::egui::{self, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_app::TimeView;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{ClipContent, Id, Note, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::{Lumen, keys};

const Q: i64 = PPQ;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

/// A MIDI track "Keys" with one clip from beat 0 to 8 holding four notes,
/// and an audio-less second MIDI track "Empty" with no clip.
fn project() -> Project {
    let notes = vec![
        Note::new(q(0), q(1), 60, 100),
        Note::new(q(1), q(1), 64, 100),
        Note::new(q(2), q(1), 67, 100),
        Note::new(q(4), q(2), 72, 100),
    ];
    let empty = Track::new(
        Id(11),
        "Empty",
        TrackKind::Midi,
        lumen_project::model::Color(0xC0504D),
    );
    project_of(
        "Notes",
        vec![
            midi_track(1, "Keys", "poly", (0, 8), None, notes, vec![]),
            empty,
        ],
    )
}

const KEYS_CLIP: Id = Id(3);

fn launch(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 2);
    lumen.app_mut().set_detail_size(460.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    lumen
}

/// Opens the Keys clip by double-clicking it, and lets the editor draw.
fn open(lumen: &mut Lumen) {
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, KEYS_CLIP)
        .expect("the clip is in view");
    lumen.double_click(r.center());
    lumen.frames(20);
    assert_eq!(lumen.app().editor_open_clip(), Some(KEYS_CLIP));
}

fn notes(lumen: &Lumen) -> Vec<Note> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == KEYS_CLIP)
        .map(|c| match &c.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => panic!("not MIDI"),
        })
        .expect("the clip")
}

/// The middle of a note's first half beat, in the row of its pitch.
fn at(lumen: &Lumen, start: Ticks, pitch: u8) -> Pos2 {
    lumen
        .app()
        .editor_point(start, pitch)
        .expect("the editor has drawn")
}

fn beat(n: f32) -> Ticks {
    Ticks((n * Q as f32) as i64)
}

/// A click without moving: press and release in one frame.
fn click(lumen: &mut Lumen, p: Pos2, modifiers: Modifiers) {
    lumen
        .harness
        .event(egui::Event::ModifiersChanged(modifiers));
    lumen.harness.hover_at(p);
    lumen.frames(2);
    for pressed in [true, false] {
        lumen.harness.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    lumen.frames(3);
    lumen
        .harness
        .event(egui::Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(2);
}

fn hover(lumen: &mut Lumen, p: Pos2) {
    lumen.harness.hover_at(p);
    lumen.frames(2);
}

fn undo(lumen: &mut Lumen) {
    lumen.key(Modifiers::COMMAND, Key::Z);
}

fn pick_all(lumen: &mut Lumen) {
    hover(lumen, at(lumen, beat(0.5), 64));
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(lumen.app().editor_picked(), [0, 1, 2, 3]);
}

#[test]
fn double_clicking_a_midi_clip_opens_it_in_the_clip_tab() {
    let mut lumen = launch("notes-open");
    assert!(!lumen.app().clip_tab_shown());
    open(&mut lumen);
    assert!(lumen.app().clip_tab_shown());
    assert!(lumen.shows("Note editor: Keys"));
    assert!(lumen.shows("Clip tab"));
    assert!(lumen.shows("Pointer tool") && lumen.shows("Draw tool"));
    assert!(lumen.shows("Pop out"));
}

#[test]
fn the_midi_clip_body_above_the_old_midpoint_selects_time_and_the_header_moves() {
    let mut lumen = launch("notes-header-body");
    // Fit tracks to the available lane height so the body has visible
    // space above the former midpoint even with compact default tracks.
    lumen.key(Modifiers::NONE, Key::H);
    lumen.frames(40);
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    // Just below the visible 15 pt title, still above the old midpoint.
    let y = r.top() + 20.0;
    assert!(y < r.center().y);
    lumen.drag(
        Pos2::new(r.left() + 60.0, y),
        Pos2::new(r.left() + 180.0, y),
        Modifiers::NONE,
    );
    let (start, end, _) = lumen.app().time_range().expect("the body selects time");
    assert_eq!((start, end), (q(1), q(3)));
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, q(0));

    let from = Pos2::new(r.left() + 60.0, r.top() + 14.0);
    lumen.drag(from, from + vec2(120.0, 0.0), Modifiers::NONE);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, q(2));
    assert!(lumen.app().time_range().is_none());
    assert_eq!(lumen.app().selected_clips(), [KEYS_CLIP]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, q(0));
}

#[test]
fn playing_double_click_opens_the_midi_editor_without_rewinding() {
    let mut lumen = launch("notes-open-playing");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback past the click position", |app| {
        app.is_playing() && app.playhead() > q(5)
    });
    let before = lumen.app().playhead();
    open(&mut lumen);
    lumen.listen(std::time::Duration::ZERO, 256);
    assert!(lumen.app().is_playing());
    assert!(lumen.app().playhead() >= before);
    assert_eq!(lumen.app().selected_clips(), [KEYS_CLIP]);
    assert!(lumen.shows("Note editor: Keys"));
}

#[test]
fn the_clip_tab_is_there_only_once_a_clip_is_opened() {
    // DP1: no tab opens onto an empty page.
    let mut lumen = launch("notes-empty");
    assert!(!lumen.shows("Clip tab"));
    assert_eq!(lumen.app().editor_open_clip(), None);
    open(&mut lumen);
    assert!(lumen.shows("Clip tab"));
    assert!(lumen.app().clip_tab_shown());
}

#[test]
fn double_clicking_an_empty_spot_of_a_midi_lane_makes_a_one_bar_clip_there() {
    let mut lumen = launch("notes-make");
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    // Beat 13, on the Keys track, past its clip: bar 4 (beats 12 to 16).
    let spot = Pos2::new(r.left() + 13.0 * 60.0, r.center().y);
    lumen.double_click(spot);
    lumen.frames(20);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start, q(12));
    assert_eq!(clips[1].length, q(4));
    assert_eq!(lumen.app().editor_open_clip(), Some(clips[1].id));
    assert!(lumen.app().clip_tab_shown());
    undo(&mut lumen);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1, "one step");
}

#[test]
fn the_create_command_makes_a_bar_at_the_playhead_on_the_selected_track() {
    let mut lumen = launch("notes-create");
    lumen.harness.get_by_label_contains("Track Empty,").click();
    lumen.frames(5);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    lumen.frames(10);
    let clips = &lumen.app().project().tracks[1].clips;
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].start, clips[0].length), (q(0), q(4)));
    assert_eq!(lumen.app().editor_open_clip(), Some(clips[0].id));
    undo(&mut lumen);
    assert!(lumen.app().project().tracks[1].clips.is_empty());
}

#[test]
fn the_create_command_uses_the_time_range_when_there_is_one() {
    let mut lumen = launch("notes-create-range");
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    // The body of a clip selects time: beat 1 to beat 3.
    let y = r.bottom() - 6.0;
    lumen.drag(
        Pos2::new(r.left() + 60.0, y),
        Pos2::new(r.left() + 180.0, y),
        Modifiers::NONE,
    );
    assert!(lumen.app().time_range().is_some(), "a range is selected");
    let (a, b, _) = lumen.app().time_range().unwrap();
    // The range is on the Keys track: the command makes its clip there.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    lumen.frames(10);
    let clips = &lumen.app().project().tracks[0].clips;
    let made = clips
        .iter()
        .find(|c| c.start == a)
        .expect("a clip at the range");
    assert_eq!(made.start + made.length, b);
}

/// ADR-0049 (owner, MT-MIDI-4 step 7): a drag in empty lane space selects
/// time on its track, and Create MIDI clip puts the clip on that track.
#[test]
fn create_after_an_empty_lane_drag_puts_the_clip_on_that_track() {
    let mut lumen = launch("notes-create-lane");
    let empty = lumen.app().project().tracks[1].id;
    let ctx = lumen.harness.ctx.clone();
    let zoom = lumen.app().view().zoom as f32;
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    // The Empty track has no clips: its lane is at the Keys clip's height
    // one row down, so take its y from its header.
    let y = lumen
        .harness
        .get_by_label_contains("Track Empty,")
        .rect()
        .center()
        .y;
    let x = |beat: f32| r.left() + beat * zoom;
    lumen.drag(
        Pos2::new(x(10.2), y),
        Pos2::new(x(14.1), y),
        Modifiers::NONE,
    );
    let (a, b, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!(tracks, [empty], "the lane it was dragged in");
    assert_eq!(lumen.app().selected_tracks(), [empty]);

    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    lumen.frames(10);
    let project = lumen.app().project();
    assert_eq!(project.tracks[0].clips.len(), 1, "Keys is untouched");
    let made = project.tracks[1]
        .clips
        .iter()
        .find(|c| c.start == a)
        .expect("the clip is on Empty, at the range");
    assert_eq!(made.start + made.length, b);
}

#[test]
fn a_click_with_the_draw_tool_adds_a_note_of_one_grid_step() {
    let mut lumen = launch("notes-draw");
    open(&mut lumen);
    lumen.key(Modifiers::NONE, Key::B);
    assert!(lumen.app().editor_draw_tool(), "B switches to Draw");
    let before = notes(&lumen).len();
    let p = at(&lumen, beat(6.3), 65);
    click(&mut lumen, p, Modifiers::NONE);
    let after = notes(&lumen);
    assert_eq!(after.len(), before + 1);
    let (cell, end) = lumen
        .app()
        .editor_grid_cell_at(beat(6.3))
        .expect("the editor has drawn");
    let drawn = after.iter().find(|n| n.pitch() == 65).expect("drawn");
    assert_eq!(drawn.start(), cell);
    assert_eq!(drawn.length(), end - cell);
    assert_eq!(
        lumen.app().editor_picked().len(),
        1,
        "the new note is picked"
    );
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), before, "one undo step");
}

#[test]
fn dragging_with_the_draw_tool_sets_the_length_of_the_new_note() {
    let mut lumen = launch("notes-draw-drag");
    open(&mut lumen);
    lumen.key(Modifiers::NONE, Key::B);
    let from = at(&lumen, beat(5.0) + Ticks(10), 62);
    let to = at(&lumen, beat(7.0), 62);
    lumen.drag(from, to, Modifiers::NONE);
    let drawn = notes(&lumen)
        .into_iter()
        .find(|n| n.pitch() == 62)
        .expect("drawn");
    assert_eq!(drawn.start(), q(5));
    assert_eq!(drawn.length(), q(2), "the end snapped to the grid");
    undo(&mut lumen);
    assert!(notes(&lumen).iter().all(|n| n.pitch() != 62));
}

#[test]
fn dragging_left_with_the_draw_tool_keeps_the_cell_pressed() {
    let mut lumen = launch("notes-draw-left");
    open(&mut lumen);
    lumen.key(Modifiers::NONE, Key::B);
    // Pressed in the grid cell at beat 6, dragged left to beat 4: the note
    // runs from the pointer's snapped place to the end of the pressed cell,
    // so the cell pressed is part of it.
    let (_, press_end) = lumen
        .app()
        .editor_grid_cell_at(beat(6.1))
        .expect("the editor has drawn");
    let from = at(&lumen, beat(6.1), 62);
    let to = at(&lumen, beat(4.0), 62);
    lumen.drag(from, to, Modifiers::NONE);
    let drawn = notes(&lumen)
        .into_iter()
        .find(|n| n.pitch() == 62)
        .expect("drawn");
    assert_eq!((drawn.start(), drawn.end()), (q(4), press_end));
    assert_eq!(
        lumen.app().editor_picked().len(),
        1,
        "the new note is picked"
    );
    undo(&mut lumen);
    assert!(
        notes(&lumen).iter().all(|n| n.pitch() != 62),
        "one undo step"
    );
}

#[test]
fn a_click_on_a_note_with_the_draw_tool_removes_it() {
    let mut lumen = launch("notes-draw-remove");
    open(&mut lumen);
    lumen.key(Modifiers::NONE, Key::B);
    let p = at(&lumen, beat(1.5), 64);
    click(&mut lumen, p, Modifiers::NONE);
    assert_eq!(notes(&lumen).len(), 3);
    assert!(notes(&lumen).iter().all(|n| n.pitch() != 64));
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), 4);
}

#[test]
fn clicking_a_note_picks_it_without_moving_the_playhead() {
    let mut lumen = launch("notes-pick");
    open(&mut lumen);
    let head = lumen.app().playhead();
    let p = at(&lumen, beat(1.5), 64);
    click(&mut lumen, p, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked(), [1]);
    assert_eq!(lumen.app().playhead(), head, "the transport stayed");
    // Shift adds, and picking on empty space clears.
    let p = at(&lumen, beat(2.5), 67);
    click(&mut lumen, p, Modifiers::SHIFT);
    assert_eq!(lumen.app().editor_picked(), [1, 2]);
    let empty = at(&lumen, beat(6.5), 62);
    click(&mut lumen, empty, Modifiers::NONE);
    assert!(lumen.app().editor_picked().is_empty());
}

#[test]
fn the_ruler_moves_the_playhead() {
    let mut lumen = launch("notes-ruler");
    open(&mut lumen);
    let body = lumen.app().editor_body().unwrap();
    let x = at(&lumen, q(2), 60).x;
    click(&mut lumen, Pos2::new(x, body.top() - 8.0), Modifiers::NONE);
    lumen.wait_for("the playhead", |app| app.playhead() == q(2));
}

#[test]
fn dragging_a_note_moves_it_snapped_in_one_undo_step() {
    let mut lumen = launch("notes-move");
    open(&mut lumen);
    let before = notes(&lumen);
    let from = at(&lumen, beat(1.5), 64);
    let to = at(&lumen, beat(3.5), 66);
    lumen.drag(from, to, Modifiers::NONE);
    let after = notes(&lumen);
    assert_eq!(after.len(), before.len());
    let moved = after.iter().find(|n| n.pitch() == 66).expect("moved");
    assert_eq!(moved.start(), q(3));
    assert_eq!(lumen.app().editor_picked().len(), 1);
    undo(&mut lumen);
    assert_eq!(notes(&lumen), before);
}

#[test]
fn live_a_the_document_changes_once_on_release() {
    let mut lumen = launch("notes-live");
    open(&mut lumen);
    let before = notes(&lumen);
    let history = lumen.app().undo_label().map(str::to_owned);
    let from = at(&lumen, beat(1.5), 64);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=6 {
        lumen
            .harness
            .hover_at(from + vec2(20.0 * step as f32, -16.0));
        lumen.frames(2);
        assert_eq!(notes(&lumen), before, "nothing changes while dragging");
        assert!(lumen.app().editor_gesture());
    }
    lumen.harness.drop_at(from + vec2(120.0, -16.0));
    lumen.frames(4);
    assert!(!lumen.app().editor_gesture());
    assert_ne!(notes(&lumen), before);
    assert_ne!(lumen.app().undo_label().map(str::to_owned), history);
}

#[test]
fn escape_cancels_a_drag_and_changes_nothing() {
    let mut lumen = launch("notes-cancel");
    open(&mut lumen);
    let before = notes(&lumen);
    let label = lumen.app().undo_label().map(str::to_owned);
    let from = at(&lumen, beat(1.5), 64);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    lumen.harness.hover_at(from + vec2(100.0, -20.0));
    lumen.frames(2);
    assert!(lumen.app().editor_gesture());
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().editor_gesture(), "Esc ended the drag");
    lumen.harness.drop_at(from + vec2(100.0, -20.0));
    lumen.frames(4);
    assert_eq!(notes(&lumen), before);
    assert_eq!(lumen.app().undo_label().map(str::to_owned), label);
}

#[test]
fn ctrl_dragging_copies_the_note() {
    let mut lumen = launch("notes-copy");
    open(&mut lumen);
    let from = at(&lumen, beat(0.5), 60);
    let to = at(&lumen, beat(5.5), 60);
    lumen.drag(from, to, Modifiers::COMMAND);
    let after = notes(&lumen);
    assert_eq!(after.len(), 5, "the original stays");
    assert!(after.iter().any(|n| n.start() == q(0) && n.pitch() == 60));
    assert!(after.iter().any(|n| n.start() == q(5) && n.pitch() == 60));
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), 4);
}

#[test]
fn dragging_a_note_s_ends_resizes_it() {
    let mut lumen = launch("notes-resize");
    open(&mut lumen);
    // The end of the note at beat 1 (E4), out to beat 3. The grab is two
    // points inside the end: the roll fits the clip, so a beat is wide.
    let on = at(&lumen, q(2), 64);
    let end = Pos2::new(on.x - 2.0, on.y);
    let to = at(&lumen, q(3), 64);
    lumen.drag(end, to, Modifiers::NONE);
    let n = notes(&lumen).into_iter().find(|n| n.pitch() == 64).unwrap();
    assert_eq!((n.start(), n.length()), (q(1), q(2)));
    undo(&mut lumen);
    // The start of the note at beat 2 (G4), back to beat 2.5.. snapped to 3
    // is past its end, so it stops at the minimum: drag it half a beat.
    let on = at(&lumen, q(2), 67);
    let start = Pos2::new(on.x + 2.0, on.y);
    let to = at(&lumen, beat(2.5), 67);
    lumen.drag(start, to, Modifiers::NONE);
    let n = notes(&lumen).into_iter().find(|n| n.pitch() == 67).unwrap();
    assert!(n.start() > q(2));
    assert_eq!(n.end(), q(3), "the end stayed");
}

#[test]
fn a_marquee_picks_the_notes_it_touches() {
    let mut lumen = launch("notes-marquee");
    open(&mut lumen);
    let from = at(&lumen, beat(-0.0) + Ticks(PPQ / 2), 70);
    let from = Pos2::new(from.x - 6.0, from.y);
    let to = at(&lumen, beat(3.2), 58);
    lumen.drag(from, to, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked(), [0, 1, 2]);
    assert_eq!(notes(&lumen).len(), 4, "a marquee changes nothing");
}

#[test]
fn delete_removes_the_picked_notes_and_undo_brings_them_back() {
    let mut lumen = launch("notes-delete");
    open(&mut lumen);
    let p = at(&lumen, beat(1.5), 64);
    click(&mut lumen, p, Modifiers::NONE);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(notes(&lumen).len(), 3);
    assert!(lumen.app().editor_picked().is_empty());
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), 4);
}

#[test]
fn the_edit_keys_each_make_one_undo_step() {
    let mut lumen = launch("notes-edits");
    open(&mut lumen);
    let original = notes(&lumen);

    // Mute (0), twice.
    pick_all(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(notes(&lumen).iter().all(|n| n.muted()));
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(notes(&lumen).iter().all(|n| !n.muted()));
    undo(&mut lumen);
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);

    // Transpose: Up, Shift+Up for an octave.
    pick_all(&mut lumen);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert_eq!(notes(&lumen)[0].pitch(), 61);
    undo(&mut lumen);
    pick_all(&mut lumen);
    lumen.key(Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(notes(&lumen)[0].pitch(), 48);
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);

    // Nudge by a grid step; Alt finely.
    pick_all(&mut lumen);
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    let step = {
        let (a, b) = lumen
            .app()
            .editor_grid_cell_at(Ticks::ZERO)
            .expect("the editor has drawn");
        b - a
    };
    assert_eq!(notes(&lumen)[0].start(), step);
    undo(&mut lumen);
    pick_all(&mut lumen);
    lumen.key(Modifiers::ALT, Key::ArrowRight);
    assert_eq!(notes(&lumen)[0].start(), Ticks(PPQ / 32));
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);

    // Duplicate puts copies after the originals and picks them.
    let first = at(&lumen, beat(0.5), 60);
    click(&mut lumen, first, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::D);
    assert_eq!(notes(&lumen).len(), 5);
    assert_eq!(lumen.app().editor_picked().len(), 1, "the copy is picked");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);

    // Legato stretches each note to the next one's start.
    pick_all(&mut lumen);
    lumen.key(Modifiers::NONE, Key::L);
    assert_eq!(notes(&lumen)[2].length(), q(2), "G4 reaches C5 at beat 4");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);
}

/// Moves the playhead by clicking the editor's ruler, above the roll.
fn playhead_to(lumen: &mut Lumen, t: Ticks) {
    let body = lumen.app().editor_body().unwrap();
    let x = at(lumen, t, 60).x;
    click(lumen, Pos2::new(x, body.top() - 8.0), Modifiers::NONE);
    // The playhead is the engine's, which a slow device moves a poll later.
    lumen.wait_for("the playhead", |app| app.playhead() == t);
}

/// The two halves of the C5 note (beats 4 to 6) after a split, in order.
fn c5_halves(lumen: &Lumen) -> Vec<Note> {
    let mut halves: Vec<Note> = notes(lumen)
        .into_iter()
        .filter(|n| n.pitch() == 72)
        .collect();
    halves.sort_by_key(|n| n.start());
    assert_eq!(halves.len(), 2, "the C5 note was cut in two");
    halves
}

#[test]
fn split_and_glue_cut_and_join_notes() {
    let mut lumen = launch("notes-split");
    open(&mut lumen);
    let original = notes(&lumen);
    // Pick the long note (C5, beats 4 to 6). The playhead is at beat 5, so
    // Ctrl+E cuts there, with the pointer resting on the note at beat 4.5.
    playhead_to(&mut lumen, q(5));
    let mid = at(&lumen, beat(4.5), 72);
    click(&mut lumen, mid, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::E);
    let after = notes(&lumen);
    assert_eq!(after.len(), 5);
    let halves = c5_halves(&lumen);
    assert_eq!(halves[0].end(), halves[1].start());
    assert_eq!(halves[0].end(), q(5), "cut at the playhead");
    assert_eq!(
        lumen.app().editor_picked().len(),
        2,
        "both halves stay picked"
    );
    // Glue joins them again.
    lumen.key(Modifiers::COMMAND, Key::J);
    assert_eq!(notes(&lumen), original);
    undo(&mut lumen);
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original);
}

#[test]
fn ctrl_e_cuts_the_picked_notes_at_the_playhead_not_under_the_pointer() {
    let mut lumen = launch("notes-split-playhead");
    open(&mut lumen);
    playhead_to(&mut lumen, q(5));
    let on_note = at(&lumen, beat(4.5), 72);
    click(&mut lumen, on_note, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked(), [3]);
    // The pointer moves over the roll at beat 4.25, somewhere else again.
    let elsewhere = at(&lumen, beat(4.25), 72);
    hover(&mut lumen, elsewhere);
    lumen.key(Modifiers::COMMAND, Key::E);
    let halves = c5_halves(&lumen);
    assert_eq!(halves[0].start(), q(4));
    assert_eq!(halves[0].end(), q(5), "cut at the playhead, beat 5");
    assert_eq!(halves[1].end(), q(6));
    assert_eq!(lumen.app().playhead(), q(5), "the playhead did not move");
}

#[test]
fn ctrl_e_cuts_at_the_pointer_when_the_playhead_is_outside_the_clip() {
    let mut lumen = launch("notes-split-pointer");
    open(&mut lumen);
    // The clip runs from beat 0 to 8: the playhead at beat 8 is past its end.
    playhead_to(&mut lumen, q(8));
    let on_note = at(&lumen, beat(4.5), 72);
    click(&mut lumen, on_note, Modifiers::NONE);
    hover(&mut lumen, on_note);
    lumen.key(Modifiers::COMMAND, Key::E);
    let halves = c5_halves(&lumen);
    let cut = halves[0].end().0;
    assert!(
        (cut - beat(4.5).0).abs() < Q / 4,
        "cut under the pointer, near beat 4.5, not at beat 8: {cut}"
    );
}

#[test]
fn split_here_in_the_menu_cuts_where_the_pointer_was_clicked() {
    let mut lumen = launch("notes-split-menu");
    open(&mut lumen);
    // The playhead is at beat 5; the menu's cut is where the pointer was.
    playhead_to(&mut lumen, q(5));
    let on_note = at(&lumen, beat(4.5), 72);
    lumen.right_click(on_note);
    assert_eq!(lumen.app().editor_picked(), [3]);
    lumen.harness.get_by_label_contains("Split here").click();
    lumen.frames(5);
    let halves = c5_halves(&lumen);
    let cut = halves[0].end().0;
    assert!(
        (cut - beat(4.5).0).abs() < Q / 4,
        "the menu cut near beat 4.5, not at the playhead: {cut}"
    );
}

#[test]
fn the_palette_runs_the_note_commands() {
    let mut lumen = launch("notes-palette");
    open(&mut lumen);
    pick_all(&mut lumen);
    for (query, check) in [("Transpose up an octave", 72u8), ("Transpose down", 71)] {
        lumen.key(Modifiers::COMMAND, Key::K);
        lumen.key(Modifiers::COMMAND, Key::A);
        lumen.harness.event(egui::Event::Text(query.into()));
        lumen.frames(2);
        lumen.key(Modifiers::NONE, Key::Enter);
        lumen.frames(2);
        assert_eq!(notes(&lumen)[0].pitch(), check, "{query}");
    }
}

#[test]
fn the_context_menu_acts_on_notes() {
    let mut lumen = launch("notes-menu");
    open(&mut lumen);
    let p = at(&lumen, beat(1.5), 64);
    lumen.right_click(p);
    assert_eq!(
        lumen.app().editor_picked(),
        [1],
        "the note was picked first"
    );
    assert!(lumen.shows(&keys("Duplicate Ctrl+D")));
    lumen.harness.get_by_label("Delete Delete").click();
    lumen.frames(3);
    assert_eq!(notes(&lumen).len(), 3);
    // The empty editor has its own menu.
    let empty = at(&lumen, beat(6.5), 62);
    lumen.right_click(empty);
    assert!(
        lumen
            .harness
            .query_by_label_contains("Select all")
            .is_some()
    );
}

#[test]
fn the_editor_works_from_the_keyboard_alone() {
    let mut lumen = launch("notes-keyboard");
    // Select the clip from the arrangement, open it with Enter.
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    click(&mut lumen, r.center(), Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [KEYS_CLIP]);
    // Move the pointer off everything, so only the keyboard decides.
    hover(&mut lumen, Pos2::new(2.0, 2.0));
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    assert_eq!(lumen.app().editor_open_clip(), Some(KEYS_CLIP));
    // F6 until the detail panel holds the keyboard.
    for _ in 0..2 {
        lumen.key(Modifiers::NONE, Key::F6);
    }
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(lumen.app().editor_picked(), [0, 1, 2, 3]);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert_eq!(notes(&lumen)[0].pitch(), 61);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert!(notes(&lumen).is_empty());
    undo(&mut lumen);
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), 4);
}

#[test]
fn popping_out_and_docking_back() {
    let mut lumen = launch("notes-pop");
    open(&mut lumen);
    assert!(!lumen.app().editor_popped());
    lumen.click("Pop out");
    lumen.frames(10);
    assert!(lumen.app().editor_popped());
    // The panel says where the editor went, and offers it back.
    assert!(lumen.shows("Dock editor"));
    // The editor works in the window (embedded here).
    let p = at(&lumen, beat(1.5), 64);
    click(&mut lumen, p, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked(), [1]);
    lumen.click("Dock editor");
    lumen.frames(5);
    assert!(!lumen.app().editor_popped());
    assert!(lumen.shows("Note editor: Keys"));
    // The command toggles too.
    lumen.key(
        Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT,
        Key::E,
    );
    assert!(lumen.app().editor_popped());
    lumen.key(
        Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT,
        Key::E,
    );
    assert!(!lumen.app().editor_popped());
}

/// PR 14: send keys only to the separate viewport, bypassing the main
/// window's input. Embedded pop-outs cannot catch this dispatch bug.
#[test]
fn the_native_editor_plays_computer_keyboard_notes_before_editing_shortcuts() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use lumen_core::midi::Msg;

    let mut lumen = launch("notes-native-keyboard");
    open(&mut lumen);
    let note = at(&lumen, beat(2.25), 67);
    click(&mut lumen, note, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::A);
    assert_eq!(lumen.app().editor_picked().len(), 4);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    lumen.click("Pop out");
    let original = notes(&lumen);
    lumen.app_mut().computer_keyboard_sent();

    let pending = Rc::new(RefCell::new(Vec::new()));
    let events = pending.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let mut input = egui::RawInput {
            viewport_id: viewport.ids.this,
            events: std::mem::take(&mut *events.borrow_mut()),
            ..Default::default()
        };
        input.viewports.insert(
            viewport.ids.this,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                focused: Some(true),
                ..Default::default()
            },
        );
        let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
        // This headless viewport exercises input and layout, without a GPU.
        output.textures_delta.clear();
    });
    lumen.harness.ctx.set_embed_viewports(false);
    lumen.frames(3);

    let letter = |pressed| egui::Event::Key {
        key: Key::L,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    pending.borrow_mut().push(letter(true));
    lumen.frames(2);
    assert_eq!(lumen.app().computer_keyboard_held(), [74]);
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_on(0, 74, 100)]
    );
    assert_eq!(
        notes(&lumen),
        original,
        "L must not run Legato in MIDI mode"
    );

    pending.borrow_mut().push(letter(false));
    lumen.frames(2);
    assert!(lumen.app().computer_keyboard_held().is_empty());
    assert_eq!(
        lumen.app_mut().computer_keyboard_sent(),
        [Msg::note_off(0, 74)]
    );

    // With MIDI mode off, the same native-window key is still a command.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    assert!(!lumen.app().computer_keyboard_on());
    pending.borrow_mut().push(letter(true));
    lumen.frames(2);
    assert_ne!(notes(&lumen), original, "L runs Legato outside MIDI mode");
    pending.borrow_mut().push(letter(false));
    lumen.frames(2);
}

#[test]
fn deleting_the_open_clip_closes_the_editor() {
    let mut lumen = launch("notes-gone");
    open(&mut lumen);
    // Back to the arrangement: select the clip and delete it.
    let ctx = lumen.harness.ctx.clone();
    let r = lumen.app().clip_rect(&ctx, KEYS_CLIP).unwrap();
    hover(&mut lumen, Pos2::new(2.0, 2.0));
    click(
        &mut lumen,
        Pos2::new(r.center().x, r.top() + 6.0),
        Modifiers::NONE,
    );
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(10);
    assert_eq!(lumen.app().editor_open_clip(), None);
}

#[test]
fn pressing_a_note_auditions_it_and_escape_lets_go() {
    let mut lumen = launch("notes-audition");
    open(&mut lumen);
    let p = at(&lumen, beat(1.5), 64);
    hover(&mut lumen, p);
    lumen.harness.drag_at(p);
    lumen.wait_for("the picked note sounds", |app| app.audition_sounding());
    assert!(lumen.app().editor_gesture());
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(2);
    assert!(!lumen.app().editor_gesture());
    assert!(
        !lumen.app().audition_sounding(),
        "Escape lets the preview go"
    );
    lumen.harness.drop_at(p);
    lumen.frames(3);
}

/// Turns the wheel `notches` at `p` (up is positive) with `modifiers` held,
/// then lets egui's smoothed scroll settle.
fn wheel(lumen: &mut Lumen, p: Pos2, notches: i32, modifiers: Modifiers) {
    hover(lumen, p);
    for _ in 0..notches.abs() {
        lumen.harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 40.0 * notches.signum() as f32),
            phase: egui::TouchPhase::Move,
            modifiers,
        });
        lumen.frames(2);
    }
    lumen.frames(30);
}

/// The x at which the roll draws clip time `local` (pitch 60's row).
fn roll_x(lumen: &Lumen, local: Ticks) -> f32 {
    lumen
        .app()
        .editor_point(local, 60)
        .expect("the roll has drawn")
        .x
}

#[test]
fn opening_a_clip_frames_it_in_the_roll() {
    let mut lumen = launch("notes-frame");
    open(&mut lumen);
    let body = lumen.app().editor_body().unwrap();
    let (start, end) = (roll_x(&lumen, Ticks::ZERO), roll_x(&lumen, q(8)));
    assert!(start >= body.left() && end <= body.right(), "{start} {end}");
    assert!(end - start > body.width() * 0.9, "the clip fills the roll");
    assert!(
        start - body.left() < body.width() * 0.05,
        "little room before"
    );
    assert!(
        body.right() - end < body.width() * 0.05,
        "little room after"
    );
}

#[test]
fn the_roll_zooms_and_scrolls_on_its_own() {
    let mut lumen = launch("notes-own-view");
    open(&mut lumen);
    let ctx = lumen.harness.ctx.clone();
    let lane = lumen
        .app()
        .clip_rect(&ctx, KEYS_CLIP)
        .expect("the clip is in view")
        .center();
    let body = lumen.app().editor_body().unwrap();
    let roll = Pos2::new(body.left() + body.width() * 0.8, body.center().y);
    let (arrangement0, roll0) = (lumen.app().view(), lumen.app().editor_view().unwrap());
    let x0 = roll_x(&lumen, beat(4.0));

    // Ctrl+wheel over the arrangement zooms the arrangement, and only it.
    wheel(&mut lumen, lane, 2, Modifiers::COMMAND);
    let arrangement1 = lumen.app().view();
    assert!(arrangement1.zoom > arrangement0.zoom, "{arrangement1:?}");
    assert_eq!(lumen.app().editor_view(), Some(roll0), "the roll stays");
    assert_eq!(roll_x(&lumen, beat(4.0)), x0, "its notes stay put");

    // Ctrl+wheel over the roll zooms the roll, and only it.
    wheel(&mut lumen, roll, 2, Modifiers::COMMAND);
    let roll1 = lumen.app().editor_view().unwrap();
    assert!(roll1.zoom > roll0.zoom, "{roll0:?} -> {roll1:?}");
    assert_eq!(lumen.app().view(), arrangement1, "the arrangement stays");
    assert!(
        (roll_x(&lumen, beat(4.0)) - x0).abs() > 1.0,
        "the notes moved"
    );

    // Shift+wheel scrolls the roll in time, and only it.
    wheel(&mut lumen, roll, 2, Modifiers::SHIFT);
    let roll2 = lumen.app().editor_view().unwrap();
    assert_ne!(roll2.left, roll1.left, "the roll scrolled");
    assert_eq!(lumen.app().view(), arrangement1, "the arrangement stays");

    // The zoom keys follow the pointer: over the roll they zoom the roll...
    hover(&mut lumen, roll);
    lumen.key(Modifiers::NONE, Key::Equals);
    lumen.frames(30);
    let roll3 = lumen.app().editor_view().unwrap();
    assert!(roll3.zoom > roll2.zoom, "+ zooms the roll");
    assert_eq!(lumen.app().view(), arrangement1, "the arrangement stays");

    // ...and over the arrangement they zoom the arrangement.
    hover(&mut lumen, lane);
    lumen.key(Modifiers::NONE, Key::Equals);
    lumen.frames(30);
    assert!(
        lumen.app().view().zoom > arrangement1.zoom,
        "+ zooms the arrangement"
    );
    assert_eq!(lumen.app().editor_view(), Some(roll3), "the roll stays");
}

#[test]
fn drawing_snaps_to_the_rolls_grid_not_the_arrangements() {
    let mut lumen = launch("notes-draw-roll-grid");
    open(&mut lumen);
    let body = lumen.app().editor_body().unwrap();
    let roll = Pos2::new(body.left() + body.width() * 0.8, body.center().y);
    // Zoom the roll in alone, so its automatic grid is finer than the
    // arrangement's.
    wheel(&mut lumen, roll, 6, Modifiers::COMMAND);
    let (a0, a1) = lumen.app().grid_cell_at(beat(6.1));
    let (r0, r1) = lumen.app().editor_grid_cell_at(beat(6.1)).unwrap();
    assert!(
        r1 - r0 < a1 - a0,
        "the roll's cells are finer: {r0:?} {a0:?}"
    );
    lumen.key(Modifiers::NONE, Key::B);
    let p = at(&lumen, beat(6.1), 62);
    click(&mut lumen, p, Modifiers::NONE);
    let drawn = notes(&lumen)
        .into_iter()
        .find(|n| n.pitch() == 62)
        .expect("drawn");
    assert_eq!((drawn.start(), drawn.end()), (r0, r1), "the roll's cell");
}

#[test]
fn double_clicking_empty_roll_space_with_the_pointer_adds_a_note() {
    let mut lumen = launch("notes-double");
    open(&mut lumen);
    assert!(!lumen.app().editor_draw_tool(), "the pointer is the tool");
    let before = notes(&lumen).len();
    let p = at(&lumen, beat(6.3), 65);
    lumen.double_click(p);
    let after = notes(&lumen);
    assert_eq!(after.len(), before + 1, "exactly one note");
    let (cell, end) = lumen
        .app()
        .editor_grid_cell_at(beat(6.3))
        .expect("the editor has drawn");
    let drawn = after.iter().find(|n| n.pitch() == 65).expect("drawn");
    assert_eq!(drawn.start(), cell, "in the cell under the pointer");
    assert_eq!(drawn.length(), end - cell);
    let at = after.iter().position(|n| n.pitch() == 65);
    assert_eq!(lumen.app().editor_picked(), [at.expect("drawn")]);
    undo(&mut lumen);
    assert_eq!(notes(&lumen).len(), before, "one undo step");
}

#[test]
fn double_clicking_empty_space_and_dragging_sets_the_length() {
    let mut lumen = launch("notes-double-drag");
    open(&mut lumen);
    let from = at(&lumen, beat(5.0) + Ticks(10), 62);
    let to = at(&lumen, beat(7.0), 62);
    lumen.frames(40);
    // Click once, then press again and drag.
    click(&mut lumen, from, Modifiers::NONE);
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(to);
    lumen.frames(3);
    let drawn = notes(&lumen)
        .into_iter()
        .find(|n| n.pitch() == 62)
        .expect("drawn");
    assert_eq!((drawn.start(), drawn.length()), (q(5), q(2)));
}

#[test]
fn double_clicking_a_note_does_not_add_one() {
    let mut lumen = launch("notes-double-note");
    open(&mut lumen);
    let before = notes(&lumen).len();
    lumen.double_click(at(&lumen, beat(1.5), 64));
    assert_eq!(notes(&lumen).len(), before);
}

#[test]
fn two_slow_or_distant_clicks_are_not_a_double_click() {
    let mut lumen = launch("notes-not-double");
    open(&mut lumen);
    let before = notes(&lumen).len();
    let p = at(&lumen, beat(6.3), 65);
    click(&mut lumen, p, Modifiers::NONE);
    lumen.frames(40);
    click(&mut lumen, p, Modifiers::NONE);
    assert_eq!(notes(&lumen).len(), before, "too far apart in time");
    lumen.frames(40);
    click(&mut lumen, p, Modifiers::NONE);
    click(&mut lumen, p + vec2(40.0, 0.0), Modifiers::NONE);
    assert_eq!(notes(&lumen).len(), before, "too far apart in space");
}
