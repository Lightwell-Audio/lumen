//! The Lumen application: the single-window layout from the specification
//! (transport bar, browser, arrangement, detail panel) on egui, with the
//! arrangement canvas drawn by `lumen-render` and audio from `lumen-engine`.
//! Features arrive milestone by milestone; controls that do not work yet are
//! shown disabled and say when they arrive.
//!
//! ```text
//! lumen                          run the application
//! lumen --audio asio:"Focusrite USB ASIO"
//!                                use this device (driver: asio, wasapi,
//!                                exclusive; cpal on macOS and Linux)
//! lumen --open-audio drums.wav   add an audio file at bar 1 (or drag files
//!                                from Explorer onto the arrangement)
//! lumen --scale 1.5              UI scale
//! lumen --audio-settings         open the audio settings window at start
//! lumen --probe-asio <clsid>     (helper) report one ASIO driver as JSON
//! lumen --stress                 open the generated quality-bar project
//!                                (200 tracks, 20,000 clips)
//! lumen --stress --bench-frames out.json
//!                                frame-time benchmark: scroll and zoom it with
//!                                vsync off, report, exit (status 3 below 60 fps);
//!                                --bench-size 3440x1400 sets the window
//! lumen --smoke-test             render a few frames, report start-up time, exit
//! lumen --screenshot out.png     render, save a screenshot, exit
//! lumen --audio test:fake --settings s.json --autosave-dir dir
//!                                isolated from the user (tests): the silent
//!                                test device, its own settings and autosaves
//! ```
//!
//! More test flags are in `launch.rs` and `docs/DEVELOPMENT.md`.

mod app;
mod arrangement;
mod audio;
mod audio_settings;
mod autosave;
mod bench;
mod canvas;
mod crash;
mod document;
mod files;
mod fonts;
mod icons;
mod lanes;
mod launch;
mod media;
mod motion;
mod owner;
mod panels;
mod position;
mod probe;
mod ruler;
mod settings;
pub mod testing;
mod theme;
mod timefmt;
mod timeview;
mod trace;
mod transport;
mod widgets;

pub use launch::Launch;
pub use timeview::TimeView;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use lumen_core::product::{PRODUCT_NAME, display_name};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature};

use app::projects::{FileDialog, Next};
use transport::Transport;

