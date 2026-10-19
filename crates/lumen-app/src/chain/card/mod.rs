//! One device as a card (design system v3, the owner's C4): a picture that
//! fades into the header, the power, the name and vendor, a kind badge, a
//! parameter strip (eight knobs and the button for every parameter), and a footer with
//! the signal and the time it holds back. A loading, failed or missing
//! device swaps the strip for what happened and what to do.

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

/// What became of the device, as the card shows it.
pub(super) enum Face<'a> {
    Ready,
    /// Its host is starting.
    Loading,
    /// Crashed, hung or refused; the words say what.
    Failed(Failure<'a>),
    /// Not installed; `saved` is the size of the settings kept.
    Missing {
        saved: usize,
    },
    /// Safe mode: not loaded, settings kept (`saved` is their size).
    Safe {
        saved: usize,
    },
}

/// A device that stopped, and what Reload would do about it.
pub(super) struct Failure<'a> {
    /// What happened.
    pub words: &'a str,
    /// The sound Reload starts it from, and how old it is.
    pub from: &'a str,
    /// It has stopped three times in two minutes.
    pub keeps: bool,
}

/// The preset field of a running plugin's card.
pub(super) struct Field<'a> {
    /// The sound chosen, when one was.
    pub name: Option<&'a str>,
    /// Changed since it was chosen.
    pub changed: bool,
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
    /// The eight knobs of the strip (empty when there is nothing to show).
    pub slots: &'a [Option<strip::Slot>],
    /// How many parameters the plugin lists.
    pub parameters: usize,
    /// The preset field, for a running plugin.
    pub field: Option<Field<'a>>,
    /// The plugin's own window: what its button says.
    pub editor: &'a strip::Editor,
    /// Learn is on: the next control touched in the plugin's window takes a
    /// slot.
    pub learning: bool,
    /// The sidechain row of a plugin effect that has a sidechain input.
    pub sidechain: Option<&'a sidechain::Look>,
    /// The outputs row of a plugin instrument that has more than one output.
    pub outputs: Option<&'a outputs::Look>,
}

/// What a card's own buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Reload,
    /// Start it again with no sound of its own.
    ReloadClean,
    Rescan,
    /// Load this one plugin although safe mode is on.
    LoadOne,
}

pub(super) struct Out {
    /// The card as a whole: click chooses it, right-click opens its menu.
    pub body: Response,
    pub power: Response,
    /// The preset field (it opens the menu), and the arrow pressed: true for
    /// next.
    pub field: Option<Response>,
    pub step: Option<bool>,
    pub action: Option<Action>,
    /// What the strip's knobs and button asked for.
    pub strip: Vec<strip::Output>,
    /// The sidechain row's chooser (it opens the menu).
    pub sidechain: Option<Response>,
    /// The outputs row's chooser (it opens the menu).
    pub outputs: Option<Response>,
}

/// A card's corner radius.
const RADIUS: u8 = theme::radius::XL;
/// The picture's fade into the header.
const FADE: f32 = 28.0;
/// The parameter strip's rows.
const ROWS: usize = 3;

/// The track-coloured band down a header's left edge. Where the header's top
/// corner is rounded (a short tray has no picture above it) the band follows
/// the curve, so it never sticks out past the card.
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
    // The arc's inner edge, from where it is WIDTH in from the card's edge
    // down to where the edge is straight.
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
        // The track's colour marks an instrument's header.
        track_band(&painter, header, top_rounded, look.track_color);
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
    )
    .round_to_pixels(ui.pixels_per_point());
    // A running plugin has its window's button where the kind badge was (the
    // picture still says the kind); everything else keeps the badge.
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

    // Between header and footer: the strip, or what replaces it.
    let strip_area = strip.shrink2(vec2(theme::space::M, theme::space::S));
    let key_room = if look.sidechain.is_some() || look.outputs.is_some() {
        sidechain::ROOM
    } else {
        0.0
    };
    let (action, mut strip) = paint_strip(ui, strip_area, id, look, key_room);
    strip.extend(header_out);
    // The sidechain row of an effect or the outputs row of an instrument,
    // between the knobs and the buttons under them.
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
        .map(|shown| sidechain::paint(ui, key_row, sidechain::chooser_id_of(id), shown));
    let outs = look
        .outputs
        .filter(|_| ready)
        .map(|shown| outputs::paint(ui, key_row, outputs::chooser_id_of(id), shown));

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
        field,
        step,
        action,
        strip,
        sidechain: key,
        outputs: outs,
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
        field: None,
        step: None,
        strip: Vec::new(),
        action: None,
        sidechain: None,
        outputs: None,
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
        Face::Failed { .. } | Face::Missing { .. } | Face::Safe { .. } => {
            ("bypassed".to_owned(), kept(&ms))
        }
        Face::Ready if look.bypass => ("bypassed".to_owned(), kept(&ms)),
        Face::Ready => (signal.to_owned(), ms),
    }
}

mod body;
mod picture;

use body::{paint_field, paint_strip};
use picture::{has_window_button, paint_badge, paint_picture, paint_snapshot, power_button};

#[cfg(test)]
mod tests;
