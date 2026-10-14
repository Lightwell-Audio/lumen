//! The velocity and controller lane under the piano roll (M5 slice 4b):
//! drawing velocities across stems, controllers with the pencil, a line and
//! a curve that can be bent, moving, boxing and deleting points, the lane
//! tabs and tools, Live A (the document changes once, on release), Esc, and
//! hearing what was drawn on a MIDI port. Every edit is one undo step.

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use lumen_app::TimeView;
use lumen_core::time::{PPQ, Ticks};
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::model::{ClipContent, ClipEvent, EventKind, Id, MidiTo, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

fn beat(n: f32) -> Ticks {
    Ticks((n * PPQ as f32) as i64)
}

const CLIP: Id = Id(3);

fn cc(at: i64, number: u8, value: u8) -> ClipEvent {
    ClipEvent {
        at: q(at),
        kind: EventKind::Controller { number, value },
    }
}

/// One MIDI track whose clip (beats 0 to 8) holds four notes of velocity 100
/// at beats 0, 1, 2 and 4, and `events`; sent to the fake port "Synth" on
/// channel 4 when `port`.
fn project(events: Vec<ClipEvent>, port: bool) -> Project {
    let notes = vec![
        Note::new(q(0), q(1), 60, 100),
        Note::new(q(1), q(1), 64, 100),
        Note::new(q(2), q(1), 67, 100),
        Note::new(q(4), q(2), 72, 100),
    ];
    project_with(notes, events, port)
}

/// [`project`] with the clip's `notes` in place of the four.
fn project_with(notes: Vec<Note>, events: Vec<ClipEvent>, port: bool) -> Project {
    let mut track = midi_track(1, "Keys", "poly", (0, 8), None, notes, events);
    if port {
        track.midi_to = Some(MidiTo {
            port: "Synth".into(),
            channel: 3,
        });
    }
    let mut project = project_of("Lanes", vec![track]);
    project.tempo[0].bpm = 480.0;
    project
}

fn launch(name: &str, events: Vec<ClipEvent>) -> Lumen {
    launch_project(name, project(events, false))
}

/// [`launch`] with a clip of `notes`.
fn launch_notes(name: &str, notes: Vec<Note>) -> Lumen {
    launch_project(name, project_with(notes, vec![], false))
}

fn launch_project(name: &str, project: Project) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.app_mut().set_detail_size(560.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    open(&mut lumen);
    lumen
}

fn open(lumen: &mut Lumen) {
    let ctx = lumen.harness.ctx.clone();
    let r = lumen
        .app()
        .clip_rect(&ctx, CLIP)
        .expect("the clip is in view");
    lumen.double_click(r.center());
    lumen.frames(20);
    assert_eq!(lumen.app().editor_open_clip(), Some(CLIP));
}

fn content(lumen: &Lumen) -> (Vec<Note>, Vec<ClipEvent>) {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == CLIP)
        .map(|c| match &c.content {
            ClipContent::Midi { notes, events } => (notes.clone(), events.clone()),
            _ => panic!("not MIDI"),
        })
        .expect("the clip")
}

fn velocities(lumen: &Lumen) -> Vec<u8> {
    content(lumen).0.iter().map(|n| n.velocity()).collect()
}

/// The values of controller `number`'s events, with their times.
fn controller(lumen: &Lumen, number: u8) -> Vec<(Ticks, u8)> {
    content(lumen)
        .1
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::Controller { number: n, value } if n == number => Some((e.at, value)),
            _ => None,
        })
        .collect()
}

fn bends(lumen: &Lumen) -> Vec<(Ticks, i16)> {
    content(lumen)
        .1
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::PitchBend { value } => Some((e.at, value)),
            _ => None,
        })
        .collect()
}

/// Where the lane shows `value` at `beats` into the clip.
fn at(lumen: &Lumen, beats: f32, value: i32) -> Pos2 {
    lumen
        .app()
        .lane_point(beat(beats), value)
        .expect("the lane has drawn")
}

