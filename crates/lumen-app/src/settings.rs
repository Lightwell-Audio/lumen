//! User settings that survive restarts: the audio device and the metronome
//! (M2 slice 8), the appearance (M3 slice 2), and changed shortcuts and the
//! palette's recent commands (M3 slice 7). Preferences proper (paths and
//! the rest) join them in M9.
//!
//! The file is versioned JSON in the user's settings folder. It is written
//! atomically (to a temporary file, then renamed), so a crash never leaves
//! it half-written. A file that cannot be read is moved aside as
//! `settings.json.bad` and Lumen starts with defaults, saying why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lumen_core::product::PRODUCT_NAME;
use lumen_engine::{ClickSound, MetronomeSettings};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

/// The device chosen in the audio settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSettings {
    /// `asio`, `wasapi`, `exclusive` or `cpal`.
    pub driver: String,
    /// The backend's stable device id.
    pub device_id: String,
    /// Shown when the device is missing.
    pub device_name: String,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub buffer_frames: Option<u32>,
    #[serde(default)]
    pub input_channels: Vec<usize>,
    #[serde(default = "stereo")]
    pub output_channels: Vec<usize>,
}

fn stereo() -> Vec<usize> {
    vec![0, 1]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metronome {
    pub enabled: bool,
    pub volume: f32,
    /// `Beep` or `Wood`.
    pub sound: String,
}

impl Default for Metronome {
    fn default() -> Self {
        Self::from(MetronomeSettings::default())
    }
}

impl From<MetronomeSettings> for Metronome {
    fn from(m: MetronomeSettings) -> Self {
        Self {
            enabled: m.enabled,
            volume: m.volume,
            sound: m.sound.name().to_owned(),
        }
    }
}

impl From<&Metronome> for MetronomeSettings {
    fn from(m: &Metronome) -> Self {
        Self {
            enabled: m.enabled,
            volume: m.volume.clamp(0.0, 1.0),
            sound: ClickSound::ALL
                .into_iter()
                .find(|s| s.name() == m.sound)
                .unwrap_or_default(),
        }
    }
}

/// Theme, motion and UI scale (`docs/DESIGN.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// `dark`, `light` or `system`.
    pub theme: String,
    pub reduce_motion: bool,
    /// 1.0 = 100%.
    pub ui_scale: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: crate::theme::Appearance::default().key().to_owned(),
            reduce_motion: false,
            ui_scale: 1.0,
        }
    }
}

/// UI scales offered in the app menu.
pub const UI_SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

impl Appearance {
    pub fn scale(&self) -> f32 {
        if (0.5..=3.0).contains(&self.ui_scale) {
            self.ui_scale
        } else {
            1.0
        }
    }
}

/// How the arrangement follows the playhead while playing (M3 slice 4d;
/// owner: page by default, on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Follow {
    Off,
    /// The view jumps a page when the playhead reaches the right edge.
    #[default]
    Page,
    /// The view scrolls under a playhead that stays in place.
    Continuous,
}

impl Follow {
    pub fn name(self) -> &'static str {
        match self {
            Follow::Off => "Off",
            Follow::Page => "Page",
            Follow::Continuous => "Continuous",
        }
    }
}

/// Projects: recently opened, and whether saving copies audio in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Projects {
    /// Newest first, at most ten.
    pub recent: Vec<String>,
    /// Copy imported audio from outside the project folder into
    /// `Samples/Imported` when saving.
    pub copy_imported: bool,
}

impl Default for Projects {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            copy_imported: true,
        }
    }
}

/// Commands (M3 slice 7): the shortcuts changed from the defaults, by
/// command id ("edit.duplicate": ["Ctrl+D"]; an empty list means none),
/// and the palette's recent commands, newest first. An added field with a
/// default, so older files read as before (no version change).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Commands {
    pub shortcuts: BTreeMap<String, Vec<String>>,
    pub recent: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub audio: Option<AudioSettings>,
    pub metronome: Metronome,
    pub appearance: Appearance,
    pub projects: Projects,
    pub follow: Follow,
    /// The time ruler's format (M3 slice 5b).
    pub time_format: crate::timefmt::TimeFormat,
    pub commands: Commands,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            audio: None,
            metronome: Metronome::default(),
            appearance: Appearance::default(),
            projects: Projects::default(),
            follow: Follow::default(),
            time_format: crate::timefmt::TimeFormat::default(),
            commands: Commands::default(),
        }
    }
}

/// The settings file: `%APPDATA%\Lumen` on Windows, `~/Library/Application
/// Support/Lumen` on macOS, `$XDG_CONFIG_HOME/lumen` (or `~/.config/lumen`)
/// on Linux.
pub fn default_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join(PRODUCT_NAME)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support")
            .join(PRODUCT_NAME)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?
            .join(PRODUCT_NAME.to_lowercase())
    };
    Some(dir.join("settings.json"))
}

