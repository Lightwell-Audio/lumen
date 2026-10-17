//! One device as a card (design system v3, the owner's C4): a picture that
//! fades into the header, the power, the name and vendor, a kind badge, a
//! parameter strip (its frame for now; slice 3 fills it), and a footer with
//! the signal and the time it holds back. A loading, failed or missing
//! device swaps the strip for what happened and what to do.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Shadow, Stroke, StrokeKind,
    Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;

use super::art;
use super::geometry::{FOOTER, HEADER};
use crate::icons::{self, Icon};
use crate::widgets::raised;
use crate::{fonts, motion, theme, widgets};

/// What became of the device, as the card shows it.
pub(super) enum Face<'a> {
    Ready,
    /// Its host is starting.
    Loading,
    /// Crashed, hung or refused; the words say what.
    Failed(&'a str),
    /// Not installed; `saved` is the size of the settings kept.
    Missing {
        saved: usize,
    },
}

pub(super) struct Look<'a> {
    pub name: &'a str,
    pub vendor: &'a str,
    /// What the picture is made from: a class id, or a built-in name.
    pub identity: &'a str,
    pub instrument: bool,
    pub bypass: bool,
    pub selected: bool,
    pub face: Face<'a>,
    /// The time it holds back, in milliseconds.
    pub latency_ms: f32,
    /// Whether the device reports a latency (a plugin does; the built-in
    /// synth has none to report, so its footer says nothing of one).
    pub reports_latency: bool,
    pub track_color: Color32,
    /// The picture's height now (0 when the tray is short).
    pub band: f32,
    /// Folded to a strip: the power button and the name, set on end.
    pub folded: bool,
    /// The plugin's own picture, when its bundle has one and the device is
    /// running: it takes the artwork band.
    pub picture: Option<&'a egui::TextureHandle>,
}

/// What a card's own buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Reload,
    Rescan,
}

pub(super) struct Out {
    /// The card as a whole: click chooses it, right-click opens its menu.
    pub body: Response,
    pub power: Response,
    pub action: Option<Action>,
}

/// A card's corner radius.
const RADIUS: u8 = theme::radius::XL;
/// The picture's fade into the header.
const FADE: f32 = 28.0;
/// The parameter strip's rows.
const ROWS: usize = 3;

