//! Fades and clip gain (M3 slice 6d, the owner's F3 and G1): drag a fade's
//! handle along, bend it by its middle dot, drag the gain line, reset any
//! of them with a double-click, and pick any fade shape from the menu.
//! Each is one undo step, and the audio follows.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, Id};

use egui_kittest::kittest::Queryable;

use super::{Lumen, wav};

/// Lumen with one two-second file dropped in, zoomed in until its clip is
/// wide enough for its handles.
fn with_a_wide_clip(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("tone.wav"), 2.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    for _ in 0..2 {
        lumen.key(Modifiers::NONE, Key::Equals);
    }
    lumen.frames(20);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    assert!(handles(&lumen, clip).wave.width() > 200.0, "a wide clip");
    (lumen, clip)
}

fn handles(lumen: &Lumen, clip: Id) -> lumen_app::testing::ClipHandlesAt {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .clip_handles(&ctx, clip)
        .expect("the clip shows its handles")
}

fn clip(lumen: &Lumen, id: Id) -> Clip {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .map(|c| (**c).clone())
        .expect("the clip exists")
}

fn gain(lumen: &Lumen, id: Id) -> f32 {
    match clip(lumen, id).content {
        ClipContent::Audio { gain_db, .. } => gain_db,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    }
}

/// Clicks twice at `p`, quickly, after a pause (so it is not counted
/// with an earlier double-click).
fn double_click(lumen: &mut Lumen, p: Pos2) {
    lumen.frames(40);
    lumen.harness.hover_at(p);
    lumen.frames(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            lumen.harness.event(Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        lumen.frames(1);
    }
    lumen.frames(2);
}

#[test]
fn dragging_a_fade_handle_fades_the_clip_in_one_step() {
    let (mut lumen, id) = with_a_wide_clip("fade-drag");
    let at = handles(&lumen, id);
    lumen.harness.hover_at(at.fade_in);
    lumen.frames(2);
    assert!(
        lumen.info().starts_with("Fade in: drag to set its length"),
        "{}",
        lumen.info()
    );

    // A quarter of the way in: a quarter of the clip, unsnapped.
    let to = at.fade_in + vec2(at.wave.width() * 0.25, 0.0);
    lumen.drag(at.fade_in, to, Modifiers::NONE);
    let c = clip(&lumen, id);
    let quarter = c.length.0 as f64 / 4.0;
    assert!(
        (c.fade_in.length.0 as f64 - quarter).abs() < quarter * 0.1,
        "{:?} of {:?}",
        c.fade_in.length,
        c.length
    );
    assert_eq!(c.start, Ticks::ZERO, "the clip stays put");
    assert_eq!(lumen.app().undo_label(), Some("Fade in"));

    // The fade out, from the other corner, can never overlap it.
    let out = handles(&lumen, id).fade_out;
    lumen.drag(
        out,
        Pos2::new(at.wave.left() - 50.0, out.y),
        Modifiers::NONE,
    );
    let c = clip(&lumen, id);
    assert_eq!(c.fade_out.length, c.length - c.fade_in.length);
    assert_eq!(lumen.app().undo_label(), Some("Fade out"));

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(clip(&lumen, id).fade_in.is_none(), "one undo step each");
}

/// The owner's C3: the dot moves freely, left and right as well as up and
/// down, and the curve passes through it; Alt adds S-shape.
#[test]
fn the_dot_bends_a_fade_freely_and_alt_adds_s_shape() {
    let (mut lumen, id) = with_a_wide_clip("fade-bend");
    let at = handles(&lumen, id);
    lumen.drag(
        at.fade_in,
        at.fade_in + vec2(at.wave.width() * 0.3, 0.0),
        Modifiers::NONE,
    );
    let dot = handles(&lumen, id).bend_in.expect("a bend dot");
    lumen.harness.hover_at(dot);
    lumen.frames(2);
    assert!(
        lumen
            .info()
            .starts_with("Fade curve: drag to bend · Alt+drag to S-shape"),
        "{}",
        lumen.info()
    );

    // Left and up: the dot follows the pointer, and the curve with it.
    let to = dot + vec2(-15.0, -6.0);
    lumen.drag(dot, to, Modifiers::NONE);
    let moved = handles(&lumen, id).bend_in.expect("a bend dot");
    assert!(
        moved.distance(to) < 1.5,
        "the dot at {moved:?}, the pointer at {to:?}"
    );
    let curve = clip(&lumen, id).fade_in.curve;
    let bent = curve.handle();
    assert!(bent.dot[0] < 0.45 && bent.dot[1] > 0.55, "{bent:?}");
    assert_eq!(lumen.app().undo_label(), Some("Fade curve"));

    // Right and down: the other way.
    let to = moved + vec2(30.0, 10.0);
    lumen.drag(moved, to, Modifiers::NONE);
    let other = clip(&lumen, id).fade_in.curve.handle();
    assert!(
        other.dot[0] > bent.dot[0] && other.dot[1] < bent.dot[1],
        "{other:?}"
    );

    // Alt and up: S-shape, the dot staying where it is.
    let dot = handles(&lumen, id).bend_in.expect("a bend dot");
    lumen.drag(dot, dot - vec2(0.0, 20.0), Modifiers::ALT);
    let s = clip(&lumen, id).fade_in.curve.handle();
    assert!((s.s - 0.5).abs() < 0.1, "{s:?}");
    assert!((s.dot[0] - other.dot[0]).abs() < 1e-3 && (s.dot[1] - other.dot[1]).abs() < 1e-3);

    let dot = handles(&lumen, id).bend_in.expect("a bend dot");
    double_click(&mut lumen, dot);
    assert!(clip(&lumen, id).fade_in.curve.is_linear(), "straight again");
    let handle = handles(&lumen, id).fade_in;
    double_click(&mut lumen, handle);
    assert!(clip(&lumen, id).fade_in.is_none(), "no fade");
}

#[test]
fn the_gain_line_sets_clip_gain_finely_with_shift() {
    let (mut lumen, id) = with_a_wide_clip("clip-gain");
    let line = handles(&lumen, id).gain;
    lumen.harness.hover_at(line);
    lumen.frames(2);
    assert!(
        lumen.info().starts_with("Clip gain 0.0 dB: drag to change"),
        "{}",
        lumen.info()
    );

    lumen.drag(line, line + vec2(0.0, 16.0), Modifiers::NONE);
    let coarse = gain(&lumen, id);
    // A short clip: half a dB a point at most, so 16 points are 8 dB.
    assert!((coarse + 8.0).abs() < 0.6, "{coarse} dB");
    assert_eq!(lumen.app().undo_label(), Some("Clip gain"));
    assert!(handles(&lumen, id).gain.y > line.y, "the line went down");

    let line = handles(&lumen, id).gain;
    lumen.drag(line, line + vec2(0.0, 16.0), Modifiers::SHIFT);
    let fine = gain(&lumen, id) - coarse;
    assert!(
        (fine - coarse / 10.0).abs() < 0.2,
        "Shift moves a tenth as far: {fine} dB against {coarse} dB"
    );

    let line = handles(&lumen, id).gain;
    double_click(&mut lumen, line);
    assert_eq!(gain(&lumen, id), 0.0);
}

#[test]
fn every_fade_shape_is_in_the_right_click_menu() {
    let (mut lumen, id) = with_a_wide_clip("fade-menu");
    let at = handles(&lumen, id);
    lumen.drag(
        at.fade_out,
        at.fade_out - vec2(at.wave.width() * 0.3, 0.0),
        Modifiers::NONE,
    );
    let p = Pos2::new(at.wave.center().x, at.wave.bottom() - 3.0);
    lumen.harness.hover_at(p);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
    // One Fades submenu (owner's A, MT-APP-14 run 2); a submenu's name ends
    // with its arrow ("Fades ⏵").
    lumen.harness.get_by_label_contains("Fades").click();
    lumen.frames(3);
    lumen.click("Fade out: S-curve");
    assert_eq!(clip(&lumen, id).fade_out.curve, FadeShape::SCurve.curve());
}

/// Heard as drawn: a fade in starts the clip from silence, and clip gain
/// turns the whole clip down.
#[test]
fn fades_and_gain_are_heard() {
    let (mut lumen, id) = with_a_wide_clip("fade-heard");
    let at = handles(&lumen, id);
    lumen.drag(
        at.fade_in,
        at.fade_in + vec2(at.wave.width() * 0.25, 0.0),
        Modifiers::NONE,
    );
    let line = handles(&lumen, id).gain;
    // Down about 12 dB, at half a dB a point.
    lumen.drag(line, line + vec2(0.0, 24.0), Modifiers::NONE);
    let db = gain(&lumen, id);
    let fade = clip(&lumen, id).fade_in.length;
    // 120 BPM: 24,000 frames a beat.
    let fade_frames = (fade.0 as f64 / Ticks::quarters(1).0 as f64 * 24_000.0) as usize;

    lumen.app_mut().heard();
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the fade to have played", |app| {
        app.playhead() > fade + Ticks::quarters(1)
    });
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.app_mut().heard();
    let loudest = |frames: &[(f32, f32)]| frames.iter().map(|s| s.0.abs()).fold(0.0, f32::max);
    // The test tone peaks at about 0.0916; a centred track plays 3 dB
    // down (the default pan law, M4).
    let full = 0.0916 * 10f32.powf(db / 20.0) * std::f32::consts::FRAC_1_SQRT_2;
    let start = loudest(&heard[..fade_frames / 4]);
    let after = loudest(&heard[fade_frames + 100..fade_frames + 4_000]);
    assert!(start < full * 0.3, "quiet at first: {start} of {full}");
    assert!(
        (after - full).abs() < full * 0.1,
        "then full: {after} of {full}"
    );
}
