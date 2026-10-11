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
use crate::{motion, theme, widgets};

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
    selected: bool,
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
    let (fill, ink, icon) = if on {
        (
            palette
                .accent_muted
                .lerp_to_gamma(palette.accent, 0.25 * hover),
            palette.accent,
            Icon::Speaker,
        )
    } else {
        (
            look.fill.lerp_to_gamma(look.hover, hover),
            palette.text_hint,
            Icon::SpeakerOff,
        )
    };
    let p = ui.painter();
    p.rect_filled(rect, theme::radius::SMALL, fill);
    if on && selected {
        // A selected row is nearly the badge's own fill: a hairline keeps
        // it a badge there.
        p.rect_stroke(
            rect,
            theme::radius::SMALL,
            egui::Stroke::new(1.0, palette.accent.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
    }
    p.extend(icons::shapes(icon, rect.shrink2(egui::vec2(1.0, 0.0)), ink));
    response.clicked().then_some(match track.monitor {
        Monitor::In => Monitor::Off,
        Monitor::Off | Monitor::Auto => Monitor::Auto,
    })
}
