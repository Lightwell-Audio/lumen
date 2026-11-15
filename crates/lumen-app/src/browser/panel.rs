use eframe::egui::{self, Key, Modifiers, Rect, vec2};

use super::Tab;
use super::rows::{Row, Rows};
use super::view::{Selection, View};
use crate::icons::{self, Icon};
use crate::{fonts, theme, widgets};

pub(crate) const FOOTER: f32 = 150.0;

pub(crate) fn tabs(ui: &mut egui::Ui, open: &mut Tab) {
    let choices = Tab::ALL.map(|tab| {
        widgets::Choice::new(tab, "")
            .icon(tab_icon(tab))
            .named(tab.access())
            .tip(tab.tip())
    });
    let id = egui::Id::new("browser tabs");
    if let Some(tab) = widgets::segmented_full(ui, id, *open, &choices) {
        *open = tab;
    }
    if let Some(tab) = tab_arrows(ui, id, *open) {
        *open = tab;
    }
    ui.add_space(theme::space::XS);
    let tab = *open;
    let palette = theme::p();
    let mut line = egui::text::LayoutJob::default();
    line.append(
        tab.name(),
        0.0,
        egui::TextFormat::simple(fonts::medium(theme::size::CAPTION), palette.text),
    );
    line.append(
        &format!(" \u{b7} {}", tab.holds()),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(theme::size::CAPTION),
            palette.text_secondary,
        ),
    );
    line.wrap.max_width = ui.available_width();
    line.wrap.max_rows = 1;
    ui.add(egui::Label::new(line).selectable(false));
    ui.add_space(theme::space::XS);
}

pub(crate) fn no_match(ui: &mut egui::Ui, area: Rect, view: &mut View) {
    let state = widgets::EmptyState {
        icon: None,
        icon_size: theme::space::L,
        title: "No devices match",
        hint: "Try other words, or clear the filters",
        action: Some(widgets::EmptyAction {
            label: "Clear filters",
            keys: None,
            tip: "Show every device again",
        }),
    };
    if widgets::empty_state(ui, area, &state) {
        view.clear_filters();
    }
}

pub(crate) fn is_folded(rows: &Rows, vendor: &str) -> bool {
    rows.list.iter().any(
        |r| matches!(r, Row::Vendor { name, folded, .. } if name.to_lowercase() == vendor && *folded),
    )
}

pub(crate) fn vendor_listed(rows: &Rows, selection: &Selection) -> bool {
    rows.list.iter().any(|r| {
        matches!((r, selection), (Row::Vendor { name, .. }, Selection::Vendor(s)) if &name.to_lowercase() == s)
    })
}

fn tab_icon(tab: Tab) -> fn(&egui::Painter, egui::Pos2, egui::Color32) {
    fn draw(icon: Icon, painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32) {
        let rect = Rect::from_center_size(at, vec2(TAB_ICON, TAB_ICON));
        painter.extend(icons::shapes(icon, rect, colour));
    }
    match tab {
        Tab::Devices => |p, at, c| draw(Icon::Rack, p, at, c),
        Tab::Samples => |p, at, c| draw(Icon::Wave, p, at, c),
        Tab::Projects => |p, at, c| draw(Icon::Document, p, at, c),
        Tab::Places => |p, at, c| draw(Icon::Folder, p, at, c),
        Tab::Media => |p, at, c| draw(Icon::Disc, p, at, c),
    }
}

const TAB_ICON: f32 = 14.0;

fn tab_arrows(ui: &egui::Ui, id: egui::Id, open: Tab) -> Option<Tab> {
    let focused = (0..Tab::ALL.len()).any(|i| ui.memory(|m| m.has_focus(id.with(i))));
    if !focused {
        return None;
    }
    let at = Tab::ALL.iter().position(|t| *t == open)?;
    let step = ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            -1
        } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            1
        } else {
            0
        }
    });
    let to = at
        .checked_add_signed(step)
        .filter(|i| *i < Tab::ALL.len())?;
    if to == at {
        return None;
    }
    ui.memory_mut(|m| m.request_focus(id.with(to)));
    Some(Tab::ALL[to])
}
