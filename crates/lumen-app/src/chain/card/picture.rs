use super::*;

pub(super) fn paint_snapshot(
    ui: &mut Ui,
    rect: Rect,
    look: &Look<'_>,
    texture: &egui::TextureHandle,
    muted: bool,
) {
    let palette = theme::p();
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), look.band));
    let size = texture.size_vec2();
    let (want, have) = (area.width() / area.height(), size.x / size.y);
    let uv = if have > want {
        let share = want / have;
        Rect::from_min_max(
            pos2((1.0 - share) / 2.0, 0.0),
            pos2((1.0 + share) / 2.0, 1.0),
        )
    } else {
        let share = have / want;
        Rect::from_min_max(
            pos2(0.0, (1.0 - share) / 2.0),
            pos2(1.0, (1.0 + share) / 2.0),
        )
    };
    let top = CornerRadius {
        nw: RADIUS,
        ne: RADIUS,
        sw: 0,
        se: 0,
    };
    egui::Image::from_texture(egui::load::SizedTexture::new(texture.id(), size))
        .uv(uv)
        .corner_radius(top)
        .tint(if muted {
            theme::PICTURE_OFF
        } else {
            Color32::WHITE
        })
        .paint_at(ui, area);
    let fade = Rect::from_min_max(
        pos2(area.left(), area.bottom() - FADE.min(area.height())),
        area.max,
    );
    widgets::gradient(ui.painter(), fade, Color32::TRANSPARENT, palette.header);
}

pub(super) fn paint_picture(
    painter: &egui::Painter,
    rect: Rect,
    look: &Look<'_>,
    muted: bool,
    radius: f32,
) {
    let palette = theme::p();
    let base = art::colour(look.identity);
    let (top, ink) = if muted {
        let grey = theme::grey_of(base);
        (palette.raised.lerp_to_gamma(grey, 0.35), palette.text_hint)
    } else {
        (
            palette.raised.lerp_to_gamma(base, 0.7),
            if theme::is_light() {
                base.lerp_to_gamma(Color32::BLACK, 0.45)
            } else {
                base.lerp_to_gamma(Color32::WHITE, 0.55)
            },
        )
    };
    let picture = Rect::from_min_size(rect.min, vec2(rect.width(), look.band + FADE.min(HEADER)));
    raised::gradient(painter, picture, radius, top, palette.header);
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), look.band));
    let clipped = painter.with_clip_rect(area);
    match look.face {
        Face::Failed { .. } => glyph(&clipped, area, Icon::Warning, palette.warning),
        Face::Missing { .. } => {
            let step = 8.0;
            let mut x = area.left() - area.height();
            while x < area.right() {
                clipped.line_segment(
                    [pos2(x, area.bottom()), pos2(x + area.height(), area.top())],
                    Stroke::new(
                        theme::stroke::HAIRLINE,
                        palette.border.gamma_multiply(theme::alpha::EDGE_DIM),
                    ),
                );
                x += step;
            }
            glyph(&clipped, area, Icon::PlugOff, palette.text_hint);
        }
        _ => {
            match crate::builtin::identity(look.identity) {
                Some(personality) => paint_idea(&clipped, area, personality, muted),
                None => {
                    clipped.text(
                        pos2(area.left() + 14.0, area.bottom() - 6.0),
                        Align2::LEFT_BOTTOM,
                        art::monogram(look.name),
                        fonts::display((look.band * 0.62).clamp(18.0, 64.0)),
                        ink,
                    );
                    paint_kind(&clipped, area, look, ink);
                }
            }
        }
    }
}

fn paint_kind(painter: &egui::Painter, area: Rect, look: &Look<'_>, ink: Color32) {
    let kind = if look.instrument {
        Icon::Keys
    } else {
        Icon::Sliders
    };
    let side = (look.band * 0.4).clamp(16.0, 36.0);
    let at = Rect::from_center_size(
        pos2(
            area.right() - 14.0 - side / 2.0,
            area.top() + 14.0 + side / 2.0,
        ),
        vec2(side, side),
    );
    painter.extend(icons::shapes(
        kind,
        at,
        ink.gamma_multiply(theme::alpha::INK_QUIET),
    ));
}

