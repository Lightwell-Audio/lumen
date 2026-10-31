//! The first-run window (M9 slice R7b, ADR-0115; the owner's F A, S A, T A,
//! D A, P A, M A and W A): a `Dialog` over the empty song with three steps,
//! Sound, Plugins and Ready. The song behind stays usable; ✕, Esc and Skip
//! setup close it and keep what was set. It opens on a first start until it
//! is finished or closed, and from "Set up Lumen…". Its state is
//! `welcome.rs`; the device and folder rows are the Preferences pages' own.

use eframe::egui::{self, Align, Key, Layout, RichText, Ui, pos2, vec2};
use lumen_core::product::PRODUCT_NAME;
use lumen_engine::chime::{self, Sounding};

use crate::audio::{DEFAULT_DRIVER, Status, driver_label};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::plugins::Status as Scan;
use crate::preferences::{Marks, audio};
use crate::settings::{self, AudioSettings};
use crate::welcome::{self, AsioOffer, Health, Mark, Step};
use crate::widgets::{hint, row, section, warning};
use crate::{App, fonts, theme, widgets};

/// How much of a short window's height the window's top sits below the
/// app's (at most `size::PREFERENCES_TOP`), as Preferences does.
const TOP_SHARE: f32 = 0.12;
/// The Plugins step when this run was started without plugins.
const OFF_THIS_RUN: &str = "This run started with plugins off. Start again normally to load them.";
/// How strong a lit light's ring is at silence; it reaches full at the
/// chime's peak.
const RING_LEAST: f32 = 0.38;
/// What the footer's hint says.
const KEYS_HINT: &str = "Tab moves between controls \u{b7} Esc skips";

/// What the footer asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Footer {
    Skip,
    Back,
    Next,
    Finish,
}

/// Which of Lumi's poses a step shows.
#[derive(Debug, Clone, Copy)]
enum Pose {
    Waving,
    Celebrating,
    Curious,
}

impl App {
    /// Each frame: draws the window while it is open.
    pub(crate) fn welcome_window(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        // Esc goes to an open menu, a field or another window first; only
        // then does it skip setup.
        let escape = ctx.input(|i| i.key_pressed(Key::Escape)) && !self.welcome.esc_for_child;
        let content = ctx.content_rect();
        let margins = ctx.global_style().spacing.window_margin.sum().x + 2.0 * theme::space::M;
        let width =
            (content.width() - margins).clamp(theme::size::PREFERENCES_LEAST, theme::size::WELCOME);
        let top = content.top() + theme::size::PREFERENCES_TOP.min(content.height() * TOP_SHARE);
        let title = format!("Welcome to {PRODUCT_NAME}");
        let step = self.welcome.step;
        let marks = Step::ALL.map(|s| self.step_mark(s));
        let mut picked = None;
        let mut footer = None;
        let closed = widgets::Dialog::new("welcome", &title, width)
            .at(pos2(content.center().x, top), egui::Align2::CENTER_TOP)
            .esc_to_caller()
            .head(|ui| step_pills(ui, step, marks, &mut picked))
            .show(ctx, |ui| {
                let room = content.bottom() - ui.cursor().top() - theme::size::PREFERENCES_BELOW;
                footer = self.welcome_body(ui, room);
            });
        if let Some(step) = picked {
            self.welcome.go(step);
        }
        match footer {
            Some(Footer::Back) => {
                if let Some(back) = self.welcome.step.back() {
                    self.welcome.go(back);
                }
            }
            Some(Footer::Next) => {
                if let Some(next) = self.welcome.step.next() {
                    self.welcome.go(next);
                }
            }
            Some(Footer::Skip | Footer::Finish) => self.finish_welcome(ctx),
            None => {}
        }
        if closed || escape {
            self.finish_welcome(ctx);
        }
        self.welcome.esc_for_child =
            egui::Popup::is_any_open(ctx) || ctx.text_edit_focused() || self.other_window_open();
    }

