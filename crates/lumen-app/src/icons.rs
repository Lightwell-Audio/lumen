//! Vector icons (owner request, M2 slice 4). Each icon is geometry on a
//! 16 × 16 grid, scaled to the target rectangle when painted, so it is sharp
//! at every UI scale. They are drawn with egui's painter: at icon sizes that
//! is crisp and cheap, and Vello stays reserved for dense canvases (ADR-0018).

use eframe::emath::GuiRounding as _;
use std::f32::consts::{PI, TAU};

use eframe::egui::{
    self, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, vec2,
};
use lumen_project::model::TrackKind;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ToStart,
    Play,
    Pause,
    Stop,
    Record,
    /// Counting in (M5 slice 6d): the record ring, hollow, that pulses.
    RecordRing,
    Loop,
    Metronome,
    /// Follow the playhead: a playhead line with an arrow.
    Follow,
    /// Snapping on: a horseshoe magnet.
    Magnet,
    /// Snapping off: the magnet, struck through.
    MagnetOff,
    /// A locked clip (M3 slice 6e): a padlock.
    Lock,
    /// The command palette (M3 slice 7): a magnifier.
    Search,
    /// A mouse, before a gesture in the info line (M4 slice 0).
    Mouse,
    /// The computer keyboard plays notes (M5 slice 2): a keycap, a rounded
    /// square with a smaller one set up inside it.
    Keycap,
    /// MIDI (M5 slice 2d): a five-pin DIN socket.
    Din,
    /// Monitor In (M5 slice 2 fix): a speaker with one wave.
    Speaker,
    /// Monitor Off: the speaker, struck through.
    SpeakerOff,
    /// Overdub (M5 slice 6d): layers, a stack of three.
    Layers,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 19] = [
        Icon::ToStart,
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::Record,
        Icon::RecordRing,
        Icon::Loop,
        Icon::Metronome,
        Icon::Follow,
        Icon::Magnet,
        Icon::MagnetOff,
        Icon::Lock,
        Icon::Search,
        Icon::Mouse,
        Icon::Keycap,
        Icon::Din,
        Icon::Speaker,
        Icon::SpeakerOff,
        Icon::Layers,
    ];
}

/// Maps the 16-unit icon grid onto a square centred in `rect`.
struct Grid {
    origin: Pos2,
    unit: f32,
}

impl Grid {
    fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height());
        Self {
            origin: rect.center() - Vec2::splat(side / 2.0),
            unit: side / 16.0,
        }
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.unit
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
}

