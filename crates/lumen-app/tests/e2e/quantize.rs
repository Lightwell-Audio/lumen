//! Quantize in the clip editor (M5 slice 5): Q with the last settings on the
//! picked notes (or every note), the Shift+Q popover (its preview, Apply and
//! Esc), strength and iteration, and the palette tools that share the picks
//! (Fixed length, Reverse). Every change is one undo step, and controller and
//! pitch bend events are never touched.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_app::TimeView;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{ClipContent, ClipEvent, EventKind, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

const CLIP: Id = Id(3);

/// A sixteenth note: the 1/16 grid the tests set.
const SIXTEENTH: i64 = PPQ / 4;

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

/// `n` beats, rounded to the tick (so 0.04 beats is exact).
fn beat(n: f64) -> Ticks {
    Ticks((n * PPQ as f64).round() as i64)
}

/// Four loose notes off the grid (pitches C4, E4, G4 and C5), starting at
/// beats 0.04, 1.03, 2.06 and 4.10. The first three are a beat long, the
/// last two beats. On the 1/16 grid their starts go to 0, 1, 2 and 4 beats.
fn loose_notes() -> Vec<Note> {
    vec![
        Note::new(beat(0.04), q(1), 60, 100),
        Note::new(beat(1.03), q(1), 64, 100),
        Note::new(beat(2.06), q(1), 67, 100),
        Note::new(beat(4.10), q(2), 72, 100),
    ]
}

/// A controller 1 event at `at` beats.
fn cc(at: f64, value: u8) -> ClipEvent {
    ClipEvent {
        at: beat(at),
        kind: EventKind::Controller { number: 1, value },
    }
}

/// A pitch bend at `at` beats.
fn bend(at: f64, value: i16) -> ClipEvent {
    ClipEvent {
        at: beat(at),
        kind: EventKind::PitchBend { value },
    }
}

/// Controller and bend events between the notes, off the grid.
fn sample_events() -> Vec<ClipEvent> {
    vec![cc(0.37, 20), bend(1.58, 4000), cc(3.21, 100)]
}

/// One MIDI track whose clip (beats 0 to 8) holds the loose notes and
/// `events`.
fn project(events: Vec<ClipEvent>) -> Project {
    project_of(
        "Quantize",
        vec![midi_track(
            1,
            "Keys",
            "poly",
            (0, 8),
            None,
            loose_notes(),
            events,
        )],
    )
}

/// Starts Lumen with the project, the grid at 1/16 and the clip open.
fn launch(name: &str, events: Vec<ClipEvent>) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(events), &path, &Default::default())
        .expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.app_mut().set_detail_size(560.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(40);
    sixteenths(&mut lumen);
    open(&mut lumen);
    assert_eq!(grid_step(&lumen), SIXTEENTH, "the roll's grid is 1/16");
    lumen
}

/// Sets the editing grid to 1/16 and fixes it, so the roll's grid does not
/// change with its zoom: Ctrl+1 narrows until it shows, Ctrl+5 fixes it.
fn sixteenths(lumen: &mut Lumen) {
    for _ in 0..6 {
        if lumen.shows("Grid: 1/16") {
            break;
        }
        lumen.key(Modifiers::COMMAND, Key::Num1);
    }
    assert!(lumen.shows("Grid: 1/16"), "the grid can be 1/16");
    lumen.key(Modifiers::COMMAND, Key::Num5);
    assert!(lumen.shows("Grid: 1/16"), "fixed at 1/16");
}

/// Opens the clip by double-clicking it in the arrangement.
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

