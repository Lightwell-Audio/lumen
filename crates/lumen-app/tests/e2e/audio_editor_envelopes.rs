//! The audio editor's envelopes and loop brace (M9.5 S2b, D20, D21, DR A,
//! LB A, D28, D29): the Gain and Pan pills show one envelope at a time; a
//! click adds a point; Draw (B) lights Gain and one stroke is one undo step;
//! the editor's picks are its own; a click on the brace's band loops the
//! clip and a drag of its end is one step; the envelope and the loop from
//! the keyboard; a brace end dragged past the view scrolls it.

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::envelope::Kind;
use lumen_project::model::Id;

use super::Lumen;
use super::audio_editor_handles::{clip, opened, opened_with, shown, to_the_detail_panel, x_of};

fn q(beats: f64) -> Ticks {
    Ticks((beats * PPQ as f64).round() as i64)
}

/// The points of the clip's `kind` envelope.
fn points(lumen: &Lumen, id: Id, kind: Kind) -> Vec<(Ticks, f32)> {
    clip(lumen, id)
        .envelopes
        .get(kind)
        .map(|lane| lane.points().iter().map(|p| (p.at, p.value)).collect())
        .unwrap_or_default()
}

/// Beat `q` on the wave, at a quarter of its height from the top.
fn high(lumen: &Lumen, q: f64) -> Pos2 {
    let body = shown(lumen).body;
    Pos2::new(x_of(lumen, q), body.top() + body.height() * 0.25)
}

/// The middle of the brace's band at beat `q`.
fn band_at(lumen: &Lumen, q: f64) -> Pos2 {
    Pos2::new(x_of(lumen, q), shown(lumen).band.center().y)
}

fn looping(lumen: &Lumen, id: Id) -> Option<Ticks> {
    clip(lumen, id).loop_length.filter(|l| *l > Ticks::ZERO)
}

/// Presses Tab until the editor's stop `name` has the keyboard.
fn tab_to_stop(lumen: &mut Lumen, name: &str) {
    let ctx = lumen.harness.ctx.clone();
    for _ in 0..90 {
        if lumen.app().audio_stop_focused(&ctx) == Some(name) {
            return;
        }
        lumen.key(Modifiers::NONE, Key::Tab);
    }
    panic!("Tab never reached the editor's {name} stop");
}

#[test]
fn the_pills_show_one_envelope_and_a_click_adds_a_point_in_one_step() {
    let (mut lumen, id) = opened("editor-envelope-pills");
    assert_eq!(lumen.app().audio_editor_envelope(), None);
    lumen.click("Gain envelope");
    assert_eq!(lumen.app().audio_editor_envelope(), Some("gain"));
    lumen.click("Pan envelope");
    assert_eq!(
        lumen.app().audio_editor_envelope(),
        Some("pan"),
        "one at a time"
    );
    lumen.click("Pan envelope");
    assert_eq!(lumen.app().audio_editor_envelope(), None, "again hides it");

    lumen.click("Gain envelope");
    lumen.harness.hover_at(high(&lumen, 2.0));
    lumen.frames(2);
    assert!(lumen.info().contains("add a point"), "{}", lumen.info());
    lumen.click_at(high(&lumen, 2.0), Modifiers::NONE);
    let added = points(&lumen, id, Kind::Gain);
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(
        points(&lumen, id, Kind::Pan),
        [],
        "the pan lane is untouched"
    );
    assert!(lumen.app().audio_editor_cursor().is_none(), "no cursor");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(points(&lumen, id, Kind::Gain), [], "one undo step");
}

