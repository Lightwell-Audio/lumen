//! The card's tests.

use super::body::kilobytes;
use super::*;

static CLOSED: strip::Editor = strip::Editor::Closed;

fn look(face: Face<'_>, bypass: bool, instrument: bool) -> Look<'_> {
    Look {
        name: "Tape Delay",
        vendor: "Vendor D",
        identity: "x",
        instrument,
        bypass,
        selected: false,
        face,
        latency_ms: 2.7,
        reports_latency: true,
        running: true,
        track_color: Color32::WHITE,
        band: 96.0,
        folded: false,
        picture: None,
        slots: &[],
        parameters: 0,
        field: None,
        editor: &CLOSED,
        learning: false,
        sidechain: None,
        outputs: None,
        personal: None,
        compact: false,
        message: Default::default(),
    }
}

#[test]
fn the_footer_says_the_signal_or_what_was_kept() {
    assert_eq!(
        footer_words(&look(Face::Ready, false, true)),
        ("MIDI \u{2192} audio".into(), "2.7 ms".into())
    );
    assert_eq!(
        footer_words(&look(Face::Ready, false, false)),
        ("audio \u{2192} audio".into(), "2.7 ms".into())
    );
    assert_eq!(
        footer_words(&look(Face::Ready, true, false)),
        ("bypassed".into(), "2.7 ms kept".into())
    );
    assert_eq!(
        footer_words(&look(
            Face::Failed(Failure {
                words: "x",
                from: "y",
                keeps: false
            }),
            false,
            false
        ))
        .0,
        "bypassed"
    );
    assert_eq!(
        footer_words(&look(Face::Loading, false, false)).0,
        "starting\u{2026}"
    );
}

#[test]
fn a_device_with_no_latency_to_report_says_nothing_of_one() {
    let mut quiet = look(Face::Ready, false, true);
    quiet.reports_latency = false;
    assert_eq!(
        footer_words(&quiet),
        ("MIDI \u{2192} audio".into(), String::new())
    );
    let mut bypassed = look(Face::Ready, true, true);
    bypassed.reports_latency = false;
    assert_eq!(footer_words(&bypassed), ("bypassed".into(), String::new()));
}

#[test]
fn saved_settings_are_counted_in_whole_kilobytes_rounded_up() {
    assert_eq!(kilobytes(0), "0 KB");
    assert_eq!(kilobytes(1), "1 KB");
    assert_eq!(kilobytes(14 * 1024), "14 KB");
    assert_eq!(kilobytes(14 * 1024 + 1), "15 KB");
}

/// What drawing a set piece costs a frame (run in release: `cargo test
/// --release -p lumen-app --lib set_piece_cost -- --ignored --nocapture`).
/// Prints the average and the slowest frame, per step, with the screen idle
/// and loud; the bar is 0.3 ms a frame.
#[test]
#[ignore = "a measurement: prints times, asserts only a generous bound"]
fn set_piece_cost() {
    use crate::chain::face::drawing_of;
    use crate::chain::geometry::Step;
    use eframe::egui::{Context, RawInput, pos2, vec2};

    let ctx = Context::default();
    crate::fonts::install(&ctx);
    let drawing = drawing_of(lumen_devices::utility::NAME).expect("Prism has a drawing");
    let slots: Vec<strip::Slot> = drawing
        .params()
        .into_iter()
        .map(|id| strip::Slot {
            id,
            name: "Param".into(),
            text: "0.0 dB".into(),
            value: 0.6,
            default: 0.5,
            steps: 0,
            read_only: false,
            choices: Vec::new(),
            over: false,
            mapped: None,
            waiting: false,
        })
        .collect();
    for step in [Step::Whole, Step::Short, Step::Compact] {
        for level in [0.0_f32, 0.9] {
            let size = step.size(drawing.card);
            let mut times = Vec::new();
            for frame in 0..400 {
                let started = std::time::Instant::now();
                let input = RawInput {
                    time: Some(f64::from(frame) / 60.0),
                    screen_rect: Some(eframe::egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(400.0, 400.0),
                    )),
                    ..Default::default()
                };
                let output = ctx.run_ui(input, |ui| {
                    let mut look = look(Face::Ready, false, false);
                    look.name = "Prism";
                    look.identity = lumen_devices::utility::NAME;
                    look.personal = Some(screen::Personal {
                        hue: Color32::from_rgb(0x9A, 0xA4, 0xB4),
                        drawing,
                        slots: &slots,
                        table: lumen_devices::utility::TABLE,
                        step,
                        level,
                    });
                    let rect = eframe::egui::Rect::from_min_size(pos2(10.0, 10.0), size);
                    let _ = paint(ui, rect, eframe::egui::Id::new("cost"), &look);
                });
                // The tessellation is part of what a frame pays.
                let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            // The first frames lay out fonts: leave them out.
            let steady = &times[40..];
            let mean = steady.iter().sum::<f64>() / steady.len() as f64;
            let worst = steady.iter().cloned().fold(0.0, f64::max);
            println!("{step:?} level {level}: mean {mean:.3} ms, worst {worst:.3} ms");
            assert!(mean < 3.0, "{step:?} {mean}");
        }
    }
}