/// Paints the card in `rect` and answers its pointer. `id` is stable per
/// device.
pub(super) fn paint(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>) -> Out {
    let palette = *theme::p();
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let radius = f32::from(RADIUS);
    let painter = ui.painter().with_clip_rect(rect.expand(24.0));
    // The body first, so the power and the buttons sit on top of it. A press
    // that moves picks the card up.
    let body = ui.interact(rect, id.with("card"), Sense::click_and_drag());
    if look.folded {
        return paint_rail(ui, rect, id, look, body);
    }
    let hover = motion::hover(ui.ctx(), id.with("card hover"), body.hovered());

    painter.add(
        Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: palette.shadow,
        }
        .as_shape(rect, CornerRadius::same(RADIUS)),
    );
    painter.rect_filled(rect, radius, palette.raised);

    let muted = look.bypass || !matches!(look.face, Face::Ready | Face::Loading);
    let header = Rect::from_min_size(
        pos2(rect.left(), rect.top() + look.band),
        vec2(rect.width(), HEADER),
    );
    let footer = Rect::from_min_max(pos2(rect.left(), rect.bottom() - FOOTER), rect.max);
    let strip = Rect::from_min_max(
        pos2(rect.left(), header.bottom()),
        pos2(rect.right(), footer.top()),
    );

    // The header band, then the picture over it: it ends in the header's
    // colour, so it fades into it.
    let top_rounded = if look.band > 0.0 { 0 } else { RADIUS };
    painter.rect_filled(
        header,
        CornerRadius {
            nw: top_rounded,
            ne: top_rounded,
            sw: 0,
            se: 0,
        },
        palette.header,
    );
    if look.band > 0.0 {
        match look.picture {
            Some(texture) if matches!(look.face, Face::Ready | Face::Loading) => {
                paint_snapshot(ui, rect, look, texture, muted);
            }
            _ => paint_picture(&painter, rect, look, muted, radius),
        }
    }
    painter.rect_filled(
        footer,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: RADIUS,
            se: RADIUS,
        },
        palette.header,
    );
    if look.instrument {
        // The track's colour marks an instrument's header.
        painter.rect_filled(
            Rect::from_min_size(header.min, vec2(3.0, header.height())),
            0.0,
            look.track_color,
        );
    }

    // The header: power, name and vendor, the kind badge.
    let power_rect = Rect::from_center_size(
        pos2(
            header.left() + 12.0 + theme::size::CONTROL / 2.0,
            header.center().y,
        ),
        vec2(theme::size::CONTROL, theme::size::CONTROL),
    )
    .round_to_pixels(ui.pixels_per_point());
    let power = power_button(ui, power_rect, id, look);
    let badge = Rect::from_center_size(
        pos2(
            header.right() - 12.0 - theme::size::CONTROL / 2.0,
            header.center().y,
        ),
        vec2(theme::size::CONTROL, theme::size::CONTROL),
    );
    paint_badge(&painter, badge, look);
    let text_left = power_rect.right() + theme::space::S;
    let room = badge.left() - theme::space::S - text_left;
    let ink = if muted {
        palette.text_secondary
    } else {
        palette.text
    };
    let name_font = fonts::medium(theme::size::BODY);
    let name = widgets::fit(&painter, look.name, name_font.clone(), room);
    painter.text(
        pos2(text_left, header.center().y - 1.0),
        Align2::LEFT_BOTTOM,
        name,
        name_font,
        ink,
    );
    let vendor_font = FontId::proportional(theme::size::CAPTION);
    let vendor = widgets::fit(&painter, look.vendor, vendor_font.clone(), room);
    painter.text(
        pos2(text_left, header.center().y + 1.0),
        Align2::LEFT_TOP,
        vendor,
        vendor_font,
        palette.text_secondary,
    );

    // Between header and footer: the strip, or what replaces it.
    let action = paint_strip(
        ui,
        strip.shrink2(vec2(theme::space::M, theme::space::S)),
        id,
        look,
    );

    // The footer: the signal, and the time held back.
    let mono = FontId::monospace(theme::size::MONO_SMALL);
    let (left, right) = footer_words(look);
    painter.text(
        pos2(footer.left() + theme::space::M, footer.center().y),
        Align2::LEFT_CENTER,
        left,
        mono.clone(),
        palette.text_secondary,
    );
    painter.text(
        pos2(footer.right() - theme::space::M, footer.center().y),
        Align2::RIGHT_CENTER,
        right,
        mono,
        palette.text_secondary,
    );

    // The edge: chosen is a 1.5 pt accent line, otherwise the strong edge,
    // and Ember washes a card under the hand.
    painter.rect_filled(
        rect,
        radius,
        palette
            .text
            .gamma_multiply(crate::light::EMBER_WASH * hover * 0.5),
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
        action,
    }
}

