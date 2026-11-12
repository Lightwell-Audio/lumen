//! The tray's drawings: the chips at either end, the ports, the cables between
//! them, where a dragged device would land, the device that follows the
//! pointer, and where a track's sound goes in words.

use super::*;

/// A chip at either end of the chain.
pub(super) struct Chip {
    pub(super) name: String,
    /// "MIDI", "in" or "out": said in mono beside the name.
    pub(super) kind: &'static str,
    pub(super) colour: Color32,
}

pub(super) fn paint_chip(painter: &egui::Painter, rect: Rect, chip: &Chip) {
    let palette = theme::p();
    let face = raised::Face::new(theme::controls(false));
    raised::paint(painter, rect, theme::radius::FULL, &face);
    painter.rect_filled(
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(8.0, 8.0)),
        2.0,
        chip.colour,
    );
    let mono = FontId::monospace(theme::size::MONO_SMALL);
    let kind = painter.layout_no_wrap(chip.kind.to_owned(), mono.clone(), palette.text_hint);
    let name_room = rect.width() - 24.0 - kind.size().x - 16.0;
    let font = FontId::proportional(theme::size::CAPTION);
    let name = widgets::fit(painter, &chip.name, font.clone(), name_room);
    painter.text(
        pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        font,
        palette.text,
    );
    painter.galley(
        pos2(
            rect.right() - 10.0 - kind.size().x,
            rect.center().y - kind.size().y / 2.0,
        ),
        kind,
        palette.text_hint,
    );
}

/// Where a dragged device would land: an accent line across the tray and a
/// plus on the cable. Neighbours stay put.
pub(super) fn paint_drop_mark(painter: &egui::Painter, at: Pos2, y: Rangef) {
    let palette = theme::p();
    light::vline(
        painter,
        light::LIT,
        at.x,
        y.shrink(theme::space::M),
        palette.accent,
        1.0,
    );
    painter.circle_filled(at, 9.0, palette.accent);
    let stroke = Stroke::new(theme::stroke::LIT, palette.on_accent);
    painter.hline(Rangef::new(at.x - 4.0, at.x + 4.0), at.y, stroke);
    painter.vline(at.x, Rangef::new(at.y - 4.0, at.y + 4.0), stroke);
}

/// The device being dragged, as a small pill that follows the pointer.
pub(crate) fn paint_drag_ghost(ctx: &egui::Context, name: &str) {
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let palette = theme::p();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("device drag ghost"),
    ));
    let font = FontId::proportional(theme::size::BODY);
    let galley = painter.layout_no_wrap(name.to_owned(), font, palette.text);
    let rect = Rect::from_min_size(
        pointer + vec2(12.0, 8.0),
        galley.size() + vec2(2.0 * theme::space::M, theme::space::S),
    );
    painter.rect_filled(rect, theme::radius::MEDIUM, palette.overlay);
    painter.rect_stroke(
        rect,
        theme::radius::MEDIUM,
        Stroke::new(theme::stroke::HAIRLINE, palette.accent),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.min + vec2(theme::space::M, theme::space::S / 2.0),
        galley,
        palette.text,
    );
}

/// A port: a ring on the card's edge, lit with the cable.
pub(super) fn paint_port(painter: &egui::Painter, at: Pos2, beam: f32) {
    let palette = theme::p();
    painter.circle(
        at,
        PORT,
        palette.well,
        Stroke::new(theme::stroke::LIT, palette.border_strong),
    );
    light::dot(painter, light::BEAM, at, PORT - 1.0, palette.accent, beam);
}

/// The cables between the chips and the cards, solid for audio and dotted
/// for the notes into an instrument.
pub(super) fn paint_cables(
    painter: &egui::Painter,
    layout: &Layout,
    origin: Pos2,
    dotted_first: bool,
    beam: f32,
) {
    let palette = theme::p();
    let y = layout.cable_y + origin.y;
    for (i, (from, to)) in layout.cables().into_iter().enumerate() {
        let (from, to) = (from + origin.x, to + origin.x);
        if i == 0 && dotted_first {
            let colour = palette.border_strong.lerp_to_gamma(palette.accent, beam);
            painter.extend(egui::Shape::dotted_line(
                &[pos2(from, y), pos2(to, y)],
                colour,
                5.0,
                1.5,
            ));
        } else {
            painter.hline(
                Rangef::new(from, to),
                y,
                Stroke::new(theme::stroke::FOCUS, palette.border_strong),
            );
            light::hline(
                painter,
                light::BEAM,
                Rangef::new(from, to),
                y,
                palette.accent,
                beam,
            );
        }
    }
}

/// Where a track's sound goes, in words for the output chip.
pub(super) fn output_name(project: &Project, track: &Track) -> String {
    let named = |id: Id| {
        all_tracks(project)
            .find(|t| t.id == id)
            .map(|t| t.name.clone())
    };
    if track.kind == TrackKind::Master {
        // The master plays to a pair of the device (the main one by default).
        return match track.output {
            Output::Device(pair) => format!("Out {}/{}", 2 * pair + 1, 2 * pair + 2),
            _ => "Out 1/2".to_owned(),
        };
    }
    match track.output {
        Output::Master => "Master".to_owned(),
        Output::Track(id) => named(id).unwrap_or_else(|| "Master".to_owned()),
        Output::Device(pair) => format!("Out {}/{}", 2 * pair + 1, 2 * pair + 2),
        Output::Auto => track
            .parent
            .and_then(named)
            .unwrap_or_else(|| "Master".to_owned()),
    }
}
