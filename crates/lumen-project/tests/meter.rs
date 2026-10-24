//! Format v24 (M7 slice A6): the time signature may change along the song.
//! Older files hold the one signature they always had, as the first change at
//! zero; a damaged map is refused with a reason.

use std::fs;
use std::path::PathBuf;

use lumen_core::meter::{Meter, MeterPoint};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature, constant_meter};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};

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

/// The model the v24 golden file must always load as. Never change it.
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
    // The second tempo glides to the third; the others hold.
    project.tempo[1].ramp = true;
    // Two bars of 4/4, two of 7/8 (3.5 quarters each), then 3/4.
    project.meter = vec![at(0, 4, 4), at(8, 7, 8), at(15, 3, 4)];
    project
}

#[test]
fn golden_v24_file_loads_forever() {
    let path = fixture("v24/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v24_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v24 golden file must load");
    assert_eq!(project, golden_v24_expected());
}

#[test]
fn an_older_project_keeps_its_one_signature_as_the_first_change() {
    // The v23 golden holds 4/4; the v9 golden is older than the mixer.
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
    // 6 quarters is in the middle of a 4/4 bar.
    let why = refused(&|p| p.meter[1].at = Ticks::quarters(6));
    assert!(why.contains("bar line"), "{why}");
    let why = refused(&|p| p.meter[1].at = Ticks::ZERO);
    assert!(why.contains("not after"), "{why}");
    let why = refused(&|p| p.meter[1].meter.denominator = 3);
    assert!(why.contains("not valid"), "{why}");
    let why = refused(&|p| p.meter = vec![at(0, 65, 4)]);
    assert!(why.contains("out of range"), "{why}");
    let why = refused(&|p| {
        // 4/4 and 3/4 take turns every 12 quarters, so each is on a bar line.
        p.meter = (0..5_000)
            .map(|i| at(i * 12, 4 - (i % 2) as u32, 4))
            .collect();
    });
    assert!(why.contains("too many"), "{why}");
}

#[test]
fn a_damaged_meter_file_is_not_opened() {
    let text = String::from_utf8(to_bytes(&golden_v24_expected()).unwrap()).unwrap();
    // The file says 7/8 begins at an unnamed tick: break its order.
    let damaged = text.replacen("\"numerator\": 7", "\"numerator\": 700", 1);
    assert_ne!(text, damaged);
    assert!(from_bytes(damaged.as_bytes()).is_err());
}
