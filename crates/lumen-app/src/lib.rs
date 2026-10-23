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
//! lumen --bench-callback out.json [--bench-seconds 30] song.lumen
//!                                the callback benchmark: plays the project
//!                                and reports the audio callback's time
//!                                against the block (callback_bench.rs)
//! lumen --stress --bench-frames out.json
//!                                frame-time benchmark: scroll and zoom it with
//!                                vsync off, report, exit (status 3 below 60 fps);
//!                                --bench-size 3440x1400 sets the window;
//!                                --bench-editor opens the 100,000-note clip
//!                                in the clip editor while it runs;
//!                                --bench-lanes shows a 200-point volume lane
//!                                over every track
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
mod audition;
mod auto_record;
mod autosave;
mod bench;
mod brand;
mod browser;
mod callback_bench;
mod canvas;
mod chain;
#[cfg(test)]
mod clip_flags_render;
mod commands;
mod crash;
mod document;
mod files;
mod fonts;
mod hosting;
mod icons;
mod lane_bake;
mod lanes;
mod launch;
mod light;
mod live;
mod live_tap;
mod media;
mod meters;
mod midi;
mod midi_export;
mod midi_files;
mod midi_out;
mod midi_play;
mod midi_runs;
mod midi_settings;
mod motion;
mod normalize;
mod note_editor;
mod owner;
mod panels;
mod piano_roll;
mod playhead;
mod plugins;
mod position;
mod preset_files;
mod probe;
mod record_settings;
mod recorder;
mod routing;
mod ruler;
mod settings;
mod song_lanes;
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
    /// MIDI learn and the controllers that move controls (M7 slice A5).
    controllers: app::controllers::Controllers,
    /// The lane passes being recorded (M7 slice A4b).
    recorder: auto_record::Recorder,
    /// The passes written into the project in parts as they go (M7 review M1).
    lane_commits: app::auto_recording::Commits,
    /// The plugin gestures that are open: a control held in a plugin's own
    /// window, by track and parameter (A4b).
    gestures_open:
        std::collections::BTreeSet<(lumen_project::model::Id, lumen_project::automation::Target)>,
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
    /// A save waiting for the running plugins' states to be read: where to,
    /// whether to normalize the path, and since when.
    save_waiting: Option<(std::path::PathBuf, bool, std::time::Instant)>,
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
    /// Recording MIDI (M5 slice 6).
    record: app::record::Record,
    /// The piano roll's note preview (M5 slice 3d).
    audition: audition::Audition,
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
    /// (slice 6e), for audio files and for MIDI files (M5 slice 7).
    drop_into: document::Places,
    /// MIDI files being read in the background (M5 slice 7).
    midi_reader: midi_files::Reader,
    /// The browser's own state (M6 slice 1).
    browser: browser::Browser,
    /// Installed plugins: the scan thread and the catalog the browser
    /// shows (M6 slice 1).
    plugins: plugins::Plugins,
    /// The host processes of the project's plugin devices (M6 slice 2c).
    hosting: hosting::Hosting,
    /// Safe mode (M6 slice 5e): no plugin is loaded until it is left.
    safe: app::safe_mode::SafeMode,
    /// The Devices tab's own state (M6 slice 2d).
    chain: chain::Chain,
    /// An exported MIDI file being made, before the dialog asks where it
    /// goes (M5 slice 7).
    midi_exporting: Option<std::sync::mpsc::Receiver<Result<midi_files::Exported, String>>>,
    /// The peak scan behind Normalize, while it runs (M7b slice E1).
    normalizing: Option<app::clip_normalize::Running>,
    /// An exported MIDI file being written (M5 slice 7).
    midi_writing: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    /// A MIDI file's tempo and meter, offered in a banner (M5 slice 7, F2).
    tempo_offer: Option<app::midi_files::TempoOffer>,
    /// The tempo and meter lanes under the bar ruler (M7 slice A6).
    song: song_lanes::SongLanes,
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
    /// Which tab of the detail panel shows (M5 slice 3c).
    detail_tab: app::detail::DetailTab,
    /// The MIDI note editor: in the Clip tab, or popped out (slice 3c).
    note_editor: note_editor::NoteEditor,
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
    /// `--bench-callback`: the callback benchmark in progress, and its report
    /// once it has finished (M6 slice 7c).
    callback_bench: Option<callback_bench::Callback>,
    callback_report: Option<Result<callback_bench::Report, String>>,
}