fn tab(lumen: &mut Lumen, name: &str) {
    lumen.click(&format!("{name} tab"));
    lumen.frames(4);
}

fn tool(lumen: &mut Lumen, name: &str) {
    lumen.click(name);
    lumen.frames(4);
}

fn undo(lumen: &mut Lumen) {
    lumen.key(Modifiers::COMMAND, Key::Z);
}

#[test]
fn the_lane_sits_under_the_roll_and_starts_on_velocity() {
    let lumen = launch("lane-shown", vec![]);
    assert!(lumen.app().lane_shown());
    assert_eq!(lumen.app().lane_name(), "Velocity");
    let body = lumen.app().editor_body().expect("the roll");
    let lane = lumen.app().lane_body().expect("the lane");
    assert!(lane.top() >= body.bottom(), "the lane is below the notes");
    assert!(lumen.shows("Velocity tab"));
    assert!(lumen.shows("Mod wheel tab"));
    assert!(lumen.shows("Pitch bend tab"));
    assert!(lumen.shows("Pressure tab"));
}

#[test]
fn the_lane_button_hides_and_shows_it() {
    let mut lumen = launch("lane-toggle", vec![]);
    let roll = lumen.app().editor_body().expect("the roll").height();
    lumen.click("Lane");
    lumen.frames(4);
    assert!(!lumen.app().lane_shown(), "hidden");
    assert!(lumen.app().editor_body().expect("the roll").height() > roll);
    lumen.click("Lane");
    lumen.frames(4);
    assert!(lumen.app().lane_shown(), "shown again");
}

#[test]
fn the_tabs_switch_the_lane() {
    let mut lumen = launch("lane-tabs", vec![]);
    tab(&mut lumen, "Pitch bend");
    assert_eq!(lumen.app().lane_name(), "Pitch bend");
    tab(&mut lumen, "Mod wheel");
    assert_eq!(lumen.app().lane_name(), "Mod wheel");
    tab(&mut lumen, "Velocity");
    assert_eq!(lumen.app().lane_name(), "Velocity");
}

#[test]
fn the_cc_menu_adds_a_controller_tab() {
    let mut lumen = launch("lane-cc", vec![]);
    lumen.click("+ CC");
    lumen.click("Volume");
    assert_eq!(lumen.app().lane_name(), "Volume");
    assert!(lumen.shows("Volume tab"));
}

#[test]
fn dragging_across_stems_sets_their_velocities_in_one_undo_step() {
    let mut lumen = launch("lane-velocity", vec![]);
    let from = at(&lumen, 0.0, 20);
    let to = at(&lumen, 2.0, 20);
    lumen.drag(from, to, Modifiers::NONE);
    let v = velocities(&lumen);
    assert!(v[..3].iter().all(|&x| (18..=22).contains(&x)), "{v:?}");
    assert_eq!(v[3], 100, "the stem at beat 4 was not touched");
    undo(&mut lumen);
    assert_eq!(velocities(&lumen), [100, 100, 100, 100], "one step undone");
}

#[test]
fn live_a_the_lane_changes_the_document_once_on_release() {
    let mut lumen = launch("lane-live", vec![]);
    let before = content(&lumen);
    let from = at(&lumen, 0.0, 30);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=5 {
        lumen.harness.hover_at(from + vec2(30.0 * step as f32, 0.0));
        lumen.frames(2);
        assert_eq!(content(&lumen), before, "nothing changes while dragging");
        assert!(lumen.app().lane_gesture());
    }
    lumen.harness.drop_at(from + vec2(150.0, 0.0));
    lumen.frames(4);
    assert!(!lumen.app().lane_gesture());
    assert_ne!(content(&lumen), before);
}