/// The application: one window, one project.
pub struct App {
    /// The open project with its undo history (M3 slice 3).
    document: document::Document,
    /// The document state the transport's timeline was built from.
    synced: Option<u64>,
    /// The undo history window is open.
    show_history: bool,
    /// Track headers, heights, folding and selection (M3 slice 4).
    arrangement: arrangement::Arrangement,
    /// The note shown after undo, redo or save, and since when.
    toast: Option<(String, Instant)>,
    /// File work in progress (slice 3c): a dialog, a save or an open.
    dialog: Option<std::sync::mpsc::Receiver<FileDialog>>,
    saving: Option<std::sync::mpsc::Receiver<Result<files::Saved, String>>>,
    opening: Option<std::sync::mpsc::Receiver<Result<files::Opened, String>>>,
    /// What to do once unsaved changes are dealt with.
    prompt: Option<Next>,
    /// What to do after the save in progress succeeds.
    after_save: Option<Next>,
    /// A problem with saving or opening, shown until dismissed.
    file_notice: Option<String>,
    /// Closing for real, after the unsaved-changes prompt.
    quitting: bool,
    /// The main window, to own file dialogs.
    owner: Option<owner::Owner>,
    /// Autosave of unsaved changes (slice 3d). An `Option` only so it can
    /// be taken out while the app builds the save request.
    autosave: Option<autosave::Autosave>,
    /// An autosave left by a crash, offered for recovery.
    leftover: Option<autosave::Leftover>,
    /// A crash report from an earlier run not shown yet, and `--crash-test`.
    crash_report: Option<PathBuf>,
    crash_test: Option<crash::Test>,
    /// The leftover being recovered (its open is in progress).
    recovering: Option<autosave::Leftover>,
    transport: Transport,
    /// Audio clips on the simple timeline (M2 slice 6).
    lanes: lanes::Lanes,
    /// Where dropped files go: under the pointer, snapped, or the playhead.
    drop_target: Ticks,
    /// Where dropped files go: new tracks, or the track under the pointer
    /// (slice 6e).
    drop_into: document::DropInto,
    settings: settings::Settings,
    settings_path: Option<PathBuf>,
    /// Settings changed at this time and are not saved yet (saved once
    /// changes stop for half a second, so dragging a slider is one write).
    settings_changed: Option<Instant>,
    /// Writes settings on a background thread (audit H1).
    settings_writer: settings::Writer,
    show_browser: bool,
    show_detail: bool,
    /// Full panel sizes, for sliding (kept while the panels are open).
    browser_size: f32,
    detail_size: f32,
    /// Set by the raw-input hook when Shift+Tab is pressed.
    toggle_detail_requested: bool,
    /// Where the arrangement looks (zoom, scroll, fit, back): slice 4d.
    nav: app::navigate::Nav,
    /// The editing grid and snapping (slice 5a); saved with the project.
    grid: lumen_core::grid::Grid,
    /// The selected locator and the one being renamed (slice 5d).
    locators: app::locators::Locators,
    /// The drag hint drawn this frame, if any (MT-APP-13 run 1); the
    /// end-to-end tests read it.
    hint: Option<String>,
    /// Where the lanes were drawn last frame.
    canvas: egui::Rect,
    /// What Copy and Cut put aside (slice 6b), for Paste.
    clipboard: Option<lumen_project::edit::Clipboard>,
    /// A text field had the keyboard at the end of the last frame. egui
    /// takes its focus away on Esc before the app's keys run, so that Esc
    /// is the field's (it cancels), not a clip or track key (slice 6e).
    typing: bool,
    /// What the lanes' right-click menu is for (owner's A, MT-APP-14 run 2).
    menu_for: app::editing::MenuFor,
    /// The clip being renamed and the pinned properties card (slice 6e).
    clip_props: app::clip_card::ClipProps,
    smoke_test_frames: Option<u32>,
    /// Where `--screenshot` saves, once the frames have rendered.
    screenshot: Option<PathBuf>,
    started: Instant,
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    autoplay: Option<Duration>,
    autoplay_count: u32,
    /// `--save-as <path>` (tests): save once imports are done, then close.
    save_as: Option<PathBuf>,
    /// Playing or not, as last traced, to log only changes.
    traced: Option<bool>,
    /// `--scale` or `--appearance` given: a one-off, not saved; the file
    /// keeps `saved_appearance` until the user changes it in the menu.
    appearance_override: bool,
    saved_appearance: settings::Appearance,
    /// The OS theme when the appearance was last applied ("Match system").
    system_theme: Option<egui::Theme>,
    /// The device banner's last message, kept while it slides away.
    banner: Option<(String, bool)>,
    /// `--bench-frames`: the frame-time benchmark in progress.
    bench: Option<bench::Bench>,
}

pub(crate) fn empty_project() -> Project {
    Project {
        name: "Untitled".into(),
        sample_rate: 48_000,
        tempo: vec![TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
        }],
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tracks: Vec::new(),
        media: Vec::new(),
        markers: Vec::new(),
        transport: Default::default(),
        view: Default::default(),
    }
}

fn arrives(milestone: &str) -> String {
    format!("Arrives in {milestone}")
}

impl eframe::App for App {
    /// A normal close: nothing is left to recover (the user saved, or chose
    /// not to).
    fn on_exit(&mut self) {
        // A change still settling is saved now; the writer finishes its
        // queue when it is dropped with the app.
        if self.settings_changed.is_some() {
            self.write_settings();
        }
        if let Some(autosave) = &mut self.autosave {
            autosave.clear();
        }
    }