/// The grid step the roll draws with, at the clip's start.
fn grid_step(lumen: &Lumen) -> i64 {
    let (a, b) = lumen
        .app()
        .editor_grid_cell_at(Ticks::ZERO)
        .expect("the roll has drawn");
    b.0 - a.0
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

fn notes(lumen: &Lumen) -> Vec<Note> {
    content(lumen).0
}

fn events(lumen: &Lumen) -> Vec<ClipEvent> {
    content(lumen).1
}

fn starts(notes: &[Note]) -> Vec<Ticks> {
    notes.iter().map(|n| n.start()).collect()
}

fn ends(notes: &[Note]) -> Vec<Ticks> {
    notes.iter().map(|n| n.end()).collect()
}

fn lengths(notes: &[Note]) -> Vec<Ticks> {
    notes.iter().map(|n| n.length()).collect()
}

fn pitches(notes: &[Note]) -> Vec<u8> {
    notes.iter().map(|n| n.pitch()).collect()
}

/// The grid line nearest `t` (1/16), and how far `t` is from it.
fn nearest_line(t: Ticks) -> (i64, i64) {
    let line = (t.0 + SIXTEENTH / 2).div_euclid(SIXTEENTH) * SIXTEENTH;
    (line, (t.0 - line).abs())
}

/// The point in the roll at `t` beats, in `pitch`'s row.
fn at(lumen: &Lumen, t: Ticks, pitch: u8) -> Pos2 {
    lumen
        .app()
        .editor_point(t, pitch)
        .expect("the editor has drawn")
}

/// An empty spot of the roll: beat 6.5 on D4, where no note is.
fn empty_spot(lumen: &Lumen) -> Pos2 {
    at(lumen, beat(6.5), 62)
}

fn hover(lumen: &mut Lumen, p: Pos2) {
    lumen.harness.hover_at(p);
    lumen.frames(2);
}

/// A click without moving: press and release in one frame.
fn click(lumen: &mut Lumen, p: Pos2, modifiers: Modifiers) {
    lumen.harness.event(Event::ModifiersChanged(modifiers));
    lumen.harness.hover_at(p);
    lumen.frames(2);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    lumen.frames(3);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(2);
}

/// Presses `key` with the pointer over the roll, where the editor's keys
/// are live.
fn roll_key(lumen: &mut Lumen, modifiers: Modifiers, key: Key) {
    let p = empty_spot(lumen);
    hover(lumen, p);
    lumen.key(modifiers, key);
}

fn undo(lumen: &mut Lumen) {
    lumen.key(Modifiers::COMMAND, Key::Z);
}

/// Runs the palette command that `query` finds first.
fn palette(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(2);
    assert!(!lumen.app().palette_open(), "{query} ran");
}

/// Shift+Q: the popover opens, with its controls drawn.
fn open_popover(lumen: &mut Lumen) {
    roll_key(lumen, Modifiers::SHIFT, Key::Q);
    lumen.frames(20);
    assert!(lumen.shows("Apply"), "the popover is open");
}

/// Types `percent` into the Strength slider: click it, type the digits,
/// press Enter.
fn set_strength(lumen: &mut Lumen, percent: &str) {
    lumen.harness.get_by_label_contains("Strength:").click();
    lumen.frames(3);
    lumen.harness.event(Event::Text(percent.into()));
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
}

#[test]
fn q_with_nothing_picked_moves_every_note_onto_the_grid_and_undo_restores_them() {
    let mut lumen = launch("quantize-all", vec![]);
    let before = notes(&lumen);
    assert!(lumen.app().editor_picked().is_empty(), "nothing is picked");

    roll_key(&mut lumen, Modifiers::NONE, Key::Q);

    let after = notes(&lumen);
    assert_eq!(
        starts(&after),
        [q(0), q(1), q(2), q(4)],
        "every start on the grid"
    );
    assert_eq!(lengths(&after), lengths(&before), "lengths kept");
    assert_eq!(pitches(&after), pitches(&before), "pitches kept");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), before, "one undo restores the exact notes");
}

