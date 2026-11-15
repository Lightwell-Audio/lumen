use std::path::PathBuf;

use lumen_core::meter::{Meter, MeterPoint};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature, constant_meter};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};

mod common;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn at(quarters: i64, numerator: u32, denominator: u32) -> MeterPoint {
    MeterPoint {
        at: Ticks::quarters(quarters),
        meter: Meter {
            numerator,
            denominator,
        },
    }
}

fn golden_v24_expected() -> Project {
    let mut project = project_of("Golden v24", vec![]);
    project.auto_crossfade = false;
    project.tempo = vec![
        TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        },
        TempoPoint {
            at: Ticks::quarters(8),
            bpm: 96.5,
            ramp: false,
        },
        TempoPoint {
            at: Ticks::quarters(20),
            bpm: 140.0,
            ramp: false,
        },
    ];
    project.tempo[1].ramp = true;
    project.meter = vec![at(0, 4, 4), at(8, 7, 8), at(15, 3, 4)];
    project
}

#[test]
fn golden_v24_file_loads_forever() {
    common::golden("v24", golden_v24_expected);
}

#[test]
fn an_older_project_keeps_its_one_signature_as_the_first_change() {
    for old in ["v1/golden.lumen", "v9/golden.lumen", "v23/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert_eq!(
            project.meter,
            constant_meter(TimeSignature {
                numerator: 4,
                denominator: 4
            }),
            "{old}"
        );
    }
}

#[test]
fn a_project_is_written_with_a_meter_list_and_no_scalar() {
    let text = String::from_utf8(to_bytes(&golden_v24_expected()).unwrap()).unwrap();
    assert!(text.contains("\"meter\""));
    assert!(!text.contains("time_signature"));
}

#[test]
fn a_meter_map_that_cannot_work_is_refused_with_a_reason() {
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v24_expected();
        edit(&mut project);
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let why = refused(&|p| p.meter.clear());
    assert!(why.contains("meter"), "{why}");
    let why = refused(&|p| p.meter[0].at = Ticks::quarters(1));
    assert!(why.contains("at 0"), "{why}");
    let why = refused(&|p| p.meter[1].at = Ticks::quarters(6));
    assert!(why.contains("bar line"), "{why}");
    let why = refused(&|p| p.meter[1].at = Ticks::ZERO);
    assert!(why.contains("not after"), "{why}");
    let why = refused(&|p| p.meter[1].meter.denominator = 3);
    assert!(why.contains("not valid"), "{why}");
    let why = refused(&|p| p.meter = vec![at(0, 65, 4)]);
    assert!(why.contains("out of range"), "{why}");
    let why = refused(&|p| {
        p.meter = (0..5_000)
            .map(|i| at(i * 12, 4 - (i % 2) as u32, 4))
            .collect();
    });
    assert!(why.contains("too many"), "{why}");
}

#[test]
fn a_damaged_meter_file_is_not_opened() {
    let text = String::from_utf8(to_bytes(&golden_v24_expected()).unwrap()).unwrap();
    let damaged = text.replacen("\"numerator\": 7", "\"numerator\": 700", 1);
    assert_ne!(text, damaged);
    assert!(from_bytes(damaged.as_bytes()).is_err());
}