    /// Shift+Tab toggles the detail panel (spec §5). It is taken here, before
    /// egui sees it, so it does not also move keyboard focus backwards.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        for event in &raw_input.events {
            if let egui::Event::PointerButton { pos, pressed, .. } = event {
                trace::log(|| {
                    format!(
                        "input: mouse {} at ({:.0}, {:.0})",
                        if *pressed { "down" } else { "up" },
                        pos.x,
                        pos.y
                    )
                });
            }
            if let egui::Event::Key {
                key, pressed: true, ..
            } = event
            {
                trace::log(|| format!("input: key {key:?}"));
            }
        }
        raw_input.events.retain(|event| {
            let toggle = matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                    if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            );
            if toggle {
                self.toggle_detail_requested = true;
            }
            // Drop the release too, so egui never sees half of the shortcut.
            !matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            )
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.owner.is_none() {
            self.owner = owner::Owner::of(frame);
        }
        let ctx = ui.ctx().clone();
        if let Some(test) = self.crash_test {
            crash::fail(test);
        }
        self.drive_bench(&ctx);
        self.hint = None;
        self.transport.update();
        self.transport.shortcuts(&ctx);
        self.undo_shortcuts(&ctx);
        self.grid_shortcuts(&ctx);
        self.locator_shortcuts(&ctx);
        self.clip_property_shortcuts(&ctx);
        self.clip_shortcuts(&ctx);
        self.arrangement.shortcuts(&ctx, &mut self.document);
        self.file_shortcuts(&ctx);
        self.poll_files(&ctx);
        self.autosave(&ctx);
        let rate = self.transport.state().sample_rate;
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            trace::log(|| {
                format!(
                    "drop: {} file(s) at {:?} into {:?}",
                    dropped.len(),
                    self.drop_target,
                    self.drop_into
                )
            });
            self.lanes.add_files(
                dropped,
                self.drop_target,
                self.drop_into,
                rate,
                &ctx,
                &mut self.document,
            );
        }
        self.lanes.update(
            rate,
            &ctx,
            &mut self.document,
            &mut self.transport.audio.clips,
        );
        self.sync_document(&ctx);
        self.transport.clip_memory = self.lanes.library.memory();
        self.save_settings_when_settled(&ctx);
        if let Some(e) = self.settings_writer.failure() {
            self.transport.audio.notice = Some(format!("Could not save settings: {e}"));
        }
        if let Some(path) = self.save_as.clone()
            && !self.lanes.is_loading()
            && !self.document.project().tracks.is_empty()
        {
            if self.saving.is_none() && self.document.is_dirty() {
                self.start_save(files::project_file_for(&path));
            } else if self.saving.is_none() {
                println!(
                    "saved {}",
                    self.document.path.as_deref().unwrap_or(&path).display()
                );
                self.save_as = None;
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Some(every) = self.autoplay {
            // Toggle play and stop every `every`, as Space would.
            let elapsed = self.started.elapsed();
            let due = every * self.autoplay_count;
            if elapsed >= due {
                self.autoplay_count += 1;
                trace::log(|| format!("autoplay: toggle {}", self.autoplay_count));
                self.transport.play_stop();
            } else {
                ctx.request_repaint_after(due - elapsed);
            }
        }
        if trace::enabled() {
            let state = self.transport.state();
            if self.traced != Some(state.playing) {
                trace::log(|| {
                    format!(
                        "frame: shows {} at sample {}",
                        if state.playing { "PLAYING" } else { "STOPPED" },
                        state.sample
                    )
                });
                self.traced = Some(state.playing);
            }
        }
        if ctx.system_theme() != self.system_theme {
            self.apply_appearance(&ctx);
        }
        let panels_before = (self.show_browser, self.show_detail);
        if std::mem::take(&mut self.toggle_detail_requested) {
            self.show_detail = !self.show_detail;
        }

        egui::Panel::top("transport").show(ui, |ui| self.transport(ui));
        self.device_banner(ui);
        self.crash_banner(ui);
        self.file_banner(ui);
        self.recovery_banner(ui);
        self.transport.windows(&ctx);
        self.history_window(&ctx);
        self.prompt_window(&ctx);

        // A focused control must not vanish with its panel: AccessKit
        // rejects focus on a node that no longer exists. Release focus
        // whenever a panel is hidden, whatever hid it.
        let hid_a_panel =
            (panels_before.0 && !self.show_browser) || (panels_before.1 && !self.show_detail);
        if hid_a_panel {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        // Panels slide open and closed (motion: GLIDE, panels.rs).
        panels::sliding(
            ui,
            "browser",
            panels::Side::Left,
            self.show_browser,
            &mut self.browser_size,
            |ui| {
                ui.label(RichText::new("Browser").font(fonts::display(theme::size::HEADING)));
                ui.add_space(8.0);
                ui.label(RichText::new("Devices, samples, projects and your places").weak());
                ui.label(RichText::new(arrives("Milestones 3 and 6")).weak().small());
            },
        );
        panels::sliding(
            ui,
            "detail",
            panels::Side::Bottom,
            self.show_detail,
            &mut self.detail_size,
            |ui| {
                ui.horizontal(|ui| {
                    for (tab, milestone) in [
                        ("Devices", "Milestone 6"),
                        ("Clip", "Milestone 5"),
                        ("Mixer", "Milestone 4"),
                    ] {
                        ui.add_enabled(false, egui::Button::new(tab))
                            .on_disabled_hover_text(arrives(milestone));
                    }
                });
                ui.label(
                    RichText::new(
                        "Select a track or clip to see its devices, editor or mixer strip.",
                    )
                    .weak(),
                );
            },
        );
        egui::CentralPanel::default().show(ui, |ui| self.arrangement(ui));
        self.typing = ctx.egui_wants_keyboard_input();

        // Scheduled after drawing, because buttons are clicked while drawing.
        // Every frame while playing or settling after a command; otherwise
        // once a second for device events, so idle CPU stays near zero.
        if self.transport.animating() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        if let Some(path) = &self.screenshot {
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                    _ => None,
                })
            });
            if let Some(image) = image {
                match save_png(path, &image) {
                    Ok(()) => println!("screenshot saved to {}", path.display()),
                    Err(e) => eprintln!("screenshot: {e}"),
                }
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // A screenshot shows the settled state: it waits while a project
        // opens or files load (at most 30 s, so a stuck load cannot hang a
        // test run).
        let settling = self.screenshot.is_some()
            && (self.opening.is_some() || self.lanes.library.is_loading())
            && self.started.elapsed() < Duration::from_secs(30);
        if let Some(frames) = self.smoke_test_frames.as_mut() {
            if !settling {
                *frames = frames.saturating_sub(1);
            }
            if *frames == 0 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_test_frames = None;
            } else if *frames == 0 {
                println!(
                    "smoke test: {} frames rendered; first frame within {:.0} ms of start",
                    SMOKE_FRAMES,
                    FIRST_FRAME_MS.get().copied().unwrap_or(f64::NAN)
                );
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
        FIRST_FRAME_MS.get_or_init(|| self.started.elapsed().as_secs_f64() * 1e3);
    }
}

