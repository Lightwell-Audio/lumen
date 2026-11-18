use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::chain::geometry::Step;

pub(super) const SPECIMEN: Vec2 = Vec2::new(352.0, 176.0);
pub(super) const COLUMNS: usize = 23;
pub(super) const ROWS: usize = 12;
pub(super) const CELLS: usize = COLUMNS * ROWS;
pub(super) const FOCUS: (usize, usize) = (11, 6);
pub(super) const MIDDLE: (f32, f32) = (11.0, 6.2);
pub(super) const SPREAD: (f32, f32) = (3.3, 2.5);

const LEAST_SCALE: f32 = 0.3;
const LEAST_BODY_SCALE: f32 = 0.4;
const COLUMN_PITCH: f32 = 15.5;
const ROW_TOP: f32 = 22.0;
const ROW_SPAN: f32 = 150.0;
const ROW_CURVE: f32 = 1.35;
const WIDTH_CURVE: f32 = 0.8;
const WIDTH_FAR: f32 = 0.5;
const LIFT: f32 = 40.0;
const LIFT_FAR: f32 = 0.45;

const RING_BASE: f32 = 54.0;
const RING_REACH: f32 = 40.0;
const PULL_RANGE_DB: f32 = 45.0;
const RING_SQUASH: f32 = 0.3;
const RING_DOWN: f32 = 8.0;

pub(super) const BODY: f32 = 15.0;
pub(super) const BODY_UP: f32 = 2.0;
const BODY_GRAB: f32 = 30.0;
const BODY_TRAVEL: f32 = 30.0;
const BODY_PUSH: f32 = 0.8;

const READOUT_WHOLE: (f32, f32, f32, f32, f32) = (14.0, 34.0, 50.0, 66.0, 26.0);
const READOUT_SHORT: (f32, f32, f32, f32, f32) = (10.0, 23.0, 36.0, 52.0, 16.0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Row {
    pub across: f32,
    pub down: f32,
    pub lift: f32,
}

pub(super) fn row_of(j: usize) -> Row {
    let t = j as f32 / (ROWS - 1) as f32;
    Row {
        across: WIDTH_FAR + (1.0 - WIDTH_FAR) * t.powf(WIDTH_CURVE),
        down: ROW_TOP + ROW_SPAN * t.powf(ROW_CURVE),
        lift: LIFT * (LIFT_FAR + (1.0 - LIFT_FAR) * t),
    }
}

pub(super) fn rows() -> [Row; ROWS] {
    std::array::from_fn(row_of)
}

pub(super) fn scale(rect: Rect) -> f32 {
    (rect.width() / SPECIMEN.x)
        .min(rect.height() / SPECIMEN.y)
        .clamp(LEAST_SCALE, 1.0)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    origin: Pos2,
    s: f32,
}

impl Frame {
    pub(super) fn of(rect: Rect) -> Self {
        let s = scale(rect);
        Self {
            origin: rect.center() - SPECIMEN * 0.5 * s,
            s,
        }
    }

    pub(super) fn s(&self) -> f32 {
        self.s
    }

    pub(super) fn body_scale(&self) -> f32 {
        self.s.max(LEAST_BODY_SCALE)
    }

    pub(super) fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.s
    }

    pub(super) fn grid(&self, column: usize, row: &Row, height: f32) -> Pos2 {
        self.at(
            SPECIMEN.x / 2.0 + (column as f32 - MIDDLE.0) * COLUMN_PITCH * row.across,
            row.down - height * row.lift,
        )
    }
}

pub(super) fn focus_row() -> Row {
    row_of(FOCUS.1)
}

pub(super) fn focus_down(height: f32) -> f32 {
    let row = focus_row();
    row.down - height * row.lift
}

pub(super) const GLOW_DOWN: f32 = 10.0;

pub(super) fn middle() -> f32 {
    SPECIMEN.x / 2.0
}

pub(super) fn ring_half(threshold_db: f32) -> f32 {
    RING_BASE + RING_REACH * (-threshold_db / PULL_RANGE_DB)
}

pub(super) fn ring(rect: Rect, threshold_db: f32) -> (Pos2, Vec2) {
    let frame = Frame::of(rect);
    let half = ring_half(threshold_db) * frame.s;
    (
        frame.at(middle(), focus_down(0.0) + RING_DOWN),
        vec2(half, half * RING_SQUASH),
    )
}

pub(super) fn ring_box(rect: Rect, threshold_db: f32) -> Rect {
    let (at, radii) = ring(rect, threshold_db);
    Rect::from_center_size(at, radii * 2.0).intersect(rect)
}

pub(super) fn ring_value_db(pointer: Pos2, rect: Rect) -> f32 {
    let frame = Frame::of(rect);
    let (at, _) = ring(rect, 0.0);
    let away = (pointer - at) / frame.s;
    let reach = away.x.hypot(away.y / RING_SQUASH);
    -(reach - RING_BASE) / RING_REACH * PULL_RANGE_DB
}

pub(super) fn body_box(rect: Rect) -> Rect {
    let frame = Frame::of(rect);
    let halfway = frame.at(middle(), focus_down(0.0) - BODY_UP + BODY_TRAVEL / 2.0);
    Rect::from_center_size(halfway, Vec2::splat(2.0 * BODY_GRAB * frame.s)).intersect(rect)
}

pub(super) fn body_value(pointer: Pos2, rect: Rect) -> f64 {
    f64::from((pointer.y - rect.top()) / (rect.height() * BODY_PUSH))
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Readout {
    pub hero: Pos2,
    pub sub: Pos2,
    pub rest: Pos2,
    pub hero_size: f32,
}

pub(super) fn readout(rect: Rect, step: Step) -> Readout {
    let (inset, hero, sub, rest, hero_size) = if step == Step::Short {
        READOUT_SHORT
    } else {
        READOUT_WHOLE
    };
    let right = rect.right() - inset;
    Readout {
        hero: pos2(right, rect.top() + hero),
        sub: pos2(right, rect.top() + sub),
        rest: pos2(right, rect.top() + rest),
        hero_size,
    }
}
