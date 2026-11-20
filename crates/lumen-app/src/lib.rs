mod app;
mod arrangement;
mod association;
mod audio;
mod audio_editor;
mod audition;
mod bake;
mod bench;
mod browser;
mod builtin;
mod callback_bench;
mod chain;
#[cfg(test)]
mod clip_flags_render;
mod commands;
mod design;
mod disk;
mod document;
mod drum_editor;
mod engine;
mod export;
mod hosting;
mod instance;
mod live;
mod live_tap;
mod measure_latency;
mod media;
mod meters;
mod midi;
mod note_editor;
mod panels;
mod piano_roll;
mod plugins;
mod pool;
mod preferences;
mod preset_files;
mod probe;
mod project_files;
mod recording;
mod settings;
mod song_lanes;
mod spawn;
mod strays;
mod system;
pub mod testing;
mod timeline;
mod transport;
mod widgets;

pub use system::launch::Launch;
pub use timeline::timeview::TimeView;

pub(crate) use bake::{bounce, consolidate, crossfades, normalize};
pub(crate) use design::{fonts, icons, light, motion, theme};
pub(crate) use engine::{lane_bake, lanes, played, routing};
pub(crate) use project_files::{autosave, files, relink};
pub(crate) use system::{brand, crash, launch, owner, trace};
pub(crate) use timeline::{canvas, playhead, position, ruler, timefmt, timeview};

pub(crate) use app::welcome;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_core::product::{PRODUCT_NAME, display_name};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature};

use app::projects::{FileDialog, Next};
use transport::Transport;

pub struct App {
    document: document::Document,

    synced: Option<u64>,

    show_history: bool,

    popped_pointer_down: bool,

    tap: app::tap_tempo::TapRun,

    arrangement: arrangement::Arrangement,

    controllers: app::controllers::Controllers,

    recorder: recording::auto_record::Recorder,

    lane_commits: app::auto_recording::Commits,

    gestures_open:
        std::collections::BTreeSet<(lumen_project::model::Id, lumen_project::automation::Target)>,

    toast: Option<(String, Instant)>,

    dialog: Option<std::sync::mpsc::Receiver<FileDialog>>,

    dialog_answers: Option<std::collections::VecDeque<Vec<PathBuf>>>,
    saving: Option<std::sync::mpsc::Receiver<Result<files::Saved, String>>>,
    opening: Option<std::sync::mpsc::Receiver<Result<files::Opened, String>>>,

    opening_mark: u64,

    checks: Option<(
        std::sync::mpsc::Sender<app::handover::Checked>,
        std::sync::mpsc::Receiver<app::handover::Checked>,
    )>,

    prompt: Option<Next>,

    prompt_enter: bool,

    after_save: Option<Next>,

    save_waiting: Option<app::copies::Waiting>,

    save_dialog: bool,

    collecting: Option<app::copies::Running>,

    templates: app::templates::Templates,

    revert: app::revert::Revert,

    drum_choice: Option<app::drum_choice::Choice>,

    drum_map: app::drum_map_window::State,

    project_settings: app::project_settings::ProjectSettings,
    export: app::audio_export::Export,

    instance: Option<instance::Instance>,

    holding: Option<app::handover::Holding>,

    association: association::Association,

    association_status: app::association::Reported,

    left_out: Option<(PathBuf, usize)>,

    file_notice: Option<String>,

    quitting: bool,

    owner: Option<owner::Owner>,

    autosave: Option<autosave::Autosave>,

    leftover: Option<autosave::Leftover>,

    leftovers: app::recovery::Leftovers,

    crash_report: Option<PathBuf>,
    crash_test: Option<crash::Test>,

    crash_dir: Option<PathBuf>,

    recovering: Option<autosave::Leftover>,
    transport: Transport,

    midi: midi::Midi,

    midi_out: Option<midi::out::Output>,

    live_keys: Option<live::LiveKeys>,

    record: app::record::Record,

    audition: audition::Audition,

    midi_ports_seen: u64,

    keyboard: app::keyboard::ComputerKeyboard,
    midi_checked: Option<(u64, u64, u64)>,

    lanes: lanes::Lanes,

    drop_target: Ticks,

    drop_into: document::Places,

    file_drop: Option<Vec<PathBuf>>,

    midi_reader: midi::files::Reader,

    browser: browser::Browser,

    media_pool: app::media_pool::MediaPool,

    plugins: plugins::Plugins,

