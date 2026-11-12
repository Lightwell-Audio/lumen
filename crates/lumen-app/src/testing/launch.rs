//! How a test starts Lumen: [`Launch::for_test`] and the choices a
//! scenario adds to it (a project, plugins, a driver, settings).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio;
use crate::launch::Launch;
use crate::settings;

impl Launch {
    /// A clean start for a test: default settings that are never saved,
    /// autosaves in `dir/autosave`, and the silent test device. Audio plays
    /// from memory unless the scenario asks for the disk
    /// (`playing_from_disk`): the page pool takes a moment to make in a debug
    /// build, and most scenarios play short files that stay in memory anyway.
    pub fn for_test(dir: &Path) -> Self {
        let mut settings = settings::Settings::default();
        settings.playback.from_disk = false;
        Self {
            started: Instant::now(),
            settings,
            settings_warning: None,
            settings_path: None,
            audio: audio::Choice::test_device(),
            midi: crate::midi::Source::Test(lumen_midi::fake::FakeControl::new()),
            plugins: crate::plugins::Source::Off,
            autosave_dir: Some(dir.join("autosave")),
            user_dir: None,
            autosave_after: None,
            crash_dir: Some(dir.join("crashes")),
            crash_test: None,
            open: None,
            open_audio: None,
            stress: false,
            bench_lanes: false,
            audio_settings: false,
            recover_latest: false,
            scale: None,
            appearance: None,
            smoke_test: false,
            screenshot: None,
            bench: None,
            bench_size: None,
            callback_bench: None,
            autoplay: None,
            test_latency: None,
            slow_disk: None,
            safe_mode: false,
            preset_places: None,
            plugin_marker: Some(dir.join(crate::app::safe_mode::MARKER)),
            save_as: None,
            scripted_dialogs: true,
            first_run: false,
            first_start_rule: false,
            instance: None,
            association: crate::association::Source::Isolated,
            quit_after: None,
        }
    }

    /// Opens the first-run window at start, as a first start does (M9
    /// R7b); the harness never opens it otherwise.
    pub fn first_run(mut self) -> Self {
        self.first_run = true;
        self
    }

    /// Opens the first-run window as a real start would: while the
    /// settings say setup is not done (use with `with_settings_file`).
    pub fn with_first_start_rule(mut self) -> Self {
        self.first_start_rule = true;
        self
    }

    /// Runs the callback benchmark (`--bench-callback`) for `seconds` and
    /// keeps the app open after its report; see [`App::callback_report`].
    pub fn with_callback_bench(mut self, out: PathBuf, seconds: f64) -> Self {
        self.callback_bench = Some(
            crate::callback_bench::Callback::new(out, seconds, Instant::now()).keeping_alive(),
        );
        self
    }

    /// Opens `project` at start, as `lumen song.lumen` does.
    pub fn opening(mut self, project: PathBuf) -> Self {
        self.open = Some(project);
        self
    }

    /// Starts as `--recover-latest` does: the newest leftover is opened at
    /// once instead of offered.
    pub fn recovering_latest(mut self) -> Self {
        self.recover_latest = true;
        self
    }

    /// Starts with MIDI ports from `fake`, which the test plugs and
    /// unplugs.
    pub fn with_midi(mut self, fake: lumen_midi::fake::FakeControl) -> Self {
        self.midi = crate::midi::Source::Test(fake);
        self
    }

    /// Starts with the silent test device at a buffer of `frames`, kept to
    /// real time: it makes up for a poll that came late, so on a busy
    /// machine the playhead stays with the wall-clock stamps that scenarios
    /// give live input (recording). A bigger buffer is a coarser clock.
    pub fn with_audio_buffer(mut self, frames: u32) -> Self {
        self.audio = audio::Choice::Saved(settings::AudioSettings {
            driver: audio::TEST_PACED_DRIVER.into(),
            device_id: "fake".into(),
            device_name: "Fake device".into(),
            sample_rate: None,
            buffer_frames: Some(frames),
            input_channels: Vec::new(),
            all_inputs: true,
            output_channels: vec![0, 1],
            cue_channels: Vec::new(),
        });
        self
    }

