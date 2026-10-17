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
    pub track_color: Color32,
    /// The picture's height now (0 when the tray is short).
    pub band: f32,
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
    // The body first, so the power and the buttons sit on top of it.
    let body = ui.interact(rect, id.with("card"), Sense::click());
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
        paint_picture(&painter, rect, look, muted, radius);
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

/// What the footer says on each side: the signal that passes, and what the
/// device holds back (kept, when it is bypassed or not running).
fn footer_words(look: &Look<'_>) -> (String, String) {
    let ms = format!("{:.1} ms", look.latency_ms);
    let signal = if look.instrument {
        "MIDI \u{2192} audio"
    } else {
        "audio \u{2192} audio"
    };
    match look.face {
        Face::Loading => ("starting\u{2026}".to_owned(), String::new()),
        Face::Failed(_) | Face::Missing { .. } => ("bypassed".to_owned(), format!("{ms} kept")),
        Face::Ready if look.bypass => ("bypassed".to_owned(), format!("{ms} kept")),
        Face::Ready => (signal.to_owned(), ms),
    }
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
            &format!("Bypassed, timing kept. {words}"),
            ("Reload", Action::Reload, Some(Icon::Loop)),
        ),
        Face::Missing { saved } => message(
            ui,
            area,
            id,
            "Not installed",
            palette.text,
            &format!(
                "Settings kept ({}). Bypassed, timing kept.",
                kilobytes(saved)
            ),
            ("Rescan plugins", Action::Rescan, None),
        ),
    }
}

fn kilobytes(bytes: usize) -> String {
    format!("{} KB", bytes.div_ceil(1024))
}

/// A title, a line of words and a button, in the strip's place.
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
    let galley = painter.layout(
        words.to_owned(),
        FontId::proportional(theme::size::CAPTION),
        palette.text_secondary,
        area.width(),
    );
    let words_at = pos2(area.left(), area.top() + 20.0);
    let bottom = words_at.y + galley.size().y;
    painter.galley(words_at, galley, palette.text_secondary);
    let (label, action, _icon) = button;
    let size = vec2(
        (label.len() as f32 * 7.0 + 28.0).min(area.width()),
        theme::size::CONTROL,
    );
    let at = pos2(
        area.left(),
        (bottom + theme::space::S).min(area.bottom() - size.y),
    );
    let rect = Rect::from_min_size(at, size).round_to_pixels(ui.pixels_per_point());
    let response = ui.interact(rect, id.with(label), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let face = raised::face_of(ui, &response);
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    ui.painter().text(
        body.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme::size::BODY),
        face.text(),
    );
    raised::focus_ring(ui.painter(), &response, rect);
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
            track_color: Color32::WHITE,
            band: 96.0,
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
    fn saved_settings_are_counted_in_whole_kilobytes_rounded_up() {
        assert_eq!(kilobytes(0), "0 KB");
        assert_eq!(kilobytes(1), "1 KB");
        assert_eq!(kilobytes(14 * 1024), "14 KB");
        assert_eq!(kilobytes(14 * 1024 + 1), "15 KB");
    }
}
