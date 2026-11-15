use eframe::egui::Pos2;
use lumen_project::edit::steps::{Cell, Change, STEP_VELOCITY, Target};

use super::geometry::{INSET, ROW};

pub(crate) const DRAG_THRESHOLD: f32 = 3.0;

const PER_POINT: f32 = 127.0 / (ROW - 2.0 * INSET);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Gesture {
    pub target: Target,
    pub row: usize,
    pub column: usize,
    origin: Pos2,

    base: u8,

    was_on: bool,

    was_accent: bool,

    accent_modifier: bool,
    moved: bool,
    velocity: u8,
}

impl Gesture {
    pub fn press(
        target: Target,
        row: usize,
        column: usize,
        cell: &Cell,
        origin: Pos2,
        accent_modifier: bool,
        accent: u8,
    ) -> Self {
        let base = if cell.on {
            cell.velocity
        } else {
            STEP_VELOCITY
        };
        Self {
            target,
            row,
            column,
            origin,
            base,
            was_on: cell.on,
            was_accent: cell.on && cell.velocity >= accent,
            accent_modifier,
            moved: false,
            velocity: base,
        }
    }

    pub fn drag(&mut self, p: Pos2) -> Option<u8> {
        if !self.moved && (p.y - self.origin.y).abs() < DRAG_THRESHOLD {
            return None;
        }
        self.moved = true;
        let delta = (self.origin.y - p.y) * PER_POINT;
        self.velocity = (f32::from(self.base) + delta).round().clamp(1.0, 127.0) as u8;
        Some(self.velocity)
    }

    pub fn is_drag(&self) -> bool {
        self.moved
    }

    pub fn velocity(&self) -> u8 {
        self.velocity
    }

    pub fn release(&self, accent: u8) -> Change {
        if self.moved {
            return Change::Set(self.target, self.velocity);
        }
        if self.accent_modifier {
            return accent_toggle(self.target, self.was_on, self.was_accent, accent);
        }
        Change::Toggle(self.target, STEP_VELOCITY)
    }
}

pub(crate) fn accent_toggle(target: Target, on: bool, accented: bool, accent: u8) -> Change {
    if on && accented {
        Change::Set(target, STEP_VELOCITY)
    } else {
        Change::Set(target, accent.clamp(1, 127))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Move {
    Left,
    Right,
    Up,
    Down,

    Home,

    End,
}

pub(crate) fn step_cursor(
    cursor: (usize, i64),
    movement: Move,
    rows: usize,
    in_clip: i64,
) -> (usize, i64) {
    let (row, index) = cursor;
    let last_row = rows.saturating_sub(1);
    let last_index = (in_clip - 1).max(0);
    match movement {
        Move::Left => (row, (index - 1).max(0)),
        Move::Right => (row, (index + 1).min(last_index)),
        Move::Up => (row.saturating_sub(1), index),
        Move::Down => ((row + 1).min(last_row), index),
        Move::Home => (row, 0),
        Move::End => (row, last_index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn target() -> Target {
        Target {
            pitch: 36,
            index: 4,
        }
    }

    fn on(velocity: u8) -> Cell {
        Cell {
            on: true,
            velocity,
            hits: 1,
            ..Cell::default()
        }
    }

    #[test]
    fn a_click_toggles_and_a_shift_click_toggles_the_accent() {
        let origin = pos2(100.0, 100.0);
        let off = Cell::default();
        let click = Gesture::press(target(), 0, 4, &off, origin, false, 127);
        assert_eq!(
            click.release(127),
            Change::Toggle(target(), STEP_VELOCITY),
            "a click on an off step"
        );
        let on_click = Gesture::press(target(), 0, 4, &on(80), origin, false, 127);
        assert_eq!(
            on_click.release(127),
            Change::Toggle(target(), STEP_VELOCITY),
            "the toggle takes an on step away whatever the velocity"
        );

        let off_shift = Gesture::press(target(), 0, 4, &off, origin, true, 127);
        assert_eq!(off_shift.release(127), Change::Set(target(), 127));
        let plain_shift = Gesture::press(target(), 0, 4, &on(100), origin, true, 127);
        assert_eq!(plain_shift.release(127), Change::Set(target(), 127));
        let accent_shift = Gesture::press(target(), 0, 4, &on(127), origin, true, 127);
        assert_eq!(
            accent_shift.release(127),
            Change::Set(target(), STEP_VELOCITY)
        );

        let softer = Gesture::press(target(), 0, 4, &on(112), origin, true, 112);
        assert_eq!(softer.release(112), Change::Set(target(), STEP_VELOCITY));
    }

    #[test]
    fn a_vertical_drag_sets_the_velocity_and_is_not_a_click() {
        let origin = pos2(100.0, 100.0);
        let mut g = Gesture::press(target(), 0, 4, &on(60), origin, false, 127);
        assert_eq!(g.drag(pos2(100.0, 101.0)), None, "under the threshold");
        assert!(!g.is_drag());
        let louder = g.drag(pos2(100.0, 90.0)).expect("a drag");
        assert!(louder > 60, "up is louder: {louder}");
        assert!(g.is_drag());
        assert_eq!(g.release(127), Change::Set(target(), louder));

        let back = g.drag(pos2(100.0, 100.0)).expect("still a drag");
        assert_eq!(back, 60);
        assert_eq!(g.release(127), Change::Set(target(), 60));

        assert_eq!(g.drag(pos2(100.0, -400.0)), Some(127));
        assert_eq!(g.drag(pos2(100.0, 600.0)), Some(1));
    }

    #[test]
    fn a_drag_from_an_off_step_starts_at_a_clicks_velocity() {
        let origin = pos2(0.0, 50.0);
        let mut g = Gesture::press(target(), 0, 0, &Cell::default(), origin, false, 127);
        let v = g.drag(pos2(0.0, 50.0 - DRAG_THRESHOLD)).expect("a drag");
        assert!(v >= STEP_VELOCITY, "{v}");
    }

    #[test]
    fn the_keyboards_step_stays_inside_the_rows_and_the_clips_steps() {
        let start = (1, 5);
        assert_eq!(step_cursor(start, Move::Left, 4, 32), (1, 4));
        assert_eq!(step_cursor(start, Move::Right, 4, 32), (1, 6));
        assert_eq!(step_cursor(start, Move::Up, 4, 32), (0, 5));
        assert_eq!(step_cursor(start, Move::Down, 4, 32), (2, 5));
        assert_eq!(step_cursor((0, 0), Move::Left, 4, 32), (0, 0));
        assert_eq!(step_cursor((0, 0), Move::Up, 4, 32), (0, 0));
        assert_eq!(step_cursor((3, 31), Move::Right, 4, 32), (3, 31));
        assert_eq!(step_cursor((3, 31), Move::Down, 4, 32), (3, 31));
        assert_eq!(step_cursor(start, Move::Home, 4, 32), (1, 0));
        assert_eq!(step_cursor(start, Move::End, 4, 24), (1, 23));
        assert_eq!(step_cursor((0, 0), Move::Right, 0, 0), (0, 0), "no clip");
    }
}