fn paint_idea(
    painter: &egui::Painter,
    area: Rect,
    personality: crate::builtin::Identity,
    muted: bool,
) {
    let mut hue = personality.colour;
    if muted {
        hue = theme::grey_of(hue);
    }
    let top = theme::personality::SCREEN.lerp_to_gamma(hue, theme::personality::SCREEN_TINT);
    raised::gradient(
        painter,
        area,
        f32::from(theme::radius::SMALL),
        top,
        theme::personality::SCREEN,
    );
    painter.rect_stroke(
        area,
        f32::from(theme::radius::SMALL),
        Stroke::new(
            theme::stroke::HAIRLINE,
            hue.gamma_multiply(if muted {
                theme::alpha::EDGE_DIM
            } else {
                theme::alpha::INK_QUIET
            }),
        ),
        StrokeKind::Inside,
    );
    if personality.timing.name == lumen_devices::utility::NAME {
        paint_prism(painter, area, muted);
    }
}

fn paint_prism(painter: &egui::Painter, area: Rect, muted: bool) {
    let height = area.height();
    if height < 24.0 {
        return;
    }
    let centre = area.center().y;
    let inset = (area.width() * 0.07).clamp(8.0, 22.0);
    let half = (height * 0.20).clamp(7.0, 26.0);
    let apex = pos2(area.left() + area.width() * 0.34, centre);
    let foot = pos2(apex.x - half * 0.9, centre + half * 1.25);
    let toe = pos2(apex.x + half * 1.25, centre + half * 1.25);
    painter.line_segment(
        [
            pos2(area.left() + inset, centre),
            pos2(apex.x - half * 0.2, centre),
        ],
        Stroke::new(
            theme::stroke::LIT,
            Color32::WHITE.gamma_multiply(if muted { 0.25 } else { 0.85 }),
        ),
    );
    let from = pos2(apex.x + half * 0.55, centre + half * 0.15);
    let far = area.right() - inset;
    let spread = (height * 0.42).clamp(9.0, 44.0);
    for (index, colour) in theme::personality::PRISM_BEAMS.iter().enumerate() {
        let step = index as f32 - 2.0;
        let colour = if muted {
            theme::grey_of(*colour).gamma_multiply(0.5)
        } else {
            *colour
        };
        painter.line_segment(
            [from, pos2(far, centre + spread * step * 0.5 + half * 0.15)],
            Stroke::new(theme::stroke::FOCUS, colour),
        );
    }
    painter.add(egui::Shape::convex_polygon(
        vec![apex, foot, toe],
        Color32::WHITE.gamma_multiply(if muted { 0.05 } else { 0.12 }),
        Stroke::new(
            theme::stroke::LIT,
            Color32::WHITE.gamma_multiply(if muted { 0.3 } else { 0.8 }),
        ),
    ));
}

pub(super) fn glyph(painter: &egui::Painter, area: Rect, icon: Icon, colour: Color32) {
    let side = (area.height() * 0.4).clamp(16.0, 40.0);
    let rect = Rect::from_center_size(area.center(), vec2(side, side));
    painter.extend(icons::shapes(icon, rect, colour));
}

pub(super) fn has_window_button(look: &Look<'_>) -> bool {
    look.reports_latency && matches!(look.face, Face::Ready)
}

pub(super) fn paint_badge(painter: &egui::Painter, rect: Rect, look: &Look<'_>) {
    let palette = theme::p();
    raised::paint_sunken(painter, rect, theme::radius::for_height(rect.height()));
    let icon = if look.instrument {
        Icon::Keys
    } else {
        Icon::Sliders
    };
    painter.extend(icons::shapes(
        icon,
        Rect::from_center_size(
            rect.center(),
            egui::Vec2::splat(theme::size::button_icon(rect.height())),
        ),
        palette.text_secondary,
    ));
}

pub(super) fn power_button(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>) -> Response {
    let response = ui.interact(rect, id.with("power"), Sense::click());
    let on = !look.bypass;
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            on,
            format!("Power {}", look.name),
        )
    });
    let lit = motion::hover(ui.ctx(), id.with("power lit"), on);
    let face = raised::face_of(ui, &response).lit(lit, theme::Tone::On);
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    ui.painter().extend(icons::shapes(
        Icon::Power,
        Rect::from_center_size(
            body.center(),
            egui::Vec2::splat(theme::size::button_icon(rect.height())),
        ),
        face.text(),
    ));
    raised::focus_ring(ui.painter(), &response, rect);
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
