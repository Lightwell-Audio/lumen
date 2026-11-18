use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use lumen_devices::utility as device;

use super::super::Values;

pub(super) const SPECIMEN: Vec2 = Vec2::new(272.0, 136.0);
pub(super) const SPREAD_MOST: f32 = 0.17;
pub(super) const BEAMS: usize = 5;
pub(super) const GLASS: [Vec2; 3] = [
    Vec2::new(0.0, -44.0),
    Vec2::new(-28.0, 36.0),
    Vec2::new(28.0, 36.0),
];
pub(super) const GLASS_FILL_FROM: Vec2 = Vec2::new(-28.0, -40.0);
pub(super) const GLASS_FILL_TO: Vec2 = Vec2::new(28.0, 36.0);

const GLASS_AT: f32 = 0.33;
const LEAST_SCALE: f32 = 0.2;
const LEAST_STROKE_SCALE: f32 = 0.5;
const PAN_SHIFT: f32 = 12.0;
const ENTRY: Vec2 = Vec2::new(-12.0, 12.0);
const EXIT: Vec2 = Vec2::new(13.0, 10.0);
const STEM_LEAST: f32 = 4.0;
const STEM_MORE: f32 = 34.0;
const STEM_LEAN: f32 = 0.02;
const RIGHT_MARGIN: f32 = 6.0;
const GRAB_FROM: f32 = 0.6;
const GRAB_HALF: f32 = 10.0;
const FIT_MARGIN: f32 = 3.0;

pub(super) fn scale(rect: Rect) -> f32 {
    (rect.width() / SPECIMEN.x)
        .min(rect.height() / SPECIMEN.y)
        .clamp(LEAST_SCALE, 1.0)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    rect: Rect,
    s: f32,
    glass: Pos2,
    spread: f32,
    entry: Pos2,
    exit: Pos2,
    root: Pos2,
    length: f32,
    fit: f32,
}

impl Frame {
    pub(super) fn of(values: &Values<'_>, rect: Rect, compact: bool) -> Self {
        let s = scale(rect);
        let pan = (values.plain(device::PAN) / 100.0).clamp(-1.0, 1.0);
        let spread = values.norm(device::WIDTH).clamp(0.0, 1.0) * SPREAD_MOST;
        let bass = values.norm(device::BASS_MONO).clamp(0.0, 1.0);
        let glass = pos2(
            rect.left() + rect.width() * GLASS_AT,
            rect.center().y + pan * PAN_SHIFT * s,
        );
        let exit = glass + EXIT * s;
        let stem = (STEM_LEAST + STEM_MORE * bass) * s;
        let root = pos2(exit.x + stem, exit.y - stem * STEM_LEAN);
        let length = (rect.right() - RIGHT_MARGIN * s - root.x).max(1.0);
        let widest = length * (2.0 * spread).sin();
        let room = (root.y - rect.top()).min(rect.bottom() - root.y) - FIT_MARGIN;
        let fit = if compact && widest > room.max(1.0) {
            room.max(1.0) / widest
        } else {
            1.0
        };
        Self {
            rect,
            s,
            glass,
            spread,
            entry: glass + ENTRY * s,
            exit,
            root,
            length,
            fit,
        }
    }

    pub(super) fn s(&self) -> f32 {
        self.s
    }

    pub(super) fn k(&self) -> f32 {
        self.s.max(LEAST_STROKE_SCALE)
    }

    pub(super) fn spread(&self) -> f32 {
        self.spread
    }

    pub(super) fn entry(&self) -> Pos2 {
        self.entry
    }

    pub(super) fn exit(&self) -> Pos2 {
        self.exit
    }

    pub(super) fn root(&self) -> Pos2 {
        self.root
    }

    pub(super) fn length(&self) -> f32 {
        self.length
    }

    pub(super) fn left(&self) -> f32 {
        self.rect.left()
    }

    pub(super) fn glass_point(&self, corner: usize) -> Pos2 {
        self.glass + GLASS[corner] * self.s
    }

    pub(super) fn glass_fill(&self) -> (Pos2, Pos2) {
        (
            self.glass + GLASS_FILL_FROM * self.s,
            self.glass + GLASS_FILL_TO * self.s,
        )
    }

    pub(super) fn tip(&self, beam: usize) -> Pos2 {
        let angle = (beam as f32 - 2.0) * self.spread;
        self.root + vec2(angle.cos(), angle.sin() * self.fit) * self.length
    }

    pub(super) fn pinned_tip(&self, upper: bool) -> Pos2 {
        let tip = self.tip(if upper { 0 } else { BEAMS - 1 });
        pos2(
            tip.x,
            tip.y
                .clamp(self.rect.top() + GRAB_HALF, self.rect.bottom() - GRAB_HALF),
        )
    }

    pub(super) fn grab(&self, upper: bool) -> Rect {
        let y = self.pinned_tip(upper).y;
        Rect::from_min_max(
            pos2(
                self.rect.left() + self.rect.width() * GRAB_FROM,
                y - GRAB_HALF,
            ),
            pos2(self.rect.right(), y + GRAB_HALF),
        )
        .intersect(self.rect)
    }

    pub(super) fn width_norm_at(&self, pointer_y: f32) -> f32 {
        let reach =
            ((pointer_y - self.root.y).abs() / self.length).clamp(0.0, (2.0 * SPREAD_MOST).sin());
        reach.asin() / (2.0 * SPREAD_MOST)
    }
}
