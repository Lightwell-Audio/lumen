//! Prism's drawing (the utility device): light split by a prism, from the
//! round-2 specimen (`docs/specimens/m8/devices.html`, `prism()`) fitted to
//! the whole card's 72 pt band. A white beam enters, the glass splits it into
//! five coloured beams, and Width spreads them: at 0 % they are one beam (mono),
//! at 130 % they fan as in the specimen, at 200 % they fan half as wide again.
//! The screen reads "width 130%" from the device's value, at its top left (the
//! top beam fans into the right corner at 72 pt). Prism is on the
//! generic strip: Gain, Pan, Width, Bass mono, polarity, swap and the DC block
//! are the strip's knobs and All parameters.

use eframe::egui::{Align2, FontId, Shape, Stroke, pos2};
use lumen_devices::utility as device;

use super::{Drawing, Place, Scene};
use crate::theme::personality as p;

/// The specimen's screen was 136 pt tall; the band is 72, so the glass and
/// the lines scale by what fits.
const SPECIMEN_HEIGHT: f32 = 136.0;
/// How much of the screen's height the specimen's glass took (54 of 136),
/// and the band's share we let it have: the glass is drawn a little larger
/// than a straight scaling so it reads at 72 pt.
const GLASS_SCALE: f32 = 1.2;
/// The glass's stand: how far across the screen it is, as a share.
const GLASS_AT: f32 = 0.36;
/// The glass's half width and its height above and below the beam, in the
/// specimen's points.
const GLASS_HALF: f32 = 16.0;
const GLASS_BELOW: f32 = 28.0;
const GLASS_ABOVE: f32 = 26.0;
/// The beam in starts this far from the screen's left, and stops short of the
/// glass.
const BEAM_IN_FROM: f32 = 8.0;
const BEAM_IN_SHORT: f32 = 14.0;
const BEAM_IN_WIDTH: f32 = 3.0;
const BEAM_IN_ALPHA: f32 = 0.9;
/// The beams out leave this far right of the glass's stand and stop this far
/// from the screen's right.
const BEAMS_FROM: f32 = 10.0;
const BEAMS_TO: f32 = 8.0;
/// The beams' halo and core widths, and the halo's opacity.
const HALO: [(f32, f32); 2] = [(7.0, 0.16), (4.0, 0.3)];
const CORE: f32 = 1.5;
/// The glass: its fill and edge.
const GLASS_FILL: f32 = 0.12;
const GLASS_EDGE: f32 = 1.4;
/// The percent the specimen's fan was drawn at.
const SPECIMEN_WIDTH: f32 = 130.0;

pub(crate) static DRAWING: Drawing = Drawing {
    place: Place::Band,
    deep: p::SCREEN,
    reads: &[device::WIDTH],
    draw,
    controls: &[],
};

fn draw(scene: &Scene<'_>) {
    let (painter, rect) = (scene.painter, scene.rect);
    let width = scene.values.plain(device::WIDTH);
    let scale = (rect.height() / SPECIMEN_HEIGHT * GLASS_SCALE).clamp(0.3, 1.0);
    let centre = rect.center().y;
    let glass_x = rect.left() + rect.width() * GLASS_AT;
    // The beam in.
    painter.line_segment(
        [
            pos2(rect.left() + BEAM_IN_FROM, centre),
            pos2(glass_x - BEAM_IN_SHORT * scale, centre),
        ],
        Stroke::new(
            BEAM_IN_WIDTH * scale.max(0.6),
            scene
                .ink(eframe::egui::Color32::WHITE)
                .gamma_multiply(BEAM_IN_ALPHA),
        ),
    );
    // The beams out: five, fanning with the width.
    let from = pos2(glass_x + BEAMS_FROM * scale, centre);
    let length = rect.right() - BEAMS_TO - from.x;
    for (index, colour) in p::PRISM_BEAMS.iter().enumerate() {
        let angle = (index as f32 - 2.0) * p::PRISM_FAN * width / SPECIMEN_WIDTH;
        let to = pos2(from.x + length, centre + length * angle.tan());
        let colour = scene.ink(*colour);
        for (line, alpha) in HALO {
            painter.line_segment(
                [from, to],
                Stroke::new(line * scale.max(0.6), colour.gamma_multiply(alpha)),
            );
        }
        painter.line_segment([from, to], Stroke::new(CORE, colour));
    }
    // The glass, over the beams.
    let white = scene.ink(eframe::egui::Color32::WHITE);
    painter.add(Shape::convex_polygon(
        vec![
            pos2(glass_x - GLASS_HALF * scale, centre + GLASS_BELOW * scale),
            pos2(glass_x + GLASS_HALF * scale, centre + GLASS_BELOW * scale),
            pos2(glass_x, centre - GLASS_ABOVE * scale),
        ],
        white.gamma_multiply(GLASS_FILL),
        Stroke::new(GLASS_EDGE, white),
    ));
    // The readout, from the device's own value.
    painter.text(
        pos2(
            rect.left() + p::READOUT_INSET,
            rect.top() + p::READOUT_INSET + 2.0,
        ),
        Align2::LEFT_CENTER,
        format!("width {width:.0}%"),
        FontId::monospace(p::READOUT_SIZE),
        white,
    );
}