#[test]
fn q_with_two_notes_picked_changes_only_those() {
    let mut lumen = launch("quantize-picked", vec![]);
    let before = notes(&lumen);
    // The C4 at beat 0.04 and the G4 at beat 2.06.
    let c4 = at(&lumen, beat(0.5), 60);
    let g4 = at(&lumen, beat(2.5), 67);
    click(&mut lumen, c4, Modifiers::NONE);
    click(&mut lumen, g4, Modifiers::SHIFT);
    assert_eq!(lumen.app().editor_picked(), [0, 2]);

    roll_key(&mut lumen, Modifiers::NONE, Key::Q);

    let after = notes(&lumen);
    assert_eq!(after[0].start(), q(0), "the picked C4 moved");
    assert_eq!(after[2].start(), q(2), "the picked G4 moved");
    assert_eq!(after[1], before[1], "the E4 was not picked");
    assert_eq!(after[3], before[3], "the C5 was not picked");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), before, "one undo restores both");
}

#[test]
fn shift_q_previews_without_changing_the_notes_and_apply_changes_them_in_one_step() {
    let mut lumen = launch("quantize-popover", vec![]);
    let before = notes(&lumen);
    let label = lumen.app().undo_label().map(str::to_owned);
    open_popover(&mut lumen);

    // The controls, by the names they are given.
    for name in [
        "Strength: 100%",
        "Swing: 0%",
        "Window: 100%",
        "Straight grid",
        "Triplet grid",
        "Dotted grid",
        "Quantize note starts",
        "Quantize note ends",
        "Quantize starts and ends",
        "Quantize note lengths",
        "Reset",
        "Apply",
    ] {
        assert!(lumen.shows(name), "a control named {name:?}");
    }
    assert!(
        lumen
            .harness
            .query_by_label_contains("Quantize grid")
            .is_some(),
        "the grid dropdown"
    );

    lumen.click("Quantize note ends");
    lumen.frames(20);
    assert_eq!(
        notes(&lumen),
        before,
        "the popover previews; nothing changes"
    );
    assert_eq!(lumen.app().undo_label().map(str::to_owned), label);
    assert!(
        lumen
            .harness
            .query_by_label_contains("4 of 4 notes move")
            .is_some(),
        "the readout counts the moves it would make"
    );

    lumen.click("Apply");
    lumen.frames(10);
    let after = notes(&lumen);
    assert_eq!(
        starts(&after),
        starts(&before),
        "note ends moves, not starts"
    );
    assert_eq!(
        ends(&after),
        [q(1), q(2), q(3), q(6)],
        "every end on the grid"
    );
    assert!(!lumen.shows("Apply"), "Apply closes the popover");

    undo(&mut lumen);
    assert_eq!(notes(&lumen), before, "one undo restores the exact notes");
}

#[test]
fn esc_closes_the_popover_and_changes_nothing() {
    let mut lumen = launch("quantize-esc", vec![]);
    let before = notes(&lumen);
    let label = lumen.app().undo_label().map(str::to_owned);
    open_popover(&mut lumen);
    lumen.click("Quantize note lengths");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(40);
    assert!(!lumen.shows("Apply"), "Esc closed it");
    assert_eq!(notes(&lumen), before, "Esc changes nothing");
    assert_eq!(lumen.app().undo_label().map(str::to_owned), label);

    // The toolbar's Quantize button opens it too, and Esc closes it again.
    lumen.click("Quantize");
    assert!(lumen.shows("Apply"), "the button opens the popover");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(40);
    assert!(!lumen.shows("Apply"));
    assert_eq!(notes(&lumen), before);
}

