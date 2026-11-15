use crate::icons::Icon;

pub(super) const MARK: f32 = 9.0;
pub(super) const GAP: f32 = 3.0;
pub(super) const DOT: f32 = 5.0;
pub(super) const NAME_LEAST: f32 = 28.0;
pub(super) const NAME_FROM: f32 = 24.0;
pub(super) const DOT_LEAST: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    Reverse,
    Polarity,
    Transpose(i32),
}

impl Mark {
    pub(super) fn icon(self) -> Option<Icon> {
        match self {
            Self::Reverse => Some(Icon::Reverse),
            Self::Polarity => Some(Icon::Polarity),
            Self::Transpose(_) => None,
        }
    }

    pub(super) fn text(self) -> Option<String> {
        match self {
            Self::Transpose(cents) => Some(transpose_text(cents)),
            Self::Reverse | Self::Polarity => None,
        }
    }

    pub(super) fn words(self) -> String {
        match self {
            Self::Reverse => "Reversed".to_owned(),
            Self::Polarity => "Polarity inverted".to_owned(),
            Self::Transpose(cents) => format!("Transposed {} st", transpose_text(cents)),
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Marks {
    pub(crate) reverse: bool,
    pub(crate) polarity: bool,
    pub(crate) transpose: i32,
}

impl Marks {
    pub(crate) fn of(clip: &lumen_project::model::Clip) -> Self {
        if matches!(
            clip.content,
            lumen_project::model::ClipContent::Audio { .. }
        ) {
            Self {
                reverse: clip.reversed,
                polarity: clip.inverted,
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

    pub(super) fn list(self) -> impl Iterator<Item = Mark> {
        [
            (self.transpose != 0).then_some(Mark::Transpose(self.transpose)),
            self.reverse.then_some(Mark::Reverse),
            self.polarity.then_some(Mark::Polarity),
        ]
        .into_iter()
        .flatten()
    }

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

    pub(crate) fn words(self) -> Option<String> {
        let words: Vec<String> = self.list().map(Mark::words).collect();
        (!words.is_empty()).then(|| words.join(" \u{00B7} "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Placed {
    Nothing,
    Marks(f32),
    Dot,
    Alone,
}

impl Placed {
    pub(super) fn takes(self) -> f32 {
        match self {
            Self::Marks(width) => width + GAP,
            Self::Dot => DOT + GAP,
            Self::Nothing | Self::Alone => 0.0,
        }
    }
}

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
        let need = 21.0 + GAP + NAME_LEAST;
        assert_eq!(place(200.0, need, icons(2)), Placed::Marks(21.0));
        assert_eq!(place(200.0, need - 0.5, icons(2)), Placed::Dot);
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
        assert_ne!(place(24.0, 20.0, icons(1)), Placed::Alone);
    }

    #[test]
    fn marks_never_take_more_than_the_room_they_were_given() {
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
        clip.content = ClipContent::midi();
        assert_eq!(Marks::of(&clip), Marks::default());
    }
}
