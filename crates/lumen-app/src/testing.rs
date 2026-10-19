//! What the end-to-end tests (`tests/e2e/`, ADR-0029) may see and do,
//! beyond what a user sees on screen: a clean start, and read-only looks at
//! state the screen shows only as pixels (the playhead, the zoom). Tests
//! drive the app through its controls; this is the small, deliberate API
//! for the rest, so the app's internals stay private.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Project};

use crate::audio::{self, Status};
use crate::document::find_device;
use crate::launch::Launch;
use crate::timeview::TimeView;
use crate::{App, files, settings};

/// A note edit for tests (`App::edit_notes`).
#[derive(Debug, Clone, Copy)]
pub enum NoteEdit {
    Move {
        time: Ticks,
        pitch: i32,
    },
    Delete,
    Mute(bool),
    /// Lengthens the end by this much.
    Lengthen(Ticks),
}

impl Launch {
    /// A clean start for a test: default settings that are never saved,
    /// autosaves in `dir/autosave`, and the silent test device.
    pub fn for_test(dir: &Path) -> Self {
        Self {
            started: Instant::now(),
            settings: settings::Settings::default(),
            settings_warning: None,
            settings_path: None,
            audio: audio::Choice::test_device(),
            midi: crate::midi::Source::Test(lumen_midi::fake::FakeControl::new()),
            plugins: crate::plugins::Source::Off,
            autosave_dir: Some(dir.join("autosave")),
            autosave_after: None,
            crash_dir: Some(dir.join("crashes")),
            crash_test: None,
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
            callback_bench: None,
            autoplay: None,
            test_latency: None,
            safe_mode: false,
            preset_places: None,
            plugin_marker: Some(dir.join(crate::app::safe_mode::MARKER)),
            save_as: None,
            scripted_dialogs: true,
        }
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
            output_channels: vec![0, 1],
            cue_channels: Vec::new(),
        });
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

/// Installs Lumen's GPU renderer (grid, clips, waveforms) when the harness
/// renders with wgpu, as `run` does for the real window.
pub fn install_renderer(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    match cc.wgpu_render_state.as_ref() {
        Some(state) => crate::canvas::install(state, None).map_err(|e| format!("renderer: {e}")),
        None => Ok(()),
    }
}

/// Where a project called `name` is saved in `dir` (its own folder).
pub fn project_file(dir: &Path, name: &str) -> PathBuf {
    files::project_file_for(&dir.join(name))
}

/// What a meter shows (`App::meter`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterShown {
    pub peak_db: f32,
    pub rms_db: f32,
    pub hold_db: f32,
    pub clipped: bool,
}

/// The extension of project files (and autosaves).
pub const PROJECT_EXTENSION: &str = files::EXTENSION;

impl App {
    /// What the callback benchmark came to, once it has ended: the text
    /// report, whether it passed and how many callbacks it measured, or why
    /// it could not run.
    pub fn callback_report(&self) -> Option<Result<(String, bool, u64), String>> {
        self.callback_report.as_ref().map(|result| match result {
            Ok(r) => Ok((r.text(), r.pass(), r.callbacks)),
            Err(why) => Err(why.clone()),
        })
    }

    /// The open project.
    pub fn project(&self) -> &Project {
        self.document.project()
    }

    /// Unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    /// The audio device is running.
    pub fn audio_running(&self) -> bool {
        matches!(self.transport.audio.status, Status::Running { .. })
    }

    pub fn is_playing(&self) -> bool {
        self.transport.state().playing
    }

    pub fn playhead(&self) -> Ticks {
        self.transport.state().ticks
    }

    /// The engine's transport as of its last block: recording, count-in and
    /// playing flags, and the sample and tick positions (M5 slice 6).
    pub fn transport_state(&self) -> lumen_engine::TransportSnapshot {
        self.transport.state()
    }

    /// The output latency of the audio device, in frames: the playhead as
    /// the engine renders it leads what is heard by this much (M5 slice 6,
    /// the recorder's timing).
    pub fn output_latency_frames(&self) -> i64 {
        self.transport
            .audio
            .info
            .as_ref()
            .map_or(0, |info| i64::from(info.output_latency))
    }

    /// Whether a recording is under way, or counting in to one (M5 slice 6).
    pub fn recording(&self) -> bool {
        self.is_recording()
    }

    /// The take as the arrangement draws it each frame (after MT-MIDI-7):
    /// where it begins and ends, and the notes played so far on the first
    /// track recording. None when nothing records.
    pub fn live_take(&self) -> Option<(Ticks, Ticks, usize)> {
        self.take_so_far()
    }

    /// The pitch range the first track recording is laid out to in what the
    /// arrangement draws this frame, the take included
    /// (`piano_roll::mini::Fit`). None before a note is played.
    pub fn live_range(&self) -> Option<(u8, u8)> {
        let shown = self.arrangement.preview.as_ref()?;
        let track = shown
            .tracks
            .iter()
            .find(|t| t.armed && t.kind == lumen_project::model::TrackKind::Midi)?;
        crate::piano_roll::mini::track_range(&track.clips)
    }

    /// The clips of track `index` as the arrangement draws them this frame:
    /// while recording, the take placed as far as it has gone.
    pub fn drawn_clips(&self, index: usize) -> Vec<std::sync::Arc<lumen_project::model::Clip>> {
        let project = self
            .arrangement
            .preview
            .as_deref()
            .unwrap_or_else(|| self.project());
        project
            .tracks
            .get(index)
            .map(|t| t.clips.clone())
            .unwrap_or_default()
    }

    /// The pitch range every MIDI clip on track `index` is laid out to.
    pub fn track_pitch_range(&self, index: usize) -> Option<(u8, u8)> {
        crate::piano_roll::mini::track_range(&self.project().tracks.get(index)?.clips)
    }