    /// Closes the window in any way: setup counts as done, and the settings
    /// are written at once. The keyboard goes back to the song.
    fn finish_welcome(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        self.welcome.open = false;
        self.transport.audio.stop_sound_check();
        self.settings.setup_done = true;
        self.write_settings();
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        ctx.request_repaint();
    }

    /// How the Sound step stands.
    fn sound_health(&self) -> Health {
        let audio = &self.transport.audio;
        if audio.changing().is_some() || !audio.can_change() {
            return Health::Busy;
        }
        match audio.status {
            Status::Running { .. } => Health::Good,
            Status::Starting => Health::Busy,
            Status::Lost(_) | Status::Off(_) => Health::Needs,
        }
    }

    /// How the Plugins step stands.
    fn plugins_health(&self) -> Health {
        if self.safe.on || self.plugins.is_off() {
            return Health::Needs;
        }
        match self.plugins.status() {
            Scan::Failed(_) | Scan::Off => Health::Needs,
            Scan::Loading | Scan::Scanning { .. } => Health::Busy,
            Scan::Idle if self.plugins.waiting().is_some() => Health::Busy,
            Scan::Idle => Health::Good,
        }
    }

    fn step_mark(&self, step: Step) -> Mark {
        let seen = self.welcome.seen(step);
        match step {
            Step::Sound => welcome::mark(self.sound_health(), seen),
            Step::Plugins => welcome::mark(self.plugins_health(), seen),
            Step::Ready => Mark::None,
        }
    }

    /// The step, scrolling between the head and the footer, then the
    /// footer, so the pills and Next are always in reach.
    fn welcome_body(&mut self, ui: &mut Ui, room: f32) -> Option<Footer> {
        let footer = theme::size::CONTROL + 2.0 * theme::space::M + theme::stroke::HAIRLINE;
        let body = (room - footer).max(theme::size::PREFERENCES_LEAST_PAGE);
        egui::ScrollArea::vertical()
            .id_salt(("welcome step", self.welcome.step))
            .max_height(body)
            .auto_shrink([false, true])
            .show(ui, |ui| match self.welcome.step {
                Step::Sound => self.welcome_sound(ui),
                Step::Plugins => self.welcome_plugins(ui),
                Step::Ready => self.welcome_ready(ui),
            });
        ui.add_space(theme::space::S);
        ui.separator();
        ui.add_space(theme::space::XS);
        self.welcome_footer(ui)
    }

