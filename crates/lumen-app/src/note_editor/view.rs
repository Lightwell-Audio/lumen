//! Where notes are on the screen (M5 slice 3c): the editor's geometry. The
//! horizontal axis is the editor's own [`TimeView`] (zoom and scroll are
//! the editor's, not the arrangement's: owner decision, ADR-0048), the
//! vertical one is pitch, scrolled and sized by the editor alone. Both use
//! the same beats as the arrangement, so a clip sits at the same place in
//! both. A note's start is in clip time (ticks from the clip's start,
//! `lumen_project::edit::note_time`).

use eframe::egui::{Pos2, Rect, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use crate::ruler;
use crate::timeview::{Limits, TimeView};

/// Pitches there are: 0 to 127.
pub(crate) const PITCHES: i32 = 128;
/// The tallest row a pitch can have, and the shortest (points).
pub(crate) const ROW_MIN: f32 = 8.0;
pub(crate) const ROW_MAX: f32 = 32.0;
pub(crate) const ROW_DEFAULT: f32 = 16.0;
/// Width of the key column (points).
pub(crate) const KEY_WIDTH: f32 = 56.0;
/// Height of the time ruler above the notes (points).
pub(crate) const RULER_HEIGHT: f32 = 20.0;
/// How near a note's end the pointer grabs the end (points), at most a
/// third of the note's width.
pub(crate) const EDGE: f32 = 6.0;

/// The view that frames a clip when it opens: its span (`start` to
/// `start + length`) across `width` points, with the small margin
/// [`TimeView::fit`] leaves either side.
pub(crate) fn fit_clip(start: Ticks, length: Ticks, width: f32, limits: Limits) -> TimeView {
    TimeView::fit(
        start.as_quarters(),
        (start + length).as_quarters(),
        width,
        limits,
    )
}

/// What the editor shows: the note area, its own time axis and the pitch
/// scroll.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorView {
    /// The area notes are drawn in (right of the key column, below the
    /// ruler).
    pub body: Rect,
    /// The editor's zoom and scroll (its own, not the arrangement's).
    pub time: TimeView,
    /// Where the open clip starts on the arrangement.
    pub clip_start: Ticks,
    /// Points a pitch's row is tall.
    pub row_height: f32,
    /// Points scrolled down from the top of pitch 127.
    pub scroll: f32,
}

/// Part of a note the pointer is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zone {
    Start,
    Body,
    End,
}

impl EditorView {
    /// The x of clip time `local`.
    pub fn x_of(&self, local: Ticks) -> f32 {
        self.time
            .x(self.body.left(), (self.clip_start + local).as_quarters())
    }

    /// The clip time at `x`, unsnapped (it may be before the clip: negative).
    pub fn local_at(&self, x: f32) -> Ticks {
        let beat = self.time.beat_at(self.body.left(), x);
        Ticks((beat * lumen_core::time::PPQ as f64).round() as i64) - self.clip_start
    }

    /// The arrangement time at `x`, never before the start.
    pub fn absolute_at(&self, x: f32) -> Ticks {
        ruler::ticks_at(self.time.beat_at(self.body.left(), x))
    }

    /// The top edge of pitch `pitch`'s row.
    pub fn row_top(&self, pitch: i32) -> f32 {
        self.body.top() + (PITCHES - 1 - pitch) as f32 * self.row_height - self.scroll
    }

    /// The pitch whose row contains `y` (may lie outside 0..=127).
    pub fn pitch_at(&self, y: f32) -> i32 {
        let row = ((y - self.body.top() + self.scroll) / self.row_height).floor() as i32;
        PITCHES - 1 - row
    }

    /// The furthest the view scrolls down.
    pub fn max_scroll(&self) -> f32 {
        (PITCHES as f32 * self.row_height - self.body.height()).max(0.0)
    }

    /// Where `note` is drawn (not cut to the body).
    pub fn note_rect(&self, note: Note) -> Rect {
        let top = self.row_top(i32::from(note.pitch()));
        let (left, right) = (self.x_of(note.start()), self.x_of(note.end()));
        Rect::from_min_max(
            pos2(left, top),
            pos2(right.max(left + 1.0), top + self.row_height),
        )
    }

    /// Whether `note` could show: inside the body's times and pitches.
    pub fn shows(&self, note: Note) -> bool {
        let r = self.note_rect(note);
        r.right() >= self.body.left()
            && r.left() <= self.body.right()
            && r.bottom() >= self.body.top()
            && r.top() <= self.body.bottom()
    }

    /// The top-most note under `p` and the part of it, if any. Muted notes
    /// count: they are there to be seen and unmuted.
    pub fn note_at(&self, notes: &[Note], p: Pos2) -> Option<(usize, Zone)> {
        if !self.body.contains(p) {
            return None;
        }
        let pitch = self.pitch_at(p.y);
        // Notes of the pitch under the pointer, the latest first: the one
        // drawn on top.
        let (i, note) = notes.iter().enumerate().rev().find(|(_, n)| {
            let r = self.note_rect(**n);
            i32::from(n.pitch()) == pitch && p.x >= r.left() - 1.0 && p.x <= r.right() + 1.0
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

    /// Keeps the scroll inside its limits.
    pub fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }
}

/// The scroll that puts pitch `pitch` in the middle of a body `height`
/// tall.
pub(crate) fn scroll_to_centre(pitch: i32, row_height: f32, height: f32) -> f32 {
    ((PITCHES - 1 - pitch) as f32 + 0.5) * row_height - height / 2.0
}

/// "C4", "F#3": middle C (60) is C4.
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
        // Row of pitch 127 starts at body top - scroll.
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
        // Beats 4 to 12 across 800 points: the span fills the width, with
        // the same small margin on each side.
        let view = fit_clip(Ticks::quarters(4), Ticks::quarters(8), 800.0, limits(800.0));
        let (left, right) = (view.x(0.0, 4.0), view.x(0.0, 12.0));
        assert!(left > 0.0 && right < 800.0, "{left} {right}");
        assert!(left < 40.0, "a little margin, not much: {left}");
        assert!(
            (left - (800.0 - right)).abs() < 1e-3,
            "centred: {left} vs {}",
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
