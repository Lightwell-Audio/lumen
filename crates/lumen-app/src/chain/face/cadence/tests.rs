use eframe::egui::{Context, Id, RawInput};

use super::*;

fn frame(ctx: &Context, time: f64, level: f32, still: bool) -> (f32, Option<Duration>) {
    forget_asks(ctx);
    let mut awake = 0.0;
    let input = RawInput {
        time: Some(time),
        ..RawInput::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        awake = wake(ui.ctx(), Id::new("face"), level, still);
    });
    out.textures_delta.clear();
    (awake, asked(ctx))
}

#[test]
fn a_face_with_no_signal_asks_for_no_frame() {
    let ctx = Context::default();
    for step in 0..10 {
        let (awake, ask) = frame(&ctx, f64::from(step) * 0.016, 0.0, false);
        assert_eq!(awake, 0.0);
        assert_eq!(ask, None, "frame {step}");
    }
}

#[test]
fn a_face_with_signal_asks_for_the_face_frame() {
    let ctx = Context::default();
    for step in 0..10 {
        let (_, ask) = frame(&ctx, f64::from(step) * 0.016, 0.5, false);
        assert_eq!(ask, Some(motion::FACE_FRAME), "frame {step}");
    }
}

#[test]
fn a_face_stops_asking_once_the_signal_has_gone_and_it_has_settled() {
    let ctx = Context::default();
    let mut time = 0.0;
    for _ in 0..10 {
        frame(&ctx, time, 0.5, false);
        time += 0.016;
    }
    let mut asked_for = 0.0;
    let mut last = Some(motion::FACE_FRAME);
    while last.is_some() {
        let (_, ask) = frame(&ctx, time, 0.0, false);
        last = ask;
        time += 0.016;
        asked_for += 0.016;
        assert!(asked_for < 3.0, "it keeps asking");
    }
    assert!(
        asked_for > f64::from(motion::SLEEP_HOLD),
        "it holds for the pause: {asked_for}"
    );
    let (awake, ask) = frame(&ctx, time, 0.0, false);
    assert_eq!((awake, ask), (0.0, None));
}

#[test]
fn a_face_asks_for_nothing_while_motion_is_reduced() {
    let ctx = Context::default();
    for step in 0..10 {
        let (awake, ask) = frame(&ctx, f64::from(step) * 0.016, 0.5, true);
        assert_eq!(ask, None);
        assert!(awake <= 1.0);
    }
}

#[test]
fn the_shortest_ask_of_a_frame_is_the_one_kept() {
    let ctx = Context::default();
    forget_asks(&ctx);
    ask(&ctx, motion::AGE_FRAME);
    ask(&ctx, motion::FACE_FRAME);
    ask(&ctx, motion::METER_FRAME);
    assert_eq!(asked(&ctx), Some(motion::FACE_FRAME));
    forget_asks(&ctx);
    assert_eq!(asked(&ctx), None);
}

#[test]
fn again_asks_the_face_frame_unless_motion_is_reduced() {
    let ctx = Context::default();
    forget_asks(&ctx);
    again(&ctx, true);
    assert_eq!(asked(&ctx), None);
    again(&ctx, false);
    assert_eq!(asked(&ctx), Some(motion::FACE_FRAME));
}