    hosting: hosting::Hosting,

    builtin: builtin::Builtins,

    sampler_load: builtin::samples::Loading,

    safe: app::safe_mode::SafeMode,

    chain: chain::Chain,

    midi_exporting: Option<std::sync::mpsc::Receiver<Result<midi::files::Exported, String>>>,

    normalizing: Option<app::clip_normalize::Running>,
    consolidating: Option<app::clip_consolidate::Running>,

    bounce_waiting: Option<app::clip_bounce::Waiting>,
    bouncing: Option<app::clip_bounce::Running>,
    show_bounce: bool,

    freeze_waiting: Option<app::track_freeze::Waiting>,
    freezing: Option<app::track_freeze::Running>,

    midi_writing: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,

    tempo_offer: Option<app::midi_files::TempoOffer>,

    song: song_lanes::SongLanes,

    drag_pointer: Option<egui::Pos2>,

    system_cursor: Option<egui::Pos2>,
    settings: settings::Settings,
    settings_path: Option<PathBuf>,

    user_dir: Option<PathBuf>,

    settings_changed: Option<Instant>,

    disk_scene: Option<app::streaming::SceneKey>,

    slow_disk: Option<std::time::Duration>,

    cache: app::cache_folder::CacheFolder,

    settings_writer: settings::Writer,
    show_browser: bool,
    show_detail: bool,

    detail_tab: app::detail::DetailTab,

    detail: app::detail::DetailState,

    note_editor: note_editor::NoteEditor,

    drum_sounding: std::cell::RefCell<app::drum_routing::Sounding>,

    audio_editor: audio_editor::AudioEditor,

    follow_jobs: app::follow_detect::Jobs,

    browser_size: f32,
    detail_size: f32,

    toggle_detail_requested: bool,

    keymap: commands::Keymap,

    palette: app::palette::Palette,

    shortcuts: app::shortcuts::Shortcuts,

    preferences: preferences::Window,

    welcome: welcome::Welcome,

    rebind: Option<app::shortcuts::Rebind>,

    panes: app::panes::Panes,

    nav: app::navigate::Nav,

    grid: lumen_core::grid::Grid,

    locators: app::locators::Locators,

    hint: Option<String>,

    info: Option<app::info::Info>,
    info_shown: Option<app::info::Info>,

    canvas: egui::Rect,

    clipboard: Option<lumen_project::edit::Clipboard>,

    typing: bool,

    clipboard_keys: app::clipboard_keys::Pending,

    meters_at: Option<f64>,

    inputs: app::audio_inputs::InputSync,

    mark: widgets::mark::Look,

    menu_for: app::editing::MenuFor,

    clip_props: app::clip_card::ClipProps,
    smoke_test_frames: Option<u32>,

    quit_at: Option<Instant>,

    screenshot: Option<PathBuf>,
    started: Instant,

    autoplay: Option<Duration>,
    autoplay_count: u32,

    test_latency: Option<f64>,

    save_as: Option<PathBuf>,

    traced: Option<bool>,

    appearance_override: bool,
    saved_appearance: settings::Appearance,

    min_window: Option<egui::Vec2>,

    min_window_monitor: Option<egui::Vec2>,

    min_window_ppp: Option<f32>,

    system_theme: Option<egui::Theme>,

    banner: Option<(String, bool)>,

    bench: Option<bench::Bench>,

    callback_bench: Option<callback_bench::Callback>,
    callback_report: Option<Result<callback_bench::Report, String>>,
}

pub(crate) fn empty_project() -> Project {
    Project {
        name: "Untitled".into(),
        sample_rate: 48_000,
        auto_crossfade: true,
        tempo: vec![TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }],
        meter: lumen_project::model::constant_meter(TimeSignature::FOUR_FOUR),
        tracks: Vec::new(),

        master: std::sync::Arc::new(lumen_project::model::Track::master(
            lumen_project::model::Id(0),
        )),
        pan_law: Default::default(),
        media: Vec::new(),
        markers: Vec::new(),
        transport: Default::default(),
        view: Default::default(),
        mappings: Vec::new(),
        notes: String::new(),
        new_track_color: None,
        drum_maps: Vec::new(),
    }
}