const SMOKE_FRAMES: u32 = 30;
static FIRST_FRAME_MS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = image.size;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .map_err(|e| e.to_string())
}

impl App {
    /// Starts Lumen as `launch` says: from the command line, or a test.
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let Launch {
            started,
            settings: saved,
            settings_warning,
            settings_path,
            ..
        } = launch;
        let mut document = document::Document::new(if launch.stress {
            lumen_project::testing::synthetic(42, 200, 100, 10, 0)
        } else {
            empty_project()
        });
        let mut transport = Transport::new(document.project(), &launch.audio);
        transport.restore_metronome((&saved.metronome).into());
        transport.restore_follow(saved.follow);
        transport.time_format = saved.time_format;
        transport.settings_window.open = launch.audio_settings;
        if let Some(warning) = settings_warning {
            transport.audio.notice = Some(match transport.audio.notice.take() {
                Some(notice) => format!("{warning}\n{notice}"),
                None => warning,
            });
        }
        let mut lanes = lanes::Lanes::default();
        // `--stress`: every file of the generated project plays the same
        // generated audio (one copy in memory, one set of peaks).
        if launch.stress {
            let rate = transport.state().sample_rate;
            let channels = bench::stress_audio(rate);
            let peaks = lumen_media::Peaks::build(&channels, &AtomicBool::new(false)).map(Arc::new);
            let audio = Arc::new(lumen_engine::ClipAudio {
                sample_rate: rate,
                channels,
            });
            for media in &document.project().media {
                if let Some(peaks) = &peaks {
                    lanes.library.insert_ready(
                        media.id.0,
                        &media.path,
                        Arc::clone(&audio),
                        Arc::clone(peaks),
                    );
                }
            }
        }
        if let Some(path) = launch.open_audio {
            lanes.add_files(
                vec![path],
                Ticks::ZERO,
                document::DropInto::NewTracks { before: None },
                transport.state().sample_rate,
                &cc.egui_ctx,
                &mut document,
            );
        }
        let saved_appearance = saved.appearance.clone();
        let mut app = App {
            transport,
            lanes,
            drop_target: Ticks::ZERO,
            drop_into: document::DropInto::NewTracks { before: None },
            settings: saved,
            settings_path,
            settings_changed: None,
            settings_writer: settings::Writer::start(),
            document,
            synced: None,
            show_history: false,
            arrangement: arrangement::Arrangement::default(),
            toast: None,
            dialog: None,
            saving: None,
            opening: None,
            prompt: None,
            after_save: None,
            file_notice: None,
            quitting: false,
            owner: None,
            autosave: None,
            leftover: None,
            crash_report: launch.crash_dir.as_deref().and_then(crash::unseen),
            crash_test: launch.crash_test,
            recovering: None,
            show_browser: true,
            show_detail: true,
            browser_size: 220.0,
            detail_size: 220.0,
            toggle_detail_requested: false,
            nav: app::navigate::Nav::default(),
            grid: lumen_core::grid::Grid::default(),
            locators: app::locators::Locators::default(),
            hint: None,
            canvas: egui::Rect::NOTHING,
            clipboard: None,
            typing: false,
            menu_for: app::editing::MenuFor::default(),
            clip_props: app::clip_card::ClipProps::default(),
            smoke_test_frames: launch.smoke_test.then_some(SMOKE_FRAMES),
            screenshot: launch.screenshot,
            started,
            autoplay: launch.autoplay,
            autoplay_count: 1,
            save_as: launch.save_as,
            traced: None,
            appearance_override: false,
            saved_appearance: saved_appearance.clone(),
            system_theme: None,
            banner: None,
            bench: launch.bench,
        };
        // `--scale` and `--appearance` are one-off overrides (tests,
        // screenshots); they are not saved unless changed in the menu.
        if let Some(scale) = launch.scale {
            app.settings.appearance.ui_scale = scale;
            app.appearance_override = true;
        }
        if let Some(theme) = launch.appearance {
            app.settings.appearance.theme = theme::Appearance::from_key(&theme).key().into();
            app.appearance_override = true;
        }
        fonts::install(&cc.egui_ctx);
        app.apply_appearance(&cc.egui_ctx);
        let mut autosave = autosave::Autosave::new(launch.autosave_dir);
        if let Some(delay) = launch.autosave_after {
            autosave = autosave.with_delay(delay);
        }
        app.autosave = Some(autosave);
        app.leftover = app.next_leftover();
        if launch.recover_latest
            && let Some(leftover) = app.leftover.take()
        {
            app.opening = Some(files::open(leftover.file.clone()));
            app.recovering = Some(leftover);
        }
        // `lumen <file>.lumen` (and opening a project from Explorer).
        if let Some(path) = launch.open {
            app.opening = Some(files::open(path));
        }
        app
    }
}

