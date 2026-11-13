//! A built-in's personality on its card (M8, the owner's chosen card in
//! `docs/specimens/m8/chosen.html`, "Device faces"): the header tinted with
//! the device's hue, the power button in the hue, the name in the display
//! face with what the device is under it, the kind badge, the footer's "All n"
//! button, the folded rail's bar, and the device's drawing on its deep
//! screen. The contract for a device's drawing is `chain/face/mod.rs`.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Pos2, Shape, Vec2};

use super::*;
use crate::chain::face::{Drawing, Kind, Place, Scene, Values};
use theme::personality as p;

/// What a built-in adds to its card: its hue, and its drawing with the values
/// the drawing reads.
pub(in crate::chain) struct Personal<'a> {
    pub hue: Color32,
    pub drawn: Option<Drawn<'a>>,
}

/// A device's drawing and its values: one slot for each parameter it reads
/// or has a control for.
pub(in crate::chain) struct Drawn<'a> {
    pub drawing: &'static Drawing,
    pub slots: &'a [strip::Slot],
    pub table: &'static [lumen_devices::params::ParamInfo],
}

impl Personal<'_> {
    /// The hue as the card shows it: grey while the device is off.
    pub(super) fn shown(&self, muted: bool) -> Color32 {
        if muted {
            theme::grey_of(self.hue)
        } else {
            self.hue
        }
    }

    /// The drawing is a face: it takes the strip's place.
    pub(super) fn face(&self) -> Option<&Drawn<'_>> {
        self.drawn
            .as_ref()
            .filter(|drawn| drawn.drawing.place == Place::Face)
    }
}

/// `text` at `x` with its baseline on `baseline`, aligned by `align` (left,
/// centre or right); `spacing` is the extra space between letters.
pub(super) fn on_baseline(
    painter: &Painter,
    x: f32,
    baseline: f32,
    align: egui::Align,
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
        egui::Align::Min => x,
        egui::Align::Center => x - galley.size().x / 2.0,
        egui::Align::Max => x - galley.size().x,
    };
    painter.galley(pos2(left, baseline - base), galley, colour);
}

/// The header's tint: the hue fading down from the card's top edge.
pub(super) fn tint(painter: &Painter, header: Rect, top_radius: u8, hue: Color32) {
    raised::gradient(
        painter,
        header,
        f32::from(top_radius),
        hue.gamma_multiply(p::HEAD_TINT),
        Color32::TRANSPARENT,
    );
    painter.hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(theme::stroke::HAIRLINE, theme::p().border),
    );
}

/// The ink on a lit-hue ground: white in the dark theme, the hue itself in
/// the light, where white would not show.
fn on_hue(hue: Color32) -> Color32 {
    if theme::is_light() {
        hue
    } else {
        Color32::WHITE
    }
}

