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
//! lumen --test-latency 12.4      the engine's synthetic latent device on the
//!                                first track, to try delay compensation
//! lumen --smoke-test            render a few frames, report start-up time, exit
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
mod brand;
mod canvas;
mod commands;
mod crash;
mod document;
mod files;
mod fonts;
mod icons;
mod lanes;
mod launch;
mod light;
mod live;
mod media;
mod meters;
mod midi;
mod midi_out;
mod midi_play;
mod midi_settings;
mod motion;
mod owner;
mod panels;
mod playhead;
mod position;
mod probe;
mod routing;
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

use eframe::egui;
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
    /// In tests, the answers file dialogs give instead of showing (`None`:
    /// real dialogs).
    dialog_answers: Option<std::collections::VecDeque<Vec<PathBuf>>>,
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
    /// MIDI ports and the user's switches for them (M5 slice 0).
    midi: midi::Midi,
    /// Times MIDI to hardware outputs; starts with the first one a track names.
    midi_out: Option<midi_out::Output>,
    /// Sends the computer keyboard's notes into the engine, once the live
    /// feed is handed over (slice 2).
    live_keys: Option<live::LiveKeys>,
    /// Counts changes to the MIDI ports, and what the used outputs were last
    /// checked against (document, its state, port changes).
    midi_ports_seen: u64,
    /// The computer keyboard as a MIDI keyboard (M5 slice 2): a mode, not saved.
    keyboard: app::keyboard::ComputerKeyboard,
    midi_checked: Option<(u64, u64, u64)>,
    /// Audio clips on the simple timeline (M2 slice 6).
    lanes: lanes::Lanes,
    /// Where dropped files go: under the pointer, snapped, or the playhead.
    drop_target: Ticks,
    /// Where dropped files go: new tracks, or the track under the pointer
    /// (slice 6e).
    drop_into: document::DropInto,
    /// Where the pointer is while files are dragged in from the system.
    /// egui does not know: on Windows the window gets no pointer moves
    /// during the drag, and the window integration drops the position the
    /// system reports, so it is read from the system (owner, MT-APP-14
    /// run 3: drops between tracks were never offered).
    drag_pointer: Option<egui::Pos2>,
    /// Stands in for the system's cursor in tests (`testing`).
    system_cursor: Option<egui::Pos2>,
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
    /// Which keys run which commands (slice 7).
    keymap: commands::Keymap,
    /// The command palette (Ctrl+K).
    palette: app::palette::Palette,
    /// The Keyboard shortcuts window.
    shortcuts: app::shortcuts::Shortcuts,
    /// A chord being recorded for a command (the window or the palette).
    rebind: Option<app::shortcuts::Rebind>,
    /// Where the keyboard is among the panes (F6).
    panes: app::panes::Panes,
    /// Where the arrangement looks (zoom, scroll, fit, back): slice 4d.
    nav: app::navigate::Nav,
    /// The editing grid and snapping (slice 5a); saved with the project.
    grid: lumen_core::grid::Grid,
    /// The selected locator and the one being renamed (slice 5d).
    locators: app::locators::Locators,
    /// The drag hint drawn this frame, if any (MT-APP-13 run 1); the
    /// end-to-end tests read it.
    hint: Option<String>,
    /// What the info line shows (M4 slice 0): set by hovers this frame,
    /// drawn from the frame before.
    info: Option<app::info::Info>,
    info_shown: Option<app::info::Info>,
    /// Where the lanes were drawn last frame.
    canvas: egui::Rect,
    /// What Copy and Cut put aside (slice 6b), for Paste.
    clipboard: Option<lumen_project::edit::Clipboard>,
    /// A text field had the keyboard at the end of the last frame. egui
    /// takes its focus away on Esc before the app's keys run, so that Esc
    /// is the field's (it cancels), not a clip or track key (slice 6e).
    typing: bool,
    /// Clipboard chords egui-winit turned into events, being put back as
    /// keys (`app/clipboard_keys.rs`).
    clipboard_keys: app::clipboard_keys::Pending,
    /// When the meters last moved on (M4 slice 5).
    meters_at: Option<Instant>,
    /// How the transport's mark was last lit (ADR-0043).
    mark: widgets::mark::Look,
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
    /// `--test-latency <ms>`: the synthetic latent device on the first
    /// track.
    test_latency: Option<f64>,
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
        // Id 0: a new project's other ids count on from 1 (`fresh_id`);
        // older projects got the next free id when they were migrated.
        master: std::sync::Arc::new(lumen_project::model::Track::master(
            lumen_project::model::Id(0),
        )),
        pan_law: Default::default(),
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

    /// The detail panel's key (Shift+Tab, spec §5) is taken here, before
    /// egui sees it, so it does not also move keyboard focus backwards.
    /// Except where moving back is what Shift+Tab is for (the owner's
    /// choice, ADR-0015): typing in a field, or with the keyboard in an
    /// open window, it is left to egui.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
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
        // While a chord is recorded, it may be the panel's own.
        if self.rebind.is_some() {
            return;
        }
        let window = self.window_open();
        let focused = ctx.memory(|m| m.focused().is_some());
        if ctx.text_edit_focused() || (window && focused) {
            return;
        }
        let keymap = &self.keymap;
        let mut toggle = false;
        raw_input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = *event
            else {
                return true;
            };
            let chord = commands::Chord::pressed(modifiers, key);
            if keymap.command_for(chord) != Some(commands::Command::ToggleDetail) {
                return true;
            }
            toggle |= pressed;
            // Drop the release too, so egui never sees half of the shortcut.
            false
        });
        self.toggle_detail_requested |= toggle;
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.owner.is_none() {
            self.owner = owner::Owner::of(frame);
        }
        let ctx = ui.ctx().clone();
        // Before anything scrolls: whose this frame's wheel is (M4 slice 5).
        widgets::wheel::route(&ctx);
        self.transport.advance_playhead(ctx.input(|i| i.time));
        self.follow_file_drag(&ctx);
        if let Some(test) = self.crash_test {
            crash::fail(test);
        }
        self.drive_bench(&ctx);
        self.hint = None;
        self.transport.update();
        self.restore_clipboard_keys(&ctx);
        self.capture_chord(&ctx);
        self.computer_keyboard_keys(&ctx);
        self.command_keys(&ctx);
        if std::mem::take(&mut self.arrangement.import_requested) {
            self.run(&ctx, commands::Command::ImportAudio);
        }
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
        if let Some(ms) = self.test_latency
            && let Some(first) = self.document.project().tracks.first()
        {
            let frames = (ms * f64::from(rate) / 1e3).round() as u32;
            let latent = &mut self.lanes.devices.latent;
            if latent.len() != 1 || latent.get(&first.id) != Some(&frames) {
                latent.clear();
                latent.insert(first.id, frames);
            }
        }
        let cue = self.cue();
        self.lanes.update(
            rate,
            &ctx,
            &mut self.document,
            lanes::Handles {
                clips: &mut self.transport.audio.clips,
                midi: &mut self.transport.audio.midi,
                mixer: &mut self.transport.audio.mixer,
            },
            &cue,
        );
        self.sync_document(&ctx);
        let meters_moving = self.update_meters();
        self.update_outputs(&ctx);
        self.transport.clip_memory = self.lanes.library.memory();
        self.save_settings_when_settled(&ctx);
        if let Some(e) = self.settings_writer.failure() {
            self.transport.audio.notice = Some(format!("Could not save settings: {e}"));
        }
        self.test_modes_before(&ctx);
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
        for action in self.transport.windows(&ctx, &self.midi) {
            self.midi.apply(&action);
            if let midi::Action::Use(id, using) = &action {
                midi::Midi::remember(&mut self.settings.midi, id, *using);
                self.settings_changed = Some(Instant::now());
            }
        }
        if self.midi.poll() {
            self.midi_ports_seen += 1;
            ctx.request_repaint();
        }
        if let Some(notice) = self.midi.notices.pop() {
            self.transport.audio.notice = Some(notice);
        }
        self.update_midi_hardware(&ctx);
        self.history_window(&ctx);
        self.prompt_window(&ctx);
        self.shortcuts_window(&ctx);
        self.palette_window(&ctx);

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
        // Full width under everything else, so declared before the panels.
        self.draw_info_line(ui);
        // Panels slide open and closed (motion: GLIDE, panels.rs).
        panels::sliding(
            ui,
            "browser",
            (panels::Side::Left, egui::Rangef::new(96.0, f32::INFINITY)),
            self.show_browser,
            &mut self.browser_size,
            |ui| {
                let rect = ui.max_rect();
                self.panes.anchor(ui, app::panes::Pane::Browser, rect);
                // Its title in the display face, inset like the window
                // titles (it sat against the panel's corner), then an empty
                // state until its milestone, said as the unbuilt tabs say it
                // (the interface audit's S5).
                let inset = egui::vec2(theme::space::M, theme::space::M);
                let title = ui.painter().text(
                    rect.left_top() + inset,
                    egui::Align2::LEFT_TOP,
                    "Browser",
                    fonts::display(theme::size::HEADING),
                    theme::p().text,
                );
                let rect =
                    egui::Rect::from_min_max(egui::pos2(rect.left(), title.bottom()), rect.max);
                let state = widgets::EmptyState {
                    icon: Some(|p, at, colour| {
                        let square = egui::Rect::from_center_size(at, egui::vec2(16.0, 16.0));
                        p.extend(icons::shapes(icons::Icon::Search, square, colour));
                    }),
                    icon_size: theme::space::L,
                    title: "Nothing to browse yet",
                    hint: "Devices, samples and projects arrive in Milestone 6",
                    action: None,
                };
                widgets::empty_state(ui, rect, &state);
                ui.advance_cursor_after_rect(rect);
            },
        );
        // The detail panel: the Mixer tab (M4 slice 5); Devices and Clip
        // arrive with their milestones. Never shorter than a strip. Its
        // content is drawn after the arrangement, so Tab reaches it in
        // reading order.
        let mut detail_size = self.detail_size;
        let mut detail_content = None;
        let detail_min = arrangement::strips::min_height() + DETAIL_CHROME;
        // Never so tall that the arrangement loses its rulers and a track.
        let detail_max = ui.available_rect_before_wrap().height() - ARRANGEMENT_MIN;
        panels::sliding(
            ui,
            "detail",
            (
                panels::Side::Bottom,
                egui::Rangef::new(detail_min, detail_max),
            ),
            self.show_detail,
            &mut detail_size,
            |ui| {
                let rect = ui.max_rect();
                self.panes.anchor(ui, app::panes::Pane::Detail, rect);
                // Tabs as words over a sliding underline (the interface
                // audit's T4); Devices and Clip arrive with their milestones.
                let devices = arrives("Milestone 6");
                let clip = arrives("Milestone 5");
                let tabs = [
                    widgets::Tab {
                        label: "Devices",
                        disabled_because: Some(&devices),
                        tip: "",
                    },
                    widgets::Tab {
                        label: "Clip",
                        disabled_because: Some(&clip),
                        tip: "",
                    },
                    widgets::Tab {
                        label: "Mixer",
                        disabled_because: None,
                        tip: "Every track's channel strip",
                    },
                ];
                widgets::tabs(ui, egui::Id::new("detail tabs"), 2, &tabs);
                // Claimed without interaction: egui sizes the panel by what
                // its contents take, so an unclaimed area snapped it back to
                // its smallest after every change.
                let content = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(content);
                detail_content = Some(content);
            },
        );
        self.detail_size = detail_size;
        egui::CentralPanel::default().show(ui, |ui| self.arrangement(ui));
        if let Some(content) = detail_content {
            self.lend_cue_controls();
            let mut mixer = ui.new_child(egui::UiBuilder::new().max_rect(content));
            mixer.set_clip_rect(content.intersect(ui.clip_rect()));
            self.arrangement
                .mixer_panel(&mut mixer, &mut self.document, &self.keymap);
            self.take_cue_controls();
        }
        // Windows that closed fade out (M4.5).
        widgets::paint_gone(&ctx);
        self.typing = ctx.text_edit_focused();

        // Scheduled after drawing, because buttons are clicked while drawing.
        // Every frame while playing or settling after a command; otherwise
        // once a second for device events, so idle CPU stays near zero.
        // Meters draw until they come to rest (risk R5).
        if self.transport.animating() || meters_moving {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        self.test_modes_after(&ctx);
    }
}

