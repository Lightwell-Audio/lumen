use eframe::egui::{self, Rect, Response, Ui};
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::auto_state::{AutoView, MapRequest};
use crate::document::Document;
use crate::widgets::{self, Ask};

pub(crate) fn request_of(ask: Ask, track: Id, target: Target) -> Option<MapRequest> {
    match ask {
        Ask::Automate => None,
        Ask::Learn => Some(MapRequest::Learn(track, target)),
        Ask::StopLearning => Some(MapRequest::StopLearning),
        Ask::Clear => Some(MapRequest::Clear(track, target)),
        Ask::Window => Some(MapRequest::Window),
    }
}

pub(crate) fn ask(auto: &mut AutoView, ask: Ask, track: Id, target: Target) {
    if ask == Ask::Automate {
        auto.shown.insert(track, target);
    }
    if let Some(request) = request_of(ask, track, target) {
        auto.requests.push(request);
    }
}

pub(crate) fn control_menu(
    response: &Response,
    doc: &Document,
    auto: &mut AutoView,
    track: Id,
    target: Target,
) {
    let mapped = doc.mapping_of(track, target).map(|m| (m.channel, m.number));
    let waiting = auto.learning == Some((track, target));
    let mut name = auto.name_of(doc.project(), target);
    if let Some(owner) = lumen_project::routing::all_tracks(doc.project()).find(|t| t.id == track) {
        name = format!("{name} \u{00B7} {}", owner.name);
    }
    let asked = widgets::Menu::context(response)
        .show(|ui| {
            widgets::menu_header(ui, &name);
            widgets::control_items(ui, mapped, waiting)
        })
        .flatten();
    if let Some(asked) = asked {
        self::ask(auto, asked, track, target);
    }
}

pub(crate) fn control_menu_for(
    ui: &Ui,
    id: egui::Id,
    doc: &Document,
    auto: &mut AutoView,
    track: Id,
    target: Target,
) {
    if ui
        .data(|d| d.get_temp::<String>(id.with("typing")))
        .is_some()
    {
        return;
    }
    if let Some(response) = ui.ctx().read_response(id) {
        control_menu(&response, doc, auto, track, target);
    }
}

pub(crate) fn waiting_ring(ui: &Ui, auto: &AutoView, rect: Rect, track: Id, target: Target) {
    if auto.learning == Some((track, target)) {
        super::follow::ring_with(ui, rect, crate::theme::p().accent);
    }
}
