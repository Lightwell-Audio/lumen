//! User settings that survive restarts: the audio device and the metronome
//! (M2 slice 8), the appearance (M3 slice 2), changed shortcuts and the
//! palette's recent commands (M3 slice 7), and the record settings (M5
//! slice 6c, in `record_settings.rs`). Preferences proper (paths and the
//! rest) join them in M9.
//!
//! The file is versioned JSON in the user's settings folder. It is written
//! atomically (to a temporary file, flushed, then renamed), so a crash never
//! leaves it half-written. A file that cannot be read is moved aside as
//! `settings.json.bad` and Lumen starts with defaults, saying why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lumen_core::product::PRODUCT_NAME;
use lumen_engine::{ClickSound, MetronomeSettings};
use serde::{Deserialize, Serialize};

use crate::record_settings::RecordSettings;

/// The file's version. A section added with defaults does not change it (an
/// older file reads as before; ADR-0079); a field removed or given another
/// meaning does, with a migration.
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
    /// Every input the device has is open, so any audio track can record from
    /// any of them (M9 slice R1); `input_channels` then lists none. An added
    /// field, no version change: files without it open every input.
    #[serde(default = "yes")]
    pub all_inputs: bool,
    #[serde(default = "stereo")]
    pub output_channels: Vec<usize>,
    /// The cue pair (M4 slice 4, the owner's O1): opened after the main
    /// pair. Empty: no cue. An added field, no version change.
    #[serde(default)]
    pub cue_channels: Vec<usize>,
}

fn yes() -> bool {
    true
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
    /// `main` or `cue` (M4 slice 4: an added field, main by default).
    pub output: String,
}

/// The cue pair's first channel: the main pair comes first, then the cue
/// pair (the owner's O1).
pub const CUE_CHANNEL: u16 = 2;

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
            output: if m.channel == CUE_CHANNEL {
                "cue"
            } else {
                "main"
            }
            .to_owned(),
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
            // On a device without a cue pair, the engine plays it on the
            // main pair.
            channel: if m.output == "cue" { CUE_CHANNEL } else { 0 },
        }
    }
}

/// How mixing controls behave (M4 slice 4): an added section with
/// defaults, so older files read as before (no version change).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Mixing {
    /// A click on Solo adds the track to what is soloed, and Ctrl+click
    /// solos it alone (the owner's D2: off by default).
    pub additive_solo: bool,
    /// The master's Solo/Cue switch is on Cue: the S buttons cue tracks to
    /// the cue pair (the owner's C1).
    pub cue_mode: bool,
    /// The cue bus's level, in dB.
    pub cue_volume_db: f32,
    /// Meters in their track's colour rather than the accent (the owner's
    /// M2, a setting; M1, the accent, by default). M4 slice 5, an added
    /// field.
    pub track_colour_meters: bool,
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
    /// The info line along the bottom of the window (M4 slice 0): an
    /// added field, on unless turned off.
    pub info_line: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: crate::theme::Appearance::default().key().to_owned(),
            reduce_motion: false,
            ui_scale: 1.0,
            info_line: true,
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

/// What the user switched on for one MIDI port (M5 slice 0). Ports are
/// found by direction and name, and stay remembered while unplugged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiPort {
    /// `input` or `output`.
    pub direction: String,
    pub name: String,
    #[serde(default)]
    pub track: bool,
    #[serde(default)]
    pub remote: bool,
    /// An output that gets MIDI clock (M5 slice 7; an added field).
    #[serde(default, skip_serializing_if = "is_false")]
    pub sync: bool,
    /// How much later than the song the clock goes, in milliseconds,
    /// −20 to +50 (the owner's F4; an added field).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sync_delay_ms: i32,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

/// The MIDI page of the settings: an added field with a default, so older
/// files read as before (no version change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Midi {
    pub ports: Vec<MidiPort>,
    /// The piano roll plays a note while it is drawn or changes pitch
    /// (M5 slice 3d; an added field, on by default).
    pub audition: bool,
}

impl Default for Midi {
    fn default() -> Self {
        Self {
            ports: Vec::new(),
            audition: true,
        }
    }
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
    pub mixing: Mixing,
    pub midi: Midi,
    /// The count-in, record-only metronome and Overdub (M5 slice 6c): an
    /// added section with defaults, so older files read as before (no
    /// version change).
    pub record: RecordSettings,
    /// Plugin folders, favorites and recents (M6 slice 1): an added section
    /// with defaults, so older files read as before (no version change).
    pub plugins: crate::plugins::PluginSettings,
    /// How clip edits treat automation (M7 slice A7): an added section with
    /// defaults, so older files read as before (no version change).
    pub editing: Editing,
    /// What Bounce did last (M7b slice E4): an added section with defaults,
    /// so older files read as before (no version change).
    pub bounce: BounceChoices,
    /// How audio files play (M9 slice R6): an added section with defaults,
    /// so older files read as before (no version change).
    pub playback: Playback,
}

/// How audio files play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Playback {
    /// "Play audio from disk": long files stream rather than load into
    /// memory. Off until the soak and MT-STREAM-1 pass (the owner's R5 A).
    pub from_disk: bool,
}

