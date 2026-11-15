mod access;
mod audio_clip_properties;
mod audio_editor;
mod audio_editor_envelopes;
mod audio_editor_handles;
mod audio_editor_hearing;
mod audio_editor_range;
mod audio_export;
mod audio_input;
mod audio_record;
mod audition;
mod automation;
mod barfit;
mod bench_callback;
mod bounce;
mod browser_files;
mod builtins;
mod capture_midi;
mod chrome;
mod clip_badges;
mod clip_commands;
mod clip_envelopes;
mod clips;
mod clock;
mod commands;
mod compensation;
mod consolidate;
mod controllers;
#[path = "../shared/controls.rs"]
mod controls;
mod copies;
mod coverage;
mod crossfades;
mod cue;
mod delay_reverb;
mod detail_tabs;
mod device_change;
mod devices;
mod dropdown;
mod drum_editor;
mod drum_lanes_bench;
mod drum_routing;
mod drum_steps;
mod drums;
mod dynamics;
mod editing;
mod editors;
mod eq;
mod fades;
mod files;
mod first_run;
mod follow_tempo;
mod freeze;
mod grid;
mod groups;
mod hidden_notes;
mod hosting;
mod info;
mod instance;
mod keyboard;
mod keys;
mod lane_edit;
mod lane_override;
mod lane_record;
mod lane_view;
mod lanes;
mod light;
mod live_lights;
mod locators;
mod looping;
mod measure_latency;
mod media_pool;
mod meters;
mod midi;
mod midi_chip;
mod midi_from;
mod midi_play;
mod midi_trim;
mod midi_walkthrough;
mod mixer;
mod mixing;
mod monitor_badge;
mod motion;
mod navigation;
mod notes;
mod outputs;
mod overdub_seams;
mod palette;
mod parameters;
mod pitch_popover;
mod playback;
mod plugin_lanes;
mod plugins;
mod pops;
mod preferences;
mod preset_menu;
mod presets;
mod project_settings;
mod properties;
mod quantize;
mod record;
mod render_rate;
mod resilience;
mod returns;
mod revert;
mod rulers;
mod safe_mode;
mod scrubbing;
mod sends;
mod shortcuts;
mod sidechain;
mod smf;
mod solo;
mod song_lanes;
mod stepped;
mod streaming;
mod synth;
mod tap_tempo;
mod templates;
mod tools;
mod track_resize;
mod transport_commands;
mod walkthrough;
mod walkthrough_automation;
mod walkthrough_editing;
mod walkthrough_mix;
mod walkthrough_plugins;
mod walkthrough_song;
mod walkthrough_warp;
mod warp_chip;
mod warp_markers;
mod warp_pitch;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::testing::TempDir;

use lumen_app::{App, Launch};

const SIZE: egui::Vec2 = egui::vec2(1600.0, 900.0);
pub fn patience() -> Duration {
    lumen_testkit::patience()
}
const TEST_RATE: u32 = 48_000;

pub struct Lumen {
    pub harness: Harness<'static, App>,
    dir: TempDir,
}

impl Lumen {
    pub fn start(name: &str) -> Self {
        Self::start_with(TempDir::new(&format!("e2e-{name}")), |launch| launch)
    }

    pub fn start_with(dir: TempDir, adjust: impl FnOnce(Launch) -> Launch) -> Self {
        let launch = adjust(Launch::for_test(dir.path()));
        let harness = Harness::builder()
            .with_size(SIZE)
            .with_step_dt(1.0 / 60.0)
            .build_eframe(|cc| App::new(cc, launch));
        let mut lumen = Self { harness, dir };
        lumen.wait_for("the audio to start", App::audio_running);
        lumen
    }

    pub fn app(&self) -> &App {
        self.harness.state()
    }

