//! The compact card (D2, the owner's A): as wide as a full card, 116 pt
//! tall. A 36 pt header with the power and the window's buttons at the small
//! control's height, one row of the strip's first four knobs at 18 pt, and a
//! 24 pt footer with the signal and latency and "All N", which opens the
//! parameter panel. The preset, Learn and All are also in the card's menu
//! (right-click, or the name). A card that is not running says what happened
//! in the knobs' place, whole, and puts its buttons in the footer; such a
//! card is as tall as its words need.

use super::*;
use crate::chain::geometry::{COMPACT_FOOTER, COMPACT_H, COMPACT_HEADER};

/// The knobs' row starts this far under the header, and sits this far in
/// from the card's sides.
const ROW_TOP: f32 = theme::space::XS;
const ROW_SIDE: f32 = 6.0;
/// A message's words: their line, and the room above and below them.
const LINE: f32 = 14.0;
const WORDS_TOP: f32 = theme::space::S;
const WORDS_BOTTOM: f32 = theme::space::XS;
/// A footer with buttons has this much room above and below them (DESIGN:
/// a control never touches a card's edge).
const BUTTONS_PAD: f32 = 6.0;

/// What a card that is not running (or has lost its sidechain's source)
/// says in its knobs' place.
struct Message {
    title: &'static str,
    warn: bool,
    words: String,
    buttons: Vec<(&'static str, Action)>,
}

fn message_of(look: &Look<'_>) -> Option<Message> {
    match look.face {
        Face::Failed(ref failure) => Some(if failure.keeps {
            Message {
                title: "Keeps stopping",
                warn: true,
                words: format!(
                    "{} It has stopped three times in two minutes. Reload starts it again from {}. Reload without sound starts it clean; the saved sound stays until you save.",
                    failure.words, failure.from
                ),
                buttons: vec![
                    ("Reload without sound", Action::ReloadClean),
                    ("Reload", Action::Reload),
                ],
            }
        } else {
            Message {
                title: "Stopped responding",
                warn: true,
                words: format!(
                    "{} Reload starts it again from {}.",
                    failure.words, failure.from
                ),
                buttons: vec![("Reload", Action::Reload)],
            }
        }),
        Face::Missing { saved } => Some(Message {
            title: "Not installed",
            warn: false,
            words: format!("Settings kept ({}).", body::kilobytes(saved)),
            buttons: vec![("Rescan plugins", Action::Rescan)],
        }),
        Face::Safe { saved } => Some(Message {
            title: "Not loaded \u{00B7} safe mode",
            warn: false,
            words: format!("Its settings ({}) are kept.", body::kilobytes(saved)),
            buttons: vec![("Load this plugin", Action::LoadOne)],
        }),
        Face::Ready
            if look
                .sidechain
                .is_some_and(|s| matches!(s.source, sidechain::Source::Gone)) =>
        {
            Some(Message {
                title: "Source removed",
                warn: true,
                words: "The track that keyed it was deleted, so it hears no sidechain. \
                        Undo brings it back, or choose another source below."
                    .to_owned(),
                buttons: Vec::new(),
            })
        }
        _ => None,
    }
}

/// The width of a footer button with `label`.
fn button_width(painter: &Painter, label: &str) -> f32 {
    let font = FontId::proportional(theme::size::CAPTION);
    painter
        .layout_no_wrap(label.to_owned(), font, Color32::WHITE)
        .size()
        .x
        + 2.0 * theme::space::M
}

/// How the footer's buttons sit: on one row when they fit, else one a row.
fn button_rows(painter: &Painter, buttons: &[(&str, Action)], room: f32) -> usize {
    if buttons.is_empty() {
        return 0;
    }
    let total: f32 = buttons
        .iter()
        .map(|(l, _)| button_width(painter, l))
        .sum::<f32>()
        + theme::space::XS * (buttons.len() - 1) as f32;
    if total <= room { 1 } else { buttons.len() }
}

/// The footer's height: 24 pt, or room for the buttons with air round them.
fn footer_height(rows: usize) -> f32 {
    if rows == 0 {
        COMPACT_FOOTER
    } else {
        let small = theme::size::CONTROL_SMALL;
        rows as f32 * small + (rows - 1) as f32 * theme::space::XS + 2.0 * BUTTONS_PAD
    }
}

/// The words' height at the card's width.
fn words_height(painter: &Painter, words: &str, width: f32) -> f32 {
    let font = FontId::proportional(theme::size::CAPTION);
    let mut job = egui::text::LayoutJob::simple(words.to_owned(), font, Color32::WHITE, width);
    job.wrap.max_rows = usize::MAX;
    painter.layout_job(job).size().y.max(LINE)
}

/// How tall the compact card of `look` is: 116 pt, or what its message
/// needs, whole.
pub(in crate::chain) fn height(ui: &Ui, look: &Look<'_>) -> f32 {
    if look.folded {
        return COMPACT_H;
    }
    let Some(message) = message_of(look) else {
        return COMPACT_H;
    };
    let painter = ui.painter();
    let width = super::super::geometry::CARD_W - 2.0 * theme::space::M;
    let rows = button_rows(painter, &message.buttons, width);
    let need = COMPACT_HEADER
        + WORDS_TOP
        + LINE
        + words_height(painter, &message.words, width)
        + WORDS_BOTTOM
        + footer_height(rows);
    need.max(COMPACT_H).ceil()
}

/// Paints the compact card in `rect`; `body` is the card's own response.
pub(super) fn paint(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>, body: Response) -> Out {
    let palette = *theme::p();
    let pixels = ui.pixels_per_point();
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
    painter.rect_filled(rect, radius, palette.raised);

    let message = message_of(look);
    let side = theme::space::M;
    let rows = message.as_ref().map_or(0, |m| {
        button_rows(&painter, &m.buttons, rect.width() - 2.0 * side)
    });
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), COMPACT_HEADER));
    let footer = Rect::from_min_max(
        pos2(rect.left(), rect.bottom() - footer_height(rows)),
        rect.max,
    );
    painter.rect_filled(
        header,
        CornerRadius {
            nw: RADIUS,
            ne: RADIUS,
            sw: 0,
            se: 0,
        },
        palette.header,
    );
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
    painter.hline(
        footer.x_range(),
        footer.top(),
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
    if look.instrument {
        track_band(&painter, header, RADIUS, look.track_color);
    }

    // The header: power, name and vendor, the window's button.
    let small = theme::size::CONTROL_SMALL;
    let power_rect = Rect::from_center_size(
        pos2(header.left() + side + small / 2.0, header.center().y),
        vec2(small, small),
    )
    .round_to_pixels(pixels);
    let power = power_button(ui, power_rect, id, look);
    let button = Rect::from_center_size(
        pos2(header.right() - side - small / 2.0, header.center().y),
        vec2(small, small),
    )
    .round_to_pixels(pixels);
    let mut strip_out = Vec::new();
    let has_button = if has_window_button(look) {
        strip_out.extend(strip::editor_button(ui, button, id, look.editor));
        true
    } else if message.is_none() {
        paint_badge(&painter, button, look);
        true
    } else {
        false
    };
    let text_left = power_rect.right() + theme::space::S;
    let text_right = if has_button {
        button.left() - theme::space::S
    } else {
        header.right() - side
    };
    let room = text_right - text_left;
    let muted = look.bypass || !matches!(look.face, Face::Ready | Face::Loading);
    let ink = if muted {
        palette.text_secondary
    } else {
        palette.text
    };
    let name_font = fonts::medium(theme::size::BODY);
    painter.text(
        pos2(text_left, header.center().y),
        Align2::LEFT_BOTTOM,
        widgets::fit(&painter, look.name, name_font.clone(), room),
        name_font,
        ink,
    );
    let vendor_font = FontId::proportional(theme::size::CAPTION);
    painter.text(
        pos2(text_left, header.center().y),
        Align2::LEFT_TOP,
        widgets::fit(&painter, look.vendor, vendor_font.clone(), room),
        vendor_font,
        palette.text_secondary,
    );
    // The name opens the card's menu; the preset menu opens under it (from
    // the card's menu, or the command).
    let names = Rect::from_min_max(
        pos2(text_left, header.top() + theme::space::XS),
        pos2(text_right, header.bottom() - theme::space::XS),
    );
    let field = look
        .field
        .is_some()
        .then(|| ui.interact(names, id.with("preset field"), Sense::hover()));
    let name = ui.interact(names, id.with("name"), Sense::click());
    name.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Menu of {}", look.name),
        )
    });
    let name = name.on_hover_text("The device's menu: presets, Learn, every parameter and more");

    // Between header and footer: the knobs, or what replaces them.
    let middle = Rect::from_min_max(header.left_bottom(), pos2(rect.right(), footer.top()));
    let mut action = None;
    match &message {
        Some(message) => {
            let area = middle.shrink2(vec2(side, 0.0));
            let top = area.top() + WORDS_TOP;
            let title_colour = if message.warn {
                palette.warning
            } else {
                palette.text
            };
            painter.text(
                pos2(area.left(), top),
                Align2::LEFT_TOP,
                message.title,
                fonts::medium(theme::size::CAPTION),
                title_colour,
            );
            let font = FontId::proportional(theme::size::CAPTION);
            let mut job = egui::text::LayoutJob::simple(
                message.words.clone(),
                font,
                palette.text_secondary,
                area.width(),
            );
            job.wrap.max_rows = usize::MAX;
            let galley = painter.layout_job(job);
            painter.galley(
                pos2(area.left(), top + LINE),
                galley,
                palette.text_secondary,
            );
            let words = ui.interact(area, id.with("words"), Sense::hover());
            words.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("{}. {}", message.title, message.words),
                )
            });
            action = footer_buttons(
                ui,
                footer.shrink2(vec2(side, BUTTONS_PAD)),
                id,
                rows,
                message,
            );
        }
        None => match look.face {
            Face::Loading => {
                // Waiting: the trough the knobs will stand in.
                let trough = Rect::from_center_size(
                    middle.center(),
                    vec2(middle.width() - 2.0 * side, theme::size::CONTROL),
                );
                raised::paint_sunken(&painter, trough, theme::radius::SMALL);
            }
            Face::Ready if look.slots.is_empty() && look.parameters == 0 => {
                let words = if look.reports_latency {
                    "This plugin lists no parameters. Its own window has them."
                } else {
                    "The sound is chosen from the track's Instrument menu."
                };
                let area = middle.shrink2(vec2(side, 0.0));
                let galley = painter.layout(
                    words.to_owned(),
                    FontId::proportional(theme::size::CAPTION),
                    palette.text_hint,
                    area.width(),
                );
                painter.galley(
                    pos2(area.left(), area.top() + WORDS_TOP),
                    galley,
                    palette.text_hint,
                );
            }
            _ => {
                let row = Rect::from_min_max(
                    pos2(middle.left() + ROW_SIDE, middle.top() + ROW_TOP),
                    pos2(middle.right() - ROW_SIDE, middle.bottom()),
                );
                strip_out.extend(strip::paint_row(ui, row, id, look.slots, look.selected));
            }
        },
    }

    // The footer: a chooser or the signal on the left, "All N" on the right.
    let mono = FontId::monospace(theme::size::MONO_SMALL);
    let (signal, held) = footer_words(look);
    let words = if held.is_empty() {
        signal
    } else {
        format!("{signal} \u{00B7} {held}")
    };
    let inner = footer.shrink2(vec2(side, 0.0));
    let mut right = inner.right();
    if message.is_none() && matches!(look.face, Face::Ready) && look.parameters > 0 {
        let (all, used) = all_word(ui, inner, id, look.parameters);
        right -= used + theme::space::S;
        if all {
            strip_out.push(strip::Output::All);
        }
    }
    let ready = matches!(look.face, Face::Ready);
    let chooser = Rect::from_min_max(pos2(inner.left(), inner.top()), pos2(right, inner.bottom()));
    let key = look.sidechain.filter(|_| ready).map(|shown| {
        sidechain::paint(
            ui,
            chooser,
            sidechain::chooser_id_of(id),
            shown,
            Some(&words),
        )
    });
    let outs = look
        .outputs
        .filter(|_| ready)
        .map(|shown| outputs::paint(ui, chooser, outputs::chooser_id_of(id), shown, Some(&words)));
    if key.is_none() && outs.is_none() && rows == 0 {
        painter.text(
            pos2(inner.left(), inner.center().y),
            Align2::LEFT_CENTER,
            widgets::fit(&painter, &words, mono.clone(), right - inner.left()),
            mono,
            palette.text_secondary,
        );
    }

    // The edge, as a full card's.
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
        field,
        name: Some(name),
        step: None,
        action,
        strip: strip_out,
        sidechain: key,
        outputs: outs,
    }
}

