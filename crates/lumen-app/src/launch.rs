//! How Lumen was started: from the command line (`run`), or by a test (the
//! end-to-end suite in `tests/e2e/`, through `testing`), which starts the same `App` with its own
//! settings, autosave folder and the silent test device, so it never touches
//! the user's.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{audio, bench, callback_bench, settings};

/// Everything start-up needs to know.
pub struct Launch {
    pub(crate) started: Instant,
    pub(crate) settings: settings::Settings,
    /// A problem reading the settings, shown once started.
    pub(crate) settings_warning: Option<String>,
    /// Where settings are saved; `None` never saves them.
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) audio: audio::Choice,
    /// Where MIDI ports come from (`--midi`).
    pub(crate) midi: crate::midi::Source,
    /// Where plugins come from (`--plugins`, `--plugin-roots`).
    pub(crate) plugins: crate::plugins::Source,
    /// Where autosaves go; `None` turns autosave off.
    pub(crate) autosave_dir: Option<PathBuf>,
    pub(crate) autosave_after: Option<Duration>,
    /// Where crash reports go (`--crash-dir`); `None`: nowhere.
    pub(crate) crash_dir: Option<PathBuf>,
    /// `--crash-test` (a panic) or `--crash-test-memory` (a failed
    /// allocation) on the first frame, to check crash reports.
    pub(crate) crash_test: Option<crate::crash::Test>,
    /// A project to open (`lumen song.lumen`).
    pub(crate) open: Option<PathBuf>,
    /// `--open-audio`: an audio file added at bar 1.
    pub(crate) open_audio: Option<PathBuf>,
    /// `--stress`: the generated quality-bar project.
    pub(crate) stress: bool,
    /// `--bench-lanes`: with `--stress`, every track has a volume lane of
    /// 200 points and shows it (the frame benchmark with lanes, M7 slice A1).
    pub(crate) bench_lanes: bool,
    /// `--audio-settings`: the audio settings window open at start.
    pub(crate) audio_settings: bool,
    /// `--recover-latest` (tests): press Recover at once.
    pub(crate) recover_latest: bool,
    /// `--scale` and `--appearance`: one-off overrides, not saved.
    pub(crate) scale: Option<f32>,
    pub(crate) appearance: Option<String>,
    /// `--smoke-test`: render a few frames, report, close.
    pub(crate) smoke_test: bool,
    /// `--screenshot`: render, save a screenshot, close.
    pub(crate) screenshot: Option<PathBuf>,
    /// `--bench-frames`: the frame-time benchmark, and `--bench-size`.
    pub(crate) bench: Option<bench::Bench>,
    pub(crate) bench_size: Option<(f32, f32)>,
    /// `--bench-callback <out.json>` with `--bench-seconds`: the callback
    /// benchmark (M6 slice 7c).
    pub(crate) callback_bench: Option<callback_bench::Callback>,
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    pub(crate) autoplay: Option<Duration>,
    /// `--test-latency <ms>`: the engine's synthetic latent device on the
    /// first track, to try delay compensation before devices exist (M4
    /// slice 6).
    pub(crate) test_latency: Option<f64>,
    /// `--safe-mode`: no plugin is scanned, loaded or hosted (M6 slice 5e).
    pub(crate) safe_mode: bool,
    /// Where preset files are kept, when the caller chose (tests); the
    /// standard folders when plugins come from this machine.
    pub(crate) preset_places: Option<crate::preset_files::Places>,
    /// Where the marker of a start that did not finish loading plugins is
    /// kept: beside the settings.
    pub(crate) plugin_marker: Option<PathBuf>,
    /// `--save-as` (tests): save once imports are done, then close.
    pub(crate) save_as: Option<PathBuf>,
    /// Tests: file dialogs are answered by the test, never shown.
    pub(crate) scripted_dialogs: bool,
}

impl Launch {
    /// From the command line (see the list at the top of `main.rs`).
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1).cloned())
        };
        let has = |flag: &str| args.iter().any(|a| a == flag);
        // `--settings` (tests) keeps a run away from the user's settings.
        let settings_path = value("--settings")
            .map(PathBuf::from)
            .or_else(settings::default_path);
        let (settings, settings_warning) = settings_path
            .as_deref()
            .map(settings::load)
            .unwrap_or_default();
        // `--audio` is a one-off choice; otherwise the saved device,
        // otherwise the system default.
        let audio = match value("--audio") {
            Some(arg) => audio::Choice::parse(&arg).unwrap_or_else(|e| {
                eprintln!("{e}; using the default output");
                audio::Choice::Default
            }),
            None => settings
                .audio
                .clone()
                .map_or(audio::Choice::Default, audio::Choice::Saved),
        };
        let midi = value("--midi").map_or(crate::midi::Source::System, |arg| {
            crate::midi::Source::parse(&arg).unwrap_or_else(|e| {
                eprintln!("{e}; using the system's MIDI");
                crate::midi::Source::System
            })
        });
        let screenshot = value("--screenshot").map(PathBuf::from);
        let plugin_marker = settings_path
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.join(crate::app::safe_mode::MARKER));
        Self {
            started: Instant::now(),
            settings,
            settings_warning,
            settings_path,
            audio,
            midi,
            plugins: crate::plugins::Source::from_args(&args),
            autosave_dir: value("--autosave-dir")
                .map(PathBuf::from)
                .or_else(crate::autosave::default_dir),
            crash_dir: value("--crash-dir")
                .map(PathBuf::from)
                .or_else(crate::crash::default_dir),
            crash_test: if has("--crash-test") {
                Some(crate::crash::Test::Panic)
            } else if has("--crash-test-memory") {
                Some(crate::crash::Test::Memory)
            } else {
                None
            },
            autosave_after: value("--autosave-after-ms")
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis),
            open: args
                .iter()
                .skip(1)
                .find(|a| a.to_ascii_lowercase().ends_with(".lumen"))
                .map(PathBuf::from),
            open_audio: value("--open-audio").map(PathBuf::from),
            stress: has("--stress"),
            bench_lanes: has("--bench-lanes"),
            audio_settings: has("--audio-settings"),
            recover_latest: has("--recover-latest"),
            scale: value("--scale")
                .and_then(|s| s.parse().ok())
                .filter(|s| (0.5..=3.0).contains(s)),
            appearance: value("--appearance"),
            smoke_test: screenshot.is_some() || has("--smoke-test"),
            screenshot,
            bench: value("--bench-frames").map(|out| {
                bench::Bench::new(PathBuf::from(out)).with_editor(has("--bench-editor"))
            }),
            callback_bench: value("--bench-callback").map(|out| {
                let seconds = value("--bench-seconds")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30.0);
                callback_bench::Callback::new(PathBuf::from(out), seconds, Instant::now())
            }),
            bench_size: value("--bench-size").and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            }),
            autoplay: value("--autoplay-ms")
                .and_then(|ms| ms.parse().ok())
                .map(Duration::from_millis),
            test_latency: value("--test-latency")
                .and_then(|ms| ms.parse().ok())
                .filter(|ms: &f64| ms.is_finite() && *ms > 0.0),
            safe_mode: has("--safe-mode"),
            preset_places: None,
            plugin_marker,
            save_as: value("--save-as").map(PathBuf::from),
            scripted_dialogs: false,
        }
    }
}