#[test]
fn escape_cancels_a_lane_drag_and_changes_nothing() {
    let mut lumen = launch("lane-cancel", vec![]);
    let before = content(&lumen);
    let from = at(&lumen, 0.0, 30);
    lumen.harness.hover_at(from);
    lumen.frames(2);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    lumen.harness.hover_at(from + vec2(100.0, 0.0));
    lumen.frames(2);
    assert!(lumen.app().lane_gesture());
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().lane_gesture(), "Esc ended the drag");
    lumen.harness.drop_at(from + vec2(100.0, 0.0));
    lumen.frames(4);
    assert_eq!(content(&lumen), before);
}

#[test]
fn the_pencil_writes_a_controller_and_undo_takes_it_back() {
    let mut lumen = launch("lane-pencil", vec![]);
    tab(&mut lumen, "Mod wheel");
    lumen.drag(at(&lumen, 1.0, 64), at(&lumen, 3.0, 64), Modifiers::NONE);
    let written = controller(&lumen, 1);
    assert!(written.len() >= 4, "{written:?}");
    assert!(written.iter().all(|&(_, v)| (62..=66).contains(&v)));
    assert!(
        written
            .iter()
            .all(|&(t, _)| t >= q(1) - beat(0.3) && t <= q(3) + beat(0.3))
    );
    undo(&mut lumen);
    assert!(controller(&lumen, 1).is_empty());
}

#[test]
fn a_line_ramps_between_two_values() {
    let mut lumen = launch("lane-line", vec![]);
    tab(&mut lumen, "Mod wheel");
    tool(&mut lumen, "Lane line tool");
    lumen.drag(at(&lumen, 1.0, 10), at(&lumen, 3.0, 120), Modifiers::NONE);
    let ramp = controller(&lumen, 1);
    assert!(
        ramp.len() >= 16,
        "a smooth ramp, not a few points: {}",
        ramp.len()
    );
    assert!((8..=13).contains(&ramp[0].1), "{ramp:?}");
    assert!(
        (117..=123).contains(&ramp.last().expect("an end").1),
        "{ramp:?}"
    );
    assert!(ramp.windows(2).all(|w| w[0].1 <= w[1].1), "rises only");
    undo(&mut lumen);
    assert!(controller(&lumen, 1).is_empty());
}

#[test]
fn a_curve_bends_when_its_dot_is_dragged() {
    let mut lumen = launch("lane-curve", vec![]);
    tab(&mut lumen, "Mod wheel");
    tool(&mut lumen, "Lane curve tool");
    lumen.drag(at(&lumen, 1.0, 0), at(&lumen, 3.0, 120), Modifiers::NONE);
    let straight = controller(&lumen, 1);
    let dot = lumen.app().lane_bend_dot().expect("the curve has a dot");
    // Pull the dot down: the curve sags below the straight line.
    lumen.drag(dot, dot + vec2(0.0, 30.0), Modifiers::NONE);
    let bent = controller(&lumen, 1);
    let mid = |v: &[(Ticks, u8)]| {
        v.iter()
            .find(|(t, _)| *t >= q(2))
            .map(|&(_, x)| x)
            .expect("a middle")
    };
    assert!(
        mid(&bent) < mid(&straight),
        "{} < {}",
        mid(&bent),
        mid(&straight)
    );
    assert!(lumen.app().lane_bend_dot().is_some(), "still bendable");
}

#[test]
fn the_pitch_bend_lane_draws_bends_around_its_centre() {
    let mut lumen = launch("lane-bend", vec![]);
    tab(&mut lumen, "Pitch bend");
    tool(&mut lumen, "Lane line tool");
    lumen.drag(at(&lumen, 1.0, 0), at(&lumen, 3.0, 4000), Modifiers::NONE);
    let bend = bends(&lumen);
    assert!(bend.len() >= 8, "{}", bend.len());
    assert!(bend[0].1.abs() < 200, "starts at the centre: {bend:?}");
    assert!(
        (3700..=4300).contains(&bend.last().expect("an end").1),
        "{bend:?}"
    );
}