    pub fn app_mut(&mut self) -> &mut App {
        self.harness.state_mut()
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub fn crash(mut self) -> TempDir {
        let started = Instant::now();
        while self.app().autosave_writing() {
            assert!(
                started.elapsed() < patience(),
                "timed out waiting for the autosave in progress"
            );
            std::thread::sleep(Duration::from_millis(5));
            self.harness.step();
        }
        self.dir
    }

    pub fn frames(&mut self, frames: usize) {
        self.harness.run_steps(frames);
    }

    pub fn listen(&mut self, settle: Duration, frames: usize) -> Vec<[f32; 4]> {
        self.frames(1);
        self.app_mut().heard_all();
        let settle = (settle.as_secs_f64() * f64::from(TEST_RATE)).ceil() as usize;
        let started = Instant::now();
        let mut heard = Vec::new();
        while heard.len() < settle + frames {
            assert!(
                started.elapsed() < patience(),
                "heard {} of {} frames",
                heard.len(),
                settle + frames
            );
            std::thread::sleep(Duration::from_millis(10));
            self.frames(1);
            heard.extend(self.app_mut().heard_all());
        }
        heard.split_off(settle)
    }

    pub fn listen_from(&mut self, start: impl FnOnce(&mut Self), frames: usize) -> Vec<[f32; 4]> {
        self.frames(1);
        self.app_mut().heard_all();
        start(self);
        let started = Instant::now();
        let mut heard = self.app_mut().heard_all();
        while heard.len() < frames {
            assert!(
                started.elapsed() < patience(),
                "heard {} of {frames} frames",
                heard.len()
            );
            std::thread::sleep(Duration::from_millis(10));
            self.frames(1);
            heard.extend(self.app_mut().heard_all());
        }
        heard
    }

    pub fn wait_for(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        let started = Instant::now();
        loop {
            self.harness.step();
            if done(self.harness.state()) {
                self.harness.step();
                return;
            }
            assert!(
                started.elapsed() < patience(),
                "timed out waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[track_caller]
    pub fn run_until(&mut self, what: &str, done: impl Fn(&Lumen) -> bool) {
        let started = Instant::now();
        loop {
            self.frames(1);
            if done(self) {
                return;
            }
            assert!(started.elapsed() < patience(), "{what} never happened");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[track_caller]
    pub fn open_preferences(&mut self, page: &str) {
        self.click("Lumen");
        self.click("Preferences\u{2026}");
        let row = format!("{page} page");
        self.run_until("the page list", |l| l.shows(&row));
        let started = Instant::now();
        loop {
            self.harness.get_by_label(&row).click();
            self.frames(3);
            if self.is_on(&row) {
                return;
            }
            assert!(
                started.elapsed() < patience(),
                "timed out waiting for the {page} page"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[track_caller]
    pub fn open_midi_settings(&mut self) {
        self.open_preferences("MIDI");
    }

    #[track_caller]
    pub fn open_audio_settings(&mut self) {
        self.open_preferences("Audio");
    }

    #[track_caller]
    pub fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.frames(3);
    }

    #[track_caller]
    pub fn pick(&mut self, button: &str, option: &str, chosen: impl Fn(&Lumen) -> bool) {
        let deadline = lumen_testkit::Deadline::new();
        while !chosen(self) {
            deadline.check(format_args!("{option:?} chosen from {button:?}"));
            let target = if self.shows(option) { option } else { button };
            if let Some(node) = self.harness.query_by_label(target) {
                node.scroll_to_me();
                self.frames(2);
            }
            match self.harness.query_by_label(target) {
                Some(node) => {
                    node.click();
                    self.frames(3);
                }
                None => self.frames(1),
            }
        }
    }

    #[track_caller]
    pub fn recover(&mut self) {
        self.run_until("the recovery banner", |lumen| lumen.shows("Recover"));
        self.click("Recover");
    }

    pub fn shows(&self, label: &str) -> bool {
        self.harness.query_by_label(label).is_some()
    }

    #[track_caller]
    pub fn is_on(&self, label: &str) -> bool {
        self.harness
            .get_by_label(label)
            .accesskit_node()
            .toggled()
            .is_some_and(|t| t == egui::accesskit::Toggled::True)
    }

    pub fn drag(&mut self, from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
        self.harness.event(egui::Event::ModifiersChanged(modifiers));
        self.harness.hover_at(from);
        self.harness.drag_at(from);
        self.frames(2);
        for step in 1..=8 {
            self.harness
                .hover_at(from + (to - from) * (step as f32 / 8.0));
            self.frames(1);
        }
        self.harness.drop_at(to);
        self.frames(3);
        self.harness
            .event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.frames(1);
    }

    pub fn show_whole_cards(&mut self) {
        self.frames(1);
        self.harness
            .get_all_by_label("Devices tab")
            .last()
            .expect("the detail panel's tabs")
            .click();
        self.frames(2);
        self.app_mut().set_detail_size(460.0);
        self.frames(20);
    }

    pub fn right_click(&mut self, p: egui::Pos2) {
        self.harness.hover_at(p);
        self.frames(1);
        for pressed in [true, false] {
            self.harness.event(egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frames(1);
        }
        self.frames(2);
    }

    pub fn click_at(&mut self, p: egui::Pos2, modifiers: Modifiers) {
        self.harness.event(egui::Event::ModifiersChanged(modifiers));
        self.harness.hover_at(p);
        self.frames(1);
        for pressed in [true, false] {
            self.harness.event(egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            });
            self.frames(1);
        }
        self.frames(2);
        self.harness
            .event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.frames(1);
    }

    pub fn double_click(&mut self, p: egui::Pos2) {
        self.frames(40);
        self.harness.hover_at(p);
        self.frames(1);
        for _ in 0..2 {
            for pressed in [true, false] {
                self.harness.event(egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
            self.frames(1);
        }
        self.frames(2);
    }

    pub fn key(&mut self, modifiers: Modifiers, key: Key) {
        let Some(event) = clipboard_event(modifiers, key) else {
            self.harness.key_press_modifiers(modifiers, key);
            self.frames(3);
            return;
        };
        self.harness.event(egui::Event::ModifiersChanged(modifiers));
        self.harness.step();
        if let Some(event) = event {
            self.harness.event(event);
            self.harness.step();
        }
        self.harness.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        });
        self.harness.step();
        self.harness
            .event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.frames(3);
    }

    pub fn paste_with_text_on_the_clipboard(&mut self, text: &str) {
        self.harness
            .event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
        self.harness.step();
        self.harness.event(egui::Event::Paste(text.into()));
        self.harness.step();
        self.harness.event(egui::Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        self.harness.step();
        self.harness
            .event(egui::Event::ModifiersChanged(Modifiers::NONE));
        self.frames(3);
    }

    pub fn drop_files(&mut self, paths: &[PathBuf]) {
        let input = self.harness.input_mut();
        input.dropped_files = paths
            .iter()
            .map(|p| std::sync::Arc::new(Dropped(p.clone())) as egui::DroppedFileHandle)
            .collect();
        self.harness.step();
        self.wait_for("the files to load", |app| !app.is_busy());
    }

    pub fn drop_files_at(&mut self, at: egui::Pos2, paths: &[PathBuf]) {
        self.harness.event(egui::Event::PointerGone);
        self.app_mut().set_system_cursor(Some(at));
        self.harness.input_mut().hovered_files = paths
            .iter()
            .map(|p| egui::HoveredFile {
                path: Some(p.clone()),
                ..Default::default()
            })
            .collect();
        self.frames(2);
        self.harness.input_mut().hovered_files.clear();
        self.drop_files(paths);
        self.app_mut().set_system_cursor(None);
    }

    pub fn info(&self) -> String {
        self.app().info_line().unwrap_or_default()
    }

    pub fn lit(&self) -> Option<String> {
        self.app().info_lit(&self.harness.ctx)
    }

    pub fn track_names(&self) -> Vec<String> {
        self.app()
            .project()
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

pub fn wav(path: &Path, seconds: f32) -> PathBuf {
    wav_at(path, seconds, 3_000.0)
}

pub fn wav_at(path: &Path, seconds: f32, peak: f32) -> PathBuf {
    wav_rated(path, seconds, peak, 48_000)
}

pub fn wav_rated(path: &Path, seconds: f32, peak: f32, rate: u32) -> PathBuf {
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let v = ((i as f32 * 0.05).sin() * peak) as i16;
            let [a, b] = v.to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the test WAV");
    path.to_path_buf()
}

pub fn keys(text: &str) -> String {
    if cfg!(target_os = "macos") {
        text.replace("Ctrl+", "Cmd+").replace("Alt+", "Option+")
    } else {
        text.to_owned()
    }
}

fn clipboard_event(modifiers: Modifiers, key: Key) -> Option<Option<egui::Event>> {
    let windows = cfg!(target_os = "windows");
    let cut =
        (modifiers.command && key == Key::X) || (windows && modifiers.shift && key == Key::Delete);
    let copy =
        (modifiers.command && key == Key::C) || (windows && modifiers.ctrl && key == Key::Insert);
    let paste =
        (modifiers.command && key == Key::V) || (windows && modifiers.shift && key == Key::Insert);
    if cut {
        Some(Some(egui::Event::Cut))
    } else if copy {
        Some(Some(egui::Event::Copy))
    } else if paste {
        Some(None)
    } else {
        None
    }
}