/// Runs Lumen as the command line says (the `lumen` binary).
pub fn run() -> eframe::Result {
    // The helper mode for safe ASIO queries (probe.rs): no window.
    let args: Vec<String> = std::env::args().collect();
    if let Some(clsid) = args
        .iter()
        .position(|a| a == probe::FLAG)
        .and_then(|i| args.get(i + 1))
    {
        std::process::exit(probe::run_helper(clsid));
    }
    let launch = Launch::from_args();
    if let Some(dir) = &launch.crash_dir {
        crash::install(dir.clone());
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(display_name())
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0]),
        ..Default::default()
    };
    // The benchmark measures how fast frames can be drawn, not the display's
    // refresh rate; `--bench-size 3440x1400` sets its window.
    if launch.bench.is_some() {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
        if let Some((w, h)) = launch.bench_size {
            options.viewport = options
                .viewport
                .with_inner_size([w, h])
                .with_position([0.0, 0.0]);
        }
    }
    // Compiled GPU pipelines are cached between launches (ADR-0002).
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |=
                adapter.features() & eframe::wgpu::Features::PIPELINE_CACHE;
            descriptor
        });
    }
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(move |cc| {
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                canvas::install(state).map_err(|e| format!("renderer: {e}"))?;
            }
            Ok(Box::new(App::new(cc, launch)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_project_is_valid_and_survives_save_and_load() {
        let project = empty_project();
        assert_eq!(lumen_project::validate(&project), Ok(()));
        let bytes = lumen_project::to_bytes(&project).expect("saves");
        assert_eq!(lumen_project::from_bytes(&bytes).expect("loads"), project);
    }

    #[test]
    fn the_window_title_comes_from_the_product_name() {
        assert!(display_name().starts_with(PRODUCT_NAME));
    }
}