/// A folded card: a strip with the power button and the name set on end,
/// reading upward.
fn paint_rail(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>, body: Response) -> Out {
    let palette = *theme::p();
    let radius = f32::from(RADIUS);
    let painter = ui.painter().with_clip_rect(rect.expand(24.0));
    let hover = motion::hover(ui.ctx(), id.with("card hover"), body.hovered());
    painter.add(
        Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: palette.shadow,
        }
        .as_shape(rect, CornerRadius::same(RADIUS)),
    );
    painter.rect_filled(rect, radius, palette.header);
    if look.instrument {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + vec2(0.0, radius),
                vec2(3.0, rect.height() - 2.0 * radius),
            ),
            0.0,
            look.track_color,
        );
    }
    let power_rect = Rect::from_center_size(
        pos2(
            rect.center().x,
            rect.top() + theme::space::M + theme::size::CONTROL / 2.0,
        ),
        vec2(theme::size::CONTROL, theme::size::CONTROL),
    )
    .round_to_pixels(ui.pixels_per_point());
    let power = power_button(ui, power_rect, id, look);

    let ink = match look.face {
        Face::Failed(_) => palette.warning,
        _ if look.bypass || matches!(look.face, Face::Missing { .. }) => palette.text_secondary,
        _ => palette.text,
    };
    let font = fonts::medium(theme::size::BODY);
    let room = rect.bottom() - theme::space::M - (power_rect.bottom() + theme::space::M);
    let shown = widgets::fit(&painter, look.name, font.clone(), room);
    let galley = painter.layout_no_wrap(shown, font, ink);
    let at = pos2(
        rect.center().x - galley.size().y / 2.0,
        rect.bottom() - theme::space::M,
    );
    painter.add(
        egui::epaint::TextShape::new(at, galley, ink).with_angle(-std::f32::consts::FRAC_PI_2),
    );

    painter.rect_filled(
        rect,
        radius,
        palette
            .text
            .gamma_multiply(crate::light::EMBER_WASH * hover * 0.5),
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
        action: None,
    }
}

/// What the footer says on each side: the signal that passes, and what the
/// device holds back (kept, when it is bypassed or not running).
/// "2.7 ms kept", or nothing for a device with no latency to keep.
fn kept(ms: &str) -> String {
    if ms.is_empty() {
        String::new()
    } else {
        format!("{ms} kept")
    }
}

fn footer_words(look: &Look<'_>) -> (String, String) {
    let ms = if look.reports_latency {
        format!("{:.1} ms", look.latency_ms)
    } else {
        String::new()
    };
    let signal = if look.instrument {
        "MIDI \u{2192} audio"
    } else {
        "audio \u{2192} audio"
    };
    match look.face {
        Face::Loading => ("starting\u{2026}".to_owned(), String::new()),
        Face::Failed(_) | Face::Missing { .. } => ("bypassed".to_owned(), kept(&ms)),
        Face::Ready if look.bypass => ("bypassed".to_owned(), kept(&ms)),
        Face::Ready => (signal.to_owned(), ms),
    }
}

/// The plugin's own picture in the band, cropped to fill it and fading into
/// the header; greyed while the device is bypassed.
fn paint_snapshot(
    ui: &mut Ui,
    rect: Rect,
    look: &Look<'_>,
    texture: &egui::TextureHandle,
    muted: bool,
) {
    let palette = theme::p();
    // Above the header, so the name stays on its own colour; the picture
    // fades into the header's colour at its foot.
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), look.band));
    // Cover: the largest centred part of the picture with the band's shape.
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
            Color32::from_gray(110)
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

