//! The controls that show an icon: the transport-bar button and panel
//! toggle (`button`, `toggle`, `beam`), a window's close button, and the cue
//! button's headphones.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, vec2};
use eframe::emath::GuiRounding as _;

use crate::theme;

use super::{Icon, shapes};

/// Lights a transport button that is sounding now (Play while playing):
/// the `light::BEAM` level's halo around it (ADR-0043). Drawn over the
/// button `button` returned; the button itself gives the icon its core
/// colour and the fill its `accent_muted`.
pub fn beam(ui: &Ui, rect: egui::Rect) {
    crate::light::halo_around(
        ui.painter(),
        crate::light::BEAM,
        rect,
        f32::from(theme::radius::for_height(rect.height())),
        theme::p().accent,
    );
}

/// A transport-bar button showing `icon`, as a piece of the well it sits
/// in (the interface audit's TB1; lay it out in `widgets::well::row`).
/// `active` marks a toggle that is on (or Play while playing, Record while
/// recording). The accessible name is `label`; the tooltip adds the
/// shortcut as keycaps and a hint (`tip`).
///
/// It is bare at rest, a `text_secondary` icon; hover lays Ember over it;
/// pressed it sinks; on it is filled in [`Tone::On`], Play while playing
/// has the icon in `core`, and Record while recording is [`Tone::Record`].
pub fn button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tip: crate::widgets::Tip<'_>,
    active: bool,
    enabled: bool,
    disabled_reason: &str,
) -> Response {
    button_look(
        ui,
        icon,
        label,
        tip,
        Look {
            active,
            enabled,
            disabled_reason,
            ..Look::default()
        },
    )
}

/// How a transport button is in its state and painted (the Record button's
/// states, M5 slice 6d). The fields left at their default keep [`button`]'s
/// look.
#[derive(Debug, Clone, Copy, Default)]
pub struct Look<'a> {
    /// A toggle that is on (Record while recording, Overdub when on).
    pub active: bool,
    pub enabled: bool,
    /// Said after the label when disabled.
    pub disabled_reason: &'a str,
    /// The tone it lights in (default: Record for the record glyph, On for
    /// the rest).
    pub tone: Option<theme::Tone>,
    /// The icon's colour, in place of the face's text.
    pub ink: Option<Color32>,
    /// How lit, 0 to 1, in place of the cross-fade from `active` (a pulse
    /// that is not a state).
    pub lit: Option<f32>,
    /// How its body is drawn.
    pub body: Body,
}

/// How a transport-bar button's body is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Body {
    /// A piece of a sunk well, filled while on.
    #[default]
    Piece,
    /// Bare, and on an accent icon over a short accent underline: a panel
    /// toggle, on most of the time, stays quiet (the interface audit's TB1).
    Underlined,
}

/// A panel toggle at the bar's right: [`button`] with the underlined body,
/// on while its panel is shown.
pub fn toggle(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tip: crate::widgets::Tip<'_>,
    on: bool,
) -> Response {
    button_look(
        ui,
        icon,
        label,
        tip,
        Look {
            active: on,
            enabled: true,
            body: Body::Underlined,
            ..Look::default()
        },
    )
}

/// [`button`] with its state and look given by `look`.
pub fn button_look(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tip: crate::widgets::Tip<'_>,
    look: Look<'_>,
) -> Response {
    let Look {
        active,
        enabled,
        disabled_reason,
        ..
    } = look;
    let size = match look.body {
        Body::Piece => vec2(theme::size::TRANSPORT_ICON, theme::size::WELL_CELL),
        Body::Underlined => Vec2::splat(theme::size::TRANSPORT_ICON),
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label)
    });
    if ui.is_rect_visible(rect) {
        let c = theme::p();
        // Hover and being lit cross-fade (motion: FAST).
        let hot = enabled && (response.hovered() || response.has_focus());
        let lit = look
            .lit
            .unwrap_or_else(|| crate::motion::hover(ui.ctx(), response.id.with("lit"), active));
        let hover = crate::motion::hover(ui.ctx(), response.id.with("hover"), hot);
        let tone = look.tone.unwrap_or(if icon == Icon::Record {
            theme::Tone::Record
        } else {
            theme::Tone::On
        });
        let face = crate::widgets::raised::Face::new(theme::controls(false))
            .hover(hover)
            .pressed(enabled && response.is_pointer_button_down_on())
            .lit(lit, tone)
            .enabled(enabled);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = match look.body {
            Body::Piece => {
                crate::widgets::well::paint_piece(ui.painter(), rect, &face);
                rect
            }
            Body::Underlined => paint_underlined(ui.painter(), rect, &face),
        };
        // Play while playing reads in the core colour, like the playhead.
        let color = if matches!(icon, Icon::Play | Icon::Pause) {
            face.text().lerp_to_gamma(c.core, lit)
        } else {
            look.ink.unwrap_or_else(|| face.text())
        };
        // The icon sits on the body's centre, in whole pixels.
        let icon_rect = Rect::from_center_size(body.center().round(), Vec2::splat(ICON_SIZE));
        for shape in shapes(icon, icon_rect, color) {
            ui.painter().add(shape);
        }
        crate::widgets::raised::focus_ring(ui.painter(), &response, rect);
    }
    if crate::trace::enabled() {
        let released_here = ui.input(|i| i.pointer.primary_released()) && response.hovered();
        if response.clicked() || response.double_clicked() || released_here {
            crate::trace::log(|| {
                format!(
                    "button {label:?}: clicked {} double {} dragged {} enabled {enabled}",
                    response.clicked(),
                    response.double_clicked(),
                    response.dragged() || response.drag_stopped(),
                )
            });
        }
    }
    if let Some(command) = tip.command {
        crate::testing::coverage::drawn(ui.ctx(), label, command);
    }
    let label = if enabled {
        label.to_owned()
    } else {
        format!("{label}. {disabled_reason}")
    };
    crate::widgets::tip(response, &label, tip.keys, tip.hint)
}

