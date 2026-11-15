use eframe::egui::{Pos2, Rect, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use crate::ruler;
use crate::timeview::{Limits, TimeView};

pub(crate) use crate::piano_roll::draw::Rows;

pub(crate) const PITCHES: i32 = 128;
pub(crate) const ROW_MIN: f32 = 8.0;
pub(crate) const ROW_MAX: f32 = 32.0;
pub(crate) const ROW_DEFAULT: f32 = 16.0;
pub(crate) const KEY_WIDTH: f32 = 56.0;
pub(crate) const KEY_WIDTH_DRUMS: f32 = crate::theme::size::LANE_NAME;
pub(crate) const RULER_HEIGHT: f32 = 20.0;
pub(crate) const EDGE: f32 = 6.0;

pub(crate) fn fit_clip(start: Ticks, length: Ticks, width: f32, limits: Limits) -> TimeView {
    TimeView::fit(
        start.as_quarters(),
        (start + length).as_quarters(),
        width,
        limits,
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorView {
    pub body: Rect,
    pub time: TimeView,
    pub clip_start: Ticks,
    pub row_height: f32,
    pub scroll: f32,
    pub rows: Rows,
    pub lanes: bool,
}

const HIDDEN_ROW: f32 = 1.0e6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zone {
    Start,
    Body,
    End,
}

impl EditorView {
    pub fn x_of(&self, local: Ticks) -> f32 {
        self.time
            .x(self.body.left(), (self.clip_start + local).as_quarters())
    }

    pub fn local_at(&self, x: f32) -> Ticks {
        let beat = self.time.beat_at(self.body.left(), x);
        Ticks((beat * lumen_core::time::PPQ as f64).round() as i64) - self.clip_start
    }

    pub fn absolute_at(&self, x: f32) -> Ticks {
        ruler::ticks_at(self.time.beat_at(self.body.left(), x))
    }

    pub fn row_top(&self, pitch: i32) -> f32 {
        match self.rows.row_of(pitch) {
            Some(row) => self.top_of_row(row),
            None => HIDDEN_ROW,
        }
    }

    fn top_of_row(&self, row: usize) -> f32 {
        self.body.top() + row as f32 * self.row_height - self.scroll
    }

    pub fn pitch_at(&self, y: f32) -> i32 {
        let row = ((y - self.body.top() + self.scroll) / self.row_height).floor();
        let last = self.rows.len().saturating_sub(1) as f32;
        self.rows.pitch_of(row.clamp(0.0, last) as usize)
    }

    pub fn lane_at(&self, y: f32) -> Option<usize> {
        if !self.body.y_range().contains(y) {
            return None;
        }
        let row = ((y - self.body.top() + self.scroll) / self.row_height).floor();
        (row >= 0.0 && (row as usize) < self.rows.len()).then_some(row as usize)
    }

    pub fn max_scroll(&self) -> f32 {
        (self.rows.len() as f32 * self.row_height - self.body.height()).max(0.0)
    }

    pub fn note_rect(&self, note: Note) -> Rect {
        let top = self.row_top(i32::from(note.pitch()));
        let (left, right) = (self.x_of(note.start()), self.x_of(note.end()));
        Rect::from_min_max(
            pos2(left, top),
            pos2(right.max(left + 1.0), top + self.row_height),
        )
    }

    pub fn shows(&self, note: Note) -> bool {
        let r = self.note_rect(note);
        r.right() >= self.body.left()
            && r.left() <= self.body.right()
            && r.bottom() >= self.body.top()
            && r.top() <= self.body.bottom()
    }

    pub fn note_at(&self, notes: &[Note], p: Pos2) -> Option<(usize, Zone)> {
        if !self.body.contains(p) {
            return None;
        }
        let pitch = self.pitch_at(p.y);
        let (i, note) = notes.iter().enumerate().rev().find(|(_, n)| {
            let r = self.note_rect(**n);
            self.shows(**n)
                && i32::from(n.pitch()) == pitch
                && p.x >= r.left() - 1.0
                && p.x <= r.right() + 1.0
        })?;
        let rect = self.note_rect(*note);
        let edge = EDGE.min(rect.width() / 3.0);
        let zone = if p.x <= rect.left() + edge {
            Zone::Start
        } else if p.x >= rect.right() - edge {
            Zone::End
        } else {
            Zone::Body
        };
        Some((i, zone))
    }

    pub fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }
}

pub(crate) fn scroll_to_centre(pitch: i32, row_height: f32, height: f32) -> f32 {
    scroll_to_row((PITCHES - 1 - pitch) as usize, row_height, height)
}

pub(crate) fn scroll_to_row(row: usize, row_height: f32, height: f32) -> f32 {
    (row as f32 + 0.5) * row_height - height / 2.0
}

pub(crate) fn pitch_name(pitch: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let pitch = pitch.clamp(0, 127);
    format!("{}{}", NAMES[(pitch % 12) as usize], pitch / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> EditorView {
        EditorView {
            body: Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 250.0)),
            time: TimeView {
                left: 4.0,
                zoom: 40.0,
            },
            clip_start: Ticks::quarters(4),
            row_height: 10.0,
            scroll: 600.0,
            rows: Rows::all(),
            lanes: false,
        }
    }

    #[test]
    fn time_and_x_agree() {
        let v = view();
        assert_eq!(v.x_of(Ticks::ZERO), 100.0);
        assert_eq!(v.x_of(Ticks::quarters(2)), 180.0);
        assert_eq!(v.local_at(180.0), Ticks::quarters(2));
        assert_eq!(v.local_at(60.0), Ticks::quarters(-1));
    }

    #[test]
    fn pitch_and_y_agree() {
        let v = view();
        assert_eq!(v.row_top(127), 50.0 - 600.0);
        let y = v.row_top(60) + 3.0;
        assert_eq!(v.pitch_at(y), 60);
        assert_eq!(v.pitch_at(v.row_top(60) + v.row_height - 0.1), 60);
        assert_eq!(v.pitch_at(v.row_top(60) + v.row_height + 0.1), 59);
    }

    #[test]
    fn names_follow_middle_c() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(61), "C#4");
        assert_eq!(pitch_name(0), "C-1");
        assert_eq!(pitch_name(127), "G9");
    }

    #[test]
    fn a_note_is_found_by_its_part() {
        let v = view();
        let note = Note::new(Ticks::quarters(1), Ticks::quarters(2), 60, 100);
        let r = v.note_rect(note);
        let mid = r.center();
        assert_eq!(v.note_at(&[note], mid), Some((0, Zone::Body)));
        assert_eq!(
            v.note_at(&[note], pos2(r.left() + 1.0, mid.y)),
            Some((0, Zone::Start))
        );
        assert_eq!(
            v.note_at(&[note], pos2(r.right() - 1.0, mid.y)),
            Some((0, Zone::End))
        );
        assert_eq!(v.note_at(&[note], pos2(mid.x, mid.y + 12.0)), None);
    }

    #[test]
    fn the_scroll_centres_a_pitch() {
        let s = scroll_to_centre(60, 10.0, 200.0);
        let mut v = view();
        v.scroll = s;
        let mid = v.body.center().y;
        assert_eq!(v.pitch_at(mid), 60);
    }

    #[test]
    fn the_lanes_have_no_row_below_the_last_one() {
        use crate::piano_roll::draw::Rows;
        let mut v = view();
        v.lanes = true;
        v.rows = Rows::lanes([36_u8, 38]);
        v.row_height = crate::theme::size::LANE_ROW;
        v.scroll = 0.0;
        v.body = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 200.0));
        assert_eq!(v.lane_at(1.0), Some(0));
        assert_eq!(v.lane_at(28.0 + 1.0), Some(1));
        assert_eq!(v.lane_at(2.0 * 28.0 + 1.0), None, "below the last lane");
        assert_eq!(v.pitch_at(2.0 * 28.0 + 1.0), 38, "the roll clamps instead");
        v.rows = Rows::lanes(std::iter::empty::<u8>());
        assert_eq!(v.lane_at(1.0), None, "a map with no lanes has no row");
        assert_eq!(v.lane_at(-1.0), None, "nor has the area above");
    }

    #[test]
    fn a_folded_roll_shows_only_the_maps_sounds_packed_together() {
        use crate::design::icons::DrumGlyph;
        let sounds = vec![
            crate::piano_roll::draw::KeySound {
                input: 36,
                name: "Kick".into(),
                icon: DrumGlyph::Kick,
                color: eframe::egui::Color32::WHITE,
                silent: false,
            },
            crate::piano_roll::draw::KeySound {
                input: 46,
                name: "Open hat".into(),
                icon: DrumGlyph::OpenHat,
                color: eframe::egui::Color32::WHITE,
                silent: false,
            },
        ];
        let rows = Rows::mapped(&sounds);
        assert_eq!(rows.len(), 2, "one row a sound");
        assert_eq!(rows.row_of(46), Some(0), "the higher note is on top");
        assert_eq!(rows.row_of(36), Some(1));
        assert_eq!(rows.row_of(38), None, "a key the map does not use");
        assert_eq!((rows.pitch_of(0), rows.pitch_of(1)), (46, 36));
        assert_eq!(rows.pitch_of(9), 36, "past the end clamps to the last");
        let mut v = view();
        v.rows = rows;
        v.scroll = 0.0;
        v.body = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 100.0));
        assert_eq!(v.row_top(46), 0.0);
        assert_eq!(v.row_top(36), 10.0);
        assert_eq!(v.row_top(60), HIDDEN_ROW, "a hidden pitch has no row");
        assert_eq!(v.max_scroll(), 0.0, "two rows fit");
        assert_eq!(v.pitch_at(5.0), 46);
        assert_eq!(v.pitch_at(15.0), 36);
        let note = Note::new(Ticks::ZERO, Ticks::quarters(1), 60, 100);
        assert!(!v.shows(note));
        assert_eq!(v.note_at(&[note], v.body.center()), None);
        assert!(Rows::mapped(&[]).is_all());
    }

    fn limits(width: f32) -> Limits {
        Limits {
            min: 0.001,
            max: 1e6,
            end: 1e9,
            width,
        }
    }

    #[test]
    fn a_clip_opens_framed_with_a_little_margin_either_side() {
        let view = fit_clip(Ticks::quarters(4), Ticks::quarters(8), 800.0, limits(800.0));
        let (left, right) = (view.x(0.0, 4.0), view.x(0.0, 12.0));
        assert!(left > 0.0 && right < 800.0, "{left} {right}");
        assert!(left < 40.0, "a little margin, not much: {left}");
        assert!(
            (left - (800.0 - right)).abs() < 1e-3,
            "centered: {left} vs {}",
            800.0 - right
        );
    }

    #[test]
    fn a_clip_at_the_start_is_framed_from_the_start() {
        let view = fit_clip(Ticks::ZERO, Ticks::quarters(8), 800.0, limits(800.0));
        assert_eq!(view.left, 0.0);
        assert_eq!(view.x(0.0, 0.0), 0.0);
    }

    #[test]
    fn an_empty_clip_still_gets_a_usable_view() {
        let view = fit_clip(Ticks::quarters(4), Ticks::ZERO, 800.0, limits(800.0));
        assert!(view.zoom.is_finite() && view.zoom > 0.0, "{view:?}");
    }

    #[test]
    fn the_fitted_zoom_respects_the_limits() {
        let capped = Limits {
            max: 100.0,
            ..limits(800.0)
        };
        let view = fit_clip(Ticks::quarters(4), Ticks::quarters(1), 800.0, capped);
        assert_eq!(view.zoom, 100.0);
    }
}