    /// Whether Overdub is on: a take merges into the clip under it (M5
    /// slice 6; `settings.record`).
    pub fn overdub_on(&self) -> bool {
        self.settings.record.overdub
    }

    /// The count-in's length in bars, zero for none (M5 slice 6).
    pub fn count_in_bars(&self) -> u32 {
        self.settings.record.count_in.bars()
    }

    /// The notice banner's text, such as "Arm a MIDI track to record."
    /// (M5 slice 6).
    pub fn record_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    /// Where the playhead is drawn: smoothed while playing (`playhead.rs`).
    pub fn drawn_playhead(&self) -> Ticks {
        self.transport.playhead()
    }

    /// The test device drops `n` buffers, as a real one does under load.
    pub fn cause_dropouts(&mut self, n: u64) {
        if let Some(meters) = self.transport.audio.meters() {
            meters.count_dropouts(n);
        }
    }

    /// Puts the engine's synthetic latent device on track `id`, adding
    /// `frames` of latency (0 takes it off), as a device with latency will
    /// be from M6: what delay compensation is tested with (M4 slice 6).
    pub fn set_test_latency(&mut self, id: Id, frames: u32) {
        let latent = &mut self.lanes.devices.latent;
        if frames == 0 {
            latent.remove(&id);
        } else {
            latent.insert(id, frames);
        }
    }

    /// The name of the sound the card of `device` shows (the field), and
    /// whether it is marked changed; none when the card has no field or no
    /// sound was chosen.
    pub fn device_preset(&self, device: Id) -> Option<(String, bool)> {
        let look = self.field_look(device)?;
        look.name.map(|name| (name, look.changed))
    }

    /// Whether the card of `device` shows a preset field.
    pub fn device_has_preset_field(&self, device: Id) -> bool {
        self.field_look(device).is_some()
    }

    /// Gives a plugin effect a sidechain input, and the light of `track`'s
    /// sidechain a level (0 to 1) and, when `silent_seconds` is over two
    /// while the song plays, the warning ring: screenshots of the card's row.
    pub fn give_device_sidechain(
        &mut self,
        device: Id,
        track: Id,
        level: f32,
        silent_seconds: f32,
    ) {
        self.hosting.pretend_sidechain_input(device);
        self.chain.sidechain.pretend(track, level, silent_seconds);
    }

    /// Gives a plugin instrument `extra` outputs besides its main one, as if
    /// its host had listed them: screenshots of the card's outputs row.
    pub fn give_device_outputs(&mut self, device: Id, extra: &[&str]) {
        self.hosting.pretend_outputs(device, extra);
    }

    /// How lit the sidechain light of the shown track is, 0 to 1.
    pub fn sidechain_light(&self, track: Id) -> f32 {
        self.chain.sidechain.level_of(track)
    }

    /// The folders "Show presets folder" opened, oldest first.
    pub fn revealed_folders(&self) -> Vec<PathBuf> {
        self.chain.presets.revealed.clone()
    }

    /// Whether Lumen is in safe mode, and whether it is offering it.
    pub fn safe_mode(&self) -> (bool, bool) {
        (self.safe.on, self.safe.offer)
    }

    /// How many plugin hosts exist.
    pub fn plugin_hosts(&self) -> usize {
        self.hosting.host_count()
    }

    /// Loads one plugin in safe mode, as its card's "Load this plugin" will.
    pub fn load_one(&mut self, device: Id) {
        self.load_one_plugin(device);
    }

    /// How a plugin device's host is doing: `loading`, `ready`, `failed: ...`
    /// or `missing`; none when the project holds no such device.
    pub fn device_state(&self, device: Id) -> Option<String> {
        use crate::hosting::State;
        self.hosting.state(device).map(|s| match s {
            State::Loading => "loading".to_owned(),
            State::Ready => "ready".to_owned(),
            State::Failed(why) => format!("failed: {why}"),
            State::Missing => "missing".to_owned(),
            State::Safe => "safe".to_owned(),
        })
    }

    /// The longest block a plugin device was promised (the device's block,
    /// at most the engine's), once its host has linked.
    pub fn device_promised_frames(&self, device: Id) -> Option<u32> {
        self.hosting.promised(device)
    }

    /// Shows the audio panel's block budget with these plugins, whatever
    /// runs, for screenshots: a block of `block_us` that used `used_us`, and
    /// each plugin's name, place, work and wait (microseconds a block) and
    /// missed blocks. Held for an hour.
    pub fn pretend_budget(
        &mut self,
        block_us: u64,
        used_us: u64,
        plugins: &[(&str, &str, u64, u64, u32)],
    ) {
        use crate::transport::budget::{Share, Snapshot};
        let us = Duration::from_micros;
        let plugins: Vec<Share> = plugins
            .iter()
            .enumerate()
            .map(|(i, &(name, place, work, waited, missed))| Share {
                device: Id(i as u64 + 1),
                name: name.into(),
                place: place.into(),
                work: us(work),
                waited: us(waited),
                missed,
            })
            .collect();
        let waited: Duration = plugins.iter().map(|p| p.waited).sum();
        let (part, whole) = lumen_engine::HOST_SHARE;
        self.transport.budget.pretend(Snapshot {
            block: us(block_us),
            used: us(used_us),
            deadline: us(block_us * part / whole),
            lumen: us(used_us).saturating_sub(waited),
            plugins,
        });
    }

    /// Shows a plugin device as `loading`, `ready`, `failed` or `missing`
    /// whatever its host is doing (screenshots of every card face).
    pub fn pretend_device_state(&mut self, device: Id, state: &str) {
        use crate::hosting::State;
        self.hosting.pretend(
            device,
            match state {
                "loading" => State::Loading,
                "ready" => State::Ready,
                "failed" => State::Failed(
                    "Crashed while loading: access violation, exception 0xc0000005.".into(),
                ),
                "safe" => State::Safe,
                _ => State::Missing,
            },
        );
    }

