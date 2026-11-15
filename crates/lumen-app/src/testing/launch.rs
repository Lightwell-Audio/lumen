use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio;
use crate::launch::Launch;
use crate::settings;

impl Launch {
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

    pub fn first_run(mut self) -> Self {
        self.first_run = true;
        self
    }

    pub fn with_first_start_rule(mut self) -> Self {
        self.first_start_rule = true;
        self
    }

    pub fn with_callback_bench(mut self, out: PathBuf, seconds: f64) -> Self {
        self.callback_bench = Some(
            crate::callback_bench::Callback::new(out, seconds, Instant::now()).keeping_alive(),
        );
        self
    }

    pub fn opening(mut self, project: PathBuf) -> Self {
        self.open = Some(project);
        self
    }

    pub fn recovering_latest(mut self) -> Self {
        self.recover_latest = true;
        self
    }

    pub fn with_midi(mut self, fake: lumen_midi::fake::FakeControl) -> Self {
        self.midi = crate::midi::Source::Test(fake);
        self
    }

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

    pub fn on_audio_driver(mut self, driver: &str) -> Self {
        self.audio = audio::Choice::Named {
            driver: driver.into(),
            name: "fake".into(),
        };
        self
    }

    pub fn with_input_device(mut self) -> Self {
        self.audio = audio::Choice::test_input_device();
        self
    }

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

    pub fn with_plugin_deadline(mut self, deadline: Duration) -> Self {
        if let crate::plugins::Source::Fixed(f) = &mut self.plugins {
            f.deadline = Some(deadline);
        }
        self
    }

    pub fn with_plugin_faults(mut self, faults: fn(&str) -> Option<&'static str>) -> Self {
        if let crate::plugins::Source::Fixed(f) = &mut self.plugins {
            f.faults = Some(faults);
        }
        self
    }

    pub fn with_plugin_folder(mut self, folder: &Path) -> Self {
        self.settings.plugins.add_folder(folder);
        self
    }

    pub fn with_preset_folders(mut self, user: PathBuf, system: Vec<PathBuf>) -> Self {
        self.preset_places = Some(crate::preset_files::Places::fixed(user, system));
        self
    }

    pub fn in_safe_mode(mut self) -> Self {
        self.safe_mode = true;
        self
    }

    pub fn without_plugin_scan_at_startup(mut self) -> Self {
        self.settings.plugins.scan_at_startup = false;
        self
    }

    pub fn with_settings_file(mut self, file: &Path) -> Self {
        self.settings = settings::load(file).0;
        self.settings_path = Some(file.to_path_buf());
        self
    }

    pub fn with_user_dir(mut self, dir: &Path) -> Self {
        self.user_dir = Some(dir.to_path_buf());
        self
    }

    pub fn with_favorite(mut self, id: &str) -> Self {
        self.settings.plugins.favorites.push(id.to_owned());
        self
    }

    pub fn with_midi_off(mut self) -> Self {
        self.midi = crate::midi::Source::Off;
        self
    }

    pub fn autosaving_after(mut self, after: Duration) -> Self {
        self.autosave_after = Some(after);
        self
    }

    pub fn appearing(mut self, appearance: &str) -> Self {
        self.appearance = Some(appearance.to_owned());
        self
    }

    pub fn with_saved_shortcut(mut self, id: &str, chords: &[&str]) -> Self {
        let chords = chords.iter().map(|c| (*c).to_owned()).collect();
        self.settings
            .commands
            .shortcuts
            .insert(id.to_owned(), chords);
        self
    }
}