    /// Lumi (when there is room), the step's display title and one line.
    fn welcome_intro(&mut self, ui: &mut Ui, title: &str, line: &str, pose: Option<Pose>) {
        let ctx = ui.ctx().clone();
        let roomy = ui.available_width() >= theme::size::WELCOME_NARROW;
        let picture = pose.filter(|_| roomy).and_then(|pose| {
            let lumi = self.welcome.lumi(&ctx)?;
            Some(match pose {
                Pose::Waving => lumi.waving.id(),
                Pose::Celebrating => lumi.celebrating.id(),
                Pose::Curious => lumi.curious.id(),
            })
        });
        ui.add_space(theme::space::S);
        ui.horizontal_top(|ui| {
            if let Some(texture) = picture {
                // Decorative: Lumi has no accessible name.
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    texture,
                    vec2(theme::size::LUMI, theme::size::LUMI),
                )));
                ui.add_space(theme::space::XL);
            }
            ui.vertical(|ui| {
                ui.add_space(if picture.is_some() {
                    theme::space::L
                } else {
                    0.0
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(title).font(fonts::display(theme::size::DISPLAY)),
                    )
                    .wrap(),
                );
                ui.add_space(theme::space::XS);
                ui.add(egui::Label::new(RichText::new(line).weak()).wrap());
            });
        });
        ui.add_space(theme::space::S);
    }

    // ── Sound ───────────────────────────────────────────────────────────

    fn welcome_sound(&mut self, ui: &mut Ui) {
        self.welcome_intro(
            ui,
            "Let\u{2019}s hear something",
            &format!("Choose where {PRODUCT_NAME} plays, then check that both speakers work."),
            Some(Pose::Waving),
        );
        self.asio_offer(ui);
        let mut actions = Vec::new();
        self.preferences
            .audio
            .device(ui, &self.transport.audio, &Marks::default(), &mut actions);
        for action in actions {
            // The device section asks only for device changes and Dismiss.
            let _ = self.transport.audio_action(action);
        }
        self.sound_recovery(ui);
        section(ui, "Sound check");
        self.sound_check_row(ui);
    }

    /// When an ASIO driver is installed and another driver model is in use:
    /// "<driver> is installed. It records with less delay", and Use ASIO.
    fn asio_offer(&mut self, ui: &mut Ui) {
        let audio = &self.transport.audio;
        if audio.on_test_device() {
            return;
        }
        let Some(drivers) = self.preferences.audio.asio_drivers(ui.ctx()) else {
            return;
        };
        let names: Vec<String> = drivers.iter().map(|(_, n)| n.clone()).collect();
        let Some(offer) = welcome::asio_offer(&names, &audio.driver) else {
            return;
        };
        let ready = audio.can_change();
        let mut chosen = None;
        ui.add_space(theme::space::S);
        ui.horizontal(|ui| {
            let words = match &offer {
                AsioOffer::One(name) => format!("{name} is installed. It records with less delay."),
                AsioOffer::Several(_) => {
                    "ASIO drivers are installed. They record with less delay.".to_owned()
                }
            };
            let reserve = theme::size::PAGE_LIST;
            let width = (ui.available_width() - reserve).max(theme::size::WRAP_LEAST);
            ui.allocate_ui(vec2(width, 0.0), |ui| {
                ui.set_max_width(width);
                ui.add(egui::Label::new(words).wrap());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(ready, |ui| match &offer {
                    AsioOffer::One(_) => {
                        if widgets::raised::button(ui, "Use ASIO")
                            .on_hover_text(
                                "Switch to the ASIO driver: the window stays usable meanwhile",
                            )
                            .clicked()
                        {
                            chosen = Some(0);
                        }
                    }
                    AsioOffer::Several(names) => {
                        let names: Vec<&str> = names.iter().map(String::as_str).collect();
                        chosen =
                            widgets::Dropdown::new("Use ASIO", "Use ASIO", theme::size::PAGE_LIST)
                                .show(ui, egui::Id::new("welcome asio"), &names, None);
                    }
                });
            });
        });
        if let Some((id, name)) = chosen.and_then(|i| drivers.get(i).cloned()) {
            let _ = self
                .transport
                .audio_action(audio::Action::Apply(AudioSettings {
                    driver: "asio".into(),
                    device_id: id.id,
                    device_name: name,
                    sample_rate: None,
                    buffer_frames: None,
                    input_channels: Vec::new(),
                    all_inputs: true,
                    output_channels: vec![0, 1],
                    cue_channels: Vec::new(),
                }));
        }
    }

    /// What to do when there is no sound: turn audio on, look again for a
    /// device, try the device again or go back to Windows audio.
    fn sound_recovery(&mut self, ui: &mut Ui) {
        let audio = &self.transport.audio;
        if !matches!(audio.status, Status::Off(_)) || !audio.can_change() {
            return;
        }
        let ctx = ui.ctx().clone();
        if audio.switched_off() {
            row(ui, "", |ui| {
                if widgets::raised::button(ui, "Audio on").clicked()
                    && self.availability(&ctx, Command::AudioOnOff).is_ok()
                {
                    self.run(&ctx, Command::AudioOnOff);
                }
            });
            return;
        }
        if !audio.has_device() {
            row(ui, "", |ui| {
                hint(
                    ui,
                    "No sound output was found. Plug one in and press \u{27f3}, or continue without sound.",
                );
            });
            return;
        }
        let again = audio.settings();
        let other_driver = audio.driver != DEFAULT_DRIVER;
        let mut retry = false;
        row(ui, "", |ui| {
            retry = widgets::raised::button(ui, "Try again")
                .on_hover_text("Open the same device again")
                .clicked();
            if other_driver
                && widgets::raised::button(ui, &format!("Use {}", driver_label(DEFAULT_DRIVER)))
                    .on_hover_text("Show this computer's own audio devices, to choose one")
                    .clicked()
            {
                self.preferences.audio.show_driver(DEFAULT_DRIVER);
            }
        });
        if retry && let Some(settings) = again {
            let _ = self.transport.audio_action(audio::Action::Apply(settings));
        }
    }

    /// Play test sound, the L and R lights that follow the chime, and the
    /// latency once the device runs.
    fn sound_check_row(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let ready = self.availability(&ctx, Command::SoundCheck);
        let (sounding, peaks) = self.transport.audio.sound_check_sounding();
        ui.horizontal(|ui| {
            let button = ui
                .add_enabled_ui(ready.is_ok(), |ui| {
                    widgets::raised::button(ui, Command::SoundCheck.name())
                })
                .inner;
            let keys = self.keymap.keys(Command::SoundCheck);
            let button = widgets::tip(
                button,
                Command::SoundCheck.name(),
                keys.as_ref(),
                Some(Command::SoundCheck.info().about),
            );
            let button = match ready {
                Err(why) => button.on_disabled_hover_text(why),
                Ok(()) => button,
            };
            if button.clicked() {
                self.run(&ctx, Command::SoundCheck);
            }
            ui.add_space(theme::space::S);
            speaker_light(ui, "L", "Left speaker", sounding.left(), peaks[0]);
            speaker_light(ui, "R", "Right speaker", sounding.right(), peaks[1]);
        });
        if sounding != Sounding::Silent {
            ctx.request_repaint();
        }
        hint(ui, "Nothing? Check the volume, or choose another device.");
        let audio = &self.transport.audio;
        if let (Status::Running { .. }, Some(info)) = (&audio.status, &audio.info) {
            let ms = |frames: u32| f64::from(frames) * 1000.0 / f64::from(info.sample_rate.max(1));
            let line = if info.input_channels > 0 {
                format!(
                    "Round trip {:.1} ms, as the driver reports it",
                    ms(info.input_latency + info.output_latency)
                )
            } else {
                format!(
                    "Output latency {:.1} ms, as the driver reports it",
                    ms(info.output_latency)
                )
            };
            hint(ui, &line);
        }
    }

    // ── Plugins ─────────────────────────────────────────────────────────

    fn welcome_plugins(&mut self, ui: &mut Ui) {
        self.welcome_intro(
            ui,
            "Bring your plugins",
            "VST3 plugins in the standard folders are found on their own. Add a folder if yours live elsewhere.",
            None,
        );
        self.scan_state(ui);
        self.plugin_folder_rows(ui, &Marks::default());
    }

    /// The scan as it stands: progress and Cancel, what is ready, what
    /// waits, or why there is no scan.
    fn scan_state(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        section(ui, "Scanning");
        if self.safe.on {
            warning(
                ui,
                "Safe mode is on, so no plugins load and none are looked for.",
                0.0,
            );
            let leave = Command::LeaveSafeMode;
            if self.availability(&ctx, leave).is_ok()
                && widgets::raised::button(ui, leave.name())
                    .on_hover_text(leave.info().about)
                    .clicked()
            {
                self.run(&ctx, leave);
            }
            return;
        }
        if self.plugins.is_off() {
            warning(ui, OFF_THIS_RUN, 0.0);
            return;
        }
        let ready = welcome::plugins_ready(self.plugins.catalog());
        let blocked = self.plugins.catalog().blocked().len();
        match self.plugins.status().clone() {
            Scan::Off => {
                warning(ui, OFF_THIS_RUN, 0.0);
            }
            Scan::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading the plugin list\u{2026}");
                });
            }
            Scan::Scanning { done, total } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if total == 0 {
                        "Looking for plugins\u{2026}".to_owned()
                    } else {
                        format!("Checking plugin files: {done} of {total}")
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::raised::button(ui, "Cancel")
                            .on_hover_text("Stop scanning; what was found stays")
                            .clicked()
                        {
                            self.plugins.cancel();
                        }
                    });
                });
                progress(
                    ui,
                    if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    },
                );
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            Scan::Failed(why) => {
                warning(ui, &why, 0.0);
                if widgets::raised::button(ui, "Retry")
                    .on_hover_text("Try the scan again")
                    .clicked()
                {
                    self.plugins.rescan(
                        lumen_plugin_catalog::scan::Mode::Changed,
                        &self.settings.plugins,
                        &ctx,
                    );
                }
            }
            Scan::Idle => {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} ready to use",
                        welcome::count(ready, "plugin", "plugins")
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.plugins.waiting().is_none()
                            && widgets::raised::button(ui, "Rescan")
                                .on_hover_text("Look for new and changed plugins")
                                .clicked()
                        {
                            self.plugins.rescan(
                                lumen_plugin_catalog::scan::Mode::Changed,
                                &self.settings.plugins,
                                &ctx,
                            );
                        }
                    });
                });
            }
        }
        if blocked > 0 {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(
                    vec2(theme::size::MENU_ICON, theme::size::MENU_ICON),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .extend(icons::shapes(Icon::Warning, icon, theme::p().warning));
                ui.label(format!(
                    "{} could not be loaded",
                    welcome::count(blocked, "plugin", "plugins")
                ));
                if ui
                    .link("See them")
                    .on_hover_text("Opens the Plugins page of Preferences, with Retry")
                    .clicked()
                    && self.availability(&ctx, Command::PreferencesPlugins).is_ok()
                {
                    self.run(&ctx, Command::PreferencesPlugins);
                }
            });
        }
        if let Some(waiting) = self.plugins.waiting() {
            for folder in waiting.folders() {
                let folder = folder.display().to_string();
                ui.add(
                    egui::Label::new(RichText::new(format!("Waiting to check {folder}")).weak())
                        .truncate(),
                )
                .on_hover_text(format!(
                    "{folder} is checked as soon as the scan under way ends"
                ));
            }
        }
    }

    // ── Ready ───────────────────────────────────────────────────────────

    fn welcome_ready(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let sound = self.sound_health();
        let plugins = self.plugins_health();
        let set = sound == Health::Good && plugins == Health::Good;
        let mut left = Vec::new();
        match sound {
            Health::Good => {}
            Health::Busy => left.push("The sound device is still starting"),
            Health::Needs => left.push("There is no sound yet"),
        }
        match plugins {
            Health::Good => {}
            Health::Busy => left.push("Plugins are still being checked"),
            Health::Needs if matches!(self.plugins.status(), Scan::Failed(_)) => {
                left.push("The plugin scan could not run")
            }
            Health::Needs => left.push("Plugins are off"),
        }
        let line = if set {
            "Sound and plugins work. Pick a look, then start.".to_owned()
        } else {
            format!(
                "{}. You can start anyway, and finish in Preferences later.",
                left.join(". ")
            )
        };
        self.welcome_intro(
            ui,
            if set { "You are set" } else { "Almost set" },
            &line,
            Some(if set {
                Pose::Celebrating
            } else {
                Pose::Curious
            }),
        );
        section(ui, "Summary");
        let sound_line = self.sound_summary();
        row(ui, "Sound", |ui| {
            ui.add(egui::Label::new(sound_line).wrap());
        });
        let plugins_line = self.plugins_summary();
        row(ui, "Plugins", |ui| {
            ui.add(egui::Label::new(plugins_line).wrap());
        });
        section(ui, "Look");
        let before = self.settings.appearance.clone();
        row(ui, "Theme", |ui| {
            let chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
            let themes = theme::Appearance::ALL.map(|a| widgets::Choice::new(a, a.name()));
            if let Some(picked) = widgets::segmented(ui, ui.id().with("theme"), chosen, &themes) {
                self.settings.appearance.theme = picked.key().to_owned();
            }
        });
        row(ui, "Interface size", |ui| {
            let scale = self.settings.appearance.scale();
            let labels = settings::UI_SCALES.map(|s| format!("{:.0}%", s * 100.0));
            let sizes: Vec<_> = settings::UI_SCALES
                .iter()
                .zip(&labels)
                .map(|(s, label)| widgets::Choice::new(*s, label))
                .collect();
            if let Some(picked) = widgets::segmented(ui, ui.id().with("scale"), scale, &sizes) {
                self.settings.appearance.ui_scale = picked;
            }
        });
        self.appearance_chosen(&before, &ctx);
        ui.add_space(theme::space::XL);
        let open = Command::OpenProject;
        let ready = self.availability(&ctx, open);
        let response = ui
            .add_enabled_ui(ready.is_ok(), |ui| {
                widgets::raised::button(ui, "Open a project\u{2026}")
            })
            .inner
            .on_hover_text("Close this window and open a project you have");
        if response.clicked() {
            self.finish_welcome(&ctx);
            self.run(&ctx, open);
        }
    }

    /// The Sound row of the summary: the driver, the device, the rate and
    /// buffer, and the round trip; or why there is no sound.
    fn sound_summary(&self) -> String {
        let audio = &self.transport.audio;
        match (&audio.status, &audio.info) {
            (Status::Running { .. }, Some(info)) => {
                let ms =
                    |frames: u32| f64::from(frames) * 1000.0 / f64::from(info.sample_rate.max(1));
                let mut line = format!(
                    "{} \u{b7} {} \u{b7} {} \u{b7} {} samples",
                    driver_label(&audio.driver),
                    audio.device,
                    widgets::khz(info.sample_rate),
                    info.buffer_frames
                );
                if info.input_channels > 0 {
                    line.push_str(&format!(
                        " \u{b7} round trip {:.1} ms",
                        ms(info.input_latency + info.output_latency)
                    ));
                }
                line
            }
            (Status::Starting, _) => "Still starting\u{2026}".to_owned(),
            _ => "No sound: choose a device on the Audio page of Preferences".to_owned(),
        }
    }

    /// The Plugins row of the summary.
    fn plugins_summary(&self) -> String {
        if self.safe.on || self.plugins.is_off() {
            return "Off for this run".to_owned();
        }
        match self.plugins.status() {
            Scan::Loading | Scan::Scanning { .. } => "Still checking".to_owned(),
            Scan::Failed(_) => "The scan could not run: see the Plugins step".to_owned(),
            Scan::Off => "Off for this run".to_owned(),
            Scan::Idle => {
                let ready = welcome::plugins_ready(self.plugins.catalog());
                let blocked = self.plugins.catalog().blocked().len();
                let mut line = format!("{} ready", welcome::count(ready, "plugin", "plugins"));
                if blocked > 0 {
                    line.push_str(&format!(" \u{b7} {blocked} blocked"));
                }
                if self.plugins.waiting().is_some() {
                    line.push_str(" \u{b7} a folder waits to be checked");
                }
                line
            }
        }
    }

    // ── Footer ──────────────────────────────────────────────────────────

    /// Skip setup or Back at the left, the keys at the middle, and the
    /// window's one filled button at the right: Next, or Start making
    /// music on Ready. Enter means Next only when nothing has the keyboard
    /// and no menu is open.
    fn welcome_footer(&mut self, ui: &mut Ui) -> Option<Footer> {
        let step = self.welcome.step;
        let mut asked = None;
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            vec2(width, theme::size::CONTROL),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(width, theme::size::CONTROL));
                let (label, tip, footer) = match step {
                    Step::Sound => (
                        "Skip setup",
                        "Close this window; everything here is also in Preferences",
                        Footer::Skip,
                    ),
                    _ => ("Back", "The step before", Footer::Back),
                };
                if widgets::raised::button(ui, label)
                    .on_hover_text(tip)
                    .clicked()
                {
                    asked = Some(footer);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (label, footer) = match step {
                        Step::Ready => ("Start making music", Footer::Finish),
                        _ => ("Next", Footer::Next),
                    };
                    let next = widgets::raised::primary_button(ui, label);
                    if std::mem::take(&mut self.welcome.focus_next) {
                        next.request_focus();
                    }
                    if next.clicked() {
                        asked = Some(footer);
                    }
                    let enter = ui.input(|i| i.key_pressed(Key::Enter))
                        && ui.ctx().memory(|m| m.focused().is_none())
                        && !egui::Popup::is_any_open(ui.ctx());
                    if enter && asked.is_none() {
                        asked = Some(footer);
                    }
                    ui.add(egui::Label::new(RichText::new(KEYS_HINT).small().weak()).truncate());
                });
            },
        );
        asked
    }
}

