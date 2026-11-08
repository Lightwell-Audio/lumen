//! The clip's loop brace (M9.5 S2b, the owner's LB A and LS A): a band along
//! the bottom of the editor's ruler. Until the clip loops it shows a thin mark
//! of the clip's window; a click turns the loop on or off, and the brace's
//! ends set where the loop starts and ends in the file. A flag marks where the
//! clip starts inside its loop. Pure: where it is and what the pointer is on.
//! The edit is `edit::clip_loop`; the gestures are the editor's
//! (`app::audio_editor::brace`).

use eframe::egui::{Pos2, Rect, pos2};
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

/// The editor's ruler with its band.
pub(crate) const RULER: f32 = theme::size::EDITOR_RULER;

/// A part of the brace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BracePart {
    /// The loop's start, in the file.
    Start,
    /// The loop's end.
    End,
    /// The band anywhere else: a click turns the loop on or off.
    Band,
}

/// Where the brace is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Brace {
    /// The band under the ruler.
    pub band: Rect,
    /// The loop's two ends on screen, while the clip loops.
    pub ends: Option<(f32, f32)>,
    /// What the clip plays when it does not loop: the thin mark's ends.
    pub window: (f32, f32),
    /// The clip's start, when it is inside its loop rather than at its
    /// start.
    pub flag: Option<f32>,
}

impl Brace {
    /// The brace of `clip`, whose file is laid as `file`, in `band`.
    pub(crate) fn of(clip: &Clip, file: &WholeFile, time: TimeView, band: Rect) -> Self {
        let x = |q: f64| time.x(band.left(), q);
        let looping = clip.loop_length.is_some_and(|p| p.0 > 0);
        let (a, b) = file.loop_window;
        let start = x(clip.start.as_quarters());
        Self {
            band,
            ends: looping.then(|| (x(a), x(b))),
            window: (x(file.played.extent().0), x(file.played.extent().1)),
            flag: (looping && clip.start.as_quarters() > a + 1e-9).then_some(start),
        }
    }

    /// What `p` is on, if it is on the band. An end wins within reach; the
    /// nearer one when both are.
    pub(crate) fn hit(&self, p: Pos2) -> Option<BracePart> {
        if !self.band.contains(p) {
            return None;
        }
        if let Some((a, b)) = self.ends {
            let reach = theme::size::BRACE_GRAB;
            let (da, db) = ((p.x - a).abs(), (p.x - b).abs());
            if da <= reach && da <= db {
                return Some(BracePart::Start);
            }
            if db <= reach {
                return Some(BracePart::End);
            }
        }
        Some(BracePart::Band)
    }

    /// Where an end's focus ring goes.
    pub(crate) fn end_rect(&self, part: BracePart) -> Option<Rect> {
        let (a, b) = self.ends?;
        let x = match part {
            BracePart::Start => a,
            BracePart::End => b,
            BracePart::Band => return Some(self.band),
        };
        let reach = theme::size::BRACE_GRAB;
        Some(Rect::from_min_max(
            pos2(x - reach, self.band.top()),
            pos2(x + reach, self.band.bottom()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_editor::source::Played;
    use lumen_core::time::Ticks;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn file(window: (f64, f64)) -> WholeFile {
        WholeFile {
            spans: Vec::new(),
            from: 0.0,
            to: 32.0,
            loop_window: window,
            played: Played {
                first: window,
                second: None,
            },
            heard: window,
        }
    }

    fn clip(start: i64, looping: Option<i64>) -> Clip {
        use lumen_project::model::{ClipContent, Id};
        Clip {
            id: Id(1),
            name: "Loop".into(),
            start: q(start),
            length: q(4),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: looping.map(q),
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            content: ClipContent::Audio {
                media: Id(9),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    fn band() -> Rect {
        Rect::from_min_max(pos2(0.0, 27.0), pos2(1_000.0, 37.0))
    }

    /// 10 points a beat from beat 0.
    fn time() -> TimeView {
        TimeView {
            left: 0.0,
            zoom: 10.0,
        }
    }

    #[test]
    fn a_clip_that_does_not_loop_shows_its_window_and_no_ends() {
        let b = Brace::of(&clip(8, None), &file((8.0, 12.0)), time(), band());
        assert_eq!(b.ends, None);
        assert_eq!(b.window, (80.0, 120.0));
        assert_eq!(b.hit(pos2(80.0, 30.0)), Some(BracePart::Band));
        assert_eq!(b.hit(pos2(80.0, 20.0)), None, "the ruler above the band");
    }

    #[test]
    fn the_ends_are_grabbed_within_reach_and_the_flag_marks_a_later_start() {
        let b = Brace::of(&clip(10, Some(4)), &file((8.0, 12.0)), time(), band());
        assert_eq!(b.ends, Some((80.0, 120.0)));
        assert_eq!(b.flag, Some(100.0));
        assert_eq!(b.hit(pos2(83.0, 30.0)), Some(BracePart::Start));
        assert_eq!(b.hit(pos2(118.0, 30.0)), Some(BracePart::End));
        assert_eq!(b.hit(pos2(100.0, 30.0)), Some(BracePart::Band));
        let at_start = Brace::of(&clip(8, Some(4)), &file((8.0, 12.0)), time(), band());
        assert_eq!(at_start.flag, None);
    }

    #[test]
    fn ends_close_together_go_to_the_nearer() {
        let b = Brace::of(&clip(8, Some(1)), &file((8.0, 8.5)), time(), band());
        assert_eq!(b.hit(pos2(81.0, 30.0)), Some(BracePart::Start));
        assert_eq!(b.hit(pos2(84.0, 30.0)), Some(BracePart::End));
    }
}
