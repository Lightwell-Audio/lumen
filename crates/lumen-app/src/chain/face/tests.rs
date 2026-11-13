//! The hook: which devices have a drawing, what a drawing reads, a value as
//! the drawing sees it, and a face's controls.

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
        choices: Vec::new(),
        over: false,
        mapped: None,
        waiting: false,
    }
}

fn nothing(_: &Scene<'_>) {}

/// A face as Iris will declare one.
static FACE: Drawing = Drawing {
    place: Place::Face,
    deep: Color32::BLACK,
    reads: &[device::GAIN, device::WIDTH],
    draw: nothing,
    controls: &[
        Control::knob(device::WIDTH, "Spread", |s| cell_at(s, 0.25, 4.0)),
        Control::pill(device::GAIN, "Level", |s| cell_at(s, 0.75, 4.0)),
    ],
};

#[test]
fn prism_has_a_band_drawing_and_an_unknown_device_has_none() {
    let prism = drawing_of(device::NAME).expect("Prism has a drawing");
    assert_eq!(prism.place, Place::Band);
    assert!(prism.controls.is_empty());
    assert_eq!(prism.params(), vec![device::WIDTH]);
    assert!(drawing_of("no such device").is_none());
}

#[test]
fn a_faces_params_are_its_controls_then_what_it_reads_once_each() {
    assert_eq!(FACE.params(), vec![device::WIDTH, device::GAIN]);
    assert_eq!(FACE.controls[0].kind, Kind::Knob);
    assert_eq!(FACE.controls[1].kind, Kind::Pill);
    assert_eq!(FACE.controls[0].name, "Spread");
}

#[test]
fn a_value_is_read_normalised_and_in_its_own_units() {
    let slots = [slot(device::WIDTH, 0.5)];
    let values = Values {
        slots: &slots,
        table: device::TABLE,
    };
    assert!((values.norm(device::WIDTH) - 0.5).abs() < 1e-6);
    let info = lumen_devices::params::find(device::TABLE, device::WIDTH).unwrap();
    assert!((values.plain(device::WIDTH) - info.value(0.5)).abs() < 1e-4);
    // A parameter the drawing did not declare reads as zero.
    assert_eq!(values.norm(device::GAIN), 0.0);
}

#[test]
fn a_face_controls_cell_stands_on_the_screens_bottom_edge() {
    let screen = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 150.0));
    for control in FACE.controls {
        let cell = (control.place)(screen);
        assert_eq!(cell.size(), vec2(CELL_W, CELL_H));
        assert!(screen.contains_rect(cell), "{cell:?} is on the screen");
    }
    let left = (FACE.controls[0].place)(screen);
    let right = (FACE.controls[1].place)(screen);
    assert!(left.center().x < right.center().x);
    assert!((left.bottom() - (screen.bottom() - 4.0)).abs() < 1e-4);
}

#[test]
fn a_scene_drains_colour_while_muted() {
    let ctx = eframe::egui::Context::default();
    let painter = ctx.layer_painter(eframe::egui::LayerId::background());
    let slots = [];
    let scene = |muted| Scene {
        painter: &painter,
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)),
        hue: Color32::RED,
        values: Values {
            slots: &slots,
            table: device::TABLE,
        },
        muted,
    };
    assert_eq!(scene(false).ink(Color32::RED), Color32::RED);
    assert_ne!(scene(true).ink(Color32::RED), Color32::RED);
}
