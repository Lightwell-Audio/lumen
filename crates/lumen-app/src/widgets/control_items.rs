use eframe::egui::Ui;

use super::menu_item;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Automate,
    Learn,
    StopLearning,
    Clear,
    Window,
}

pub fn control_items(ui: &mut Ui, mapped: Option<(u8, u8)>, waiting: bool) -> Option<Ask> {
    let mut ask = None;
    if menu_item(ui, "Automate", None, true).clicked() {
        ask = Some(Ask::Automate);
    }
    let learn = if waiting {
        "Stop waiting for a controller"
    } else if mapped.is_some() {
        "Learn another controller\u{2026}"
    } else {
        "Learn a controller\u{2026}"
    };
    if menu_item(ui, learn, None, true).clicked() {
        ask = Some(if waiting {
            Ask::StopLearning
        } else {
            Ask::Learn
        });
    }
    if let Some((channel, number)) = mapped
        && menu_item(
            ui,
            &format!("Clear controller {number} on channel {}", channel + 1),
            None,
            true,
        )
        .clicked()
    {
        ask = Some(Ask::Clear);
    }
    if menu_item(ui, "Controller mappings\u{2026}", None, true).clicked() {
        ask = Some(Ask::Window);
    }
    ask
}