/// The step pills in the title row: each can be clicked, and shows ✓ when
/// its step is good or the warning shape when it needs something.
fn step_pills(ui: &mut Ui, step: Step, marks: [Mark; 3], picked: &mut Option<Step>) {
    for (i, s) in Step::ALL.into_iter().enumerate() {
        let label = format!("{} {}", i + 1, s.name());
        let pill = widgets::Pill {
            label: &label,
            tip: match s {
                Step::Sound => "Choose and test the sound device",
                Step::Plugins => "Plugin folders and scanning",
                Step::Ready => "What is set, and the look",
            },
        };
        let chosen = if s == step { 0 } else { 1 };
        if widgets::pill_tabs(ui, egui::Id::new(("welcome step", i)), chosen, &[pill]).is_some() {
            *picked = Some(s);
        }
        let size = theme::size::MENU_ICON;
        match marks[i] {
            Mark::None => {}
            Mark::Good => {
                let response = ui.label(RichText::new("\u{2713}").color(theme::p().accent));
                let name = format!("{} is set", s.name());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name)
                });
            }
            Mark::Needs => {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
                ui.painter()
                    .extend(icons::shapes(Icon::Warning, rect, theme::p().warning));
                let name = format!("{} needs attention", s.name());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name)
                });
            }
        }
        ui.add_space(theme::space::XS);
    }
}