    /// Gives a plugin device a program list, a chosen sound (changed or not)
    /// and preset files, without a host (screenshots of the preset menu).
    pub fn give_device_presets(
        &mut self,
        device: Id,
        programs: &[&str],
        chosen: &str,
        changed: bool,
        files: &[(Option<&str>, &str, bool)],
    ) {
        self.hosting.pretend_programs(device, "Factory", programs);
        let params = find_device(self.document.project(), device)
            .map(|(_, d)| d.params.clone())
            .unwrap_or_default();
        let baseline = if changed {
            let mut other = params.clone();
            other.insert(9999, 1.0);
            other
        } else {
            params
        };
        self.chain
            .presets
            .pretend(device, chosen, Some(baseline), files);
    }

    /// Opens the preset menu of a card (screenshots).
    pub fn open_preset_menu_of(&mut self, ctx: &egui::Context, device: Id) {
        self.open_preset_menu(ctx, device, false);
    }

    /// Says a plugin device failed `count` times just now and that its last
    /// good state is `age_seconds` old (screenshots of the failed card).
    pub fn pretend_failures(&mut self, device: Id, count: usize, age_seconds: Option<u64>) {
        self.hosting.pretend_failures(
            device,
            count,
            age_seconds.map(std::time::Duration::from_secs),
        );
    }

    /// Shows safe mode (and, when `offer`, its offer) without a host: for
    /// screenshots.
    pub fn pretend_safe_mode(&mut self, on: bool, offer: bool) {
        self.safe.on = on;
        self.safe.offer = offer;
        self.hosting.set_safe(on);
    }