/// The power button of a built-in: a circle of the hue, `centre` the middle
/// of its glyph. Its name and what a click does are the plain power button's.
pub(super) fn power(
    ui: &mut Ui,
    centre: Pos2,
    id: egui::Id,
    look: &Look<'_>,
    hue: Color32,
    radius: f32,
) -> Response {
    let hit = Rect::from_center_size(centre, vec2(theme::size::CONTROL, theme::size::CONTROL));
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
        hue.gamma_multiply(p::POWER_FILL * (1.0 + 0.5 * hot)),
    );
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(p::POWER_RING)),
    );
    let glyph = Rect::from_center_size(centre, Vec2::splat(radius * 1.2));
    painter.extend(icons::shapes(
        Icon::Power,
        glyph,
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

/// Where a header's pieces sit.
pub(super) struct Header {
    /// The header's rectangle.
    pub rect: Rect,
    pub compact: bool,
}

impl Header {
    fn size(&self) -> (f32, f32, f32, f32) {
        if self.compact {
            (
                p::NAME_SIZE_COMPACT,
                p::SUBTITLE_SIZE_COMPACT,
                p::NAME_BASELINE_COMPACT,
                p::SUBTITLE_BASELINE_COMPACT,
            )
        } else {
            (
                p::NAME_SIZE,
                p::SUBTITLE_SIZE,
                p::NAME_BASELINE,
                p::SUBTITLE_BASELINE,
            )
        }
    }

    /// The power button's centre.
    pub(super) fn power_at(&self) -> Pos2 {
        let (radius, drop) = if self.compact {
            (p::POWER_RADIUS_COMPACT, 18.0)
        } else {
            (p::POWER_RADIUS, 24.0)
        };
        let _ = radius;
        pos2(self.rect.left() + 24.0, self.rect.top() + drop)
    }

    /// The power circle's radius.
    pub(super) fn power_radius(&self) -> f32 {
        if self.compact {
            p::POWER_RADIUS_COMPACT
        } else {
            p::POWER_RADIUS
        }
    }

    /// The left edge of the name and the subtitle.
    pub(super) fn text_left(&self) -> f32 {
        self.rect.left() + if self.compact { 38.0 } else { 40.0 }
    }

    /// The kind badge's rectangle.
    pub(super) fn badge(&self) -> Rect {
        let (w, h, _) = if self.compact {
            p::BADGE_COMPACT
        } else {
            p::BADGE
        };
        let top = if self.compact { 12.0 } else { 16.0 };
        Rect::from_min_size(
            pos2(self.rect.right() - 12.0 - w, self.rect.top() + top),
            vec2(w, h),
        )
    }
}

/// The name in the display face, the subtitle, and the badge.
pub(super) fn titles(
    painter: &Painter,
    header: &Header,
    look: &Look<'_>,
    hue: Color32,
    muted: bool,
) {
    let palette = theme::p();
    let (name_size, sub_size, name_base, sub_base) = header.size();
    let badge = header.badge();
    let left = header.text_left();
    let room = badge.left() - theme::space::S - left;
    let name_font = fonts::display(name_size);
    let sub_font = FontId::proportional(sub_size);
    let name = widgets::fit(painter, look.name, name_font.clone(), room);
    on_baseline(
        painter,
        left,
        header.rect.top() + name_base,
        egui::Align::Min,
        name,
        name_font,
        if muted {
            palette.text_secondary
        } else {
            palette.text
        },
        p::NAME_SPACING,
    );
    let sub = widgets::fit(painter, look.vendor, sub_font.clone(), room);
    on_baseline(
        painter,
        left,
        header.rect.top() + sub_base,
        egui::Align::Min,
        sub,
        sub_font,
        palette.text_secondary,
        0.0,
    );
    // The kind badge.
    let (_, _, size) = if header.compact {
        p::BADGE_COMPACT
    } else {
        p::BADGE
    };
    painter.rect_filled(
        badge,
        badge.height() / 2.0,
        hue.gamma_multiply(p::BADGE_FILL),
    );
    painter.rect_stroke(
        badge,
        badge.height() / 2.0,
        Stroke::new(0.75, hue.gamma_multiply(p::BADGE_RING)),
        StrokeKind::Inside,
    );
    on_baseline(
        painter,
        badge.center().x,
        badge.center().y + size * 0.36,
        egui::Align::Center,
        (if look.instrument { "INST" } else { "FX" }).to_owned(),
        FontId::monospace(size),
        if muted {
            palette.text_secondary
        } else {
            on_hue(hue)
        },
        0.0,
    );
}

/// The footer's words for a built-in: the signal and what the device holds
/// back ("audio → audio · 1.5 ms"), or why it is not running.
pub(super) fn footer_line(look: &Look<'_>) -> String {
    let (signal, held) = footer_words(look);
    let held = if look.reports_latency && held.ends_with(" ms") {
        // A whole number of milliseconds loses its decimal, as the specimen
        // has "0 ms".
        held.replace(".0 ms", " ms")
    } else {
        held
    };
    if held.is_empty() {
        signal
    } else {
        format!("{signal} \u{00B7} {held}")
    }
}

/// The footer's "All n ▸" button at its right. Returns whether it was clicked.
pub(super) fn all_button(ui: &mut Ui, footer: Rect, id: egui::Id, count: usize) -> bool {
    let palette = *theme::p();
    let (w, h) = p::ALL_BUTTON;
    let rect = Rect::from_min_size(
        pos2(
            footer.right() - theme::space::M - w,
            footer.center().y - h / 2.0,
        ),
        vec2(w, h),
    )
    .round_to_pixels(ui.pixels_per_point());
    let response = ui.interact(rect, id.with("footer all"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Open all {count} parameters"),
        )
    });
    let face = raised::face_of(ui, &response);
    let body = raised::paint(ui.painter(), rect, theme::radius::SMALL + 1, &face);
    let words = format!("All {}", strip::count_words(count));
    let font = FontId::monospace(p::ALL_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(words, font, palette.text_secondary);
    let arrow = theme::space::XS;
    let total = galley.size().x + theme::space::XS + arrow;
    let left = body.center().x - total / 2.0;
    ui.painter().galley(
        pos2(left, body.center().y - galley.size().y / 2.0),
        galley.clone(),
        palette.text_secondary,
    );
    let tip = pos2(left + total, body.center().y);
    ui.painter().add(Shape::convex_polygon(
        vec![
            tip + vec2(-arrow, -arrow * 0.75),
            tip,
            tip + vec2(-arrow, arrow * 0.75),
        ],
        palette.text_secondary,
        Stroke::NONE,
    ));
    raised::focus_ring(ui.painter(), &response, rect);
    response
        .on_hover_text("Every parameter, with search")
        .clicked()
}

/// A screen, deep and tinted by the device's hue, the same in both themes as
/// a clip is: a vertical wash of the hue fading to `deep`, with a hairline of
/// the hue.
pub(super) fn screen(painter: &Painter, rect: Rect, radius: f32, hue: Color32, deep: Color32) {
    let top = deep.lerp_to_gamma(hue, p::SCREEN_TINT);
    raised::gradient(painter, rect, radius, top, deep);
}

/// A face's screen with its drawing and its controls, in `area` (the card's
/// body between the preset row and the footer). Returns what the controls
/// asked for.
pub(super) fn paint_face(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    drawn: &Drawn<'_>,
    hue: Color32,
    muted: bool,
) -> Vec<strip::Output> {
    let screen_rect = area.shrink2(vec2(theme::space::M, 0.0));
    let painter = ui.painter().clone();
    screen(
        &painter,
        screen_rect,
        p::SCREEN_RADIUS,
        hue,
        drawn.drawing.deep,
    );
    painter.rect_stroke(
        screen_rect,
        p::SCREEN_RADIUS,
        Stroke::new(theme::stroke::HAIRLINE, hue.gamma_multiply(p::SCREEN_RING)),
        StrokeKind::Inside,
    );
    draw(&painter, screen_rect, drawn, hue, muted);
    let mut out = Vec::new();
    for control in drawn.drawing.controls {
        let Some(slot) = drawn.slots.iter().find(|s| s.id == control.param) else {
            continue;
        };
        let cell = (control.place)(screen_rect);
        let id = id.with(("face control", control.param));
        match control.kind {
            Kind::Knob => strip::face_knob(ui, cell, id, slot, control.name, hue, &mut out),
            Kind::Pill => strip::face_pill(ui, cell, id, slot, control.name, hue, &mut out),
        }
    }
    out
}

/// Calls the device's drawing on `rect`, clipped to it.
pub(super) fn draw(painter: &Painter, rect: Rect, drawn: &Drawn<'_>, hue: Color32, muted: bool) {
    (drawn.drawing.draw)(&Scene {
        painter: &painter.with_clip_rect(rect),
        rect,
        hue,
        values: Values {
            slots: drawn.slots,
            table: drawn.table,
        },
        muted,
    });
}

/// The picture of a whole strip card: the device's drawing across the card's
/// top, as wide as the card, its top corners the card's.
pub(super) fn paint_band(painter: &Painter, area: Rect, personal: &Personal<'_>, muted: bool) {
    let hue = personal.shown(muted);
    let clipped = painter.with_clip_rect(area);
    let deep = personal
        .drawn
        .as_ref()
        .map_or(p::SCREEN, |drawn| drawn.drawing.deep);
    // Rounded all round, and taller than the band by the radius, so only the
    // card's top corners show.
    let radius = f32::from(RADIUS);
    screen(
        &clipped,
        Rect::from_min_max(area.min, area.max + vec2(0.0, radius)),
        radius,
        hue,
        deep,
    );
    if let Some(drawn) = &personal.drawn {
        draw(painter, area, drawn, hue, muted);
    }
}

/// A folded built-in: the hue's bar down the left, the power, the name in the
/// display face set on end and the subtitle beside it.
pub(super) fn rail(painter: &Painter, rect: Rect, look: &Look<'_>, hue: Color32, muted: bool) {
    let palette = theme::p();
    let (inset, width) = p::RAIL_BAR;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.left() + width, rect.top() + inset),
            pos2(rect.left() + 2.0 * width, rect.bottom() - inset),
        ),
        1.5,
        hue.gamma_multiply(0.9),
    );
    let centre = rect.center().y;
    let name_font = fonts::display(p::RAIL_NAME_SIZE);
    let sub_font = FontId::monospace(p::RAIL_SUBTITLE_SIZE);
    let ink = if muted {
        palette.text_secondary
    } else {
        palette.text
    };
    let room = rect.height() - 2.0 * (theme::space::M + theme::size::CONTROL) - theme::space::M;
    for (text, font, x, colour, spacing) in [
        (look.name, name_font, p::RAIL_NAME_X, ink, p::NAME_SPACING),
        (
            look.vendor,
            sub_font,
            p::RAIL_SUBTITLE_X,
            palette.text_secondary,
            0.0,
        ),
    ] {
        let shown = widgets::fit(painter, text, font.clone(), room);
        let job = LayoutJob::single_section(
            shown,
            TextFormat {
                font_id: font,
                color: colour,
                extra_letter_spacing: spacing,
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
        let at = pos2(rect.left() + x - base, centre + galley.size().x / 2.0);
        painter.add(
            egui::epaint::TextShape::new(at, galley, colour)
                .with_angle(-std::f32::consts::FRAC_PI_2),
        );
    }
}

/// Where the parts of a whole card sit.
pub(super) struct Areas {
    pub header: Rect,
    pub preset_row: Option<Rect>,
    pub footer: Rect,
    /// Between the preset row and the footer.
    pub strip: Rect,
}

/// A whole built-in card from its header down: the tinted header, the preset
/// row, the strip or the face, the footer and the edge. The shadow, the fill
/// and the picture above were painted by the caller.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_card(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    look: &Look<'_>,
    body: Response,
    hue: Color32,
    hover: f32,
    muted: bool,
    areas: Areas,
) -> Out {
    let palette = *theme::p();
    let radius = f32::from(RADIUS);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
    let header = Header {
        rect: areas.header,
        compact: false,
    };
    let power = power(ui, header.power_at(), id, look, hue, header.power_radius());
    titles(&painter, &header, look, hue, muted);

    let (field, step) = match (&look.field, areas.preset_row) {
        (Some(shown), Some(row)) => paint_field(ui, row, id, look, shown, false),
        _ => (None, None),
    };

    // Between the preset row and the footer: the strip, or the face that
    // replaces it.
    let personal = look.personal.as_ref();
    let mut strip_out = Vec::new();
    let mut action = None;
    match personal.and_then(Personal::face) {
        Some(drawn) => {
            strip_out.extend(paint_face(ui, areas.strip, id, drawn, hue, muted));
        }
        None => {
            let strip_area = areas.strip.shrink2(vec2(theme::space::M, theme::space::S));
            let (pressed, outs) = paint_strip(ui, strip_area, id, look, 0.0);
            action = pressed;
            strip_out.extend(outs);
        }
    }

    // The footer: the signal and what it holds back, and "All n ▸".
    painter.hline(
        areas.footer.x_range(),
        areas.footer.top() + 0.5,
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
    let words = footer_line(look);
    painter.text(
        pos2(
            areas.footer.left() + theme::space::M,
            areas.footer.center().y,
        ),
        Align2::LEFT_CENTER,
        widgets::fit(
            &painter,
            &words,
            FontId::monospace(theme::size::MONO_SMALL),
            areas.footer.width() - 2.0 * theme::space::M - p::ALL_BUTTON.0 - theme::space::S,
        ),
        FontId::monospace(theme::size::MONO_SMALL),
        palette.text_secondary,
    );
    if matches!(look.face, Face::Ready)
        && look.parameters > 0
        && all_button(ui, areas.footer, id, look.parameters)
    {
        strip_out.push(strip::Output::All);
    }

    // The edge: chosen is a 1.5 pt accent line, otherwise the strong edge,
    // and Ember washes a card under the hand.
    painter.rect_filled(
        rect,
        radius,
        palette
            .text
            .gamma_multiply(crate::light::EMBER_WASH * hover * theme::alpha::EDGE),
    );
    let edge = if look.selected {
        Stroke::new(theme::stroke::LIT, palette.accent)
    } else {
        Stroke::new(theme::stroke::HAIRLINE, palette.border_strong)
    };
    painter.rect_stroke(rect, radius, edge, StrokeKind::Inside);
    if body.has_focus() {
        raised::paint_focus(&painter, rect, RADIUS);
    }
    Out {
        body,
        power,
        field,
        name: None,
        step,
        action,
        strip: strip_out,
        sidechain: None,
        outputs: None,
    }
}
