use eframe::egui::{Pos2, Rect, Vec2, pos2};

use crate::chain::geometry::Step;

pub(super) const SPECIMEN: Vec2 = Vec2::new(272.0, 176.0);
pub(super) const TOP_DB: f32 = 4.0;
pub(super) const BOTTOM_DB: f32 = -36.0;
pub(super) const GRID_DB: [f32; 3] = [-6.0, -12.0, -24.0];
pub(super) const COLUMNS: usize = 137;
pub(super) const TAKE_EVERY: f32 = 0.033;

const GRAB: f32 = 8.0;
const GRAB_INSET: f32 = 14.0;
const LABEL_ROOM: f32 = 14.0;
const READOUT_RIGHT: f32 = 10.0;
const READOUT_WIDTH: f32 = 90.0;
const READOUT_LEFT_OF_TEXT: f32 = 84.0;
const BAR_RIGHT: f32 = 4.0;
const BAR_WIDTH: f32 = 3.0;
const BAR_FOOT: f32 = 3.0;

pub(super) fn y_of(db: f32, rect: Rect) -> f32 {
    rect.top() + rect.height() * (TOP_DB - db) / (TOP_DB - BOTTOM_DB)
}

pub(super) fn level_at(y: f32, rect: Rect) -> f32 {
    TOP_DB - (y - rect.top()) / rect.height() * (TOP_DB - BOTTOM_DB)
}

pub(super) fn depth(rect: Rect) -> f32 {
    (rect.height() / SPECIMEN.y).min(1.0)
}

pub(super) fn line_box(ceiling_db: f32, rect: Rect) -> Rect {
    let y = y_of(ceiling_db, rect);
    let inset = GRAB_INSET.min(rect.width() * 0.25);
    Rect::from_x_y_ranges(
        (rect.left() + inset)..=(rect.right() - inset),
        (y - GRAB)..=(y + GRAB),
    )
    .intersect(rect)
}

pub(super) fn column_step(rect: Rect) -> f32 {
    rect.width() / (COLUMNS - 1) as f32
}

pub(super) fn column_x(rect: Rect, count: usize, index: usize, shift: f32) -> f32 {
    rect.right() + column_step(rect) * (1.0 - shift - (count - 1 - index) as f32)
}

pub(super) fn foot_drop(x: f32, rect: Rect, radius: f32) -> f32 {
    let inset = (x - rect.left()).min(rect.right() - x);
    if radius > 0.0 && inset < radius {
        radius - (radius * radius - (radius - inset).powi(2)).max(0.0).sqrt()
    } else {
        0.0
    }
}

pub(super) fn label_below(y: f32, rect: Rect) -> bool {
    y - rect.top() < LABEL_ROOM
}

pub(super) fn show_grid_words(step: Step) -> bool {
    step == Step::Whole
}

pub(super) struct Readout {
    pub plate: Rect,
    pub hero: Pos2,
    pub sub: Pos2,
    pub hero_size: f32,
}

pub(super) fn readout(rect: Rect, step: Step) -> Readout {
    let right = rect.right() - READOUT_RIGHT;
    let (plate_top, plate_foot, hero_down, sub_down, hero_size) = if step == Step::Short {
        (5.0, 43.0, 23.0, 37.0, 16.0)
    } else {
        (14.0, 58.0, 38.0, 52.0, 20.0)
    };
    Readout {
        plate: Rect::from_min_max(
            pos2(right - READOUT_LEFT_OF_TEXT, rect.top() + plate_top),
            pos2(
                right - READOUT_LEFT_OF_TEXT + READOUT_WIDTH,
                rect.top() + plate_foot,
            ),
        ),
        hero: pos2(right, rect.top() + hero_down),
        sub: pos2(right, rect.top() + sub_down),
        hero_size,
    }
}

pub(super) fn bars(rect: Rect, input_db: f32, output_db: f32) -> (Rect, Rect) {
    let right = rect.right() - BAR_RIGHT;
    let foot = rect.bottom() - BAR_FOOT;
    let at = |db: f32| y_of(db, rect).clamp(rect.top(), foot);
    let output = at(output_db);
    let input = at(input_db.max(output_db));
    (
        Rect::from_min_max(pos2(right - BAR_WIDTH, output), pos2(right, foot)),
        Rect::from_min_max(pos2(right - BAR_WIDTH, input), pos2(right, output)),
    )
}
