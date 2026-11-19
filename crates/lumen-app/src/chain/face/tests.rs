use eframe::egui::{Color32, Rect, pos2, vec2};
use lumen_devices::utility as device;

use super::*;

fn slot(id: u32, value: f64) -> Slot {
    Slot {
        id,
        name: String::new(),
        text: String::new(),
        value,
        default: 0.0,
        steps: 0,
        read_only: false,
        automatable: true,
        choices: Vec::new(),
        over: false,
        mapped: None,
        waiting: false,
    }
}

fn nothing(_: &Scene<'_>) {}

static FACE: Drawing = Drawing {
    card: vec2(380.0, 330.0),
    screen: 176.0,
    deep: Color32::BLACK,
    reads: &[device::GAIN, device::WIDTH],
    draw: nothing,
    controls: &[
        Control::knob(device::WIDTH, "Spread", |c| c),
        Control::pill(device::GAIN, "Level", |c| c),
    ],
    handles: &[],
};

#[test]
fn prism_has_a_drawing_and_an_unknown_device_has_none() {
    let prism = drawing_of(device::NAME).expect("Prism has a drawing");
    assert_eq!(prism.card, vec2(300.0, 290.0));
    assert_eq!(prism.screen, 136.0);
    assert_eq!(prism.dials().count(), 4);
    assert_eq!(
        prism.params(),
        vec![
            device::GAIN,
            device::PAN,
            device::WIDTH,
            device::BASS_MONO,
            device::POLARITY_L
        ]
    );
    assert!(drawing_of("no such device").is_none());
}

#[test]
fn a_drawings_params_are_its_controls_then_what_it_reads_once_each() {
    assert_eq!(FACE.params(), vec![device::WIDTH, device::GAIN]);
    assert_eq!(FACE.controls[0].kind, Kind::Knob);
    assert_eq!(FACE.controls[1].kind, Kind::Pill);
    assert_eq!(FACE.dials().count(), 1);
}

#[test]
fn a_value_is_read_normalised_and_in_its_own_units() {
    let slots = [slot(device::WIDTH, 0.5)];
    let values = Values {
        slots: &slots,
        table: device::TABLE,
        rate: 48_000,
    };
    assert!((values.norm(device::WIDTH) - 0.5).abs() < 1e-6);
    let info = lumen_devices::params::find(device::TABLE, device::WIDTH).unwrap();
    assert!((values.plain(device::WIDTH) - info.value(0.5)).abs() < 1e-4);
    assert!((values.norm_of(device::WIDTH, info.value(0.5)) - 0.5).abs() < 1e-4);
    assert_eq!(values.norm(device::GAIN), 0.0);
}

#[test]
fn a_scene_drains_colour_while_dim() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = [];
    let scene = |power| Scene {
        painter: &painter,
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)),
        step: Step::Whole,
        hue: Color32::RED,
        values: Values {
            slots: &slots,
            table: device::TABLE,
            rate: 48_000,
        },
        power,
        level: 0.0,
        awake: 1.0,
        still: false,
        handle_lit: [0.0; MAX_HANDLES],
        settle: 1.0,
        picked: 0,
        parts_lit: [0.0; MAX_PARTS],
        spectrum: None,
        mist: 0.0,
        bpm: 120.0,
        reading: None,
        id: eframe::egui::Id::NULL,
    };
    assert_eq!(scene(1.0).ink(Color32::RED), Color32::RED);
    assert_ne!(scene(0.0).ink(Color32::RED), Color32::RED);
}
