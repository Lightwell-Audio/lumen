//! The rows every automatable control's right-click menu ends with (M7 slice
//! A5): show its lane, learn a controller for it, clear the controller, open
//! the list of mappings. The control's own rows come first and are its own.

use eframe::egui::Ui;

use super::menu_item;

/// What the menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Show the control's lane.
    Automate,
    /// Wait for a controller to move, to map it.
    Learn,
    /// Stop waiting.
    StopLearning,
    /// Take away the controller that moves the control.
    Clear,
    /// Open the Mappings window.
    Window,
}

/// The rows, given the controller mapped to the control (channel, number) and
/// whether it waits for one now. A click closes the menu.
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