    /// Gives a plugin device a picture of `width` by `height` pixels from
    /// `pixel(x, y)`, as if its bundle held one (screenshots of the card).
    pub fn give_device_picture(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        (width, height): (usize, usize),
        pixel: impl Fn(usize, usize) -> [u8; 4],
    ) {
        let class = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.devices.iter())
            .find(|d| d.id == device)
            .and_then(|d| match &d.plugin {
                lumen_project::model::PluginRef::Vst3 { class_id, .. } => Some(class_id.clone()),
                _ => None,
            });
        let Some(class) = class else { return };
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend(pixel(x, y));
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
        let wake = ctx.clone();
        self.chain
            .pictures
            .get_or_insert_with(|| crate::chain::Pictures::new(move || wake.request_repaint()))
            .put(ctx, &class, image);
    }

    /// How a plugin device's own picture stands: `none`, `loading`, `absent`
    /// or `ready` (the card shows it when ready).
    pub fn device_picture(&self, device: Id) -> &'static str {
        let class = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.devices.iter())
            .find(|d| d.id == device)
            .and_then(|d| match &d.plugin {
                lumen_project::model::PluginRef::Vst3 { class_id, .. } => Some(class_id.clone()),
                _ => None,
            });
        match (class, &self.chain.pictures) {
            (Some(class), Some(pictures)) => pictures.state(&class),
            _ => "none",
        }
    }

    /// The latency a plugin device last reported, in frames.
    pub fn device_latency(&self, device: Id) -> Option<u32> {
        self.hosting.latency(device)
    }

    /// Starts a failed device's host again, as its card's Reload will.
    pub fn reload_device(&mut self, device: Id) {
        self.hosting.reload(device);
    }

    /// Starts a failed device's host again with no sound of its own, as its
    /// card's "Reload without sound" will.
    pub fn reload_device_clean(&mut self, device: Id) {
        self.hosting.reload_clean(device);
    }

    /// Ends a device's host as a crash would leave it (the user ending a plugin
    /// that stopped answering).
    pub fn end_device_host(&mut self, device: Id) {
        self.hosting.end_plugin(device);
    }

    /// How often running plugins are read for a checkpoint.
    pub fn set_checkpoint_every(&mut self, every: Duration) {
        self.hosting.set_checkpoint_every(every);
    }

    /// Whether a device's plugin has stopped three times in two minutes.
    pub fn device_keeps_failing(&self, device: Id) -> bool {
        self.hosting.keeps_failing(device)
    }

    /// How long ago a device's last good state was read, if one was.
    pub fn device_last_good_age(&self, device: Id) -> Option<Duration> {
        self.hosting.last_good_age(device)
    }

    /// Removes a device from a track, as the interface will (one undo step).
    pub fn remove_device(&mut self, track: Id, device: Id) {
        let _ = self.document.remove_device(track, device);
    }

    /// Folds a card to a strip, or opens it again (a view, not an edit).
    pub fn fold_device(&mut self, device: Id) {
        self.toggle_fold_of(device);
    }

    /// Moves a device to a track: right after `after` on it, or at the end.
    /// A drag will do this (one undo step).
    pub fn move_device(&mut self, from: Id, device: Id, to: Id, after: Option<Id>) {
        let slot = after.map_or(crate::document::Slot::End, crate::document::Slot::After);
        let _ = self.document.move_device(from, device, to, slot);
    }

    /// Copies a device to a track, placed like [`Self::move_device`]; returns
    /// the copy's id.
    pub fn copy_device(&mut self, from: Id, device: Id, to: Id, after: Option<Id>) -> Option<Id> {
        let slot = after.map_or(crate::document::Slot::End, crate::document::Slot::After);
        self.document.copy_device(from, device, to, slot).ok()
    }

    /// Sets a plugin's parameter (normalized) as the interface's controls will:
    /// the project holds the value, and one drag is one undo step.
    pub fn set_device_param(&mut self, device: Id, id: u32, value: f64) {
        let _ = self.document.set_device_params(
            device,
            &[(id, value)],
            "Change parameter",
            Some("test-parameter"),
        );
    }

    /// The programs a device's plugin offers: each list's name and its
    /// programs' names.
    pub fn device_programs(&self, device: Id) -> Vec<(String, Vec<String>)> {
        self.hosting
            .programs(device)
            .iter()
            .map(|l| (l.name.clone(), l.programs.clone()))
            .collect()
    }

    /// Chooses a program as the card's preset menu will.
    pub fn choose_program(&mut self, device: Id, list: usize, program: usize) {
        self.load_program(device, list, program);
    }

    /// Loads a preset file's bytes into a device's plugin as the card's preset
    /// menu will; what the file is wrong with is the error.
    pub fn load_preset_from_bytes(
        &mut self,
        device: Id,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let preset = lumen_hosting::Preset::parse(bytes).map_err(|e| e.to_string())?;
        self.load_preset(device, name, preset);
        Ok(())
    }

    /// The parameter values the project holds for a device, by id.
    pub fn device_params(&self, device: Id) -> Vec<(u32, f64)> {
        crate::document::find_device(self.document.project(), device)
            .map(|(_, d)| d.params.iter().map(|(&k, &v)| (k, v)).collect())
            .unwrap_or_default()
    }

    /// Gives a device a parameter list as if its plugin had listed it, for
    /// screenshots of the card's strip: `(title, value, text)` each.
    pub fn give_device_parameters(&mut self, device: Id, list: &[(&str, f64, &str)]) {
        self.hosting.pretend_parameters(device, list);
    }

    /// Gives a device stepped parameters after those it has (screenshots of
    /// the switches and choosers): `(title, steps, value, choices)` each.
    pub fn give_device_stepped(&mut self, device: Id, list: &[(&str, i32, f64, &[&str])]) {
        self.hosting.pretend_stepped(device, list);
    }

    /// The parameter ids on a device's card strip, in order.
    pub fn strip_parameters(&self, device: Id) -> Vec<u32> {
        self.strip_ids(device)
    }

    /// The plugin's words for each step of a stepped parameter, as far as it
    /// has said them.
    pub fn device_choices(&self, device: Id, parameter: u32) -> Vec<String> {
        self.hosting.choices(device, parameter).to_vec()
    }

    /// Puts exactly these parameters on the device's strip, as a person does
    /// from the panel.
    pub fn show_on_strip(&mut self, device: Id, ids: &[u32]) {
        self.chain.shown.insert(device, ids.to_vec());
    }

    /// Sends a parameter value to the device's plugin as a control in its own
    /// window would, with no edit of the project.
    pub fn poke_plugin(&mut self, device: Id, id: u32, value: f64) {
        self.hosting.poke_param(device, id, value);
    }

    /// How many values Lumen has sent the device's plugin to match the project.
    pub fn values_sent_to(&self, device: Id) -> u32 {
        self.hosting.values_sent(device)
    }

    /// Whether the device's plugin window is open but hidden because the
    /// Devices tab shows another track.
    pub fn editor_hidden(&self, device: Id) -> bool {
        self.hosting.editor_hidden(device)
    }

    /// Where a plugin's window is remembered to open, by the device's class:
    /// `(x, y, width, height)`.
    pub fn editor_place(&self, device: Id) -> Option<(i32, i32, u32, u32)> {
        let class = self.hosting.class(device)?;
        let p = self.settings.plugins.editors.get(class)?;
        Some((p.x, p.y, p.width, p.height))
    }

    /// Remembers a window place for the device's plugin, as if it had been
    /// left there.
    pub fn set_editor_place(&mut self, device: Id, place: (i32, i32, u32, u32)) {
        if let Some(class) = self.hosting.class(device).map(str::to_owned) {
            self.settings.plugins.remember_editor(
                &class,
                crate::plugins::EditorPlace {
                    x: place.0,
                    y: place.1,
                    width: place.2,
                    height: place.3,
                    dpi: 96,
                },
            );
        }
    }

    /// Sets how a device's plugin window looks on its card, for screenshots:
    /// `open`, `opening`, `stuck`, `none` (no editor) or `closed`; `learn`
    /// also turns Learn on.
    pub fn give_device_editor(&mut self, device: Id, state: &str, learn: bool) {
        use crate::hosting::EditorState;
        let state = match state {
            "open" => EditorState::Open,
            "opening" => EditorState::Opening,
            "stuck" => EditorState::NotResponding,
            "none" => EditorState::Refused("This plugin has no editor.".into()),
            _ => EditorState::Closed,
        };
        self.hosting.pretend_editor(device, state);
        if learn {
            self.chain.learning = Some(device);
        }
    }

    /// Whether Learn is on for the device.
    pub fn learning(&self, device: Id) -> bool {
        self.chain.learning == Some(device)
    }

    /// How many changed plugins could not be read for the save.
    pub fn unread_plugins(&self) -> usize {
        self.hosting.unread()
    }

    /// A parameter's value as far as the host bridge knows it.
    pub fn device_param_known(&self, device: Id, id: u32) -> Option<f64> {
        self.hosting.value(device, id)
    }

    /// The titles of a running plugin's parameters, in its order.
    pub fn device_param_titles(&self, device: Id) -> Vec<String> {
        self.hosting
            .parameters(device)
            .iter()
            .map(|p| p.title.clone())
            .collect()
    }

    /// Bypasses or turns on a device, as the interface will.
    pub fn set_device_bypass(&mut self, track: Id, device: Id, bypass: bool) {
        let _ = self.document.set_device_bypass(track, device, bypass);
    }

    /// The delay compensation the engine was last sent, in frames.
    pub fn compensation(&self) -> u32 {
        self.transport.audio.mixer.compensation().frames
    }

    /// What the test device has played since the last call, as (left,
    /// right) frames: the audio a user would hear.
    pub fn heard(&mut self) -> Vec<(f32, f32)> {
        self.heard_frames()
            .into_iter()
            .map(|[l, r, ..]| (l, r))
            .collect()
    }

    /// Sets MIDI From on the selected MIDI tracks, as the menu would.
    pub fn set_midi_from(&mut self, from: lumen_project::model::MidiFrom) {
        let ids = self.selected_midi_tracks();
        self.document.set_midi_from(&ids, from);
    }

    /// Sets Monitor on the selected MIDI tracks, as the menu would.
    pub fn set_monitor_of_selected(&mut self, monitor: lumen_project::model::Monitor) {
        let ids = self.selected_midi_tracks();
        self.document.set_monitor(&ids, monitor);
    }

    /// Plays `msg` live, as the computer keyboard does (M5 slice 2).
    pub fn play_live(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.play_key(msg)
    }

    /// Previews `pitch` on `track` as the piano roll does (M5 slice 3d).
    pub fn audition(&mut self, track: Id, pitch: u8, velocity: u8) -> bool {
        self.audition_play(track, pitch, velocity)
    }

    /// Previews a chord of `(pitch, velocity)` notes on `track`, as a press
    /// on picked notes does: each note its own tone, so one on a key the
    /// track's clip is sounding on hardware is skipped and the rest play.
    pub fn audition_chord(&mut self, track: Id, tones: &[(u8, u8)]) -> bool {
        let tones: Vec<crate::audition::Tone> = tones
            .iter()
            .map(|&(pitch, velocity)| crate::audition::Tone { pitch, velocity })
            .collect();
        self.audition_tones(track, &tones, true)
    }

    /// Lets go of the preview (the end of a gesture).
    pub fn release_audition(&mut self) {
        self.audition_release();
    }

    /// Whether a previewed note is sounding.
    pub fn audition_sounding(&self) -> bool {
        self.audition.is_sounding()
    }

    /// Turns the piano roll's audition on or off, in the settings.
    pub fn set_audition_setting(&mut self, on: bool) {
        self.settings.midi.audition = on;
    }

    /// Edits the notes of a clip as the piano roll will (M5 slice 3a's
    /// `Document` methods), for tests of notes committed while playing.
    pub fn edit_notes(&mut self, clip: Id, selection: &[usize], edit: NoteEdit) {
        use crate::document::Resize;
        use lumen_project::edit::notes::End;
        let base = self.document.project().clone();
        match edit {
            NoteEdit::Move { time, pitch } => {
                self.document
                    .move_notes(&base, clip, selection, time, pitch, None);
            }
            NoteEdit::Delete => {
                self.document.delete_notes(clip, selection);
            }
            NoteEdit::Mute(muted) => {
                self.document.mute_notes(clip, selection, Some(muted));
            }
            NoteEdit::Lengthen(by) => {
                let resize = Resize {
                    end: End::End,
                    by,
                    min: Ticks::ZERO,
                };
                self.document
                    .resize_notes(&base, clip, selection, resize, None);
            }
        }
    }

    /// Whether the computer keyboard mode is on (M5 slice 2).
    pub fn computer_keyboard_on(&self) -> bool {
        self.keyboard.is_on()
    }

    /// The notes the computer keyboard holds now, lowest first.
    pub fn computer_keyboard_held(&self) -> Vec<u8> {
        self.keyboard.held_notes()
    }

    /// The messages the computer keyboard sent since the last call.
    pub fn computer_keyboard_sent(&mut self) -> Vec<lumen_core::midi::Msg> {
        self.keyboard.take_sent()
    }

    /// What the test device has played since the last call, frame by
    /// frame: the main pair's left and right, then the cue pair's (M4
    /// slice 4; silent without a cue pair).
    pub fn heard_all(&mut self) -> Vec<[f32; 4]> {
        self.heard_frames()
    }

    /// What track `id`'s meter shows (M4 slice 5): the louder side's
    /// peak, RMS and hold line in dB, and the clip light.
    pub fn meter(&self, id: Id) -> MeterShown {
        let reading = self.arrangement.meters.reading(id.0);
        MeterShown {
            peak_db: reading.peak[0].max(reading.peak[1]),
            rms_db: reading.rms[0].max(reading.rms[1]),
            hold_db: reading.hold_db(),
            clipped: reading.clipped,
        }
    }

    /// The detail panel's height, in points.
    pub fn detail_size(&self) -> f32 {
        self.detail_size
    }

    /// Sets the detail panel's height, as opening a project with that
    /// layout saved does (the panel takes it on the next frame).
    pub fn set_detail_size(&mut self, size: f32) {
        self.detail_size = size;
    }

    /// The browser's width, in points.
    pub fn browser_size(&self) -> f32 {
        self.browser_size
    }

    /// Sets the browser's width, as a saved layout does.
    pub fn set_browser_size(&mut self, size: f32) {
        self.browser_size = size;
    }

    /// The browser's least width and the least height of the detail panel
    /// on the Devices tab, in points.
    pub fn panel_limits(&self) -> (f32, f32) {
        (crate::BROWSER_MIN, crate::chain::panel_min_height())
    }

    /// Some meter is still moving, so the app keeps drawing frames.
    pub fn meters_moving(&self) -> bool {
        self.arrangement.meters.moving()
    }

    /// How the transport's mark is lit (ADR-0043): how far its sun has
    /// risen (0 to 1) and whether it is red for recording.
    pub fn mark(&self) -> (f32, bool) {
        (self.mark.rise, self.mark.recording)
    }

    /// The master's loudness as shown: momentary, short-term and
    /// integrated LUFS, and the true peak in dBTP.
    pub fn loudness(&self) -> (f64, f64, f64, f32) {
        let l = self.arrangement.meters.loudness();
        (l.momentary, l.short_term, l.integrated, l.true_peak)
    }

    /// A cue pair is open (chosen in the audio settings and running).
    pub fn cue_ready(&self) -> bool {
        self.transport.audio.cue_ready()
    }

    /// The metronome's output as the settings hold it: `main` or `cue`.
    pub fn saved_metronome_output(&self) -> &str {
        &self.settings.metronome.output
    }

    /// How bright the MIDI chip's IN and OUT lights were when last drawn
    /// (0 at rest, 1 at a flash, fading over `motion::NOTE`).
    pub fn midi_chip_lights(&self) -> (f32, f32) {
        (
            self.transport.midi_lights.input,
            self.transport.midi_lights.output,
        )
    }

    /// How bright the light after a MIDI track's name was when last drawn
    /// (0 at rest, and always 0 while the track does not listen).
    pub fn header_light(&self, ctx: &egui::Context, track: Id) -> f32 {
        crate::arrangement::listen_light_shown(ctx, track)
    }

    /// How bright the popover's light for a port was when last drawn
    /// (`None` when the popover is not showing it).
    pub fn midi_row_light(&self, direction: lumen_midi::Direction, name: &str) -> Option<f32> {
        self.transport
            .midi_lights
            .rows
            .iter()
            .find(|(id, _)| id.direction == direction && id.name == name)
            .map(|(_, b)| *b)
    }

    /// The MIDI switches the settings hold: (direction, name, track, remote).
    pub fn saved_midi_ports(&self) -> Vec<(String, String, bool, bool)> {
        let mut ports: Vec<_> = self
            .settings
            .midi
            .ports
            .iter()
            .map(|p| (p.direction.clone(), p.name.clone(), p.track, p.remote))
            .collect();
        ports.sort();
        ports
    }

    fn heard_frames(&mut self) -> Vec<[f32; 4]> {
        let Some(heard) = self.transport.audio.heard.as_mut() else {
            return Vec::new();
        };
        let Ok(chunk) = heard.read_chunk(heard.slots() / 4 * 4) else {
            return Vec::new();
        };
        let samples: Vec<f32> = chunk.into_iter().collect();
        samples.as_chunks::<4>().0.to_vec()
    }

    /// Background work is in progress: a save, an open, or files loading.
    pub fn is_busy(&self) -> bool {
        self.saving.is_some()
            || self.save_waiting.is_some()
            || self.opening.is_some()
            || self.lanes.is_loading()
            || self.midi_reader.busy()
            || self.midi_exporting.is_some()
            || self.midi_writing.is_some()
    }

    /// A problem with saving or opening, as shown to the user.
    pub fn file_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    /// Whether the audio file `media` is loaded with its waveform.
    pub fn has_waveform(&self, media: Id) -> bool {
        self.lanes
            .library
            .get(media.0)
            .is_some_and(|item| item.peaks.is_some())
    }

    /// The selected tracks.
    pub fn selected_tracks(&self) -> &[Id] {
        &self.arrangement.selected
    }

    /// The arrangement's horizontal view.
    pub fn view(&self) -> TimeView {
        self.nav.view
    }

    /// Jumps to `view`, as zooming and scrolling would.
    pub fn set_view(&mut self, view: TimeView) {
        self.nav.set(view);
    }

    /// The height of every track row together, in points.
    pub fn tracks_height(&self, ctx: &egui::Context) -> f32 {
        self.arrangement
            .height_of_content(ctx, self.document.project(), self.lanes.pending().len())
    }

    /// The loop the engine plays: start, end, on.
    pub fn engine_loop(&self) -> (Ticks, Ticks, bool) {
        let r = self.transport.loop_region;
        (r.start, r.end, r.enabled)
    }

    /// Where clip `id` is drawn on screen, if it is in view.
    pub fn clip_rect(&self, ctx: &egui::Context, id: Id) -> Option<egui::Rect> {
        self.arrangement
            .clip_rects(
                ctx,
                self.canvas,
                self.document.project(),
                self.lanes.pending().len(),
                self.nav.view,
            )
            .into_iter()
            .find(|(_, clip, _)| *clip == id)
            .map(|(_, _, rect)| rect)
    }

    /// Where clip `id` is drawn on the timeline now: on its way while it
    /// glides after an edit (M4.5), else where it is.
    pub fn drawn_clip_start(&self, ctx: &egui::Context, id: Id) -> Option<Ticks> {
        let now = ctx.input(|i| i.time);
        self.arrangement
            .drawn_start(self.document.project(), id, now)
    }

    /// How strongly clip `id` is washed after an undo (M4.5), 0 when not.
    pub fn clip_wash(&self, ctx: &egui::Context, id: Id) -> f32 {
        self.arrangement.wash(id, ctx.input(|i| i.time))
    }

    /// Where the handles of audio clip `id` are on screen (slice 6d), if it
    /// is in view and big enough to show them.
    pub fn clip_handles(&self, ctx: &egui::Context, id: Id) -> Option<ClipHandlesAt> {
        use crate::arrangement::handles::ClipHandles;
        use crate::arrangement::select::Edge;
        let rect = self.clip_rect(ctx, id)?;
        let clip = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| c.id == id)?;
        let view = self.nav.view;
        let left = self.canvas.left();
        let h = ClipHandles::of(clip, rect, |t| view.x(left, t.as_quarters()))?;
        Some(ClipHandlesAt {
            fade_in: h.handle_pos(Edge::Start),
            fade_out: h.handle_pos(Edge::End),
            bend_in: h.bend_pos(Edge::Start),
            bend_out: h.bend_pos(Edge::End),
            gain: egui::pos2(h.wave.center().x, h.gain_y()),
            wave: h.wave,
        })
    }

    /// The selected clips.
    pub fn selected_clips(&self) -> Vec<Id> {
        self.arrangement.clips.iter().copied().collect()
    }

    /// The selected time range: start, end and tracks.
    pub fn time_range(&self) -> Option<(Ticks, Ticks, Vec<Id>)> {
        self.arrangement
            .range
            .as_ref()
            .map(|r| (r.start, r.end, r.tracks.clone()))
    }

    /// The info line's lit action ("Ctrl+drag to copy"): the drag the
    /// keys held now choose.
    pub fn info_lit(&self, ctx: &egui::Context) -> Option<String> {
        let held = ctx.input(|i| crate::app::info::Held::of(i.modifiers));
        self.info.as_ref()?.lit_text(&self.keymap, held)
    }

    /// The live value drawn by the pointer in the last frame while dragging
    /// (the tape speed), if any.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// What the info line says about what is under the pointer this frame
    /// (M4 slice 0), as words: "Clip “kick”: drag to move · …".
    pub fn info_line(&self) -> Option<String> {
        self.info.as_ref().map(|i| i.text(&self.keymap))
    }

    /// The short note shown after undo, save, open or a refused edit.
    pub fn note(&self) -> Option<&str> {
        self.toast.as_ref().map(|(text, _)| text.as_str())
    }

    /// What Undo would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.document.history().undo_label()
    }

    /// Answers the next file dialog with `paths`, as if the user chose
    /// them (a dialog with no answer given is cancelled).
    pub fn answer_next_dialog(&mut self, paths: Vec<PathBuf>) {
        if let Some(answers) = &mut self.dialog_answers {
            answers.push_back(paths);
        }
    }

    /// Saves to `path`, as Save does once its dialog has a path.
    pub fn save_to(&mut self, path: PathBuf) {
        self.start_save(path);
    }

    /// Whether the command palette is open.
    pub fn palette_open(&self) -> bool {
        self.palette.is_open()
    }

    /// The palette's rows, top to bottom: what each says, and whether it
    /// can run now.
    pub fn palette_shows(&self, ctx: &egui::Context) -> Vec<(String, bool)> {
        self.palette_rows(ctx)
            .into_iter()
            .map(|row| (row.label, row.ready.is_ok()))
            .collect()
    }

    /// The name of the command the palette's top row runs, if it is a
    /// command (its label may say what it acts on instead).
    pub fn palette_first_command(&self, ctx: &egui::Context) -> Option<&'static str> {
        match self.palette_rows(ctx).first()?.entry {
            crate::app::palette::Entry::Command(command) => Some(command.name()),
            _ => None,
        }
    }

    /// The name of every command in the palette's table.
    pub fn command_names() -> Vec<&'static str> {
        crate::commands::ALL.iter().map(|info| info.name).collect()
    }

    /// The shortcuts of the command named `name`, as shown ("Ctrl+D").
    pub fn shortcuts_of(&self, name: &str) -> Vec<String> {
        let info = crate::commands::ALL
            .iter()
            .find(|info| info.name == name)
            .unwrap_or_else(|| panic!("no command named {name}"));
        self.keymap
            .chords(info.command)
            .iter()
            .map(|c| c.text())
            .collect()
    }

    /// The changed shortcuts the settings file will keep, by command id.
    pub fn saved_shortcuts(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        self.settings.commands.shortcuts.clone()
    }

    /// The project as Save writes it: the document with the view stamped
    /// in (zoom, scroll, panels, grid, track heights).
    pub fn project_as_saved(&self) -> Project {
        self.stamped()
    }

    /// The pane whose anchor has the keyboard (F6), by name.
    pub fn focused_pane(&self, ctx: &egui::Context) -> Option<String> {
        crate::app::panes::focused(ctx).map(|p| format!("{p:?}"))
    }

    /// Whether the detail panel is shown.
    pub fn detail_shown(&self) -> bool {
        self.show_detail
    }

    /// Whether the Keyboard shortcuts window is open.
    pub fn shortcuts_open(&self) -> bool {
        self.shortcuts.open
    }

    /// A scan, or the first read of the plugin list, is under way.
    pub fn plugins_busy(&self) -> bool {
        self.plugins.busy()
    }

    /// What the browser's scan line says is happening, in words.
    pub fn plugin_status(&self) -> String {
        format!("{:?}", self.plugins.status())
    }

    /// The installed plugins (not Lumen's own devices), by name.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins
            .catalog()
            .devices()
            .iter()
            .filter(|d| d.source == lumen_plugin_catalog::catalog::Source::Vst3)
            .map(|d| d.name.clone())
            .collect()
    }

    /// The modules Lumen will not load, by name.
    pub fn blocked_plugins(&self) -> Vec<String> {
        self.plugins
            .catalog()
            .blocked()
            .iter()
            .map(|b| b.name.clone())
            .collect()
    }

    /// The ids of the favorite devices, as the settings keep them.
    pub fn favorite_devices(&self) -> Vec<String> {
        self.settings.plugins.favorites.clone()
    }

    /// Selects these tracks, as clicking their headers would.
    pub fn select_tracks(&mut self, ids: &[Id]) {
        self.arrangement.selected = ids.to_vec();
    }

    /// Chooses a card in the Devices tab, as clicking it would.
    pub fn choose_card(&mut self, device: Id) {
        self.chain.selected = Some(device);
    }

    /// The card chosen in the Devices tab, if one is.
    pub fn chosen_card(&self) -> Option<Id> {
        self.chain.selected
    }

    /// The ids of the recently chosen devices, most recent first.
    pub fn recent_devices(&self) -> Vec<String> {
        self.settings.plugins.recents.clone()
    }

    /// The id of the device selected in the browser, if one is.
    pub fn selected_device(&self) -> Option<String> {
        match &self.browser.view.selected {
            Some(crate::browser::view::Selection::Device(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// Whether the browser pane is shown.
    pub fn browser_shown(&self) -> bool {
        self.show_browser
    }

    /// Puts the system's cursor at `at` (or nowhere), as read while files
    /// are dragged in from the system, when egui has no pointer.
    pub fn set_system_cursor(&mut self, at: Option<egui::Pos2>) {
        self.system_cursor = at;
    }
}

/// Where a clip's fade and gain handles are on screen
/// ([`App::clip_handles`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHandlesAt {
    /// The fade handles, at the top of the wave area.
    pub fade_in: egui::Pos2,
    pub fade_out: egui::Pos2,
    /// The bend dots, when a fade is long enough to show one.
    pub bend_in: Option<egui::Pos2>,
    pub bend_out: Option<egui::Pos2>,
    /// The middle of the gain line.
    pub gain: egui::Pos2,
    /// Where the waveform is drawn.
    pub wave: egui::Rect,
}

/// The `index`th colour of the clip and track palette, as the swatch
/// "Colour {index + 1}" sets it (slice 6e).
pub fn clip_colour(index: usize) -> lumen_project::model::Color {
    crate::arrangement::color_from(crate::theme::CLIP_COLORS[index])
}

/// The clip editor (M5 slice 3c), for the end-to-end tests.
impl App {
    /// The clip open in the editor.
    pub fn editor_open_clip(&self) -> Option<Id> {
        self.note_editor.clip
    }

    /// The draw tool is chosen (else the pointer).
    pub fn editor_draw_tool(&self) -> bool {
        self.note_editor.tool == crate::note_editor::Tool::Draw
    }

    /// The picked notes, as indices into the open clip's notes.
    pub fn editor_picked(&self) -> Vec<usize> {
        self.note_editor.picked().to_vec()
    }

    /// The editor is in its own window.
    pub fn editor_popped(&self) -> bool {
        self.note_editor.popped
    }

    /// A gesture is in progress (a button is down in the notes).
    pub fn editor_gesture(&self) -> bool {
        self.note_editor.gesture.is_some()
    }

    /// The detail panel shows the Clip tab.
    pub fn clip_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Clip
    }

    /// Where the notes are drawn, if the editor has been.
    pub fn editor_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_view.map(|v| v.body)
    }

    /// The clip editor's own horizontal view (its zoom and scroll), where it
    /// drew last: separate from [`App::view`], the arrangement's (ADR-0048).
    pub fn editor_view(&self) -> Option<TimeView> {
        self.note_editor.last_view.map(|v| v.time)
    }

    /// The point at the start of clip time `local`, in the row of `pitch`
    /// (the row's middle), where the editor drew it last.
    pub fn editor_point(&self, local: Ticks, pitch: u8) -> Option<egui::Pos2> {
        let v = self.note_editor.last_view?;
        let y = v.row_top(i32::from(pitch)) + v.row_height / 2.0;
        Some(egui::pos2(v.x_of(local), y))
    }

    /// The lane under the roll is showing (it was drawn last frame).
    pub fn lane_shown(&self) -> bool {
        self.note_editor.last_lane.is_some() && !self.note_editor.lane.hidden
    }

    /// The name of the lane's tab chosen ("Velocity", "Mod wheel", ...).
    pub fn lane_name(&self) -> String {
        self.note_editor.lane.kind.name()
    }

    /// The lane's plotting area, where it drew last.
    pub fn lane_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_lane.map(|v| v.body)
    }

    /// The point where the open lane shows `value` at clip time `local`.
    pub fn lane_point(&self, local: Ticks, value: i32) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        Some(egui::pos2(v.x_of(local), v.y_of(value)))
    }

    /// Where the bend handle's dot is, when a curve was just drawn.
    pub fn lane_bend_dot(&self) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        let b = self.note_editor.lane.bend?;
        Some(crate::note_editor::lane::paint::bend_dot(&v, &b))
    }

    /// A gesture is in progress in the lane.
    pub fn lane_gesture(&self) -> bool {
        self.note_editor.lane.gesture.is_some()
    }

    /// The picked points of the controller lane (indices into the clip's
    /// events).
    pub fn lane_picked(&self) -> Vec<usize> {
        self.note_editor.lane.picked.clone()
    }

    /// Makes `pitch` visible in the editor, as scrolling would.
    pub fn editor_show_pitch(&mut self, pitch: u8) {
        if let Some(v) = self.note_editor.last_view {
            self.note_editor.scroll = crate::note_editor::view::scroll_to_centre(
                i32::from(pitch),
                v.row_height,
                v.body.height(),
            );
        }
    }
}