/// The detail panel's tab row and margins, above and around its content.
const DETAIL_CHROME: f32 = 48.0;
/// The arrangement's least height beside the detail panel: its rulers, a
/// folded track and the master's row.
const ARRANGEMENT_MIN: f32 = 180.0;

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
                document::DropInto::NewTracks {
                    before: None,
                    group: None,
                },
                transport.state().sample_rate,
                &cc.egui_ctx,
                &mut document,
            );
        }
        let saved_appearance = saved.appearance.clone();
        let keymap = commands::Keymap::from_saved(&saved.commands.shortcuts);
        let palette = app::palette::Palette::with_recent(&saved.commands.recent);
        let midi = midi::Midi::start(launch.midi, &saved.midi);
        let mut app = App {
            transport,
            midi,
            midi_out: None,
            live_keys: None,
            midi_ports_seen: 0,
            keyboard: Default::default(),
            midi_checked: None,
            lanes,
            drop_target: Ticks::ZERO,
            drop_into: document::DropInto::NewTracks {
                before: None,
                group: None,
            },
            drag_pointer: None,
            system_cursor: None,
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
            dialog_answers: launch.scripted_dialogs.then(Default::default),
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
            keymap,
            palette,
            shortcuts: Default::default(),
            rebind: None,
            panes: Default::default(),
            nav: app::navigate::Nav::default(),
            grid: lumen_core::grid::Grid::default(),
            locators: app::locators::Locators::default(),
            hint: None,
            info: None,
            info_shown: None,
            canvas: egui::Rect::NOTHING,
            clipboard: None,
            typing: false,
            clipboard_keys: Default::default(),
            meters_at: None,
            mark: widgets::mark::Look::REST,
            menu_for: app::editing::MenuFor::default(),
            clip_props: app::clip_card::ClipProps::default(),
            smoke_test_frames: launch.smoke_test.then_some(app::test_modes::SMOKE_FRAMES),
            screenshot: launch.screenshot,
            started,
            autoplay: launch.autoplay,
            autoplay_count: 1,
            test_latency: launch.test_latency,
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
            .with_min_inner_size([960.0, 540.0])
            .with_icon(brand::window_icon()),
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
    let pipelines = canvas::PipelineCache::prefetch();
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(move |cc| {
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                canvas::install(state, pipelines).map_err(|e| format!("renderer: {e}"))?;
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