fn paint_picture(painter: &egui::Painter, rect: Rect, look: &Look<'_>, muted: bool, radius: f32) {
    let palette = theme::p();
    let base = art::colour(look.identity);
    let (top, ink) = if muted {
        let grey = Color32::from_gray(
            ((u16::from(base.r()) + u16::from(base.g()) + u16::from(base.b())) / 3) as u8,
        );
        (palette.raised.lerp_to_gamma(grey, 0.35), palette.text_hint)
    } else {
        (
            palette.raised.lerp_to_gamma(base, 0.7),
            // Light ink on a dark picture, dark ink on a pale one.
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
        Face::Failed(_) => glyph(&clipped, area, Icon::Warning, palette.warning),
        Face::Missing { .. } => {
            // A hatch: the picture is not there.
            let step = 8.0;
            let mut x = area.left() - area.height();
            while x < area.right() {
                clipped.line_segment(
                    [pos2(x, area.bottom()), pos2(x + area.height(), area.top())],
                    Stroke::new(1.0, palette.border.gamma_multiply(0.6)),
                );
                x += step;
            }
            glyph(&clipped, area, Icon::PlugOff, palette.text_hint);
        }
        _ => {
            clipped.text(
                pos2(area.left() + 14.0, area.bottom() - 6.0),
                Align2::LEFT_BOTTOM,
                art::monogram(look.name),
                fonts::display((look.band * 0.62).clamp(18.0, 64.0)),
                ink,
            );
            // The kind again, large and quiet, at the picture's right.
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
            clipped.extend(icons::shapes(kind, at, ink.gamma_multiply(0.55)));
        }
    }
}

/// An icon centred in the picture.
fn glyph(painter: &egui::Painter, area: Rect, icon: Icon, colour: Color32) {
    let side = (area.height() * 0.4).clamp(16.0, 40.0);
    let rect = Rect::from_center_size(area.center(), vec2(side, side));
    painter.extend(icons::shapes(icon, rect, colour));
}

/// The kind badge: a keyboard for an instrument, sliders for an effect.
fn paint_badge(painter: &egui::Painter, rect: Rect, look: &Look<'_>) {
    let palette = theme::p();
    raised::paint_sunken(painter, rect, theme::radius::for_height(rect.height()));
    let icon = if look.instrument {
        Icon::Keys
    } else {
        Icon::Sliders
    };
    painter.extend(icons::shapes(
        icon,
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        palette.text_secondary,
    ));
}

/// The power button: lit while the device is on. Its name says what a click
/// does.
fn power_button(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>) -> Response {
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
        Rect::from_center_size(body.center(), vec2(16.0, 16.0)),
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

/// The strip's rows, or the message of a card that is not running; returns
/// the button pressed, if any.
fn paint_strip(ui: &mut Ui, area: Rect, id: egui::Id, look: &Look<'_>) -> Option<Action> {
    let palette = *theme::p();
    let painter = ui.painter().with_clip_rect(area.expand(2.0));
    match look.face {
        Face::Ready | Face::Loading => {
            let loading = matches!(look.face, Face::Loading);
            let gap = 6.0;
            let row = ((area.height() - gap * (ROWS as f32 - 1.0)) / ROWS as f32)
                .clamp(0.0, theme::size::CONTROL);
            for i in 0..ROWS {
                let top = area.top() + i as f32 * (row + gap);
                let r = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), row));
                raised::paint_sunken(&painter, r, theme::radius::SMALL);
                if !loading && !look.bypass {
                    painter.text(
                        r.left_center() + vec2(theme::space::S, 0.0),
                        Align2::LEFT_CENTER,
                        "\u{2014}",
                        FontId::proportional(theme::size::CAPTION),
                        palette.text_disabled,
                    );
                }
            }
            None
        }
        Face::Failed(words) => message(
            ui,
            area,
            id,
            "Stopped responding",
            palette.warning,
            words,
            ("Reload", Action::Reload, Some(Icon::Loop)),
        ),
        Face::Missing { saved } => message(
            ui,
            area,
            id,
            "Not installed",
            palette.text,
            &format!("Settings kept ({}).", kilobytes(saved)),
            ("Rescan plugins", Action::Rescan, None),
        ),
    }
}

fn kilobytes(bytes: usize) -> String {
    format!("{} KB", bytes.div_ceil(1024))
}

