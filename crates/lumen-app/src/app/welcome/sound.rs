//! The first-run window's Sound step (ADR-0115): the device rows, the
//! ASIO offer, what to do when no sound comes, and the sound check.

use super::*;

impl App {
    pub(super) fn welcome_sound(&mut self, ui: &mut Ui) {
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
    pub(super) fn asio_offer(&mut self, ui: &mut Ui) {
        let audio = &self.transport.audio;
        if audio.on_test_device() {
            return;
        }
        let Some(drivers) = self.preferences.audio.asio_drivers(ui.ctx()) else {
            return;
        };
        let names: Vec<String> = drivers.iter().map(|(_, n)| n.clone()).collect();
        let Some(offer) = rules::asio_offer(&names, &audio.driver) else {
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
    pub(super) fn sound_recovery(&mut self, ui: &mut Ui) {
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
    pub(super) fn sound_check_row(&mut self, ui: &mut Ui) {
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
