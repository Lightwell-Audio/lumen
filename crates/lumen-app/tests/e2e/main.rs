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
mod detail_tabs;
mod device_change;
mod devices;
mod dropdown;
mod drum_editor;
mod drum_lanes_bench;
mod drums;
mod editing;
mod editors;
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
mod sampler;
mod scrubbing;
mod sends;
mod shortcuts;
mod sidechain;
mod smf;
mod solo;
mod song_lanes;
mod stepped;
mod streaming;
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

/// The window size the tests run at, in points.
const SIZE: egui::Vec2 = egui::vec2(1600.0, 900.0);
/// How long a test waits for background work (audio start, loads, saves)
/// before it calls it hung: a guard, never a measure of speed (ADR-0122,
/// `lumen_testkit`).
pub fn patience() -> Duration {
    lumen_testkit::patience()
}
/// The test device's sample rate unless a test chooses another: what
/// [`Lumen::listen`] counts its settling time in.
const TEST_RATE: u32 = 48_000;

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
    ///
    /// A real crash ends every thread with the process. Here the app is
    /// dropped and an autosave worker still writing would outlive it, holding
    /// the session's lock, so the next start would take the session for a
    /// running one and offer nothing. The write in progress is let finish
    /// first, with no frame after it that could start another.
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

    /// Runs `frames` frames.
    pub fn frames(&mut self, frames: usize) {
        self.harness.run_steps(frames);
    }

    /// Lets `settle` of audio play (past any glide), then listens until at
    /// least `frames` more frames have played: main left and right, then
    /// the cue pair's. Both are counted in frames the test device played,
    /// never in time: it plays a block per poll, so on a busy CI machine it
    /// once played 768 frames in 150 ms, and on macOS fewer (its sleeps run
    /// long), so levels were measured while still gliding.
    pub fn listen(&mut self, settle: Duration, frames: usize) -> Vec<[f32; 4]> {
        // What has played so far came before the change being listened for.
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

    /// Everything the test device plays from `start` on, for at least `frames`
    /// frames. Unlike [`Lumen::listen`], which drops what played before it is
    /// called, what played before `start` is dropped and nothing after it:
    /// pressing Play and then listening loses the song's first frames (on a
    /// slow runner a whole first hit), and a test of what plays from the
    /// start must hear all of it.
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

    /// Runs frames until `done` holds, sleeping a little between them so
    /// background work (audio, loading, saving) moves on. Panics, saying
    /// what it waited for, if it takes longer than [`patience`].
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
            assert!(
                started.elapsed() < patience(),
                "timed out waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Runs frames until `done` holds, for as long as background work may
    /// take ([`patience`]), never a count of frames: a worker thread answers
    /// in its own time, and on a loaded machine that can take far longer
    /// than a few hundred frames. For state that is drawn or that the test
    /// holds; [`wait_for`](Self::wait_for) reads the app's.
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

    /// Opens Preferences from the app menu on the page named `page`
    /// ("Audio", "MIDI"). The page's row is clicked until it is the chosen
    /// one, never a fixed number of frames after the window opens: a click
    /// that lands while the window is still being laid out does nothing (as
    /// it would for a user), and on a loaded machine that was enough to
    /// leave the page unopened.
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
            // The chosen page's row reports as on.
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

    /// Opens Preferences on its MIDI page.
    #[track_caller]
    pub fn open_midi_settings(&mut self) {
        self.open_preferences("MIDI");
    }

    /// Opens Preferences on its Audio page.
    #[track_caller]
    pub fn open_audio_settings(&mut self) {
        self.open_preferences("Audio");
    }

    /// Clicks the control named `label`, then lets the app react.
    #[track_caller]
    pub fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.frames(3);
    }

    /// Picks `option` from the dropdown named `button`, as a user does, until
    /// `chosen` holds. A click while the page scrolls or lays out lands where
    /// the control was, so a single click can miss on a busy machine (how
    /// many frames a page has had to settle depends on when the engine
    /// answered): the list is opened until the option shows, and the option
    /// clicked until the choice is made, each brought into view first.
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

    /// Clicks the recovery banner's Recover, waiting for the worker that
    /// scans the autosave folder to offer it (sweep M4a: the scan never runs
    /// on the interface's thread, so the banner comes a frame or two later).
    #[track_caller]
    pub fn recover(&mut self) {
        self.run_until("the recovery banner", |lumen| lumen.shows("Recover"));
        self.click("Recover");
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

    /// Opens the detail panel's Devices tab (the browser has one too: the
    /// panel's comes last), tall enough for whole cards: a short panel shows
    /// compact ones (D2).
    pub fn show_whole_cards(&mut self) {
        // The tab row moves with the subject's name: let a new selection
        // draw first, and the click land before the panel grows.
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

    /// Clicks at `p` with `modifiers` held.
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

    /// Presses `key` with `modifiers`, then lets the app react. The
    /// clipboard chords arrive as the real window sends them (see
    /// [`clipboard_event`]), with an empty system clipboard.
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

    /// Ctrl+V as the window sends it when the system clipboard holds
    /// `text`: a Paste event, then V's release.
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
    wav_at(path, seconds, 3_000.0)
}

/// [`wav`] with its peak at `peak` (of 32,767).
pub fn wav_at(path: &Path, seconds: f32, peak: f32) -> PathBuf {
    wav_rated(path, seconds, peak, 48_000)
}

/// [`wav_at`] at `rate`.
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

/// `text` with its chords written as this system writes them: Cmd for
/// Ctrl and Option for Alt on macOS (the app's `modifier_names`).
pub fn keys(text: &str) -> String {
    if cfg!(target_os = "macos") {
        text.replace("Ctrl+", "Cmd+").replace("Alt+", "Option+")
    } else {
        text.to_owned()
    }
}

/// What egui-winit sends for a clipboard chord's press instead of the key
/// (egui-winit 0.36 `is_cut_command` and friends): `Some(event)`, or
/// `Some(None)` for Ctrl+V with no text on the system clipboard, which
/// sends nothing; `None` for any other chord, which arrives as a key. Only
/// the release comes through as a key.
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
