//! The marks at the right end of a clip's title bar (M9.5 S3, §10.4): what
//! was done to the clip that its name does not say. S3 draws the two that
//! exist, Reverse and Polarity inverted; warp and transpose add theirs here
//! as more cases of the same function.
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
}

impl Mark {
    pub(super) fn icon(self) -> Icon {
        match self {
            Self::Reverse => Icon::Reverse,
            Self::Polarity => Icon::Polarity,
        }
    }

    /// What it says on hover and in the info line.
    pub(super) fn words(self) -> &'static str {
        match self {
            Self::Reverse => "Reversed",
            Self::Polarity => "Polarity inverted",
        }
    }
}

/// What is set on a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Marks {
    pub(crate) reverse: bool,
    pub(crate) polarity: bool,
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
            }
        } else {
            Self::default()
        }
    }

    /// The marks that are set, in drawing order.
    pub(super) fn list(self) -> impl Iterator<Item = Mark> {
        [
            self.reverse.then_some(Mark::Reverse),
            self.polarity.then_some(Mark::Polarity),
        ]
        .into_iter()
        .flatten()
    }

    pub(super) fn count(self) -> usize {
        usize::from(self.reverse) + usize::from(self.polarity)
    }

    /// "Reversed · Polarity inverted", or nothing when none is set.
    pub(crate) fn words(self) -> Option<String> {
        let words: Vec<&str> = self.list().map(Mark::words).collect();
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

/// The width of `count` marks side by side.
pub(super) fn marks_width(count: usize) -> f32 {
    match count {
        0 => 0.0,
        n => n as f32 * MARK + (n - 1) as f32 * GAP,
    }
}

/// What to draw for `count` marks on a title that is `visible` wide where it
/// is in view, with `room` left of its width for the name and the marks once
/// the padlock and the take counter have taken theirs.
pub(super) fn place(visible: f32, room: f32, count: usize) -> Placed {
    if count == 0 || visible < DOT_LEAST {
        return Placed::Nothing;
    }
    if visible < NAME_FROM {
        return Placed::Alone;
    }
    let width = marks_width(count);
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

    #[test]
    fn nothing_set_costs_nothing_at_any_width() {
        for width in 0..400 {
            assert_eq!(place(width as f32, width as f32, 0), Placed::Nothing);
        }
    }

    #[test]
    fn marks_need_four_letters_of_the_name_beside_them() {
        // Two marks: 9 + 3 + 9 = 21, a gap, four letters.
        let need = 21.0 + GAP + NAME_LEAST;
        assert_eq!(place(200.0, need, 2), Placed::Marks(21.0));
        assert_eq!(place(200.0, need - 0.5, 2), Placed::Dot);
        // One mark needs less.
        let one = MARK + GAP + NAME_LEAST;
        assert_eq!(place(200.0, one, 1), Placed::Marks(MARK));
        assert_eq!(place(200.0, one - 0.5, 1), Placed::Dot);
    }

    #[test]
    fn the_name_shortens_before_the_dot_goes() {
        assert_eq!(place(200.0, DOT + GAP, 2), Placed::Dot);
        assert_eq!(place(200.0, DOT + GAP - 0.5, 2), Placed::Nothing);
    }

    #[test]
    fn a_title_too_narrow_for_a_name_shows_a_dot_alone_down_to_ten_points() {
        for width in [10.0, 15.0, 23.9] {
            assert_eq!(place(width, width, 1), Placed::Alone, "{width}");
            assert_eq!(place(width, width, 2), Placed::Alone, "{width}");
        }
        assert_eq!(place(9.9, 9.9, 2), Placed::Nothing);
        assert_eq!(place(0.0, 0.0, 1), Placed::Nothing);
        // From 24 pt a name shows, so the dot makes room for it.
        assert_ne!(place(24.0, 20.0, 1), Placed::Alone);
    }

    #[test]
    fn marks_never_take_more_than_the_room_they_were_given() {
        for count in 0..=2 {
            for step in 0..800 {
                let room = step as f32 / 2.0;
                let placed = place(room + 8.0, room, count);
                assert!(
                    placed.takes() <= room + 1e-4,
                    "{count} marks in {room}: {placed:?}"
                );
            }
        }
    }

    #[test]
    fn marks_are_listed_in_drawing_order_with_their_words() {
        let both = Marks {
            reverse: true,
            polarity: true,
        };
        assert_eq!(both.count(), 2);
        assert_eq!(
            both.list().collect::<Vec<_>>(),
            [Mark::Reverse, Mark::Polarity]
        );
        assert_eq!(
            both.words().as_deref(),
            Some("Reversed \u{00B7} Polarity inverted")
        );
        assert_eq!(Marks::default().words(), None);
    }
}
