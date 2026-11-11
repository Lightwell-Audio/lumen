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
            user_dir,
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
            // The rate chosen for the audio device last time, when there is one.
            empty_project_at(saved.audio.as_ref().and_then(|a| a.sample_rate))
        });
        document.set_undo_limit(saved.editing.undo_steps);
        document.set_new_tracks(saved.editing.new_tracks.for_document());
        document.set_follow_tempo(saved.editing.follow_tempo);
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
        if let Some(warning) = settings_warning {
            transport.audio.notice = Some(match transport.audio.notice.take() {
                Some(notice) => format!("{warning}\n{notice}"),
                None => warning,
            });
        }
        let mut lanes = lanes::Lanes::default();
        // `--stress`: every file of the generated project plays the same
        // generated audio (one copy in memory, one set of peaks), made again
        // if the device opens at another rate.
        if launch.stress {
            let rate = transport.state().sample_rate;
            let files: Vec<(u64, &str)> = document
                .project()
                .media
                .iter()
                .map(|media| (media.id.0, media.path.as_str()))
                .collect();
            lanes
                .library
                .insert_generated(&files, rate, bench::stress_audio);
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
        // The first-run window (M9 R7b): on a first start, never in a run
        // that drives itself (tests, screenshots, benchmarks), unless asked.
        let welcome = {
            let automated = (launch.smoke_test
                || launch.stress
                || launch.bench.is_some()
                || launch.callback_bench.is_some()
                || launch.save_as.is_some()
                || launch.screenshot.is_some()
                || launch.scripted_dialogs)
                && !launch.first_start_rule;
            let mut welcome = crate::welcome::Welcome::default();
            if crate::welcome::opens_at_start(launch.first_run, saved.setup_done, automated) {
                welcome.show();
            }
            welcome
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
        // Real folders are shown in the file manager; tests only note them.
        let templates =
            app::templates::Templates::new(matches!(launch.plugins, plugins::Source::System));
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
            browser: browser::Browser {
                tab: saved.browser.tab.into(),
                ..Default::default()
            },
            media_pool: Default::default(),
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
            file_drop: None,
            midi_reader: Default::default(),
            midi_exporting: None,
            midi_writing: None,
            normalizing: None,
            consolidating: None,
            bounce_waiting: None,
            bouncing: None,
            show_bounce: false,
            export: Default::default(),
            instance: launch.instance,
            holding: None,
            association: crate::association::Association::new(launch.association),
            left_out: None,
            freeze_waiting: None,
            freezing: None,
            tempo_offer: None,
            song: Default::default(),
            drag_pointer: None,
            system_cursor: None,
            settings: saved,
            settings_path,
            user_dir,
            settings_changed: None,
            disk_scene: None,
            slow_disk: launch.slow_disk,
            cache: Default::default(),
            settings_writer: settings::Writer::start(),
            document,
            synced: None,
            show_history: false,
            popped_pointer_down: false,
            tap: Default::default(),
            arrangement,
            recorder: Default::default(),
            lane_commits: Default::default(),
            gestures_open: Default::default(),
            controllers: Default::default(),
            toast: None,
            dialog: None,
            dialog_answers: launch.scripted_dialogs.then(Default::default),
            saving: None,
            opening: None,
            opening_mark: 0,
            checks: None,
            prompt: None,
            prompt_enter: false,
            after_save: None,
            save_waiting: None,
            save_dialog: false,
            collecting: None,
            templates,
            revert: Default::default(),
            project_settings: Default::default(),
            file_notice: None,
            quitting: false,
            owner: None,
            autosave: None,
            leftover: None,
            crash_report: launch.crash_dir.as_deref().and_then(crash::unseen),
            crash_test: launch.crash_test,
            crash_dir: launch.crash_dir.clone(),
            recovering: None,
            show_browser: true,
            show_detail: true,
            detail_tab: Default::default(),
            detail: Default::default(),
            note_editor: Default::default(),
            audio_editor: Default::default(),
            follow_jobs: Default::default(),
            browser_size: 260.0,
            detail_size: 220.0,
            toggle_detail_requested: false,
            keymap,
            palette,
            shortcuts: Default::default(),
            preferences: {
                let mut window = crate::preferences::Window::default();
                if launch.audio_settings {
                    window.show_page(crate::preferences::Page::Audio);
                }
                window
            },
            welcome,
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
            inputs: Default::default(),
            mark: widgets::mark::Look::REST,
            menu_for: app::editing::MenuFor::default(),
            clip_props: app::clip_card::ClipProps::default(),
            smoke_test_frames: launch.smoke_test.then_some(app::test_modes::SMOKE_FRAMES),
            quit_at: launch.quit_after.map(|after| launch.started + after),
            screenshot: launch.screenshot,
            started,
            autoplay: launch.autoplay,
            autoplay_count: 1,
            test_latency: launch.test_latency,
            save_as: launch.save_as,
            traced: None,
            appearance_override: false,
            saved_appearance: saved_appearance.clone(),
            min_window: None,
            min_window_monitor: None,
            min_window_ppp: None,
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
        app.note_editor.quantize.options =
            crate::note_editor::quantize::Options::from_saved(&app.settings.quantize);
        fonts::install(&cc.egui_ctx);
        app.apply_appearance(&cc.egui_ctx);
        // Requests from later starts wake the window from now on.
        if let Some(instance) = &app.instance {
            instance.attach(&cc.egui_ctx);
        }
        let mut autosave = autosave::Autosave::new(launch.autosave_dir);
        autosave.set_interval(app.settings.editing.autosave());
        if let Some(delay) = launch.autosave_after {
            autosave = autosave.with_delay(delay);
        }
        app.autosave = Some(autosave);
        // Settled for the run: a folder chosen later is used from the next
        // start.
        app.cache.dir = app.chosen_cache_dir();
        app.leftover = app.next_leftover();
        app.restore_place_folder();
        if launch.recover_latest
            && let Some(leftover) = app.leftover.take()
        {
            app.begin_opening(leftover.file.clone());
            app.recovering = Some(leftover);
        }
        // Projects open from File Explorer by default (M9 R8, ADR-0117).
        if app.association.is_system() {
            app.associate_at_start();
        }
        // `lumen <file>.lumen` (and opening a project from Explorer).
        if let Some(path) = launch.open {
            app.begin_opening(path);
        }
        app
    }
}