pub(crate) fn empty_project_at(device_rate: Option<u32>) -> Project {
    let mut project = empty_project();
    if let Some(rate) = device_rate.filter(|r| (8_000..=384_000).contains(r)) {
        project.sample_rate = rate;
    }
    project
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        if self.settings_changed.is_some() {
            self.write_settings();
        }
        if let Some(autosave) = &mut self.autosave {
            autosave.clear();
        }

        self.close_take_notes();
        self.safe.closing();
        self.hosting.shutdown();
    }

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

            false
        });
        self.toggle_detail_requested |= toggle;
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.owner.is_none() {
            self.owner = owner::Owner::of(frame);
        }
        let ctx = ui.ctx().clone();

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
        self.refresh_audio_editor();
        self.refresh_detail(&ctx);
        self.refresh_chain(&ctx);
        self.take_prompt_enter(&ctx);
        self.audio_handle_keys(&ctx);
        self.steps_handle_keys(&ctx);
        self.command_keys(&ctx);
        if std::mem::take(&mut self.arrangement.import_requested) {
            self.run(&ctx, commands::Command::Import);
        }
        if std::mem::take(&mut self.arrangement.bounce_requested) {
            self.run(&ctx, commands::Command::Bounce);
        }
        if let Some(command) = self.arrangement.track_command.take() {
            self.run(&ctx, command);
        }
        self.poll_files(&ctx);
        self.poll_cache(&ctx);
        self.poll_collecting(&ctx);
        self.poll_templates(&ctx);
        self.poll_leftovers(&ctx);
        self.poll_association(&ctx);
        self.autosave(&ctx);
        let rate = self.transport.state().sample_rate;
        let mut dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        dropped.extend(self.file_drop.take().unwrap_or_default());
        if !dropped.is_empty() {
            trace::log(|| {
                format!(
                    "drop: {} file(s) at {:?} into {:?}",
                    dropped.len(),
                    self.drop_target,
                    self.drop_into
                )
            });

            if let Some(project) = dropped.iter().position(|p| files::is_project(p)) {
                let left = dropped.len() - 1;
                let project = dropped.swap_remove(project);
                self.left_out = (left > 0).then(|| (project.clone(), left));
                if left > 0 {
                    self.say(app::handover::left_out_words(left));
                }
                self.open_handed(&ctx, project);
            } else {
                self.add_files(dropped, self.drop_target, self.drop_into, &ctx);
            }
        }
        self.poll_instance(&ctx);
        self.release_holding(&ctx);
        self.poll_midi_files(&ctx);
        self.poll_normalize(&ctx);
        self.poll_consolidate(&ctx);
        self.poll_bounce(&ctx);
        self.poll_audio_export(&ctx);
        self.poll_freeze(&ctx);
        self.poll_media_pool(&ctx);
        self.poll_sampler(&ctx);
        self.poll_browser_files(&ctx);
        self.say_frozen_refusals();
        self.plugins.poll(&self.settings.plugins, &ctx);
        if let Some(wait) = self.plugins.rebuild_wait() {
            ctx.request_repaint_after(wait);
        }
        let block = self
            .transport
            .audio
            .info
            .as_ref()
            .map_or(0, |info| info.buffer_frames);

        self.arrangement.auto.at = self.transport.position();
        self.document
            .set_lock_envelopes(self.settings.editing.lock_envelopes);
        self.document
            .set_new_tracks(self.settings.editing.new_tracks.for_document());
        self.document
            .set_follow_tempo(self.settings.editing.follow_tempo);
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.set_interval(self.settings.editing.autosave());
        }
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

        if self
            .builtin
            .update(self.document.project(), &mut self.transport.audio.mixer)
        {
            self.lanes.devices.builtin = self.builtin.latencies().clone();
        }
        if let Some(note) = self.builtin.take_note() {
            self.say(note);
        }
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

        self.update_streaming();
        let cue = self.cue();
        let inputs = self.listen_inputs();
        let heard = self.transport.audio.engine.transport_reader().read();
        self.lanes
            .set_playhead(heard.playing.then_some(heard.sample));
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
        self.drive_follow_detection(&ctx);
        self.sync_document(&ctx);
        let meters_moving = self.update_meters(&ctx);
        let inputs_watched = self.update_inputs(&ctx);
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

        if self.window_minimum_stale(&ctx) {
            self.apply_window_minimum(&ctx);
        }
        let panels_before = (self.show_browser, self.show_detail);
        if std::mem::take(&mut self.toggle_detail_requested) {
            self.show_detail = !self.show_detail;
        }

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
                    bar.x_range().expand(theme::space::M),
                    bar.top() + 0.5 * theme::stroke::HAIRLINE,
                    egui::Stroke::new(theme::stroke::HAIRLINE, edge_light),
                );
                self.transport(ui);
            });
        self.device_banner(ui);
        self.crash_banner(ui);
        self.file_banner(ui);
        self.missing_media_banner(ui);
        self.revert_banner(ui);
        self.holding_banner(ui);
        self.tempo_banner(ui);
        self.recovery_banner(ui);
        self.safe_banner(ui);
        self.preferences_window(&ctx);
        self.welcome_window(&ctx);
        if self.midi.poll() {
            self.midi_ports_seen += 1;
            ctx.request_repaint();
        }
        if let Some(notice) = self.midi.notices.pop() {
            self.transport.audio.notice = Some(notice);
        }
        self.update_midi_hardware(&ctx);
        self.update_recording(&ctx);
        self.update_audition(&ctx);
        self.history_window(&ctx);
        self.bounce_window(&ctx);
        self.mappings_window(&ctx);
        self.drum_choice_window(&ctx);
        self.drum_map_window(&ctx);
        self.parameters_panel(&ctx);
        self.lane_parameters(&ctx);
        self.prompt_window(&ctx);
        self.revert_window(&ctx);
        self.save_template_window(&ctx);
        self.project_settings_window(&ctx);
        self.audio_export_window(&ctx);
        self.shortcuts_window(&ctx);
        self.palette_window(&ctx);

        let hid_a_panel =
            (panels_before.0 && !self.show_browser) || (panels_before.1 && !self.show_detail);
        if hid_a_panel {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }

        self.draw_info_line(ui);

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

        if std::mem::take(&mut self.chain.grow)
            && let Some(height) = self.chain.height
        {
            self.detail_size = self.detail_size.max(height);
        }
        let mut detail_size = self.detail_size;
        let mut detail_content = None;

        let devices = self.detail_tab == app::detail::DetailTab::Devices;
        let detail_min = if devices {
            self.chain.least_panel()
        } else {
            arrangement::strips::min_height() + DETAIL_CHROME
        };

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

                self.detail_tabs(ui);

                let content = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(content);
                detail_content = Some(content);
            },
        );
        self.detail_size = detail_size;
        if devices && self.show_detail {
            self.chain.height = Some(detail_size);
        }
        egui::CentralPanel::default().show(ui, |ui| self.arrangement(ui));
        if let Some(content) = detail_content {
            self.detail_content(ui, content);
        }
        self.note_editor_window(&ctx);
        self.audio_editor_window(&ctx);

        widgets::paint_gone(&ctx);
        self.typing = ctx.text_edit_focused();

        self.update_controllers(&ctx);
        self.answer_map_requests();
        self.keep_learning_note(&ctx);
        self.sync_live_notes_as_played();
        self.record_automation(&ctx, rate);
        if self.transport.animating() || meters_moving {
            ctx.request_repaint();
        } else if inputs_watched {
            ctx.request_repaint_after(app::audio_inputs::WATCH_EVERY);
        } else if self.transport.audio.waiting() {
            ctx.request_repaint_after(crate::motion::WAIT_FRAME);
        } else {
            ctx.request_repaint_after(crate::motion::IDLE_TICK);
        }

        self.test_modes_after(&ctx);
    }
}

pub(crate) const BROWSER_MIN: f32 = 220.0;

pub(crate) const DETAIL_CHROME: f32 = 48.0;

const ARRANGEMENT_MIN: f32 = 180.0;

pub fn run() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(clsid) = args
        .iter()
        .position(|a| a == probe::FLAG)
        .and_then(|i| args.get(i + 1))
    {
        std::process::exit(probe::run_helper(clsid));
    }

    let instance = instance::start(&args);
    let mut launch = Launch::from_args();
    launch.instance = instance;
    if let Some(dir) = &launch.crash_dir {
        crash::install(dir.clone());
    }
    let mut options = eframe::NativeOptions {
        run_and_return: !cfg!(target_os = "macos"),
        viewport: egui::ViewportBuilder::default()
            .with_title(display_name())
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0])
            .with_icon(brand::window_icon()),
        ..Default::default()
    };

    if launch.bench.is_some() {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
        if let Some((w, h)) = launch.bench_size {
            options.viewport = options
                .viewport
                .with_inner_size([w, h])
                .with_position([0.0, 0.0]);
        }
    }

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

#[cfg(test)]
lumen_rt::install_guarded_allocator!();