pub(crate) fn empty_project() -> Project {
    Project {
        name: "Untitled".into(),
        sample_rate: 48_000,
        tempo: vec![TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }],
        meter: lumen_project::model::constant_meter(TimeSignature::FOUR_FOUR),
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
        mappings: Vec::new(),
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
        self.safe.closing();
        self.hosting.shutdown();
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
        self.drive_callback_bench(&ctx);
        self.hint = None;
        self.transport.update();
        self.restore_clipboard_keys(&ctx);
        self.capture_chord(&ctx);
        self.computer_keyboard_keys(&ctx);
        self.refresh_note_editor(&ctx);
        self.refresh_chain(&ctx);
        self.command_keys(&ctx);
        if std::mem::take(&mut self.arrangement.import_requested) {
            self.run(&ctx, commands::Command::Import);
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
            self.add_files(dropped, self.drop_target, self.drop_into, &ctx);
        }
        self.poll_midi_files(&ctx);
        self.poll_normalize(&ctx);
        self.plugins.poll();
        if let Some(wait) = self.plugins.rebuild_wait() {
            ctx.request_repaint_after(wait);
        }
        let block = self
            .transport
            .audio
            .info
            .as_ref()
            .map_or(0, |info| info.buffer_frames);
        // The controls whose lanes play follow the playhead (M7 slice A4a).
        self.arrangement.auto.at = self.transport.position();
        self.document
            .set_lock_envelopes(self.settings.editing.lock_envelopes);
        self.hosting.set_overridden(self.document.overrides());
        if self.lanes.devices.overridden != *self.document.overrides() {
            self.lanes.devices.overridden = self.document.overrides().clone();
        }
        let hosts = self.hosting.update(
            self.document.project(),
            self.plugins.catalog(),
            (rate, block),
            &mut self.transport.audio.mixer,
            &mut self.lanes.devices.hosted,
        );
        self.watch_safe_mode();
        self.arrangement.output_notes = chain::outputs::notes_of(self);
        self.lanes.midi_changed(hosts.midi);
        if let Some(ms) = hosts.again_in_ms {
            ctx.request_repaint_after(std::time::Duration::from_millis(ms));
        }
        self.plugin_window_keys();
        self.apply_plugin_edits(&ctx);
        self.take_budget();
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
        let inputs = self.listen_inputs();
        self.lanes.update(
            rate,
            &ctx,
            &mut self.document,
            lanes::Handles {
                clips: &mut self.transport.audio.clips,
                midi: &mut self.transport.audio.midi,
                mixer: &mut self.transport.audio.mixer,
                hosting: &mut self.hosting,
            },
            &cue,
            &inputs,
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

        // The transport bar (the Transport card, ADR-0052): 44 pt on `app`
        // with a 1 pt `edge_light` along its top inside edge; the panel's
        // separator line is the `border` hairline under it.
        let bar_fill = theme::p().app;
        let edge_light = theme::edge_light();
        egui::Panel::top("transport")
            .exact_size(theme::size::TRANSPORT_BAR)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(bar_fill)
                    .inner_margin(egui::Margin::symmetric(theme::space::M as i8, 0)),
            )
            .show(ui, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(
                    bar.x_range(),
                    bar.top() + 0.5 * theme::stroke::HAIRLINE,
                    egui::Stroke::new(theme::stroke::HAIRLINE, edge_light),
                );
                self.transport(ui);
            });
        self.device_banner(ui);
        self.crash_banner(ui);
        self.file_banner(ui);
        self.tempo_banner(ui);
        self.recovery_banner(ui);
        self.safe_banner(ui);
        let shown = midi_settings::Shown {
            midi: &self.midi,
            saved: &self.settings.midi,
        };
        for action in self.transport.windows(&ctx, shown) {
            self.midi.apply(&action);
            match &action {
                midi::Action::Use(id, using) => {
                    midi::Midi::remember(&mut self.settings.midi, id, *using);
                    self.settings_changed = Some(Instant::now());
                }
                midi::Action::SyncDelay(name, ms) => {
                    midi::Midi::set_sync_delay(&mut self.settings.midi, name, *ms);
                    self.settings_changed = Some(Instant::now());
                }
                midi::Action::Rescan => {}
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
        self.update_recording();
        self.update_audition(&ctx);
        self.history_window(&ctx);
        self.mappings_window(&ctx);
        self.parameters_panel(&ctx);
        self.lane_parameters(&ctx);
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
        // Panels slide open and closed (motion: GLIDE, panels.rs). The size
        // is taken out while the browser draws, which borrows the whole app.
        let mut browser_size = self.browser_size;
        panels::sliding(
            ui,
            "browser",
            (
                panels::Side::Left,
                egui::Rangef::new(BROWSER_MIN, f32::INFINITY),
            ),
            self.show_browser,
            &mut browser_size,
            |ui| {
                self.browser_panel(ui);
            },
        );
        self.browser_size = browser_size;
        // The detail panel: the Mixer tab (M4 slice 5); Devices and Clip
        // arrive with their milestones. Never shorter than a strip. Its
        // content is drawn after the arrangement, so Tab reaches it in
        // reading order.
        if std::mem::take(&mut self.chain.grow) {
            self.detail_size = self.detail_size.max(chain::panel_height());
        }
        let mut detail_size = self.detail_size;
        let mut detail_content = None;
        // The Devices tab is never so short that its card leaves the tray.
        let detail_min = if self.detail_tab == app::detail::DetailTab::Devices {
            chain::panel_min_height_keyed(self.shown_chain_keyed())
        } else {
            arrangement::strips::min_height() + DETAIL_CHROME
        };
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
                // audit's T4); Devices arrive with Milestone 6.
                self.detail_tabs(ui);
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
            self.detail_content(ui, content);
        }
        self.note_editor_window(&ctx);
        // Windows that closed fade out (M4.5).
        widgets::paint_gone(&ctx);
        self.typing = ctx.text_edit_focused();

        // Scheduled after drawing, because buttons are clicked while drawing.
        // Every frame while playing or settling after a command; otherwise
        // once a second for device events, so idle CPU stays near zero.
        // Meters draw until they come to rest (risk R5).
        // After the controls were drawn: what moved in them is this frame's.
        self.update_controllers(&ctx);
        self.answer_map_requests();
        self.keep_learning_note(&ctx);
        self.record_automation(&ctx, rate);
        if self.transport.animating() || meters_moving {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        self.test_modes_after(&ctx);
    }
}

/// The browser's least width: its list shows names and the filter chips
/// whole (narrower, only icons were left).
pub(crate) const BROWSER_MIN: f32 = 220.0;
/// The detail panel's tab row and margins, above and around its content.
pub(crate) const DETAIL_CHROME: f32 = 48.0;
/// The arrangement's least height beside the detail panel: its rulers, a
/// folded track and the master's row.
const ARRANGEMENT_MIN: f32 = 180.0;

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
        // Use eframe's terminating desktop loop on macOS. Returning through
        // another AppKit display cycle after window/accessibility teardown
        // triggers a Touch Bar KVO exception on affected systems (ADR-0085).
        // eframe saves, calls on_exit and drops App before ending the process.
        run_and_return: !cfg!(target_os = "macos"),
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

// The unit tests that check the audio thread's code count heap use in it.
#[cfg(test)]
lumen_rt::install_guarded_allocator!();