#[test]
fn strength_moves_notes_part_way_and_each_q_moves_them_closer() {
    let mut lumen = launch("quantize-strength", vec![]);
    let before = notes(&lumen);
    open_popover(&mut lumen);
    set_strength(&mut lumen, "50");
    assert!(lumen.shows("Strength: 50%"), "typed 50 sets the strength");
    lumen.click("Apply");
    lumen.frames(10);
    let half = notes(&lumen);
    assert_eq!(lengths(&half), lengths(&before), "lengths kept");

    for (i, (b, h)) in before.iter().zip(&half).enumerate() {
        let (line, d0) = nearest_line(b.start());
        let (_, d1) = nearest_line(h.start());
        assert!(
            (d1 - d0 / 2).abs() <= 2,
            "note {i}: half strength moves half the way ({d0} to {d1})"
        );
        assert_eq!(
            (h.start().0 - line).signum(),
            (b.start().0 - line).signum(),
            "note {i} moved toward its line, not past it"
        );
    }

    // Q again, with the same settings: each note closer to its line.
    roll_key(&mut lumen, Modifiers::NONE, Key::Q);
    let again = notes(&lumen);
    for (i, ((b, h), a)) in before.iter().zip(&half).zip(&again).enumerate() {
        let (_, d0) = nearest_line(b.start());
        let (_, d1) = nearest_line(h.start());
        let (_, d2) = nearest_line(a.start());
        assert!(
            d2 < d1,
            "note {i}: the second Q moves it closer ({d1} to {d2})"
        );
        assert!(
            (d2 - d0 / 4).abs() <= 2,
            "note {i}: two halves make a quarter of the way ({d0} to {d2})"
        );
    }

    undo(&mut lumen);
    assert_eq!(notes(&lumen), half, "one undo takes back the second Q");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), before, "one undo takes back the Apply");
}

#[test]
fn fixed_length_and_reverse_from_the_palette_are_one_undo_step_each() {
    let mut lumen = launch("quantize-tools", vec![]);
    let original = notes(&lumen);

    palette(&mut lumen, "Fixed length");
    let fixed = notes(&lumen);
    assert!(
        fixed.iter().all(|n| n.length() == Ticks(SIXTEENTH)),
        "every note is one grid step long: {fixed:?}"
    );
    assert_eq!(starts(&fixed), starts(&original), "starts kept");
    assert_eq!(pitches(&fixed), pitches(&original), "pitches kept");

    palette(&mut lumen, "Reverse notes");
    let reversed = notes(&lumen);
    // Each note mirrors across the span the notes cover: from the first
    // start to the last end.
    let first = fixed.iter().map(|n| n.start()).min().expect("notes");
    let last = fixed.iter().map(|n| n.end()).max().expect("notes");
    // Edits keep the notes sorted by start, so match them by their pitch
    // (each pitch appears once here), not by index.
    for f in &fixed {
        let r = reversed
            .iter()
            .find(|r| r.pitch() == f.pitch())
            .expect("the note is still there");
        assert_eq!(r.start(), first + last - f.end(), "mirrored: {f:?}");
        assert_eq!(r.length(), f.length(), "length kept");
    }
    assert_eq!(reversed.len(), fixed.len(), "no note was added or lost");
    // The last note now comes first: the pitches read backwards in time.
    let mut by_time = reversed.clone();
    by_time.sort_by_key(|n| n.start());
    assert_eq!(pitches(&by_time), [72, 67, 64, 60], "the order reversed");

    // Reverse is one step, and Fixed length is another.
    undo(&mut lumen);
    assert_eq!(notes(&lumen), fixed, "one undo takes back Reverse");
    undo(&mut lumen);
    assert_eq!(notes(&lumen), original, "one undo takes back Fixed length");
}

#[test]
fn controller_events_are_never_touched() {
    let mut lumen = launch("quantize-events", sample_events());
    let (notes0, events0) = content(&lumen);
    assert_eq!(events0.len(), 3, "the clip has its events");

    roll_key(&mut lumen, Modifiers::NONE, Key::Q);
    assert_eq!(events(&lumen), events0, "Q");
    assert_ne!(notes(&lumen), notes0, "the notes did move");

    open_popover(&mut lumen);
    lumen.click("Quantize note lengths");
    lumen.click("Apply");
    lumen.frames(10);
    assert_eq!(events(&lumen), events0, "the popover's Apply");

    palette(&mut lumen, "Fixed length");
    assert_eq!(events(&lumen), events0, "Fixed length");

    palette(&mut lumen, "Reverse notes");
    assert_eq!(events(&lumen), events0, "Reverse");

    // Four changes, four undo steps, back to the start with its events.
    for _ in 0..4 {
        undo(&mut lumen);
    }
    assert_eq!(notes(&lumen), notes0, "every step taken back");
    assert_eq!(events(&lumen), events0, "the events came back untouched");
}
