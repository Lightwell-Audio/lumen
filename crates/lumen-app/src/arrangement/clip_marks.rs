//! The marks at the right end of a clip's title bar (M9.5 S3, §10.4): what
//! was done to the clip that its name does not say. S3 drew Reverse and
//! Polarity inverted; S6 adds the transpose, as the small text "+2" the owner's
//! specimen showed (B). A warp mark adds its own here later.
//!
//! A mark is an icon of a fixed size or, for the transpose, a short text whose
//! width the caller measures, so [`place`] takes the marks' total width.
//!
//! The title's right end is one budget (`clip_look::draw_name`): the padlock
//! and the take counter reserve their room first and keep their places; what
//! is left holds the name and, beside it, the marks. [`place`] decides what is
//! shown, in points and with no drawing, so it is tested at every width:
//! the marks when they and four letters of the name fit, else one dot (the
//! name shortens before the dot goes), and on a clip too narrow for a name
//! the dot alone, down to [`DOT_LEAST`] wide; below that, nothing.

use crate::icons::Icon;

/// A mark's icon, square.
pub(super) const MARK: f32 = 9.0;
/// The gap between marks, and between the marks and the name.
pub(super) const GAP: f32 = 3.0;
/// The dot's diameter.
pub(super) const DOT: f32 = 5.0;
/// The room four letters of a name take (caption type).
pub(super) const NAME_LEAST: f32 = 28.0;
/// Under this visible width no name shows: a dot stands alone, centred.
pub(super) const NAME_FROM: f32 = 24.0;
/// Under this visible width nothing is drawn.
pub(super) const DOT_LEAST: f32 = 10.0;

/// One thing done to a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    Reverse,
    Polarity,
    /// Transposed by this many cents (never 0).
    Transpose(i32),
}

impl Mark {
    /// The icon an icon mark is drawn as; the transpose is text.
    pub(super) fn icon(self) -> Option<Icon> {
        match self {
            Self::Reverse => Some(Icon::Reverse),
            Self::Polarity => Some(Icon::Polarity),
            Self::Transpose(_) => None,
        }
    }

    /// The text a text mark is drawn as.
    pub(super) fn text(self) -> Option<String> {
        match self {
            Self::Transpose(cents) => Some(transpose_text(cents)),
            Self::Reverse | Self::Polarity => None,
        }
    }

    /// What it says on hover and in the info line.
    pub(super) fn words(self) -> String {
        match self {
            Self::Reverse => "Reversed".to_owned(),
            Self::Polarity => "Polarity inverted".to_owned(),
            Self::Transpose(cents) => format!("Transposed {} st", transpose_text(cents)),
        }
    }
}

/// A transpose as the mark reads it: "+2", "\u{2212}3", "+2.5" (semitones, with
/// cents after the point when there are some).
pub(crate) fn transpose_text(cents: i32) -> String {
    let sign = if cents < 0 { "\u{2212}" } else { "+" };
    let whole = cents.abs() / 100;
    let part = cents.abs() % 100;
    match part {
        0 => format!("{sign}{whole}"),
        p if p % 10 == 0 => format!("{sign}{whole}.{}", p / 10),
        p => format!("{sign}{whole}.{p:02}"),
    }
}

/// What is set on a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Marks {
    pub(crate) reverse: bool,
    pub(crate) polarity: bool,
    /// The transpose in cents that the clip plays with, 0 for none.
    pub(crate) transpose: i32,
}

impl Marks {
    /// The marks of an audio clip.
    pub(crate) fn of(clip: &lumen_project::model::Clip) -> Self {
        if matches!(
            clip.content,
            lumen_project::model::ClipContent::Audio { .. }
        ) {
            Self {
                reverse: clip.reversed,
                polarity: clip.inverted,
                // Only a mode that transposes plays it.
                transpose: clip
                    .warp
                    .as_ref()
                    .filter(|w| w.mode.transposes())
                    .map_or(0, |w| w.transpose_cents),
            }
        } else {
            Self::default()
        }
    }

    /// The marks that are set, in drawing order.
    pub(super) fn list(self) -> impl Iterator<Item = Mark> {
        [
            (self.transpose != 0).then_some(Mark::Transpose(self.transpose)),
            self.reverse.then_some(Mark::Reverse),
            self.polarity.then_some(Mark::Polarity),
        ]
        .into_iter()
        .flatten()
    }

    /// How wide the marks are side by side: each icon [`MARK`], the transpose
    /// as wide as `text_width` says its text is.
    pub(super) fn width(self, text_width: impl Fn(&str) -> f32) -> f32 {
        let widths: Vec<f32> = self
            .list()
            .map(|mark| mark.text().map_or(MARK, |text| text_width(&text)))
            .collect();
        match widths.len() {
            0 => 0.0,
            n => widths.iter().sum::<f32>() + (n - 1) as f32 * GAP,
        }
    }

    /// "Transposed +2 st · Reversed · Polarity inverted", or nothing when none
    /// is set.
    pub(crate) fn words(self) -> Option<String> {
        let words: Vec<String> = self.list().map(Mark::words).collect();
        (!words.is_empty()).then(|| words.join(" \u{00B7} "))
    }
}

/// What the title shows of the marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Placed {
    /// Nothing is set, or there is no room.
    Nothing,
    /// The marks, this wide in all, at the right end of the room.
    Marks(f32),
    /// One dot at the right end of the room; the name has the rest.
    Dot,
    /// A dot alone, centred on the visible title (no name shows).
    Alone,
}

impl Placed {
    /// How much of the room the marks take, with the gap before the name.
    pub(super) fn takes(self) -> f32 {
        match self {
            Self::Marks(width) => width + GAP,
            Self::Dot => DOT + GAP,
            Self::Nothing | Self::Alone => 0.0,
        }
    }
}

