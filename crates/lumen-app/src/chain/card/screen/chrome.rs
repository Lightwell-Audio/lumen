//! The set piece's chrome: the header's power button, name and subtitle, the
//! Preset pill, the quiet "All n" link and the folded rail.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2,
};

use super::plan::Plan;
use crate::chain::card::Look;
use crate::chain::strip;
use crate::icons::{self, Icon};
use crate::theme::personality::card as c;
use crate::widgets::raised;
use crate::{fonts, motion, theme, widgets};

/// `text` with its baseline on `at.y`, aligned by `align` to `at.x`; `spacing`
/// is the extra space between letters.
pub(super) fn on_baseline(
    painter: &Painter,
    at: Pos2,
    align: Align,
    text: String,
    font: FontId,
    colour: Color32,
    spacing: f32,
) {
    let job = LayoutJob::single_section(
        text,
        TextFormat {
            font_id: font,
            color: colour,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    let galley = painter.layout_job(job);
    let base = galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
        .unwrap_or(galley.size().y * 0.8);
    let left = match align {
        Align::Min => at.x,
        Align::Center => at.x - galley.size().x / 2.0,
        Align::Max => at.x - galley.size().x,
    };
    painter.galley(pos2(left, at.y - base), galley, colour);
}

/// The ink on a lit-hue ground: white in the dark theme, the hue itself in the
/// light, where white would not show.
fn on_hue(hue: Color32) -> Color32 {
    if theme::is_light() {
        hue
    } else {
        Color32::WHITE
    }
}

/// The power button of a built-in: a circle of the hue with its glyph, centred
/// on `centre`. What it is called and what a click does are the plain power
/// button's.
pub(super) fn power(
    ui: &mut Ui,
    centre: Pos2,
    id: egui::Id,
    look: &Look<'_>,
    hue: Color32,
) -> Response {
    let radius = c::POWER_RADIUS;
    let hit = Rect::from_center_size(centre, Vec2::splat(theme::size::CONTROL));
    let response = ui.interact(hit, id.with("power"), Sense::click());
    let on = !look.bypass;
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            on,
            format!("Power {}", look.name),
        )
    });
    let hot = motion::hover(ui.ctx(), id.with("power hot"), response.hovered());
    let painter = ui.painter();
    painter.circle_filled(
        centre,
        radius,
        hue.gamma_multiply(c::POWER_FILL * (1.0 + 0.5 * hot)),
    );
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(c::POWER_RING)),
    );
    painter.extend(icons::shapes(
        Icon::Power,
        Rect::from_center_size(centre, Vec2::splat(radius * 1.2)),
        if on {
            on_hue(hue)
        } else {
            theme::p().text_hint
        },
    ));
    raised::focus_ring(
        ui.painter(),
        &response,
        Rect::from_center_size(centre, Vec2::splat(radius * 2.0)),
    );
    widgets::tip(
        response,
        if on {
            "Bypass this device"
        } else {
            "Turn this device on"
        },
        None,
        None,
    )
}

/// The name in the display face and, under it, what the device is and does.
pub(super) fn titles(painter: &Painter, plan: &Plan, look: &Look<'_>, muted: bool) {
    let palette = theme::p();
    let left = plan.card.left() + c::NAME_X;
    // The compact header's "All n" link sits left of the pill.
    let right = if plan.has_subtitle() {
        plan.preset.left()
    } else {
        plan.link.left()
    };
    let room = right - theme::space::S - left;
    // On the short card the pill is high enough to sit on the name's line only,
    // so the subtitle under it has the card's whole width.
    let subtitle_room = if plan.step == crate::chain::geometry::Step::Short {
        plan.card.right() - c::PRESET_INSET - left
    } else {
        room
    };
    let name_font = fonts::display(plan.name_size);
    let name = widgets::fit(painter, look.name, name_font.clone(), room);
    on_baseline(
        painter,
        pos2(left, plan.card.top() + plan.name_baseline),
        Align::Min,
        name,
        name_font,
        if muted {
            palette.text_secondary
        } else {
            palette.text
        },
        c::NAME_SPACING,
    );
    if plan.has_subtitle() {
        let font = FontId::proportional(c::SUBTITLE_SIZE);
        let subtitle = widgets::fit(painter, look.vendor, font.clone(), subtitle_room);
        on_baseline(
            painter,
            pos2(left, plan.card.top() + c::SUBTITLE_BASELINE),
            Align::Min,
            subtitle,
            font,
            palette.text_secondary,
            0.0,
        );
    }
}

