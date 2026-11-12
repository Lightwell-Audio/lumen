//! The first-run window's Ready step (ADR-0115): the summary, theme and
//! size.

use super::*;

impl App {
    pub(super) fn welcome_ready(&mut self, ui: &mut Ui) {
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
        if crate::association::Association::supported() {
            section(ui, "Projects");
            let mut wish = self.settings.projects.associate_files;
            let label = format!(
                "Open .lumen files with {}",
                lumen_core::product::PRODUCT_NAME
            );
            if widgets::switch(ui, &mut wish, &label, true)
                .on_hover_text("Projects open here from File Explorer, each with the project icon")
                .changed()
            {
                self.settings.projects.associate_files = wish;
                self.settings_changed = Some(std::time::Instant::now());
            }
            if let Some(crate::association::State::Other(path)) =
                self.association_status().map(|status| &status.state)
            {
                widgets::hint(
                    ui,
                    &format!(
                        "Another copy opens them now ({}). Preferences › Files can switch them to this one.",
                        path.display()
                    ),
                );
            }
        }
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
    pub(super) fn sound_summary(&self) -> String {
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
    pub(super) fn plugins_summary(&self) -> String {
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
}
