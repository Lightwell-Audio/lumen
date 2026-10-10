//! Empty states (the interface audit's S5, the owner's A): one layout for
//! every area with nothing in it yet: a quiet icon, a title, one line of
//! hint, and the action that fills it as a real button, all centred.

use eframe::egui::{self, Align2, FontId, Painter, Pos2, Rect, Ui, pos2, vec2};

use super::keys::{Keys, tip};
use crate::{fonts, theme};

/// What an empty area says.
pub struct EmptyState<'a> {
    /// Draws a small icon centred at a point, in a colour.
    pub icon: Option<fn(&Painter, Pos2, egui::Color32)>,
    /// The icon's height: `theme::space::L` for a small icon, more for the
    /// mark above an empty arrangement.
    pub icon_size: f32,
    pub title: &'a str,
    pub hint: &'a str,
    /// The button that fills the area: its label, its shortcut and what it
    /// does (its tooltip).
    pub action: Option<Action<'a>>,
}

/// An empty state's button.
pub struct Action<'a> {
    pub label: &'a str,
    pub keys: Option<&'a Keys>,
    pub tip: &'a str,
}

/// Draws `state` centred in `rect`; true when its button was clicked.
pub fn empty_state(ui: &mut Ui, rect: Rect, state: &EmptyState<'_>) -> bool {
    let palette = theme::p();
    let wrap = (rect.width() - 2.0 * theme::space::M).max(40.0);
    let title = ui.painter().layout(
        state.title.to_owned(),
        fonts::medium(theme::size::BODY),
        palette.text_secondary,
        wrap,
    );
    let hint = ui.painter().layout(
        state.hint.to_owned(),
        FontId::proportional(theme::size::CAPTION),
        palette.text_hint,
        wrap,
    );
    let icon_room = if state.icon.is_some() {
        state.icon_size + theme::space::S
    } else {
        0.0
    };
    let button_room = if state.action.is_some() {
        theme::space::M + theme::size::CONTROL
    } else {
        0.0
    };
    let height = icon_room + title.size().y + theme::space::XS + hint.size().y + button_room;
    let mut y = (rect.center().y - height / 2.0).max(rect.top() + theme::space::S);
    let painter = ui.painter_at(rect);
    if let Some(icon) = state.icon {
        icon(
            &painter,
            pos2(rect.center().x, y + state.icon_size / 2.0),
            palette.text_hint,
        );
        y += icon_room;
    }
    let title_height = title.size().y;
    painter.galley(
        Align2::CENTER_TOP
            .anchor_size(pos2(rect.center().x, y), title.size())
            .min,
        title,
        palette.text_secondary,
    );
    y += title_height + theme::space::XS;
    let hint_height = hint.size().y;
    painter.galley(
        Align2::CENTER_TOP
            .anchor_size(pos2(rect.center().x, y), hint.size())
            .min,
        hint,
        palette.text_hint,
    );
    y += hint_height;
    let Some(action) = &state.action else {
        return false;
    };
    y += theme::space::M;
    let label = ui.painter().layout_no_wrap(
        action.label.to_owned(),
        FontId::proportional(theme::size::BODY),
        palette.text,
    );
    let width = label.size().x + 2.0 * theme::space::M;
    let button = Rect::from_center_size(
        pos2(rect.center().x, y + theme::size::CONTROL / 2.0),
        vec2(width, theme::size::CONTROL),
    );
    let response = ui.put(button, egui::Button::new(action.label));
    tip(response, action.tip, action.keys, None).clicked()
}
