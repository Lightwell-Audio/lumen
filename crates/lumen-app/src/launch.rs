//! How Lumen was started: from the command line (`run`), or by a test (the
//! end-to-end suite in `tests/e2e/`, through `testing`), which starts the same `App` with its own
//! settings, autosave folder and the silent test device, so it never touches
//! the user's.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{audio, bench, settings};

/// Everything start-up needs to know.
pub struct Launch {
    pub(crate) started: Instant,
    pub(crate) settings: settings::Settings,
    /// A problem reading the settings, shown once started.
    pub(crate) settings_warning: Option<String>,
    /// Where settings are saved; `None` never saves them.
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) audio: audio::Choice,
    /// Where autosaves go; `None` turns autosave off.
    pub(crate) autosave_dir: Option<PathBuf>,
    pub(crate) autosave_after: Option<Duration>,
    /// A project to open (`lumen song.lumen`).
    pub(crate) open: Option<PathBuf>,
    /// `--open-audio`: an audio file added at bar 1.
    pub(crate) open_audio: Option<PathBuf>,
    /// `--stress`: the generated quality-bar project.
    pub(crate) stress: bool,
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
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    pub(crate) autoplay: Option<Duration>,
    /// `--save-as` (tests): save once imports are done, then close.
    pub(crate) save_as: Option<PathBuf>,
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
        let screenshot = value("--screenshot").map(PathBuf::from);
        Self {
            started: Instant::now(),
            settings,
            settings_warning,
            settings_path,
            audio,
            autosave_dir: value("--autosave-dir")
                .map(PathBuf::from)
                .or_else(crate::autosave::default_dir),
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
            audio_settings: has("--audio-settings"),
            recover_latest: has("--recover-latest"),
            scale: value("--scale")
                .and_then(|s| s.parse().ok())
                .filter(|s| (0.5..=3.0).contains(s)),
            appearance: value("--appearance"),
            smoke_test: screenshot.is_some() || has("--smoke-test"),
            screenshot,
            bench: value("--bench-frames").map(|out| bench::Bench::new(PathBuf::from(out))),
            bench_size: value("--bench-size").and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            }),
            autoplay: value("--autoplay-ms")
                .and_then(|ms| ms.parse().ok())
                .map(Duration::from_millis),
            save_as: value("--save-as").map(PathBuf::from),
        }
    }
}
