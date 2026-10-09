//! End-to-end smoke tests (ADR-0029): the whole app, started as a user
//! starts it and driven as a user drives it, through its accessibility tree
//! (button names, keys, dropped files), with egui's test harness.
//!
//! Each test gets its own folder for autosaves and files, default settings
//! that are never saved, and the silent test audio device, so a run never
//! touches the user's settings, autosaves or sound card. No window or GPU
//! is needed (GPU drawing is covered by `lumen-render`'s tests and the
//! real-binary smoke tests in `tests/smoke.rs`), so these run in CI.
//!
//! Run them with `cargo test -p lumen-app --test e2e`. Add a scenario for every
//! workflow a slice delivers (DEVELOPMENT.md, "End-to-end tests").

mod access;
mod clips;
mod commands;
mod cue;
mod editing;
mod fades;
mod files;
mod grid;
mod groups;
mod info;
mod keys;
mod locators;
mod looping;
mod mixing;
mod navigation;
mod palette;
mod playback;
mod properties;
mod returns;
mod rulers;
mod scrubbing;
mod sends;
mod shortcuts;
mod solo;
mod walkthrough;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::testing::TempDir;

use lumen_app::{App, Launch};

/// The window size the tests run at, in points.
const SIZE: egui::Vec2 = egui::vec2(1600.0, 900.0);
/// How long a test waits for background work (audio start, loads, saves).
const PATIENCE: Duration = Duration::from_secs(30);

/// A running Lumen.
pub struct Lumen {
    pub harness: Harness<'static, App>,
    /// Autosaves and the test's files; removed when the test ends.
    dir: TempDir,
}

impl Lumen {
    /// Starts Lumen with a fresh folder, as [`Launch::for_test`] describes.
    pub fn start(name: &str) -> Self {
        Self::start_with(TempDir::new(&format!("e2e-{name}")), |launch| launch)
    }

    /// Starts Lumen in `dir` (kept from an earlier run, for recovery), with
    /// `adjust` changing how it starts.
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

    /// The test's folder.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Stops without closing normally, as a crash would (the autosave
    /// stays), and hands back the folder.
    pub fn crash(self) -> TempDir {
        self.dir
    }

    /// Runs `frames` frames.
    pub fn frames(&mut self, frames: usize) {
        self.harness.run_steps(frames);
    }

    /// Runs frames until `done` holds, sleeping a little between them so
    /// background work (audio, loading, saving) moves on. Panics, saying
    /// what it waited for, if it takes longer than [`PATIENCE`].
    pub fn wait_for(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        let started = Instant::now();
        loop {
            self.harness.step();
            if done(self.harness.state()) {
                // State that other threads change (the audio's) can turn
                // after a frame was drawn: one more frame shows it.
                self.harness.step();
                return;
            }
            assert!(started.elapsed() < PATIENCE, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Clicks the control named `label`, then lets the app react.
    #[track_caller]
    pub fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.frames(3);
    }

    /// Whether a control named `label` is on screen.
    pub fn shows(&self, label: &str) -> bool {
        self.harness.query_by_label(label).is_some()
    }

    /// Whether the toggle named `label` is on.
    #[track_caller]
    pub fn is_on(&self, label: &str) -> bool {
        self.harness
            .get_by_label(label)
            .accesskit_node()
            .toggled()
            .is_some_and(|t| t == egui::accesskit::Toggled::True)
    }

    /// Drags with the left button from `from` to `to` in small steps,
    /// holding `modifiers`, then lets the app react.
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

    /// Right-clicks at `p`, then lets the app react (a menu opens).
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

    /// Clicks twice at `p`, quickly, after a pause (so it is not counted
    /// with an earlier double-click).
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

    /// Presses `key` with `modifiers`, then lets the app react.
    pub fn key(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.frames(3);
    }

    /// Drops files onto the middle of the arrangement, as from Explorer.
    pub fn drop_files(&mut self, paths: &[PathBuf]) {
        let input = self.harness.input_mut();
        input.dropped_files = paths
            .iter()
            .map(|p| std::sync::Arc::new(Dropped(p.clone())) as egui::DroppedFileHandle)
            .collect();
        self.harness.step();
        self.wait_for("the files to load", |app| !app.is_busy());
    }

    /// Drags files in from the system and drops them at `at`, as from
    /// Explorer onto that place (slice 6e: a track's lane, or the line
    /// between two). As on Windows, egui has no pointer while they are
    /// dragged: Lumen reads the system's cursor (MT-APP-14 run 3).
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

    /// What the info line says this frame ("" for nothing).
    pub fn info(&self) -> String {
        self.app().info_line().unwrap_or_default()
    }

    /// The info line's lit action this frame.
    pub fn lit(&self) -> Option<String> {
        self.app().info_lit(&self.harness.ctx)
    }

    /// Track names, top to bottom.
    pub fn track_names(&self) -> Vec<String> {
        self.app()
            .project()
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }
}

/// A file dropped from the system, as the window integration hands it over.
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

/// A stereo WAV of `seconds` of a quiet tone at 48 kHz, written to `path`.
pub fn wav(path: &Path, seconds: f32) -> PathBuf {
    let rate = 48_000_u32;
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let v = ((i as f32 * 0.05).sin() * 3_000.0) as i16;
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