/// A sound check light: the side's letter in a round light, lit (the
/// design system's on state, and a ring that grows with the level) while
/// that side sounds. Named "<side>, sounding" while lit.
fn speaker_light(ui: &mut Ui, letter: &str, side: &str, lit: bool, level: f32) {
    let size = theme::size::SPEAKER_LIGHT;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let p = theme::p();
    let (fill, stroke, ink) = if lit {
        (
            p.accent_muted,
            p.accent.gamma_multiply(
                RING_LEAST + (1.0 - RING_LEAST) * (level / chime::LEVEL).clamp(0.0, 1.0),
            ),
            p.accent,
        )
    } else {
        (p.well, p.border, p.text_hint)
    };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), size / 2.0, fill);
    painter.circle_stroke(
        rect.center(),
        size / 2.0 - theme::stroke::HAIRLINE / 2.0,
        egui::Stroke::new(
            if lit {
                theme::stroke::FOCUS
            } else {
                theme::stroke::HAIRLINE
            },
            stroke,
        ),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(theme::size::CAPTION),
        ink,
    );
    let name = if lit {
        format!("{side}, sounding")
    } else {
        side.to_owned()
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
}

/// A thin progress bar the width of the step.
fn progress(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::size::PROGRESS_BAR),
        egui::Sense::hover(),
    );
    let p = theme::p();
    let round = theme::size::PROGRESS_BAR / 2.0;
    ui.painter().rect_filled(rect, round, p.well);
    let filled = egui::Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter().rect_filled(filled, round, p.accent);
}