/// The Preset pill at the header's right: it opens the preset menu (load, save,
/// previous, next, the folder). Its name says whose preset it is, as the plain
/// card's field does.
pub(super) fn preset(
    ui: &mut Ui,
    plan: &Plan,
    id: egui::Id,
    look: &Look<'_>,
    hue: Color32,
) -> Response {
    let rect = plan.preset;
    let response = ui.interact(rect, id.with("preset field"), Sense::click());
    let label = format!("Preset of {}", look.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let open = widgets::Menu::is_open_for(ui.ctx(), response.id);
    let hot = motion::hover(ui.ctx(), id.with("preset hot"), response.hovered() || open);
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect_filled(
        rect,
        radius,
        hue.gamma_multiply(c::PRESET_FILL * (1.0 + 0.6 * hot)),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(c::PRESET_RING)),
        StrokeKind::Inside,
    );
    let size = if plan.step == crate::chain::geometry::Step::Compact {
        c::COMPACT_PRESET_TEXT
    } else {
        c::PRESET_TEXT
    };
    on_baseline(
        painter,
        pos2(rect.center().x, rect.center().y + size * 0.36),
        Align::Center,
        "Preset".to_owned(),
        FontId::proportional(size),
        theme::p().text,
        0.0,
    );
    let field = look.field.as_ref().map(|f| (f.name, f.changed));
    if matches!(field, Some((_, true))) {
        // A dot at the pill's corner: the sound was changed since it was chosen.
        painter.circle_filled(
            pos2(rect.right() - 2.0, rect.top() + 2.0),
            3.0,
            theme::p().accent,
        );
    }
    raised::focus_ring(ui.painter(), &response, rect);
    response.on_hover_text(match field {
        Some((Some(name), true)) => format!("{name} (changed). Choose a preset"),
        Some((Some(name), false)) => format!("{name}. Choose a preset"),
        _ => "Choose a preset".to_owned(),
    })
}

/// The quiet "All n \u{203A}" link: a text button that opens every parameter,
/// with search. Returns whether it was clicked.
pub(super) fn all_link(ui: &mut Ui, plan: &Plan, id: egui::Id, count: usize) -> bool {
    let rect = plan.link;
    let response = ui.interact(rect, id.with("all link"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("All parameters ({count})"),
        )
    });
    let hot = motion::hover(ui.ctx(), id.with("all hot"), response.hovered());
    let palette = theme::p();
    let ink = palette.text_secondary.lerp_to_gamma(palette.text, hot);
    let painter = ui.painter();
    on_baseline(
        painter,
        pos2(rect.right() - 8.0, rect.center().y + c::ALL_TEXT * 0.36),
        Align::Max,
        format!("All {}", strip::count_words(count)),
        fonts::medium(c::ALL_TEXT),
        ink,
        0.0,
    );
    // The chevron.
    let tip = pos2(rect.right() - 3.0, rect.center().y);
    painter.add(egui::Shape::line(
        vec![tip + Vec2::new(-3.0, -3.5), tip, tip + Vec2::new(-3.0, 3.5)],
        Stroke::new(theme::stroke::HAIRLINE * 1.3, ink),
    ));
    raised::focus_ring(ui.painter(), &response, rect);
    response
        .on_hover_text("Every parameter, with search")
        .clicked()
}

/// A folded built-in: the hue's bar down the left, glowing with the level, the
/// name in the display face set on end.
pub(super) fn rail(
    painter: &Painter,
    rect: Rect,
    look: &Look<'_>,
    hue: Color32,
    level: f32,
    muted: bool,
) {
    let palette = theme::p();
    let (inset, width) = c::RAIL_BAR;
    let bar = Rect::from_min_max(
        pos2(rect.left() + width, rect.top() + inset),
        pos2(rect.left() + 2.0 * width, rect.bottom() - inset),
    );
    let strength = c::RAIL_BAR_REST + (1.0 - c::RAIL_BAR_REST) * level;
    if level > 0.0 {
        painter.rect_filled(
            bar.expand(2.0 + 3.0 * level),
            3.0,
            hue.gamma_multiply(0.18 * level),
        );
    }
    painter.rect_filled(bar, 1.5, hue.gamma_multiply(strength));
    let font = fonts::display(c::RAIL_NAME_SIZE);
    let ink = if muted {
        palette.text_secondary
    } else {
        palette.text
    };
    let room = rect.height() - 2.0 * (theme::space::M + theme::size::CONTROL) - theme::space::M;
    let shown = widgets::fit(painter, look.name, font.clone(), room);
    let job = LayoutJob::single_section(
        shown,
        TextFormat {
            font_id: font,
            color: ink,
            extra_letter_spacing: c::NAME_SPACING,
            ..Default::default()
        },
    );
    let galley = painter.layout_job(job);
    // Set on end, reading upward, centred on the rail's middle.
    let base = galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first().map(|g| row.pos.y + g.pos.y))
        .unwrap_or(galley.size().y * 0.8);
    let at = pos2(
        rect.left() + c::RAIL_NAME_X + 2.0 * width - base,
        rect.center().y + galley.size().x / 2.0,
    );
    painter.add(
        egui::epaint::TextShape::new(at, galley, ink).with_angle(-std::f32::consts::FRAC_PI_2),
    );
}
