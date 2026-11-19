use eframe::egui::{Pos2, Rect, pos2};
use lumen_devices::eq;

pub(super) const VIEW_DB: f32 = 15.0;
pub(super) const LABEL_ROOM: f32 = 96.0;
pub(super) const CHIP_ROOM: f32 = 130.0;

const LEFT: f32 = 26.0;
const RIGHT: f32 = 8.0;
const TOP: f32 = 12.0;
const BOTTOM: f32 = 18.0;
const BARE_SIDE: f32 = 8.0;
const BARE_TOP: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Plot {
    pub area: Rect,
    pub labelled: bool,
    pub chip: bool,
}

impl Plot {
    pub(super) fn of(rect: Rect) -> Self {
        let labelled = rect.height() >= LABEL_ROOM;
        let area = if labelled {
            Rect::from_min_max(
                pos2(rect.left() + LEFT, rect.top() + TOP),
                pos2(rect.right() - RIGHT, rect.bottom() - BOTTOM),
            )
        } else {
            Rect::from_min_max(
                pos2(rect.left() + BARE_SIDE, rect.top() + BARE_TOP),
                pos2(rect.right() - BARE_SIDE, rect.bottom() - BARE_TOP),
            )
        };
        Self {
            area,
            labelled,
            chip: rect.height() >= CHIP_ROOM,
        }
    }

    pub(super) fn x_of_share(&self, share: f32) -> f32 {
        self.area.left() + self.area.width() * share
    }

    pub(super) fn x_of(&self, hz: f32) -> f32 {
        self.x_of_share(eq::axis(hz).clamp(0.0, 1.0))
    }

    pub(super) fn share_of_x(&self, x: f32) -> f32 {
        ((x - self.area.left()) / self.area.width().max(1.0)).clamp(0.0, 1.0)
    }

    pub(super) fn y_free(&self, db: f32) -> f32 {
        self.area.center().y - db / VIEW_DB * self.area.height() * 0.5
    }

    pub(super) fn y_of(&self, db: f32) -> f32 {
        self.y_free(db.clamp(-VIEW_DB, VIEW_DB))
    }

    pub(super) fn db_of_y(&self, y: f32) -> f32 {
        (self.area.center().y - y) / (self.area.height() * 0.5).max(1.0) * VIEW_DB
    }

    pub(super) fn zero(&self) -> f32 {
        self.area.center().y
    }

    pub(super) fn place(&self, hz: f32, db: f32) -> Pos2 {
        pos2(self.x_of(hz), self.y_of(db))
    }
}
