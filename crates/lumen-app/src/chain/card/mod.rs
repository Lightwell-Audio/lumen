use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Rect, Response, Sense, Shadow, Stroke,
    StrokeKind, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;

use super::art;
use super::geometry::{FOOTER, HEADER, PRESET};
use super::outputs;
use super::sidechain;
use super::strip;
use crate::icons::{self, Icon};
use crate::widgets::raised;
use crate::{fonts, motion, theme, widgets};

pub(super) enum Face<'a> {
    Ready,
    Loading,
    Failed(Failure<'a>),
    Missing {
        saved: usize,
    },
    Safe {
        saved: usize,
    },
}

pub(super) struct Failure<'a> {
    pub words: &'a str,
    pub from: &'a str,
    pub keeps: bool,
}

pub(super) struct Field<'a> {
    pub name: Option<&'a str>,
    pub changed: bool,
}

pub(super) struct Look<'a> {
    pub name: &'a str,
    pub vendor: &'a str,
    pub identity: &'a str,
    pub instrument: bool,
    pub bypass: bool,
    pub selected: bool,
    pub face: Face<'a>,
    pub latency_ms: f32,
    pub reports_latency: bool,
    pub running: bool,
    pub track_color: Color32,
    pub band: f32,
    pub folded: bool,
    pub picture: Option<&'a egui::TextureHandle>,
    pub slots: &'a [Option<strip::Slot>],
    pub parameters: usize,
    pub field: Option<Field<'a>>,
    pub editor: &'a strip::Editor,
    pub learning: bool,
    pub sidechain: Option<&'a sidechain::Look>,
    pub outputs: Option<&'a outputs::Look>,
    pub personal: Option<screen::Personal<'a>>,
    pub compact: bool,
    pub message: std::cell::OnceCell<Option<compact::Message>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Reload,
    ReloadClean,
    Rescan,
    LoadOne,
}

pub(super) struct Out {
    pub body: Response,
    pub power: Response,
    pub field: Option<Response>,
    pub name: Option<Response>,
    pub step: Option<bool>,
    pub action: Option<Action>,
    pub strip: Vec<strip::Output>,
    pub picked: Option<usize>,
    pub sidechain: Option<Response>,
    pub outputs: Option<Response>,
}

const RADIUS: u8 = theme::radius::XL;
const FADE: f32 = 28.0;
const ROWS: usize = 3;

fn track_band(painter: &Painter, header: Rect, top_radius: u8, colour: Color32) {
    const WIDTH: f32 = 3.0;
    let radius = f32::from(top_radius);
    if radius <= WIDTH {
        painter.rect_filled(
            Rect::from_min_size(header.min, vec2(WIDTH, header.height())),
            0.0,
            colour,
        );
        return;
    }
    let inset = |down: f32| radius - (radius * radius - (radius - down).powi(2)).sqrt();
    let first = radius - (2.0 * radius * WIDTH - WIDTH * WIDTH).sqrt();
    let steps = 8;
    let mut points = vec![
        pos2(header.left() + WIDTH, header.bottom()),
        pos2(header.left(), header.bottom()),
    ];
    for step in (0..=steps).rev() {
        let down = first + (radius - first) * step as f32 / steps as f32;
        points.push(pos2(header.left() + inset(down), header.top() + down));
    }
    points.push(pos2(header.left() + WIDTH, header.top() + first));
    painter.add(egui::Shape::convex_polygon(
        points,
        colour,
        egui::Stroke::NONE,
    ));
}