#[test]
fn a_point_dragged_up_changes_its_value_only() {
    let mut lumen = launch("lane-move", vec![cc(1, 1, 20), cc(2, 1, 60), cc(3, 1, 100)]);
    tab(&mut lumen, "Mod wheel");
    lumen.drag(at(&lumen, 2.0, 60), at(&lumen, 2.0, 90), Modifiers::NONE);
    let events = controller(&lumen, 1);
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].0, q(2), "time unchanged");
    assert!((88..=92).contains(&events[1].1), "{events:?}");
    undo(&mut lumen);
    assert_eq!(controller(&lumen, 1)[1].1, 60);
}

#[test]
fn a_box_picks_points_and_delete_removes_them_in_one_step() {
    let mut lumen = launch("lane-box", vec![cc(1, 1, 20), cc(2, 1, 60), cc(3, 1, 100)]);
    tab(&mut lumen, "Mod wheel");
    lumen.drag(at(&lumen, 0.5, 120), at(&lumen, 2.5, 5), Modifiers::SHIFT);
    assert_eq!(lumen.app().lane_picked().len(), 2, "two points in the box");
    // The keys are the editor's while the pointer is over it.
    let over = at(&lumen, 6.0, 60);
    lumen.harness.hover_at(over);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(controller(&lumen, 1), [(q(3), 100)]);
    undo(&mut lumen);
    assert_eq!(
        controller(&lumen, 1).len(),
        3,
        "all three came back at once"
    );
}

