use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{audio, bench, callback_bench, settings};

pub struct Launch {
    pub(crate) started: Instant,
    pub(crate) settings: settings::Settings,
    pub(crate) settings_warning: Option<String>,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) audio: audio::Choice,
    pub(crate) midi: crate::midi::Source,
    pub(crate) plugins: crate::plugins::Source,
    pub(crate) autosave_dir: Option<PathBuf>,
    pub(crate) user_dir: Option<PathBuf>,
    pub(crate) autosave_after: Option<Duration>,
    pub(crate) crash_dir: Option<PathBuf>,
    pub(crate) crash_test: Option<crate::crash::Test>,
    pub(crate) open: Option<PathBuf>,
    pub(crate) open_audio: Option<PathBuf>,
    pub(crate) stress: bool,
    pub(crate) bench_lanes: bool,
    pub(crate) audio_settings: bool,
    pub(crate) recover_latest: bool,
    pub(crate) scale: Option<f32>,
    pub(crate) appearance: Option<String>,
    pub(crate) smoke_test: bool,
    pub(crate) screenshot: Option<PathBuf>,
    pub(crate) bench: Option<bench::Bench>,
    pub(crate) bench_size: Option<(f32, f32)>,
    pub(crate) callback_bench: Option<callback_bench::Callback>,
    pub(crate) autoplay: Option<Duration>,
    pub(crate) test_latency: Option<f64>,
    pub(crate) slow_disk: Option<Duration>,
    pub(crate) safe_mode: bool,
    pub(crate) preset_places: Option<crate::preset_files::Places>,
    pub(crate) plugin_marker: Option<PathBuf>,
    pub(crate) save_as: Option<PathBuf>,
    pub(crate) scripted_dialogs: bool,
    pub(crate) first_run: bool,
    pub(crate) first_start_rule: bool,
    pub(crate) instance: Option<crate::instance::Instance>,
    pub(crate) association: crate::association::Source,
    pub(crate) quit_after: Option<Duration>,
}

impl Launch {
    pub(crate) fn settings_path(args: &[String]) -> Option<PathBuf> {
        args.iter()
            .position(|a| a == "--settings")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
            .or_else(settings::default_path)
    }

    pub(crate) fn project_arg(args: &[String]) -> Option<PathBuf> {
        args.iter()
            .skip(1)
            .find(|a| a.to_ascii_lowercase().ends_with(".lumen"))
            .map(PathBuf::from)
    }

    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1).cloned())
        };
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let settings_path = Self::settings_path(&args);
        let (settings, settings_warning) = settings_path
            .as_deref()
            .map(settings::load)
            .unwrap_or_default();
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
            user_dir: value("--user-dir").map(PathBuf::from).or_else(|| {
                if has("--autosave-dir") {
                    None
                } else {
                    crate::app::projects::user_folder()
                }
            }),
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
            open: Self::project_arg(&args),
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
            slow_disk: None,
            safe_mode: has("--safe-mode"),
            preset_places: None,
            plugin_marker,
            save_as: value("--save-as").map(PathBuf::from),
            scripted_dialogs: false,
            first_run: has("--first-run"),
            first_start_rule: false,
            instance: None,
            quit_after: value("--quit-after-ms")
                .and_then(|ms| ms.parse().ok())
                .map(Duration::from_millis),
            association: if has("--settings") || has("--autosave-dir") || has("--crash-dir") {
                crate::association::Source::Isolated
            } else {
                crate::association::Source::System
            },
        }
    }
}