    /// Starts on the silent test device of `driver` (one of the test
    /// drivers, such as [`TEST_HOLDS_DRIVER`]).
    pub fn on_audio_driver(mut self, driver: &str) -> Self {
        self.audio = audio::Choice::Named {
            driver: driver.into(),
            name: "fake".into(),
        };
        self
    }

    /// Starts with the silent test device whose inputs the scenario plays
    /// (`lumen_app::testing::input_device`).
    pub fn with_input_device(mut self) -> Self {
        self.audio = audio::Choice::test_input_device();
        self
    }

    /// Starts with plugins searched for in `roots` only, scanned by the host
    /// program at `host`, with the scan results kept in `cache`.
    pub fn with_plugins(mut self, roots: Vec<PathBuf>, host: &Path, cache: PathBuf) -> Self {
        self.plugins = crate::plugins::Source::Fixed(crate::plugins::Fixed {
            roots,
            host: host.to_path_buf(),
            cache: Some(cache),
            deadline: None,
            faults: None,
        });
        self
    }

    /// With plugins, how long one module may take before it is blocked.
    pub fn with_plugin_deadline(mut self, deadline: Duration) -> Self {
        if let crate::plugins::Source::Fixed(f) = &mut self.plugins {
            f.deadline = Some(deadline);
        }
        self
    }

    /// With plugins, the fault the test plugin is given for a module, by
    /// its file name without the extension (`None`: none).
    pub fn with_plugin_faults(mut self, faults: fn(&str) -> Option<&'static str>) -> Self {
        if let crate::plugins::Source::Fixed(f) = &mut self.plugins {
            f.faults = Some(faults);
        }
        self
    }

    /// Starts with `folder` among the plugin folders the person added, as a
    /// settings file with it would.
    pub fn with_plugin_folder(mut self, folder: &Path) -> Self {
        self.settings.plugins.add_folder(folder);
        self
    }

    /// Starts with preset files kept in `user` (and read from `system`).
    pub fn with_preset_folders(mut self, user: PathBuf, system: Vec<PathBuf>) -> Self {
        self.preset_places = Some(crate::preset_files::Places::fixed(user, system));
        self
    }

    /// Starts in safe mode, as `--safe-mode` does.
    pub fn in_safe_mode(mut self) -> Self {
        self.safe_mode = true;
        self
    }

    /// Starts with "scan at startup" switched off, as a saved setting.
    pub fn without_plugin_scan_at_startup(mut self) -> Self {
        self.settings.plugins.scan_at_startup = false;
        self
    }

    /// Starts with the settings kept in `file`: read from it if it exists,
    /// and written to it when they change, as the real settings file is.
    pub fn with_settings_file(mut self, file: &Path) -> Self {
        self.settings = settings::load(file).0;
        self.settings_path = Some(file.to_path_buf());
        self
    }

    /// Starts with the person's own folder at `dir`, as `Documents/Lumen`
    /// is on a real start: projects in `Projects`, templates in `Templates`.
    pub fn with_user_dir(mut self, dir: &Path) -> Self {
        self.user_dir = Some(dir.to_path_buf());
        self
    }

    /// Starts with a favorite saved, as a settings file with one would.
    pub fn with_favorite(mut self, id: &str) -> Self {
        self.settings.plugins.favorites.push(id.to_owned());
        self
    }

    /// Starts with MIDI switched off, as `--midi off` does.
    pub fn with_midi_off(mut self) -> Self {
        self.midi = crate::midi::Source::Off;
        self
    }

    /// Autosaves `after` the first unsaved change, instead of a minute.
    pub fn autosaving_after(mut self, after: Duration) -> Self {
        self.autosave_after = Some(after);
        self
    }

    /// Starts in `appearance` ("light", "dark"), as `--appearance` does.
    pub fn appearing(mut self, appearance: &str) -> Self {
        self.appearance = Some(appearance.to_owned());
        self
    }

    /// Starts with command `id`'s shortcuts saved as `chords` ("Alt+W"),
    /// as a settings file with a changed shortcut would.
    pub fn with_saved_shortcut(mut self, id: &str, chords: &[&str]) -> Self {
        let chords = chords.iter().map(|c| (*c).to_owned()).collect();
        self.settings
            .commands
            .shortcuts
            .insert(id.to_owned(), chords);
        self
    }
}
