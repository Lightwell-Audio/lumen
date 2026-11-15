use eframe::egui::{self, Rect, Sense, Stroke};

use crate::{App, theme};

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

#[derive(Debug, Default)]
pub(crate) struct Panes {
    request: Option<Pane>,
    last: Option<Pane>,
}

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

    pub(crate) fn cycle_pane(&mut self, forward: bool) {
        self.panes.request = step(&self.shown_panes(), self.panes.last, forward);
    }
}

pub(crate) fn focus(ctx: &egui::Context, pane: Pane) {
    ctx.memory_mut(|m| m.request_focus(pane.id()));
}

pub(crate) fn focused(ctx: &egui::Context) -> Option<Pane> {
    Pane::ALL
        .into_iter()
        .find(|p| ctx.memory(|m| m.has_focus(p.id())))
}

impl Panes {
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
                Stroke::new(theme::stroke::FOCUS, theme::p().accent),
                egui::StrokeKind::Inside,
            );
        }
    }
}

pub(crate) fn keep_arrows(ctx: &egui::Context, pane: Pane) {
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            pane.id(),
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
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
