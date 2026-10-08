//! How Lumen was started: from the command line (`main`), or by a test (the
//! end-to-end suite, `e2e`), which starts the same `App` with its own
//! settings, autosave folder and the silent test device, so it never touches
//! the user's.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{audio, bench, settings};

/// Everything start-up needs to know.
pub struct Launch {
    pub started: Instant,
    pub settings: settings::Settings,
    /// A problem reading the settings, shown once started.
    pub settings_warning: Option<String>,
    /// Where settings are saved; `None` never saves them.
    pub settings_path: Option<PathBuf>,
    pub audio: audio::Choice,
    /// Where autosaves go; `None` turns autosave off.
    pub autosave_dir: Option<PathBuf>,
    pub autosave_after: Option<Duration>,
    /// A project to open (`lumen song.lumen`).
    pub open: Option<PathBuf>,
    /// `--open-audio`: an audio file added at bar 1.
    pub open_audio: Option<PathBuf>,
    /// `--stress`: the generated quality-bar project.
    pub stress: bool,
    /// `--audio-settings`: the audio settings window open at start.
    pub audio_settings: bool,
    /// `--recover-latest` (tests): press Recover at once.
    pub recover_latest: bool,
    /// `--scale` and `--appearance`: one-off overrides, not saved.
    pub scale: Option<f32>,
    pub appearance: Option<String>,
    /// `--smoke-test`: render a few frames, report, close.
    pub smoke_test: bool,
    /// `--screenshot`: render, save a screenshot, close.
    pub screenshot: Option<PathBuf>,
    /// `--bench-frames`: the frame-time benchmark, and `--bench-size`.
    pub bench: Option<bench::Bench>,
    pub bench_size: Option<(f32, f32)>,
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    pub autoplay: Option<Duration>,
    /// `--save-as` (tests): save once imports are done, then close.
    pub save_as: Option<PathBuf>,
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

    /// A clean start for a test: default settings that are never saved,
    /// autosaves in `dir`, and the silent test device.
    #[cfg(test)]
    pub fn for_test(dir: &std::path::Path) -> Self {
        Self {
            started: Instant::now(),
            settings: settings::Settings::default(),
            settings_warning: None,
            settings_path: None,
            audio: audio::Choice::test_device(),
            autosave_dir: Some(dir.join("autosave")),
            autosave_after: None,
            open: None,
            open_audio: None,
            stress: false,
            audio_settings: false,
            recover_latest: false,
            scale: None,
            appearance: None,
            smoke_test: false,
            screenshot: None,
            bench: None,
            bench_size: None,
            autoplay: None,
            save_as: None,
        }
    }
}
