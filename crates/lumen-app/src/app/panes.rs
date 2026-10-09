//! Moving the keyboard between panes (M3 slice 7d; ADR-0015): F6 and
//! Shift+F6 cycle through the browser, the arrangement and the detail
//! panel, skipping hidden ones. Each pane has an anchor that takes the
//! focus, named for screen readers, with the 2 pt accent ring around the
//! pane; Tab goes on from there into the pane.

use eframe::egui::{self, Rect, Sense, Stroke};

use crate::{App, theme};

/// A pane the keyboard can move to, in the order F6 visits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Browser,
    Arrangement,
    Detail,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::Browser, Pane::Arrangement, Pane::Detail];

    fn name(self) -> &'static str {
        match self {
            Pane::Browser => "Browser",
            Pane::Arrangement => "Arrangement",
            Pane::Detail => "Detail panel",
        }
    }

    fn id(self) -> egui::Id {
        egui::Id::new(("pane", self.name()))
    }
}

/// Where the keyboard is among the panes (not saved). Its own struct, so
/// a pane drawn inside a closure that borrows other parts of the app can
/// still place its anchor.
#[derive(Debug, Default)]
pub(crate) struct Panes {
    /// The pane to focus when it next draws.
    request: Option<Pane>,
    /// The pane the keyboard was last moved to.
    last: Option<Pane>,
}

/// The pane after (or before) `from` among `shown`; from none, the
/// arrangement.
fn step(shown: &[Pane], from: Option<Pane>, forward: bool) -> Option<Pane> {
    let Some(at) = from.and_then(|p| shown.iter().position(|&s| s == p)) else {
        return shown.iter().copied().find(|&p| p == Pane::Arrangement);
    };
    let n = shown.len();
    let next = if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    };
    Some(shown[next])
}

impl App {
    fn shown_panes(&self) -> Vec<Pane> {
        Pane::ALL
            .into_iter()
            .filter(|p| match p {
                Pane::Browser => self.show_browser,
                Pane::Arrangement => true,
                Pane::Detail => self.show_detail,
            })
            .collect()
    }

    /// Moves the keyboard to the next pane (or the previous one).
    pub(crate) fn cycle_pane(&mut self, forward: bool) {
        self.panes.request = step(&self.shown_panes(), self.panes.last, forward);
    }
}

/// The pane whose anchor has the keyboard, if any.
pub(crate) fn focused(ctx: &egui::Context) -> Option<Pane> {
    Pane::ALL
        .into_iter()
        .find(|p| ctx.memory(|m| m.has_focus(p.id())))
}

impl Panes {
    /// The anchor of `pane`, covering `rect`: takes the focus when asked,
    /// and draws the focus ring around the pane while it has it. Call it
    /// first in the pane, so Tab goes on to the pane's own controls.
    pub(crate) fn anchor(&mut self, ui: &mut egui::Ui, pane: Pane, rect: Rect) {
        let response = ui.interact(rect, pane.id(), Sense::focusable_noninteractive());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, pane.name()));
        if self.request == Some(pane) {
            self.request = None;
            response.request_focus();
        }
        if response.has_focus() {
            self.last = Some(pane);
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                theme::radius::SMALL,
                Stroke::new(2.0, theme::p().accent),
                egui::StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f6_cycles_the_shown_panes_and_starts_at_the_arrangement() {
        let all = Pane::ALL;
        assert_eq!(step(&all, None, true), Some(Pane::Arrangement));
        assert_eq!(
            step(&all, Some(Pane::Arrangement), true),
            Some(Pane::Detail)
        );
        assert_eq!(step(&all, Some(Pane::Detail), true), Some(Pane::Browser));
        assert_eq!(step(&all, Some(Pane::Browser), false), Some(Pane::Detail));
        let without_browser = [Pane::Arrangement, Pane::Detail];
        assert_eq!(
            step(&without_browser, Some(Pane::Browser), true),
            Some(Pane::Arrangement),
            "a hidden pane starts over"
        );
        assert_eq!(
            step(&[Pane::Arrangement], Some(Pane::Arrangement), true),
            Some(Pane::Arrangement)
        );
    }
}