#[test]
fn what_the_lane_draws_is_sent_to_the_port() {
    let fake = FakeControl::new();
    fake.plug(Direction::Output, "Synth");
    let dir = TempDir::new("e2e-lane-port");
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(vec![], true), &path, &Default::default())
        .expect("save the test project");
    let handle = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path).with_midi(handle));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.app_mut().set_detail_size(560.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    open(&mut lumen);
    tab(&mut lumen, "Mod wheel");
    tool(&mut lumen, "Lane line tool");
    lumen.drag(at(&lumen, 0.5, 40), at(&lumen, 3.0, 110), Modifiers::NONE);
    let drawn = controller(&lumen, 1);
    assert!(!drawn.is_empty());
    // Opening the clip left the playhead in its middle: play from the start.
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    for _ in 0..1_000 {
        lumen.frames(1);
        let sent = fake.sent();
        // Channel 4 (index 3), controller 1.
        if sent
            .iter()
            .filter(|(_, m)| m.starts_with(&[0xB3, 1]))
            .count()
            >= 8
        {
            lumen.key(Modifiers::NONE, Key::Space);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the port never got the drawn controller: {:?}", fake.sent());
}

/// Runs the palette command that `query` finds first.
fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(eframe::egui::Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
}

#[test]
fn the_palette_switches_the_lane_and_its_tool() {
    let mut lumen = launch("lane-palette", vec![]);
    palette(&mut lumen, "Pitch bend lane");
    assert_eq!(lumen.app().lane_name(), "Pitch bend");
    palette(&mut lumen, "Mod wheel lane");
    assert_eq!(lumen.app().lane_name(), "Mod wheel");
    palette(&mut lumen, "Lane line");
    assert!(lumen.is_on("Lane line tool"));
    palette(&mut lumen, "Velocity and controller lane");
    assert!(!lumen.app().lane_shown(), "the palette hides the lane");
    palette(&mut lumen, "Velocity and controller lane");
    assert!(lumen.app().lane_shown());
}

#[test]
fn raise_and_lower_velocity_each_change_the_picked_notes_in_one_step() {
    let mut lumen = launch("lane-raise", vec![]);
    let over = lumen.app().editor_point(beat(1.5), 64).expect("the roll");
    lumen.harness.hover_at(over);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::A);
    palette(&mut lumen, "Raise velocity");
    assert_eq!(velocities(&lumen), [108, 108, 108, 108]);
    palette(&mut lumen, "Lower velocity");
    palette(&mut lumen, "Lower velocity");
    assert_eq!(velocities(&lumen), [92, 92, 92, 92]);
    undo(&mut lumen);
    assert_eq!(velocities(&lumen), [100, 100, 100, 100]);
}

#[test]
fn a_box_in_the_velocity_lane_picks_the_notes_of_the_stems_it_holds() {
    let mut lumen = launch("lane-stem-box", vec![]);
    // Stems at beats 0, 1, 2 and 4, heads at velocity 100.
    lumen.drag(at(&lumen, 0.5, 120), at(&lumen, -0.2, 90), Modifiers::SHIFT);
    let picked = lumen.app().editor_picked();
    assert_eq!(
        picked,
        [0],
        "only the stem at beat 0 is inside the box: {picked:?}"
    );
    // Shift adds: a second box over the stems at beats 0 and 1.
    lumen.drag(at(&lumen, 1.5, 120), at(&lumen, -0.2, 90), Modifiers::SHIFT);
    assert_eq!(lumen.app().editor_picked(), [0, 1]);
}

#[test]
fn the_number_keys_choose_the_lane_tool() {
    let mut lumen = launch("lane-keys", vec![]);
    assert!(lumen.is_on("Lane pencil tool"));
    lumen.key(Modifiers::NONE, Key::Num2);
    assert!(lumen.is_on("Lane line tool"));
    lumen.key(Modifiers::NONE, Key::Num3);
    assert!(lumen.is_on("Lane curve tool"));
    lumen.key(Modifiers::NONE, Key::Num1);
    assert!(lumen.is_on("Lane pencil tool"));
}

/// Picks notes 0 and 1 of the four, after giving note 1 velocity 70 (a
/// drag of its own head): velocities are then `[100, 70, 100, 100]`.
fn pick_first_two(lumen: &mut Lumen) {
    lumen.drag(at(lumen, 1.0, 100), at(lumen, 1.0, 70), Modifiers::NONE);
    assert_eq!(velocities(lumen), [100, 70, 100, 100]);
    lumen.drag(at(lumen, 1.5, 120), at(lumen, -0.2, 60), Modifiers::SHIFT);
    assert_eq!(lumen.app().editor_picked(), [0, 1]);
}

#[test]
fn dragging_the_head_of_a_picked_note_moves_every_picked_velocity_by_the_same_amount() {
    let mut lumen = launch("lane-heads", vec![]);
    pick_first_two(&mut lumen);
    lumen.drag(at(&lumen, 1.0, 70), at(&lumen, 1.0, 40), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [70, 40, 100, 100], "both moved by -30");
    assert_eq!(lumen.app().editor_picked(), [0, 1], "the picks are kept");
    undo(&mut lumen);
    assert_eq!(
        velocities(&lumen),
        [100, 70, 100, 100],
        "one undo step restores them all"
    );
}

#[test]
fn dragging_picked_heads_stops_each_note_at_the_ends_of_the_range() {
    let mut lumen = launch("lane-heads-clamp", vec![]);
    pick_first_two(&mut lumen);
    lumen.drag(at(&lumen, 1.0, 70), at(&lumen, 1.0, 127), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [127, 127, 100, 100], "+57 stops at 127");
    undo(&mut lumen);
    // Undo lets the picks go: pick the two again.
    lumen.drag(at(&lumen, 1.5, 120), at(&lumen, -0.2, 60), Modifiers::SHIFT);
    assert_eq!(lumen.app().editor_picked(), [0, 1]);
    lumen.drag(at(&lumen, 0.0, 100), at(&lumen, 0.0, 1), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [1, 1, 100, 100], "-99 stops at 1");
    assert_eq!(lumen.app().editor_picked(), [0, 1]);
}

#[test]
fn dragging_an_unpicked_head_picks_it_and_changes_only_it_relatively() {
    let mut lumen = launch("lane-heads-alone", vec![]);
    pick_first_two(&mut lumen);
    // Grabbed a little below the head of note 2 and dragged 10 values down.
    let head = at(&lumen, 2.0, 100);
    let travel = at(&lumen, 2.0, 90).y - head.y;
    let from = Pos2::new(head.x, head.y + 6.0);
    lumen.drag(from, from + vec2(0.0, travel), Modifiers::NONE);
    let v = velocities(&lumen);
    assert_eq!((v[0], v[1], v[3]), (100, 70, 100), "the others stay: {v:?}");
    assert!((89..=91).contains(&v[2]), "relative, no jump: {v:?}");
    assert_eq!(lumen.app().editor_picked(), [2], "only the grabbed note");
}

#[test]
fn a_head_drag_is_relative_to_where_it_was_grabbed() {
    let mut lumen = launch("lane-heads-relative", vec![]);
    // Grabbed 6 points below the head (inside the hit distance), then dragged
    // down 10 values' worth: the velocity falls by the travel, it does not
    // jump to the pointer.
    let head = at(&lumen, 1.0, 100);
    let travel = at(&lumen, 1.0, 90).y - at(&lumen, 1.0, 100).y;
    let from = Pos2::new(head.x, head.y + 6.0);
    lumen.drag(from, from + vec2(0.0, travel), Modifiers::NONE);
    let v = velocities(&lumen);
    assert_eq!(v[0], 100);
    assert!((89..=91).contains(&v[1]), "moved by the travel: {v:?}");
}

/// A clip whose notes overlap in the lane: a chord at beat 0 (100 and 60),
/// two at beat 1 with equal velocity, and two a few points apart at beat 2.
fn overlapping() -> Vec<Note> {
    vec![
        Note::new(q(0), q(1), 60, 100),
        Note::new(q(0), q(1), 64, 60),
        Note::new(q(1), q(1), 60, 90),
        Note::new(q(1), q(1), 64, 90),
        Note::new(q(2), q(1), 67, 100),
        Note::new(Ticks(2 * PPQ + PPQ / 20), q(1), 71, 50),
    ]
}

#[test]
fn each_note_of_a_chord_has_its_own_head() {
    let mut lumen = launch_notes("lane-chord", overlapping());
    lumen.drag(at(&lumen, 0.0, 60), at(&lumen, 0.0, 30), Modifiers::NONE);
    assert_eq!(
        velocities(&lumen),
        [100, 30, 90, 90, 100, 50],
        "only the quieter note of the chord changed"
    );
    assert_eq!(lumen.app().editor_picked(), [1]);
    undo(&mut lumen);
    lumen.drag(at(&lumen, 0.0, 100), at(&lumen, 0.0, 120), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [120, 60, 90, 90, 100, 50]);
}

#[test]
fn heads_that_coincide_give_the_one_drawn_on_top() {
    let mut lumen = launch_notes("lane-coincide", overlapping());
    lumen.drag(at(&lumen, 1.0, 90), at(&lumen, 1.0, 60), Modifiers::NONE);
    assert_eq!(
        velocities(&lumen),
        [100, 60, 90, 60, 100, 50],
        "the higher note, and only it"
    );
}

#[test]
fn a_head_next_to_another_start_changes_only_its_own_note() {
    let mut lumen = launch_notes("lane-near", overlapping());
    let apart = (at(&lumen, 2.05, 50).x - at(&lumen, 2.0, 50).x).abs();
    assert!(
        apart > 1.0 && apart < 8.0,
        "the two stems are closer than the 8 point hit distance: {apart}"
    );
    lumen.drag(at(&lumen, 2.05, 50), at(&lumen, 2.05, 20), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [100, 60, 90, 90, 100, 20]);
    undo(&mut lumen);
    lumen.drag(at(&lumen, 2.0, 100), at(&lumen, 2.0, 70), Modifiers::NONE);
    assert_eq!(velocities(&lumen), [100, 60, 90, 90, 70, 50]);
}