/// "All 873 ▸" at the right of `footer`: the bare word, which opens every
/// parameter. Returns whether it was clicked, and the width it took.
fn all_word(ui: &mut Ui, footer: Rect, id: egui::Id, count: usize) -> (bool, f32) {
    let palette = *theme::p();
    let font = FontId::proportional(theme::size::CAPTION);
    let words = format!("All {}", strip::count_words(count));
    let galley = ui
        .painter()
        .layout_no_wrap(words, font, palette.text_secondary);
    // The arrow is drawn: the interface fonts have no "▸".
    let arrow = theme::space::XS;
    let width = galley.size().x + theme::space::XS + arrow;
    let rect = Rect::from_min_size(
        pos2(
            footer.right() - width,
            footer.center().y - galley.size().y / 2.0,
        ),
        vec2(width, galley.size().y),
    );
    let response = ui.interact(
        rect.expand(theme::space::XXS),
        id.with("all"),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("All parameters ({count})"),
        )
    });
    let hot = motion::hover(ui.ctx(), id.with("all hover"), response.hovered());
    let ink = palette.text_secondary.lerp_to_gamma(palette.text, hot);
    let painter = ui.painter();
    painter.galley(rect.min, galley, ink);
    let tip = pos2(rect.right(), rect.center().y);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip + vec2(-arrow, -arrow * 0.75),
            tip,
            tip + vec2(-arrow, arrow * 0.75),
        ],
        ink,
        Stroke::NONE,
    ));
    raised::focus_ring(painter, &response, rect.expand(theme::space::XXS));
    let clicked = response
        .on_hover_text("Every parameter of the plugin, with search")
        .clicked();
    (clicked, width)
}

/// The message's buttons at the right of `area` (the footer inside its
/// padding), on one row or one a row from the top. Returns the action of
/// the one pressed.
fn footer_buttons(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    rows: usize,
    message: &Message,
) -> Option<Action> {
    let small = theme::size::CONTROL_SMALL;
    let mut pressed = None;
    let mut right = area.right();
    for (i, (label, action)) in message.buttons.iter().enumerate().rev() {
        let width = button_width(ui.painter(), label).min(area.width());
        let row = if rows == 1 { 0 } else { i };
        let top = area.top() + row as f32 * (small + theme::space::XS);
        if rows != 1 {
            right = area.right();
        }
        let rect = Rect::from_min_size(pos2(right - width, top), vec2(width, small))
            .round_to_pixels(ui.pixels_per_point());
        right -= width + theme::space::XS;
        let response = ui.interact(rect, id.with(*label), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *label));
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
            FontId::proportional(theme::size::CAPTION),
            face.text(),
        );
        raised::focus_ring(ui.painter(), &response, rect);
        if response.clicked() {
            pressed = Some(*action);
        }
    }
    pressed
}
