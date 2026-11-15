use super::*;

impl Transport {
    pub(super) fn follow_button(&mut self, ui: &mut egui::Ui, keymap: &Keymap) {
        use crate::settings::Follow;
        let on = self.follow != Follow::Off;
        let keys = keymap.keys(Run::ToggleFollow);
        let response = icons::button(
            ui,
            Icon::Follow,
            "Follow playhead",
            Tip {
                command: Some(Run::ToggleFollow),
                keys: keys.as_ref(),
                hint: Some("Right-click for page or continuous"),
            },
            on && !self.follow_paused,
            true,
            "",
        );
        let response = if on && self.follow_paused {
            response.on_hover_text("Paused while you look around; Play or a click resumes it")
        } else {
            response
        };
        if response.clicked() {
            self.toggle_follow();
        }
        widgets::Menu::context(&response).show(|ui| {
            widgets::menu_caption(ui, "Follow the playhead");
            let modes = [Follow::Page, Follow::Continuous, Follow::Off]
                .map(|mode| widgets::Choice::new(mode, mode.name()));
            if let Some(mode) = widgets::segmented(ui, ui.id().with("follow"), self.follow, &modes)
            {
                self.follow = mode;
                self.follow_paused = false;
                if mode != Follow::Off {
                    self.follow_last = mode;
                }
                ui.close();
            }
        });
    }

    pub(super) fn keyboard_button(&mut self, ui: &mut Ui, keymap: &Keymap) {
        let on = self.keyboard_readout.is_some();
        let keys = keymap.keys(Run::ComputerKeyboard);
        let response = icons::button(
            ui,
            Icon::Keycap,
            "Computer keyboard plays notes",
            Tip {
                command: Some(Run::ComputerKeyboard),
                keys: keys.as_ref(),
                hint: Some("A to K play notes, Z X octave, C V velocity, Esc leaves"),
            },
            on,
            true,
            "",
        );
        if response.clicked() {
            self.keyboard_clicked = true;
        }
        self.keyboard_readout_shown = false;
        if let (Some(readout), true) = (self.keyboard_readout.clone(), self.fit.plan.keyboard) {
            let before = ui.cursor().left();
            let c = theme::p();
            let galley = ui
                .painter()
                .layout_no_wrap(readout.clone(), keyboard_font(), c.text);
            let size = egui::vec2(
                galley.size().x + 2.0 * theme::space::S,
                theme::size::WELL_CELL,
            );
            let (rect, piece) = ui.allocate_exact_size(size, egui::Sense::hover());
            piece.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("Computer keyboard: {readout}"),
                )
            });
            ui.painter().galley(
                (rect.center() - galley.size() / 2.0).round(),
                galley,
                c.text,
            );
            self.keyboard_readout_shown = true;
            self.fit.took(ui, before);
        }
    }

    pub(super) fn record_button(
        &mut self,
        ui: &mut Ui,
        keymap: &Keymap,
        state: TransportSnapshot,
        record: RecordSettings,
        beats: u32,
    ) {
        let keys = keymap.keys(Run::Record);
        let count_in = format!("Count-in: {}", record.count_in.name());
        let red = theme::p().record;
        let base = Look {
            enabled: true,
            tone: Some(theme::Tone::Record),
            ..Look::default()
        };
        let (icon, look) = match record::phase(&state) {
            Phase::Rest => (
                Icon::Record,
                Look {
                    ink: Some(red.gamma_multiply(record::REST_INK)),
                    ..base
                },
            ),
            Phase::CountingIn => (
                Icon::RecordRing,
                Look {
                    active: true,
                    ink: Some(red),
                    lit: Some(record::count_in_lit(
                        state.count_in_left,
                        state.count_in_total,
                        beats,
                        motion::reduced(),
                    )),
                    ..base
                },
            ),
            Phase::Recording => (
                Icon::Record,
                Look {
                    active: true,
                    ..base
                },
            ),
        };
        let response = icons::button_look(
            ui,
            icon,
            "Record",
            Tip {
                command: Some(Run::Record),
                keys: keys.as_ref(),
                hint: Some(count_in.as_str()),
            },
            look,
        );
        if response.clicked() {
            self.requests.push(Run::Record);
        }
    }

    pub(super) fn overdub_button(&mut self, ui: &mut Ui, keymap: &Keymap, record: RecordSettings) {
        let keys = keymap.keys(Run::Overdub);
        let overdub = icons::button(
            ui,
            Icon::Layers,
            "Overdub",
            Tip {
                command: Some(Run::Overdub),
                keys: keys.as_ref(),
                hint: Some(OVERDUB_HINT),
            },
            record.overdub,
            true,
            "",
        );
        if overdub.clicked() {
            self.requests.push(Run::Overdub);
        }
    }
}

pub(super) fn solo_piece(ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
    let soloed = doc.project().tracks.iter().filter(|t| t.solo).count();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(theme::size::TRANSPORT_ICON, theme::size::WELL_CELL),
        egui::Sense::hover(),
    );
    let id = ui.id().with("clear solos");
    let lit = motion::hover(ui.ctx(), id.with("lit"), soloed > 0);
    let (response, ink) =
        widgets::well::button(ui, rect, id, (lit, theme::Tone::Accent), soloed > 0);
    let keys = keymap.keys(Run::ClearSolos);
    let hint = if soloed > 0 {
        format!("{soloed} soloed")
    } else {
        "Nothing is soloed".to_owned()
    };
    crate::testing::coverage::drawn(ui.ctx(), "Clear all solos", Run::ClearSolos);
    let response = widgets::tip(response, "Clear all solos", keys.as_ref(), Some(&hint));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, soloed > 0, "Clear all solos")
    });
    let galley = ui.painter().layout_no_wrap(
        "S".to_owned(),
        crate::fonts::medium(theme::size::CAPTION),
        ink,
    );
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    ui.painter()
        .galley((rect.center() - galley.size() / 2.0).round(), galley, ink);
    if soloed > 0 && response.clicked() {
        doc.clear_solos();
    }
}