#[test]
fn draw_lights_gain_and_one_stroke_is_one_undo_step() {
    let (mut lumen, id) = opened("editor-envelope-draw");
    lumen.harness.hover_at(high(&lumen, 2.0));
    lumen.frames(1);
    lumen.key(Modifiers::NONE, Key::B);
    assert!(lumen.app().audio_editor_draws(), "B takes the pencil");
    assert_eq!(
        lumen.app().audio_editor_envelope(),
        Some("gain"),
        "and lights Gain"
    );
    let body = shown(&lumen).body;
    let from = Pos2::new(x_of(&lumen, 1.25), body.top() + body.height() * 0.2);
    let to = Pos2::new(x_of(&lumen, 2.75), body.top() + body.height() * 0.7);
    lumen.drag(from, to, Modifiers::NONE);
    let drawn = points(&lumen, id, Kind::Gain);
    assert!(drawn.len() >= 2, "a stroke of points: {drawn:?}");
    assert!(
        drawn.first().unwrap().1 > drawn.last().unwrap().1,
        "it falls as drawn: {drawn:?}"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(points(&lumen, id, Kind::Gain), [], "one undo step");
    lumen.harness.hover_at(high(&lumen, 2.0));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::B);
    assert!(!lumen.app().audio_editor_draws(), "B again: the pointer");
}

#[test]
fn the_editors_picks_are_its_own_and_delete_takes_them() {
    let (mut lumen, id) = opened("editor-envelope-picks");
    lumen.click("Gain envelope");
    lumen.click_at(high(&lumen, 1.5), Modifiers::NONE);
    lumen.click_at(high(&lumen, 2.5), Modifiers::NONE);
    assert_eq!(points(&lumen, id, Kind::Gain).len(), 2);
    let first = lumen.app().audio_envelope_point(0).expect("drawn");
    lumen.click_at(first, Modifiers::NONE);
    assert_eq!(lumen.app().audio_editor_picked(), [0]);
    let second = lumen.app().audio_envelope_point(1).expect("drawn");
    lumen.click_at(second, Modifiers::NONE);
    assert_eq!(
        lumen.app().audio_editor_picked(),
        [1],
        "picked in the editor"
    );
    assert_eq!(
        lumen.app().arrangement_lane_picked(),
        Vec::<usize>::new(),
        "the arrangement's lanes keep their own"
    );
    lumen.harness.hover_at(second);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(points(&lumen, id, Kind::Gain).len(), 1, "Delete took it");
    assert_eq!(clip(&lumen, id).start, q(1.0), "and not the clip");
}

