//! The Monitor badge on a MIDI track's header (M5 slice 2 fix, the owner's
//! MA): a small speaker before M, shown only when Monitor is not Auto, so
//! the name keeps its room. In is the accent on its muted fill, with no halo
//! (a chosen setting, not something sounding); Off is the struck-through
//! speaker in the hint grey. Auto shows nothing: it is set from the track
//! menu, and the hollow ring on a selected Auto track stays. A click cycles
//! In, Off, Auto.

use eframe::egui::{self, Id as EguiId, Rect, Sense, Ui};
use lumen_project::model::{Monitor, Track, TrackKind};

use crate::icons::{self, Icon};
use crate::theme::Tone;
use crate::widgets::raised;
use crate::{motion, theme, widgets};
use eframe::emath::GuiRounding as _;

/// Whether `track` shows the badge.
pub(super) fn shown(track: &Track) -> bool {
    track.kind == TrackKind::Midi && track.monitor != Monitor::Auto
}

/// Draws the badge in `rect`; returns the Monitor a click chose.
pub(super) fn badge(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    track: &Track,
    look: theme::Controls,
) -> Option<Monitor> {
    let on = track.monitor == Monitor::In;
    let (word, tip) = if on {
        ("In", "Monitor: In \u{b7} always plays what you play")
    } else {
        ("Off", "Monitor: Off \u{b7} never plays what you play")
    };
    let response = ui.interact(rect, id.with("monitor"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Monitor {}: {word}", track.name),
        )
    });
    let response = widgets::tip(response, tip, None, Some("Click to change"));
    let palette = theme::p();
    let hover = motion::hover(ui.ctx(), id.with("monitor hover"), response.hovered());
    let lit = motion::hover(ui.ctx(), id.with("monitor lit"), on);
    // The premium look (ADR-0046): raised; In is lit in the accent, Off is
    // the struck-through speaker in the hint grey.
    let face = raised::Face::new(look)
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
        .lit(lit, Tone::Accent);
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let body = raised::paint(ui.painter(), rect, theme::radius::SMALL, &face);
    let (ink, icon) = if on {
        (face.text(), Icon::Speaker)
    } else {
        (palette.text_hint, Icon::SpeakerOff)
    };
    let icon_rect = Rect::from_center_size(body.center().round(), body.size());
    ui.painter().extend(icons::shapes(
        icon,
        icon_rect.shrink2(egui::vec2(1.0, 0.0)),
        ink,
    ));
    raised::focus_ring(ui.painter(), &response, rect);
    response.clicked().then_some(match track.monitor {
        Monitor::In => Monitor::Off,
        Monitor::Off | Monitor::Auto => Monitor::Auto,
    })
}