/// The shapes for `icon` in `rect`.
pub fn shapes(icon: Icon, rect: Rect, color: Color32) -> Vec<Shape> {
    let g = Grid::new(rect);
    let stroke = Stroke::new(1.6 * g.unit, color);
    let none = Stroke::NONE;
    match icon {
        Icon::Play => vec![Shape::convex_polygon(
            vec![g.p(4.0, 2.5), g.p(13.5, 8.0), g.p(4.0, 13.5)],
            color,
            none,
        )],
        Icon::Pause => vec![
            Shape::rect_filled(g.rect(3.5, 3.0, 6.5, 13.0), 0.5 * g.unit, color),
            Shape::rect_filled(g.rect(9.5, 3.0, 12.5, 13.0), 0.5 * g.unit, color),
        ],
        Icon::Stop => vec![Shape::rect_filled(
            g.rect(3.5, 3.5, 12.5, 12.5),
            0.75 * g.unit,
            color,
        )],
        Icon::Record => vec![Shape::circle_filled(g.p(8.0, 8.0), 4.75 * g.unit, color)],
        // The ring's outer edge is the filled dot's radius: the stroke is
        // centred 0.8 units inside it.
        Icon::RecordRing => vec![Shape::circle_stroke(g.p(8.0, 8.0), 3.95 * g.unit, stroke)],
        Icon::Layers => vec![
            Shape::closed_line(
                vec![g.p(8.0, 2.0), g.p(14.0, 5.0), g.p(8.0, 8.0), g.p(2.0, 5.0)],
                stroke,
            ),
            Shape::line(vec![g.p(2.0, 8.5), g.p(8.0, 11.5), g.p(14.0, 8.5)], stroke),
            Shape::line(
                vec![g.p(2.0, 11.5), g.p(8.0, 14.5), g.p(14.0, 11.5)],
                stroke,
            ),
        ],
        Icon::ToStart => vec![
            Shape::rect_filled(g.rect(3.0, 3.0, 5.0, 13.0), 0.3 * g.unit, color),
            Shape::convex_polygon(
                vec![g.p(13.0, 3.0), g.p(13.0, 13.0), g.p(5.5, 8.0)],
                color,
                none,
            ),
        ],
        Icon::Loop => {
            // An almost-closed circle with an arrowhead at its end.
            let (centre, radius) = (g.p(8.0, 8.0), 5.0 * g.unit);
            let (start, sweep) = (-PI / 2.0 + 0.55, TAU - 1.1);
            let at = |angle: f32| centre + radius * Vec2::angled(angle);
            let arc: Vec<Pos2> = (0..=32)
                .map(|i| at(start + sweep * i as f32 / 32.0))
                .collect();
            let end_angle = start + sweep;
            let tip = at(end_angle);
            let along = Vec2::angled(end_angle + PI / 2.0); // direction of travel
            let across = Vec2::angled(end_angle);
            let head = vec![
                tip + along * 2.6 * g.unit,
                tip + across * 2.2 * g.unit,
                tip - across * 2.2 * g.unit,
            ];
            vec![
                Shape::line(arc, stroke),
                Shape::convex_polygon(head, color, none),
            ]
        }
        Icon::Metronome => vec![
            Shape::closed_line(
                vec![
                    g.p(6.3, 2.5),
                    g.p(9.7, 2.5),
                    g.p(12.0, 13.5),
                    g.p(4.0, 13.5),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(8.0, 11.0), g.p(12.0, 4.0)], stroke),
            Shape::circle_filled(g.p(10.3, 7.0), 1.3 * g.unit, color),
        ],
        Icon::Magnet | Icon::MagnetOff => {
            // A horseshoe: two legs joined by a half circle on top, with
            // pole tips at the bottom.
            let thick = Stroke::new(2.4 * g.unit, color);
            let mut arch: Vec<Pos2> = (0..=12)
                .map(|k| {
                    let a = PI + PI * k as f32 / 12.0;
                    g.p(8.0 + 4.0 * a.cos(), 7.5 + 4.0 * a.sin())
                })
                .collect();
            // The legs stop short of the tips, so the notch shows at 100%.
            arch.insert(0, g.p(4.0, 10.0));
            arch.push(g.p(12.0, 10.0));
            let mut shapes = vec![
                Shape::line(arch, thick),
                Shape::rect_filled(g.rect(2.6, 11.6, 5.4, 14.4), 0.3 * g.unit, color),
                Shape::rect_filled(g.rect(10.6, 11.6, 13.4, 14.4), 0.3 * g.unit, color),
            ];
            if icon == Icon::MagnetOff {
                shapes.push(Shape::line_segment(
                    [g.p(2.0, 2.0), g.p(14.0, 14.0)],
                    stroke,
                ));
            }
            shapes
        }
        Icon::Lock => vec![
            // A shackle over a solid body.
            Shape::line(
                std::iter::once(g.p(5.0, 8.0))
                    .chain((0..=12).map(|k| {
                        let a = PI + PI * k as f32 / 12.0;
                        g.p(8.0 + 3.0 * a.cos(), 6.5 + 3.5 * a.sin())
                    }))
                    .chain([g.p(11.0, 8.0)])
                    .collect(),
                stroke,
            ),
            Shape::rect_filled(g.rect(3.0, 7.5, 13.0, 14.5), 1.0 * g.unit, color),
        ],
        Icon::Search => vec![
            Shape::circle_stroke(g.p(7.0, 7.0), 4.5 * g.unit, stroke),
            Shape::line_segment([g.p(10.3, 10.3), g.p(13.5, 13.5)], stroke),
        ],
        Icon::Mouse => vec![
            // A rounded body with the buttons' parting line.
            Shape::rect_stroke(
                g.rect(4.0, 1.5, 12.0, 14.5),
                4.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::line_segment([g.p(8.0, 1.5), g.p(8.0, 6.5)], stroke),
        ],
        Icon::Keycap => vec![
            Shape::rect_stroke(
                g.rect(1.5, 2.5, 14.5, 14.5),
                3.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::rect_stroke(
                g.rect(4.5, 4.5, 11.5, 10.0),
                1.5 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
        ],
        Icon::Din => {
            // The shell, and five pins on its upper arc (180 degrees).
            let pin = 1.1 * g.unit;
            let mut shapes = vec![Shape::circle_stroke(g.p(8.0, 8.0), 6.6 * g.unit, stroke)];
            for (x, y) in [(3.6, 9.6), (4.7, 6.0), (8.0, 4.6), (11.3, 6.0), (12.4, 9.6)] {
                shapes.push(Shape::circle_filled(g.p(x, y), pin, color));
            }
            shapes
        }
        Icon::Speaker | Icon::SpeakerOff => {
            // A cone and box, and one wave; Off adds the slash.
            let mut shapes = vec![
                Shape::convex_polygon(
                    vec![
                        g.p(2.0, 6.0),
                        g.p(5.2, 6.0),
                        g.p(9.0, 2.8),
                        g.p(9.0, 13.2),
                        g.p(5.2, 10.0),
                        g.p(2.0, 10.0),
                    ],
                    color,
                    none,
                ),
                Shape::line(
                    (0..=8)
                        .map(|k| {
                            let a = -0.9 + 1.8 * k as f32 / 8.0;
                            g.p(8.4 + 3.6 * a.cos(), 8.0 + 3.6 * a.sin())
                        })
                        .collect(),
                    stroke,
                ),
            ];
            if icon == Icon::SpeakerOff {
                shapes.push(Shape::line_segment(
                    [g.p(2.0, 14.0), g.p(14.0, 2.0)],
                    Stroke::new(1.8 * g.unit, color),
                ));
            }
            shapes
        }
        Icon::Follow => vec![
            Shape::line_segment([g.p(4.0, 2.5), g.p(4.0, 13.5)], stroke),
            Shape::line_segment([g.p(6.5, 8.0), g.p(11.5, 8.0)], stroke),
            Shape::convex_polygon(
                vec![g.p(14.0, 8.0), g.p(10.5, 5.0), g.p(10.5, 11.0)],
                color,
                none,
            ),
        ],
    }
}

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

/// A transport-bar button showing `icon` (the design system's Transport
/// card, ADR-0052). `active` marks a toggle that is on (or Play while
/// playing, Record while recording). The accessible name is `label`; the
/// tooltip adds the shortcut as keycaps and a hint (`tip`).
///
/// It is quiet: no fill and no border at rest, a `text_secondary` icon;
/// hover lays Ember over it; pressed it sinks; on it is [`Tone::On`]
/// (`accent_muted`, accent icon, a 38% hairline), Play while playing has the
/// icon in `core`, and Record while recording is [`Tone::Record`].
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
    let size = Vec2::splat(theme::size::TRANSPORT_ICON);
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
        let radius = theme::radius::for_height(rect.height());
        let body = paint_quiet(ui.painter(), rect, radius, &face);
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
    let label = if enabled {
        label.to_owned()
    } else {
        format!("{label}. {disabled_reason}")
    };
    crate::widgets::tip(response, &label, tip.keys, tip.hint)
}

/// The icon's side on a transport button: the 16 pt grid of every icon.
const ICON_SIZE: f32 = 16.0;

/// A quiet button's body (the Button card's `lm-quiet`): nothing at rest,
/// an Ember wash on hover, the sunk look while pressed, and its `tone`
/// fading in as it lights (no gradient, an `On` hairline; Record also
/// glows). Returns the rect the icon is centred in.
fn paint_quiet(
    painter: &Painter,
    rect: Rect,
    radius: u8,
    face: &crate::widgets::raised::Face,
) -> Rect {
    use crate::widgets::raised;
    if !face.enabled {
        return rect;
    }
    if face.pressed {
        return raised::paint(painter, rect, radius, face);
    }
    let c = theme::p();
    let r = f32::from(radius);
    let on = theme::lit(face.tone);
    if face.lit > 0.0 {
        if let Some(glow) = face.tone.glow() {
            crate::light::glow(painter, crate::light::FLARE, rect, r, glow, face.lit);
        }
        painter.rect_filled(rect, r, on.top.gamma_multiply(face.lit));
    }
    if face.hover > 0.0 {
        painter.rect_filled(rect, r, c.ember.gamma_multiply(face.hover));
    }
    if face.lit > 0.0 {
        painter.rect_stroke(
            rect,
            r,
            Stroke::new(theme::stroke::HAIRLINE, on.edge.gamma_multiply(face.lit)),
            egui::StrokeKind::Inside,
        );
    }
    rect
}

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
        let stroke = Stroke::new(1.5, face.text());
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
    let stroke = Stroke::new(1.4, color);
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

/// A 14 pt icon for the track type: a waveform for audio, keys for MIDI,
/// stacked bars for a group, a return arrow for a return.
pub fn draw_type_icon(painter: &Painter, center: Pos2, kind: TrackKind, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let at = |x: f32, y: f32| center + vec2(x, y);
    match kind {
        TrackKind::Audio => {
            for (x, h) in [(-5.0, 1.0), (-2.5, 3.5), (0.0, 6.0), (2.5, 2.5), (5.0, 1.0)] {
                painter.line_segment([at(x, -h), at(x, h)], stroke);
            }
        }
        TrackKind::Midi => {
            let r = Rect::from_center_size(center, vec2(12.0, 12.0));
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            for x in [-2.0, 2.0] {
                painter.line_segment([at(x, -6.0), at(x, 1.5)], stroke);
            }
        }
        TrackKind::Group | TrackKind::Master => {
            for y in [-4.0, 0.0, 4.0] {
                painter.line_segment([at(-6.0, y), at(6.0, y)], stroke);
            }
        }
        TrackKind::Return => {
            painter.line_segment([at(5.0, -5.0), at(5.0, 1.0)], stroke);
            painter.line_segment([at(5.0, 1.0), at(-5.0, 1.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, -2.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, 4.0)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn every_icon_stays_inside_its_box_at_every_scale() {
        for scale in [1.0, 1.5, 2.0] {
            let rect = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(16.0 * scale));
            for icon in Icon::ALL {
                let shapes = shapes(icon, rect, Color32::WHITE);
                assert!(!shapes.is_empty(), "{icon:?}");
                for shape in shapes {
                    let bounds = shape.visual_bounding_rect();
                    assert!(
                        rect.expand(0.5).contains_rect(bounds),
                        "{icon:?} at {scale}: {bounds:?} outside {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn icons_scale_with_their_box() {
        let small = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        let large = Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0));
        for icon in Icon::ALL {
            let width = |r| {
                shapes(icon, r, Color32::WHITE)
                    .iter()
                    .map(Shape::visual_bounding_rect)
                    .fold(Rect::NOTHING, |a, b| a.union(b))
                    .width()
            };
            let ratio = width(large) / width(small);
            assert!((1.8..2.2).contains(&ratio), "{icon:?}: {ratio}");
        }
    }
}