#[test]
fn a_click_on_the_band_loops_the_clip_and_an_end_drag_is_one_step() {
    let (mut lumen, id) = opened("editor-brace");
    assert_eq!(shown(&lumen).brace, None);
    lumen.harness.hover_at(band_at(&lumen, 2.0));
    lumen.frames(2);
    assert!(lumen.info().contains("loop the clip"), "{}", lumen.info());
    let before = clip(&lumen, id);
    lumen.click_at(band_at(&lumen, 2.0), Modifiers::NONE);
    let length = looping(&lumen, id).expect("the clip loops");
    assert_eq!(lumen.app().undo_label(), Some("Loop audio in clip"));
    assert_eq!(clip(&lumen, id).start, before.start, "the clip stays put");
    let (_, end) = shown(&lumen).brace.expect("the brace shows its ends");

    // The end, dragged a beat earlier: a shorter loop, one undo step.
    let y = shown(&lumen).band.center().y;
    let grip = Pos2::new(end - 1.0, y);
    let to = Pos2::new(grip.x - (x_of(&lumen, 2.0) - x_of(&lumen, 1.5)), y);
    lumen.drag(grip, to, Modifiers::NONE);
    let shorter = looping(&lumen, id).expect("still loops");
    assert!(shorter < length, "{shorter:?} < {length:?}");
    assert_eq!(lumen.app().undo_label(), Some("Change audio loop"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(looping(&lumen, id), Some(length), "one undo step");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);
}

#[test]
fn the_envelope_and_the_loop_from_the_keyboard() {
    let (mut lumen, id) = opened("editor-envelope-keys");
    lumen.click("Gain envelope");
    lumen.harness.hover_at(Pos2::new(5.0, 5.0));
    to_the_detail_panel(&mut lumen);
    // The cursor at the clip's start; Enter on the envelope adds a point
    // there, at the envelope's value.
    tab_to_stop(&mut lumen, "wave");
    lumen.key(Modifiers::NONE, Key::Home);
    assert_eq!(lumen.app().audio_editor_cursor(), Some(q(1.0)));
    tab_to_stop(&mut lumen, "envelope");
    lumen.key(Modifiers::NONE, Key::Enter);
    let added = points(&lumen, id, Kind::Gain);
    assert_eq!(added.len(), 1, "{added:?}");
    lumen.key(Modifiers::COMMAND, Key::ArrowRight);
    assert_eq!(
        lumen.app().audio_editor_picked(),
        [0],
        "Ctrl+Right picks it"
    );
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    let raised = points(&lumen, id, Kind::Gain);
    assert!(raised[0].1 > added[0].1, "Up raises it: {raised:?}");
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert!(
        points(&lumen, id, Kind::Gain)[0].0 > raised[0].0,
        "Right moves it later"
    );
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(points(&lumen, id, Kind::Gain), [], "Delete takes it");

    // Enter on the band loops the clip; Right moves the loop's end.
    tab_to_stop(&mut lumen, "loop");
    lumen.key(Modifiers::NONE, Key::Enter);
    let length = looping(&lumen, id).expect("Enter loops the clip");
    tab_to_stop(&mut lumen, "loop end");
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    let shorter = looping(&lumen, id).expect("still loops");
    assert!(shorter < length, "{shorter:?} < {length:?}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(looping(&lumen, id), Some(length), "one undo step");
}

#[test]
fn a_brace_end_dragged_past_the_view_scrolls_it() {
    let (mut lumen, id) = opened_with(Lumen::start("editor-brace-scroll"), 10.0, (1, 4));
    lumen.click_at(band_at(&lumen, 2.0), Modifiers::NONE);
    assert!(looping(&lumen, id).is_some());
    lumen.harness.get_by_label("Zoom to clip").click();
    lumen.frames(3);
    let s = shown(&lumen);
    let (_, end) = s.brace.expect("the brace");
    let y = s.band.center().y;
    let grip = Pos2::new(end - 1.0, y);
    let edge = Pos2::new(s.body.right() - 2.0, y);
    lumen.harness.hover_at(grip);
    lumen.harness.drag_at(grip);
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(grip + (edge - grip) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.run_until("the view scrolls on", |app| {
        app.app()
            .audio_editor_shown()
            .is_some_and(|now| now.view.left > s.view.left)
    });
    lumen.harness.drop_at(edge);
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(looping(&lumen, id).is_some(), "the drag was one step");
}

#[test]
fn a_wavy_envelope_stroke_with_pauses_is_one_undo_step_stopped_or_playing() {
    for playing in [false, true] {
        let (mut lumen, id) = opened(&format!("editor-wavy-{playing}"));
        lumen.harness.hover_at(high(&lumen, 2.0));
        lumen.frames(1);
        lumen.key(Modifiers::NONE, Key::B);
        if playing {
            lumen.key(Modifiers::NONE, Key::Space);
            lumen.frames(5);
        }
        let body = shown(&lumen).body;
        let at = |lumen: &Lumen, beats: f64, f: f32| {
            Pos2::new(x_of(lumen, beats), body.top() + body.height() * f)
        };
        let from = at(&lumen, 1.1, 0.5);
        lumen.harness.hover_at(from);
        lumen.harness.drag_at(from);
        lumen.frames(2);
        for i in 1..=30 {
            let beat = 1.1 + i as f64 * 0.06;
            let f = 0.5 + 0.3 * ((i as f32) * 0.7).sin();
            lumen.harness.hover_at(at(&lumen, beat, f));
            lumen.frames(if i % 5 == 0 { 4 } else { 1 });
        }
        lumen.harness.drop_at(at(&lumen, 2.9, 0.5));
        lumen.frames(3);
        if playing {
            lumen.key(Modifiers::NONE, Key::Space);
        }
        let drawn = points(&lumen, id, Kind::Gain);
        assert!(drawn.len() > 4, "{drawn:?}");
        lumen.key(Modifiers::COMMAND, Key::Z);
        assert_eq!(
            points(&lumen, id, Kind::Gain),
            [],
            "playing {playing}: one undo step"
        );
    }
}
