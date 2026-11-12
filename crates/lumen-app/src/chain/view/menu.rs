//! A card's menu, and its "Move to track" and "Copy to track" rows.

use super::*;

/// What a card's menu offers for its device.
pub(super) struct CardMenu {
    pub(super) id: Id,
    pub(super) bypass: bool,
    pub(super) folded: bool,
    /// The compact card: its menu also holds what the full card shows
    /// (the preset, Learn, every parameter, the window).
    pub(super) compact: bool,
    /// A running plugin's preset: the sound's name, when one was chosen.
    pub(super) preset: Option<Option<String>>,
    /// How many parameters a running plugin lists.
    pub(super) parameters: usize,
    pub(super) learning: bool,
    /// A running plugin with a window: whether it is open.
    pub(super) window: Option<bool>,
}

impl App {
    /// A card's menu (right-click, or the compact card's name). True when a
    /// row was chosen.
    pub(super) fn card_menu(&mut self, ui: &mut Ui, track: Id, menu: &CardMenu) -> bool {
        let ctx = ui.ctx().clone();
        self.chain.selected = Some(menu.id);
        let mut chosen = false;
        if menu.compact {
            if let Some(preset) = &menu.preset {
                let label = match preset {
                    Some(name) => format!("Preset: {name}\u{2026}"),
                    None => "Choose a preset\u{2026}".to_owned(),
                };
                chosen |= self.menu_command(ui, Command::DevicePreset, &label);
                for (label, forward) in [("Previous preset", false), ("Next preset", true)] {
                    if widgets::menu_item(ui, label, None, true).clicked() {
                        self.step_preset(&ctx, menu.id, forward);
                        chosen = true;
                    }
                }
                ui.separator();
            }
            if menu.parameters > 0 {
                let learn = if menu.learning {
                    "Stop learning"
                } else {
                    "Learn"
                };
                chosen |= self.menu_command(ui, Command::DeviceLearn, learn);
                let all = format!("All {} parameters\u{2026}", menu.parameters);
                chosen |= self.menu_command(ui, Command::DeviceParameters, &all);
                ui.separator();
            }
            if let Some(open) = menu.window {
                let label = if open { "Close editor" } else { "Open editor" };
                chosen |= self.menu_command(ui, Command::DeviceEditor, label);
            }
        }
        let bypass_label = if menu.bypass { "Turn on" } else { "Bypass" };
        chosen |= self.menu_command(ui, Command::DeviceBypass, bypass_label);
        let fold_label = if menu.folded { "Open" } else { "Fold" };
        chosen |= self.menu_command(ui, Command::DeviceFold, fold_label);
        chosen |= self.menu_command(ui, Command::DeviceMoveEarlier, "Move earlier");
        chosen |= self.menu_command(ui, Command::DeviceMoveLater, "Move later");
        chosen |= self.menu_command(ui, Command::DeviceDuplicate, "Duplicate");
        chosen |= self.menu_command(ui, Command::DeviceCopy, "Copy");
        chosen |= self.menu_command(ui, Command::DevicePaste, "Paste");
        self.send_menus(ui, track);
        chosen |= self.menu_command(ui, Command::DeviceRemove, "Remove");
        chosen
    }
}

impl App {
    /// "Move to track" and "Copy to track" in a card's menu: the tracks that
    /// can take the chosen device.
    fn send_menus(&mut self, ui: &mut Ui, here: Id) {
        let targets = self.send_targets();
        for (copy, label) in [(false, "Move to track"), (true, "Copy to track")] {
            let mut chosen = None;
            widgets::submenu(ui, label, |ui| {
                if targets.is_empty() {
                    ui.add_enabled(false, egui::Label::new("No track can take it"));
                }
                for (id, name) in &targets {
                    // Moving to its own track is reordering, not this.
                    let enabled = copy || *id != here;
                    if widgets::menu_item(ui, name, None, enabled).clicked() {
                        chosen = Some(*id);
                        ui.close();
                    }
                }
            });
            if let Some(to) = chosen {
                self.send_chain_device(to, copy);
            }
        }
    }
}