/// The choices in Bounce's popover, remembered (the owner's B1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BounceChoices {
    /// Replace the track's audio; off puts the file on a new track.
    pub replace: bool,
    /// The track's devices, fader and pan are in the file.
    pub effects: bool,
    /// What rings on after the end is in the file.
    pub tail: bool,
}

impl Default for BounceChoices {
    fn default() -> Self {
        Self {
            replace: false,
            effects: true,
            tail: true,
        }
    }
}

/// How clip edits treat automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Editing {
    /// A clip that moves, is copied or is deleted leaves the track's
    /// automation where it is (the owner's E3: off by default, so the points
    /// under a clip go with it).
    pub lock_envelopes: bool,
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
            mixing: Mixing::default(),
            midi: Midi::default(),
            record: RecordSettings::default(),
            plugins: Default::default(),
            editing: Editing::default(),
            bounce: BounceChoices::default(),
            playback: Playback::default(),
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
        Ok(mut settings) if settings.version <= VERSION => {
            settings.plugins.bound();
            (settings, None)
        }
        Ok(settings) => {
            // Defaults are used, and the first change would write them over
            // this file: keep it aside, where the newer version can have it
            // back.
            let aside = path.with_extension("json.newer");
            let kept = match std::fs::rename(path, &aside) {
                Ok(()) => format!("it was kept as {}", aside.display()),
                Err(e) => format!("it could not be kept aside ({e}) and may be overwritten"),
            };
            (
                Settings::default(),
                Some(format!(
                    "The settings were saved by a newer version of {PRODUCT_NAME} (format {}); \
                     starting with defaults, and {kept}",
                    settings.version
                )),
            )
        }
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
    // Flushed to disk before it replaces the old file, as a project is.
    lumen_project::write_atomic(path, text.as_bytes())
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
    use crate::record_settings::CountIn;

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
                all_inputs: false,
                output_channels: vec![0, 1],
                cue_channels: vec![2, 3],
            }),
            metronome: Metronome {
                enabled: true,
                volume: 0.3,
                sound: "Wood".into(),
                output: "cue".into(),
            },
            appearance: Appearance {
                theme: "light".into(),
                reduce_motion: true,
                ui_scale: 1.5,
                info_line: false,
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
            mixing: Mixing {
                additive_solo: true,
                cue_mode: true,
                cue_volume_db: -6.0,
                track_colour_meters: true,
            },
            midi: Midi {
                ports: vec![
                    MidiPort {
                        direction: "input".into(),
                        name: "Keys".into(),
                        track: true,
                        remote: true,
                        sync: false,
                        sync_delay_ms: 0,
                    },
                    MidiPort {
                        direction: "output".into(),
                        name: "Drum machine".into(),
                        track: false,
                        remote: false,
                        sync: true,
                        sync_delay_ms: -7,
                    },
                ],
                audition: false,
            },
            record: RecordSettings {
                count_in: CountIn::TwoBars,
                only_while_recording: true,
                overdub: true,
                offset_ms: -2.5,
                keep_played: false,
            },
            plugins: crate::plugins::PluginSettings {
                scan_at_startup: false,
                folders: vec!["/plugins/extra".into()],
                favorites: vec!["vst3:AB".into()],
                recents: vec!["builtin:synth".into(), "vst3:AB".into()],
                editors: [(
                    "vst3:AB".to_owned(),
                    crate::plugins::EditorPlace {
                        x: -1200,
                        y: 80,
                        width: 640,
                        height: 480,
                        dpi: 144,
                    },
                )]
                .into_iter()
                .collect(),
            },
            editing: Editing {
                lock_envelopes: true,
            },
            bounce: BounceChoices {
                replace: true,
                effects: false,
                tail: false,
            },
            playback: Playback { from_disk: true },
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), (settings.clone(), None));
        let metronome = MetronomeSettings::from(&settings.metronome);
        assert_eq!(metronome.sound, ClickSound::Wood);
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["settings.json"], "no temporary file left behind");
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
    fn a_newer_format_is_kept_aside_and_not_overwritten_by_the_next_save() {
        let dir = temp_dir("newer");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let newer = r#"{"version": 99, "later": true}"#;
        std::fs::write(&path, newer).unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        let warning = warning.unwrap();
        assert!(warning.contains("newer version"), "{warning}");
        assert!(warning.contains("settings.json.newer"), "{warning}");
        // The first change saves over the path, as the app does.
        save(&path, &settings).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json.newer")).unwrap(),
            newer,
            "the newer file's words are still there"
        );
        assert_eq!(load(&path).0, Settings::default());
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

    #[test]
    fn a_file_from_before_the_record_settings_reads_them_as_defaults() {
        let dir = temp_dir("record-missing");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "midi": {"ports": [], "audition": false}}"#,
        )
        .unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(warning, None);
        assert_eq!(settings.record, RecordSettings::default());
        assert!(!settings.midi.audition, "the other sections still load");
    }

    #[test]
    fn an_unknown_count_in_in_the_file_loads_as_off() {
        let dir = temp_dir("record-unknown");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "record": {"count_in": "eight_bars", "overdub": true}}"#,
        )
        .unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(warning, None);
        assert_eq!(settings.record.count_in, CountIn::Off);
        assert!(settings.record.overdub);
    }
}
