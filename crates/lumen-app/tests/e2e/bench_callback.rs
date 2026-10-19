//! The callback benchmark (M6 slice 7c): `--bench-callback` plays the opened
//! project once its plugins run and reports the audio callback's time. The
//! plugins and the host program are the real test ones; the audio device is
//! the silent test device, so the numbers are not a measurement of anything
//! but the mode itself.

use lumen_project::model::Id;
use lumen_project::testing::TempDir;

use super::hosting::{
    PLUGIN, effect_device, instrument_device, launch_with, project, wait_for_state,
};

const EFFECT: Id = Id(21);

/// Where the report goes, in a folder of its own.
fn report_path(name: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new(&format!("e2e-bench-callback-{name}"));
    let path = dir.path().join("callback.json");
    (dir, path)
}

#[test]
fn the_benchmark_plays_the_project_and_reports_each_plugins_cost() {
    let (_dir, out) = report_path("report");
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(EFFECT)]);
    let file = out.clone();
    let mut lumen = launch_with("bench-callback", &song, &["Plug"], move |launch| {
        launch.with_callback_bench(file, 1.5)
    });
    // Warm-up (3 s) and 1.5 s of measuring, after the plugins load.
    lumen.wait_for("the benchmark to end", |app| {
        app.callback_report().is_some()
    });
    // Whether it passed depends on the silent device keeping up with the
    // clock, which a busy machine does not promise: not asserted here.
    let (text, _passed, callbacks) = lumen
        .app()
        .callback_report()
        .expect("a report")
        .expect("it ran");
    assert!(callbacks > 0, "{text}");
    assert!(text.contains("Callback benchmark:"), "{text}");
    assert!(
        text.contains("Keys · instrument · Deterministic Test Instrument"),
        "{text}"
    );
    assert!(
        text.contains("Keys · effect 1 · Deterministic Test Effect"),
        "{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("the report file"))
            .expect("json");
    assert!(json["pass"].is_boolean());
    assert_eq!(json["plugins"].as_array().map(Vec::len), Some(2));
    assert!(json["callbacks"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn the_benchmark_does_not_run_over_a_plugin_that_is_not_there() {
    let (_dir, out) = report_path("missing");
    let song = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let file = out.clone();
    // No module is installed, so the plugin is missing.
    let mut lumen = launch_with("bench-callback-missing", &song, &[], move |launch| {
        launch.with_callback_bench(file, 1.0)
    });
    wait_for_state(&mut lumen, PLUGIN, "missing");
    lumen.wait_for("the benchmark to end", |app| {
        app.callback_report().is_some()
    });
    let why = lumen
        .app()
        .callback_report()
        .expect("an ending")
        .expect_err("it could not run");
    assert!(why.contains("plugins are not running"), "{why}");
    assert!(
        why.contains("Deterministic Test Instrument (not installed)"),
        "{why}"
    );
    assert!(
        !out.exists(),
        "no report is written for a run that did not happen"
    );
}