pub(super) fn paint(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>) -> Out {
    let palette = *theme::p();
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let radius = f32::from(RADIUS);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
    let body = ui.interact(rect, id.with("card"), Sense::click_and_drag());
    if look.folded {
        return paint_rail(ui, rect, id, look, body);
    }
    if look.personal.is_some() && matches!(look.face, Face::Ready) {
        return screen::paint(ui, rect, id, look, body);
    }
    if look.compact {
        return compact::paint(ui, rect, id, look, body);
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
    let preset_row = look
        .field
        .is_some()
        .then(|| Rect::from_min_size(header.left_bottom(), vec2(rect.width(), PRESET)));
    let strip = Rect::from_min_max(
        pos2(
            rect.left(),
            preset_row.map_or(header.bottom(), |row| row.bottom()),
        ),
        pos2(rect.right(), footer.top()),
    );

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
    if let Some(row) = preset_row {
        painter.rect_filled(row, 0.0, palette.header);
    }
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
        track_band(&painter, header, top_rounded, look.track_color);
    }

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
    )
    .round_to_pixels(ui.pixels_per_point());
    let mut header_out = Vec::new();
    if has_window_button(look) {
        header_out.extend(strip::editor_button(ui, badge, id, look.editor));
    } else {
        paint_badge(&painter, badge, look);
    }
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

    let (field, step) = match (&look.field, preset_row) {
        (Some(shown), Some(row)) => paint_field(ui, row, id, look, shown),
        _ => (None, None),
    };

    let strip_area = strip.shrink2(vec2(theme::space::M, theme::space::S));
    let key_room = if look.sidechain.is_some() || look.outputs.is_some() {
        sidechain::ROOM
    } else {
        0.0
    };
    let (action, mut strip) = paint_strip(ui, strip_area, id, look, key_room);
    strip.extend(header_out);
    let key_row = {
        let bottom = strip_area.bottom() - strip::BUTTONS - strip::GAP;
        Rect::from_min_max(
            pos2(strip_area.left(), bottom - sidechain::ROOM),
            pos2(strip_area.right(), bottom),
        )
    };
    let ready = matches!(look.face, Face::Ready);
    let key = look
        .sidechain
        .filter(|_| ready)
        .map(|shown| sidechain::paint(ui, key_row, sidechain::chooser_id_of(id), shown, None));
    let outs = look
        .outputs
        .filter(|_| ready)
        .map(|shown| outputs::paint(ui, key_row, outputs::chooser_id_of(id), shown, None));

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
        strip,
        picked: None,
        sidechain: key,
        outputs: outs,
    }
}

fn paint_rail(ui: &mut Ui, rect: Rect, id: egui::Id, look: &Look<'_>, body: Response) -> Out {
    let palette = *theme::p();
    let radius = f32::from(RADIUS);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
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
    painter.rect_filled(
        rect,
        radius,
        if look.personal.is_some() {
            palette.raised
        } else {
            palette.header
        },
    );
    if look.personal.is_some() {
        let power = screen::rail(ui, rect, id, look);
        return paint_rail_edge(ui, rect, look, body, hover, power);
    }
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
    if let Some(personality) = crate::builtin::identity(look.identity) {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + vec2(0.0, radius),
                vec2(3.0, rect.height() - 2.0 * radius),
            ),
            0.0,
            if look.bypass {
                theme::grey_of(personality.colour)
            } else {
                personality.colour
            },
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
        Face::Failed { .. } => palette.warning,
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

    paint_rail_edge(ui, rect, look, body, hover, power)
}

fn paint_rail_edge(
    ui: &mut Ui,
    rect: Rect,
    look: &Look<'_>,
    body: Response,
    hover: f32,
    power: Response,
) -> Out {
    let palette = *theme::p();
    let radius = f32::from(RADIUS);
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(theme::size::SHADOW_ROOM));
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
        field: None,
        name: None,
        step: None,
        strip: Vec::new(),
        picked: None,
        action: None,
        sidechain: None,
        outputs: None,
    }
}

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
        Face::Failed { .. } | Face::Missing { .. } | Face::Safe { .. } => {
            ("bypassed".to_owned(), kept(&ms))
        }
        Face::Ready if !look.running => ("not running".to_owned(), kept(&ms)),
        Face::Ready if look.bypass => ("bypassed".to_owned(), kept(&ms)),
        Face::Ready => (signal.to_owned(), ms),
    }
}

mod body;
pub(super) mod compact;
mod picture;
mod screen;

pub(in crate::chain) use screen::{Personal, gradient_panel, level_of};

use body::{paint_field, paint_strip};
use picture::{has_window_button, paint_badge, paint_picture, paint_snapshot, power_button};

#[cfg(test)]
mod tests;