/// Reads the settings, or defaults when there are none. A file that cannot
/// be read is moved aside and the reason returned, to show the user.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => {
            return (
                Settings::default(),
                Some(format!("Could not read settings: {e}")),
            );
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) if settings.version <= VERSION => (settings, None),
        Ok(settings) => (
            Settings::default(),
            Some(format!(
                "The settings were saved by a newer version of {PRODUCT_NAME} (format {}); \
                 starting with defaults and leaving the file untouched",
                settings.version
            )),
        ),
        Err(e) => {
            let aside = path.with_extension("json.bad");
            let _ = std::fs::rename(path, &aside);
            (
                Settings::default(),
                Some(format!(
                    "The settings file was damaged ({e}); it was kept as {} and defaults are in use",
                    aside.display()
                )),
            )
        }
    }
}

/// Writes the settings atomically.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// Saves settings on a background thread, one save at a time and in order,
/// so the UI never waits on the disk (audit H1). Dropping it finishes any
/// save still queued.
pub struct Writer {
    queue: Option<std::sync::mpsc::Sender<(PathBuf, Settings)>>,
    errors: std::sync::mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    pub fn start() -> Self {
        let (queue, jobs) = std::sync::mpsc::channel::<(PathBuf, Settings)>();
        let (report, errors) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("lumen-settings".into())
            .spawn(move || {
                for (path, settings) in jobs {
                    if let Err(e) = save(&path, &settings) {
                        let _ = report.send(e.to_string());
                    }
                }
            })
            .expect("spawn settings writer");
        Self {
            queue: Some(queue),
            errors,
            thread: Some(thread),
        }
    }

    pub fn save(&self, path: &Path, settings: &Settings) {
        if let Some(queue) = &self.queue {
            let _ = queue.send((path.to_path_buf(), settings.clone()));
        }
    }

    /// A save that failed since the last call, to show the user.
    pub fn failure(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Closing the queue ends the thread once queued saves are done.
        self.queue = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lumen-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_background_writer_saves_in_order_and_finishes_on_drop() {
        let path = temp_dir("writer").join("settings.json");
        let writer = Writer::start();
        for volume in [0.1, 0.2, 0.3] {
            let mut settings = Settings::default();
            settings.metronome.volume = volume;
            writer.save(&path, &settings);
        }
        drop(writer);
        let (loaded, warning) = load(&path);
        assert_eq!(warning, None);
        assert_eq!(loaded.metronome.volume, 0.3, "the last save wins");
    }

    #[test]
    fn settings_round_trip() {
        let path = temp_dir("roundtrip").join("settings.json");
        let settings = Settings {
            version: VERSION,
            audio: Some(AudioSettings {
                driver: "asio".into(),
                device_id: "{1234}".into(),
                device_name: "Focusrite USB ASIO".into(),
                sample_rate: Some(48_000),
                buffer_frames: Some(128),
                input_channels: vec![0, 1],
                output_channels: vec![0, 1],
            }),
            metronome: Metronome {
                enabled: true,
                volume: 0.3,
                sound: "Wood".into(),
            },
            appearance: Appearance {
                theme: "light".into(),
                reduce_motion: true,
                ui_scale: 1.5,
            },
            projects: Projects {
                recent: vec!["C:/Music/Song/Song.lumen".into()],
                copy_imported: false,
            },
            follow: Follow::Continuous,
            time_format: crate::timefmt::TimeFormat::Timecode25,
            commands: Commands {
                shortcuts: [("edit.duplicate".to_owned(), vec!["Ctrl+Shift+D".to_owned()])].into(),
                recent: vec!["track.add_audio".into()],
            },
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), (settings.clone(), None));
        let metronome = MetronomeSettings::from(&settings.metronome);
        assert_eq!(metronome.sound, ClickSound::Wood);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no temporary file left behind"
        );
    }

    #[test]
    fn missing_settings_are_defaults_without_a_warning() {
        let path = temp_dir("missing").join("settings.json");
        assert_eq!(load(&path), (Settings::default(), None));
    }

    #[test]
    fn a_damaged_file_is_kept_aside_and_reported() {
        let dir = temp_dir("damaged");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.unwrap().contains("damaged"));
        assert!(dir.join("settings.json.bad").exists());
        assert!(!path.exists());
    }

    #[test]
    fn a_newer_format_is_left_alone() {
        let dir = temp_dir("newer");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"version": 99}"#).unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.unwrap().contains("newer version"));
        assert!(path.exists(), "a newer file is not moved or overwritten");
    }

    #[test]
    fn unknown_or_missing_fields_fall_back_to_defaults() {
        let dir = temp_dir("partial");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "metronome": {"volume": 0.8}, "later": true}"#,
        )
        .unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(warning, None);
        assert_eq!(settings.metronome.volume, 0.8);
        assert_eq!(settings.metronome.sound, "Beep");
        assert_eq!(settings.audio, None);
        // Files from before slice 4d: Follow on, by page (owner's default).
        assert_eq!(settings.follow, Follow::Page);
    }
}