/// What to draw for marks `width` wide in all (0 for none) on a title that is
/// `visible` wide where it is in view, with `room` left of its width for the
/// name and the marks once the padlock and the take counter have taken theirs.
pub(super) fn place(visible: f32, room: f32, width: f32) -> Placed {
    if width <= 0.0 || visible < DOT_LEAST {
        return Placed::Nothing;
    }
    if visible < NAME_FROM {
        return Placed::Alone;
    }
    if room >= width + GAP + NAME_LEAST {
        Placed::Marks(width)
    } else if room >= DOT + GAP {
        Placed::Dot
    } else {
        Placed::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The width of `count` icon marks side by side.
    fn icons(count: usize) -> f32 {
        match count {
            0 => 0.0,
            n => n as f32 * MARK + (n - 1) as f32 * GAP,
        }
    }

    #[test]
    fn nothing_set_costs_nothing_at_any_width() {
        for width in 0..400 {
            assert_eq!(place(width as f32, width as f32, 0.0), Placed::Nothing);
        }
    }

    #[test]
    fn marks_need_four_letters_of_the_name_beside_them() {
        // Two marks: 9 + 3 + 9 = 21, a gap, four letters.
        let need = 21.0 + GAP + NAME_LEAST;
        assert_eq!(place(200.0, need, icons(2)), Placed::Marks(21.0));
        assert_eq!(place(200.0, need - 0.5, icons(2)), Placed::Dot);
        // One mark needs less.
        let one = MARK + GAP + NAME_LEAST;
        assert_eq!(place(200.0, one, icons(1)), Placed::Marks(MARK));
        assert_eq!(place(200.0, one - 0.5, icons(1)), Placed::Dot);
    }

    #[test]
    fn the_name_shortens_before_the_dot_goes() {
        assert_eq!(place(200.0, DOT + GAP, icons(2)), Placed::Dot);
        assert_eq!(place(200.0, DOT + GAP - 0.5, icons(2)), Placed::Nothing);
    }

    #[test]
    fn a_title_too_narrow_for_a_name_shows_a_dot_alone_down_to_ten_points() {
        for width in [10.0, 15.0, 23.9] {
            assert_eq!(place(width, width, icons(1)), Placed::Alone, "{width}");
            assert_eq!(place(width, width, icons(2)), Placed::Alone, "{width}");
        }
        assert_eq!(place(9.9, 9.9, icons(2)), Placed::Nothing);
        assert_eq!(place(0.0, 0.0, icons(1)), Placed::Nothing);
        // From 24 pt a name shows, so the dot makes room for it.
        assert_ne!(place(24.0, 20.0, icons(1)), Placed::Alone);
    }

    #[test]
    fn marks_never_take_more_than_the_room_they_were_given() {
        // A text mark's width is whatever the caller measured: try several.
        for width in [0.0, icons(1), icons(2), 14.0, 30.0 + GAP + MARK] {
            for step in 0..800 {
                let room = step as f32 / 2.0;
                let placed = place(room + 8.0, room, width);
                assert!(
                    placed.takes() <= room + 1e-4,
                    "{width} of marks in {room}: {placed:?}"
                );
            }
        }
    }

    #[test]
    fn marks_are_listed_in_drawing_order_with_their_words() {
        let all = Marks {
            reverse: true,
            polarity: true,
            transpose: 250,
        };
        assert_eq!(
            all.list().collect::<Vec<_>>(),
            [Mark::Transpose(250), Mark::Reverse, Mark::Polarity]
        );
        assert_eq!(
            all.words().as_deref(),
            Some("Transposed +2.5 st \u{00B7} Reversed \u{00B7} Polarity inverted")
        );
        assert_eq!(Marks::default().words(), None);
    }

    #[test]
    fn a_transpose_reads_as_a_short_signed_number() {
        assert_eq!(transpose_text(200), "+2");
        assert_eq!(transpose_text(-300), "\u{2212}3");
        assert_eq!(transpose_text(250), "+2.5");
        assert_eq!(transpose_text(-25), "\u{2212}0.25");
        assert_eq!(transpose_text(4_800), "+48");
    }

    #[test]
    fn the_width_counts_a_text_mark_as_measured_and_an_icon_as_a_mark() {
        let marks = Marks {
            reverse: true,
            polarity: false,
            transpose: 200,
        };
        // "+2" measured at 12: 12 + gap + 9.
        assert_eq!(marks.width(|_| 12.0), 12.0 + GAP + MARK);
        assert_eq!(Marks::default().width(|_| 12.0), 0.0);
    }

    #[test]
    fn only_a_mode_that_transposes_shows_the_transpose() {
        use lumen_core::time::Ticks;
        use lumen_project::model::{Clip, ClipContent, Id, Warp, WarpMode};
        let mut clip = Clip {
            id: Id(1),
            name: "c".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Ticks::ZERO,
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            warp: None,
            content: ClipContent::Audio {
                media: Id(2),
                offset: 0,
                gain_db: 0.0,
            },
        };
        assert_eq!(Marks::of(&clip).transpose, 0, "not warped");
        let mut warp = Warp::new(120.0, 0.0);
        warp.transpose_cents = 100;
        for (mode, cents) in [
            (WarpMode::Repitch, 0),
            (WarpMode::Rhythmic, 100),
            (WarpMode::Melodic, 100),
            (WarpMode::FullMix, 100),
        ] {
            warp.mode = mode;
            clip.warp = Some(warp.clone());
            assert_eq!(Marks::of(&clip).transpose, cents, "{mode:?}");
        }
        // A MIDI clip has none, whatever it holds.
        clip.content = ClipContent::midi();
        assert_eq!(Marks::of(&clip), Marks::default());
    }
}