/// A title, the words and a button, in the strip's place. The button is
/// pinned to the strip's bottom; the words take the room between, end in
/// "\u{2026}" when they do not fit, and are whole in the tooltip.
fn message(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    title: &str,
    title_colour: Color32,
    words: &str,
    button: (&str, Action, Option<Icon>),
) -> Option<Action> {
    let palette = *theme::p();
    let painter = ui.painter().with_clip_rect(area);
    painter.text(
        area.left_top(),
        Align2::LEFT_TOP,
        title,
        fonts::medium(theme::size::BODY),
        title_colour,
    );
    let (label, action, _icon) = button;
    let size = vec2(
        (label.len() as f32 * 7.0 + 28.0).min(area.width()),
        theme::size::CONTROL,
    );
    let button_rect = Rect::from_min_size(pos2(area.left(), area.bottom() - size.y), size)
        .round_to_pixels(ui.pixels_per_point());

    // The words, in the room above the button.
    let font = FontId::proportional(theme::size::CAPTION);
    let words_top = area.top() + 20.0;
    let room = button_rect.top() - theme::space::XS - words_top;
    let line = painter.fonts_mut(|f| f.row_height(&font));
    let mut job =
        egui::text::LayoutJob::simple(words.to_owned(), font, palette.text_secondary, area.width());
    job.wrap.max_rows = ((room / line).floor() as usize).max(1);
    let galley = painter.layout_job(job);
    let words_rect = Rect::from_min_size(pos2(area.left(), words_top), galley.size());
    painter.galley(words_rect.min, galley, palette.text_secondary);
    let hover = ui.interact(
        Rect::from_min_max(
            pos2(area.left(), area.top()),
            pos2(area.right(), button_rect.top()),
        ),
        id.with("words"),
        Sense::hover(),
    );
    hover.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{title}. {words}"))
    });
    hover.on_hover_text(words);

    let response = ui.interact(button_rect, id.with(label), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let face = raised::face_of(ui, &response);
    let body = raised::paint(
        ui.painter(),
        button_rect,
        theme::radius::for_height(button_rect.height()),
        &face,
    );
    ui.painter().text(
        body.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme::size::BODY),
        face.text(),
    );
    raised::focus_ring(ui.painter(), &response, button_rect);
    response.clicked().then_some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(face: Face<'_>, bypass: bool, instrument: bool) -> Look<'_> {
        Look {
            name: "Tape Delay",
            vendor: "Vendor D",
            identity: "x",
            instrument,
            bypass,
            selected: false,
            face,
            latency_ms: 2.7,
            reports_latency: true,
            track_color: Color32::WHITE,
            band: 96.0,
            folded: false,
            picture: None,
        }
    }

    #[test]
    fn the_footer_says_the_signal_or_what_was_kept() {
        assert_eq!(
            footer_words(&look(Face::Ready, false, true)),
            ("MIDI \u{2192} audio".into(), "2.7 ms".into())
        );
        assert_eq!(
            footer_words(&look(Face::Ready, false, false)),
            ("audio \u{2192} audio".into(), "2.7 ms".into())
        );
        assert_eq!(
            footer_words(&look(Face::Ready, true, false)),
            ("bypassed".into(), "2.7 ms kept".into())
        );
        assert_eq!(
            footer_words(&look(Face::Failed("x"), false, false)).0,
            "bypassed"
        );
        assert_eq!(
            footer_words(&look(Face::Loading, false, false)).0,
            "starting\u{2026}"
        );
    }

    #[test]
    fn a_device_with_no_latency_to_report_says_nothing_of_one() {
        let mut quiet = look(Face::Ready, false, true);
        quiet.reports_latency = false;
        assert_eq!(
            footer_words(&quiet),
            ("MIDI \u{2192} audio".into(), String::new())
        );
        let mut bypassed = look(Face::Ready, true, true);
        bypassed.reports_latency = false;
        assert_eq!(footer_words(&bypassed), ("bypassed".into(), String::new()));
    }

    #[test]
    fn saved_settings_are_counted_in_whole_kilobytes_rounded_up() {
        assert_eq!(kilobytes(0), "0 KB");
        assert_eq!(kilobytes(1), "1 KB");
        assert_eq!(kilobytes(14 * 1024), "14 KB");
        assert_eq!(kilobytes(14 * 1024 + 1), "15 KB");
    }
}