/// The icon's side on a transport button: the 16 pt grid of every icon.
const ICON_SIZE: f32 = 16.0;

/// A panel toggle's body: nothing at rest, an Ember wash on hover, the
/// sunk look while pressed, and while on a 2 pt accent line under the icon
/// (the icon itself turns accent through the face). Returns the rect the
/// icon is centred in.
fn paint_underlined(painter: &Painter, rect: Rect, face: &crate::widgets::raised::Face) -> Rect {
    use crate::widgets::raised;
    if !face.enabled {
        return rect;
    }
    let radius = theme::radius::for_height(rect.height());
    if face.pressed {
        return raised::paint(painter, rect, radius, face);
    }
    let c = theme::p();
    if face.hover > 0.0 {
        painter.rect_filled(rect, f32::from(radius), c.ember.gamma_multiply(face.hover));
    }
    if face.lit > 0.0 {
        let y = rect.bottom() - theme::stroke::HAIRLINE - theme::stroke::UNDERLINE / 2.0;
        let half = UNDERLINE / 2.0;
        painter.line_segment(
            [
                Pos2::new(rect.center().x - half, y),
                Pos2::new(rect.center().x + half, y),
            ],
            Stroke::new(theme::stroke::UNDERLINE, c.accent.gamma_multiply(face.lit)),
        );
    }
    rect
}

/// How long a panel toggle's underline is: 10 pt, under the middle of
/// its icon.
const UNDERLINE: f32 = ICON_SIZE - theme::space::XS - theme::space::XXS;

/// The close button's size.
pub const CLOSE_SIZE: f32 = 22.0;

/// A window's close button (ADR-0051): a bare X at rest, and a small raised
/// control on hover that sinks while pressed. The focus ring is 2 pt.
pub fn close_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CLOSE_SIZE), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    if ui.is_rect_visible(rect) {
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let hover = crate::motion::hover(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered() || response.has_focus(),
        );
        let face = crate::widgets::raised::Face::new(theme::controls(false))
            .hover(hover)
            .pressed(response.is_pointer_button_down_on());
        let body = if hover > 0.0 || face.pressed {
            crate::widgets::raised::paint(ui.painter(), rect, theme::radius::SMALL, &face)
        } else {
            rect
        };
        let x = body.shrink(6.5);
        let stroke = Stroke::new(theme::stroke::LIT, face.text());
        ui.painter()
            .line_segment([x.left_top(), x.right_bottom()], stroke);
        ui.painter()
            .line_segment([x.right_top(), x.left_bottom()], stroke);
        crate::widgets::raised::focus_ring(ui.painter(), &response, rect);
    }
    response.on_hover_text("Close (Esc)")
}

/// Headphones, about 11 pt, for a cue button (M4 slice 4): a band over
/// two ear cups.
pub fn draw_headphones(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(theme::stroke::GLYPH_WIDE, color);
    let r = 4.6;
    let band: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            center + vec2(a.cos() * r, a.sin() * r + 1.0)
        })
        .collect();
    painter.add(egui::Shape::line(band, stroke));
    for x in [-r, r] {
        let cup = Rect::from_center_size(center + vec2(x, 3.2), vec2(2.6, 4.4));
        painter.rect_filled(cup, 1.0, color);
    }
}
