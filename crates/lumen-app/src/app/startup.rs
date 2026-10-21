//! Start-up: `App::new` builds the app from a `Launch`.

use crate::*;

impl App {
    /// Starts Lumen as `launch` says: from the command line, or a test.
    pub fn new(cc: &eframe::CreationContext<'_>, mut launch: Launch) -> Self {
        let Launch {
            started,
            settings: saved,
            settings_warning,
            settings_path,
            ..
        } = launch;
        let mut document = document::Document::new(if launch.stress {
            let mut project = lumen_project::testing::synthetic(42, 200, 100, 10, 0);
            bench::add_dense_midi(&mut project);
            if launch.bench_lanes {
                bench::add_lanes(&mut project);
            }
            project
        } else {
            empty_project()
        });
        let mut arrangement = arrangement::Arrangement::default();
        if launch.bench_lanes {
            for track in &document.project().tracks {
                arrangement
                    .auto
                    .shown
                    .insert(track.id, lumen_project::automation::Target::Volume);
            }
        }
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
        let midi = {
            let ctx = cc.egui_ctx.clone();
            midi::Midi::start(
                launch.midi,
                &saved.midi,
                std::sync::Arc::new(move || ctx.request_repaint()),
            )
        };
        let safe = app::safe_mode::SafeMode::start(
            launch.safe_mode,
            launch.plugin_marker.clone(),
            launch.plugins.host_program().is_some(),
        );
        let places = launch.preset_places.take().unwrap_or_else(|| {
            if matches!(launch.plugins, plugins::Source::System) {
                crate::preset_files::Places::standard()
            } else {
                crate::preset_files::Places::default()
            }
        });
        let hosting = match launch.plugins.host_program() {
            Some(program) => {
                let ctx = cc.egui_ctx.clone();
                hosting::Hosting::new(program, launch.plugins.host_faults(), move || {
                    ctx.request_repaint()
                })
            }
            None => hosting::Hosting::off(),
        };
        let mut hosting = hosting;
        hosting.set_safe(safe.on);
        let plugins = plugins::Plugins::start(
            launch.plugins,
            settings_path.as_deref(),
            &saved.plugins,
            safe.on,
            &cc.egui_ctx,
        );
        let mut app = App {
            transport,
            midi,
            plugins,
            hosting,
            safe,
            chain: chain::Chain {
                presets: chain::presets::Presets::new(places),
                ..Default::default()
            },
            browser: Default::default(),
            midi_out: None,
            live_keys: None,
            record: Default::default(),
            audition: audition::Audition::default(),
            midi_ports_seen: 0,
            keyboard: Default::default(),
            midi_checked: None,
            lanes,
            drop_target: Ticks::ZERO,
            drop_into: document::Places::END,
            midi_reader: Default::default(),
            midi_exporting: None,
            midi_writing: None,
            tempo_offer: None,
            song: Default::default(),
            drag_pointer: None,
            system_cursor: None,
            settings: saved,
            settings_path,
            settings_changed: None,
            settings_writer: settings::Writer::start(),
            document,
            synced: None,
            show_history: false,
            arrangement,
            recorder: Default::default(),
            gestures_open: Default::default(),
            controllers: Default::default(),
            toast: None,
            dialog: None,
            dialog_answers: launch.scripted_dialogs.then(Default::default),
            saving: None,
            opening: None,
            prompt: None,
            after_save: None,
            save_waiting: None,
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
            detail_tab: Default::default(),
            note_editor: Default::default(),
            browser_size: 260.0,
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
            callback_bench: launch.callback_bench,
            callback_report: None,
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