impl App {
    /// The grid step that `t` falls in now (`[start, end)`).
    pub fn grid_cell_at(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid_cell(t)
    }

    /// [`Self::grid_cell_at`] as the piano roll's own zoom sets it.
    pub fn editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.note_editor.last_view?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }
}

/// The piano roll's body drawn on its own with a sample clip (slice 3b),
/// for the shots harness: the real roll arrives with slice 3c.
pub struct PianoRollDemo {
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
}

impl PianoRollDemo {
    /// The sample clip in track colour `colour` (an index of
    /// [`clip_colour`]'s palette).
    pub fn new(colour: usize) -> Self {
        Self {
            boxes: std::sync::Arc::new(crate::piano_roll::demo::sample_boxes(
                crate::theme::CLIP_COLORS[colour],
            )),
        }
    }

    /// Installs the typefaces and the theme (`light` or dark), as the app
    /// does at start, in a context that has no `App`.
    pub fn prepare(ctx: &egui::Context, light: bool) {
        crate::fonts::install(ctx);
        let appearance = if light {
            crate::theme::Appearance::Light
        } else {
            crate::theme::Appearance::Dark
        };
        crate::theme::apply(ctx, appearance, 0.0);
    }

    /// Draws the roll over all of `ui`'s free area.
    pub fn show(&self, ui: &mut egui::Ui, points_per_beat: f64, row_height: f32) {
        crate::piano_roll::demo::show(ui, &self.boxes, points_per_beat, row_height);
    }
}
