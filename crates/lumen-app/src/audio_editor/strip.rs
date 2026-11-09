//! Which layout the audio editor's properties strip takes (M9.5 S3, D33): the
//! first that fits, from what its parts measure this frame, as the header's
//! layouts are chosen (`header.rs`). The drawing is `app::audio_editor::strip`.
//!
//! Left to right the strip holds the Clip chip, Gain, Mute and Lock, the
//! fades, Reverse, Polarity and Normalize, and at the right the File chip.
//! When the row is too narrow its groups give way in the owner's order (L B):
//! Processing first, then the Warp chip (S5a, which sits beside the File
//! chip), then the Fades. Gain, Mute, Lock and the File chip never fold; the
//! Clip chip shrinks to its dot. A group that a later slice adds (transpose)
//! goes before the fades in the order and folds after Warp and before the
//! Fades.

use crate::theme;

/// The strip's layouts, widest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Layout {
    /// Every group, Processing with its words.
    #[default]
    Full,
    /// Processing as icons.
    Icons,
    /// The Clip chip is its dot.
    Dot,
    /// Processing is in the ⋯ menu.
    FoldProcessing,
    /// The Warp chip is in the ⋯ menu as well (S5a).
    FoldWarp,
    /// The Fades are in the ⋯ menu as well.
    FoldFades,
}

/// A group of the strip that folds into the ⋯ menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Processing,
    Warp,
    Fades,
}

impl Layout {
    /// The group's controls are in the row, not in the ⋯ menu.
    pub(crate) fn shows(self, group: Group) -> bool {
        match group {
            Group::Processing => self.shows_processing(),
            Group::Warp => self.shows_warp(),
            Group::Fades => self.shows_fades(),
        }
    }

    /// The Clip chip shows the clip's name.
    pub(crate) fn names_chip(self) -> bool {
        matches!(self, Self::Full | Self::Icons)
    }

    /// Processing is in the row as buttons, with or without words.
    pub(crate) fn shows_processing(self) -> bool {
        matches!(self, Self::Full | Self::Icons | Self::Dot)
    }

    /// The Warp chip is in the row, beside the File chip.
    pub(crate) fn shows_warp(self) -> bool {
        !matches!(self, Self::FoldWarp | Self::FoldFades)
    }

    /// Processing's buttons carry their words.
    pub(crate) fn words(self) -> bool {
        self == Self::Full
    }

    /// The Fades are in the row.
    pub(crate) fn shows_fades(self) -> bool {
        self != Self::FoldFades
    }

    /// The ⋯ menu is in the row.
    pub(crate) fn has_menu(self) -> bool {
        matches!(
            self,
            Self::FoldProcessing | Self::FoldWarp | Self::FoldFades
        )
    }
}

/// What the parts measure, in points, gaps within a part included.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Widths {
    /// The room for the row: the strip less its padding.
    pub room: f32,
    /// The Clip chip with the name (shortened), and as its dot.
    pub chip: f32,
    pub chip_dot: f32,
    /// Gain's label and its slider, as wide as they are drawn; and the part
    /// of that which is the label and its gap.
    pub gain: f32,
    pub gain_label: f32,
    /// Mute and Lock.
    pub toggles: f32,
    /// Both fades: a label, a slider and a shape each.
    pub fades: f32,
    /// Reverse, Polarity and Normalize with their words, and as icons.
    pub processing_words: f32,
    pub processing_icons: f32,
    /// The ⋯ button.
    pub more: f32,
    /// The File chip at the least it is drawn (its icon and a few letters).
    pub file: f32,
    /// The Warp chip (S5a), which has the right end beside the File chip and
    /// never folds: it is where the clip's tempo is set.
    pub warp: f32,
}

/// The gap between the strip's groups.
pub(crate) const GROUP_GAP: f32 = theme::space::M;

/// The least the File chip is drawn: its icon alone.
pub(crate) const FILE_LEAST: f32 = theme::size::CONTROL;

/// The first layout that fits. [`Layout::FoldFades`] is taken when none does:
/// the slider's and the chip's own least widths then give way in the drawing.
pub(crate) fn choose(w: Widths) -> Layout {
    let fits =
        |parts: &[f32]| parts.iter().sum::<f32>() + GROUP_GAP * (parts.len() - 1) as f32 <= w.room;
    let always = [w.gain, w.toggles, w.file];
    let with = |extra: &[f32]| {
        let mut parts = always.to_vec();
        parts.extend_from_slice(extra);
        parts
    };
    if fits(&with(&[w.chip, w.fades, w.processing_words, w.warp])) {
        Layout::Full
    } else if fits(&with(&[w.chip, w.fades, w.processing_icons, w.warp])) {
        Layout::Icons
    } else if fits(&with(&[w.chip_dot, w.fades, w.processing_icons, w.warp])) {
        Layout::Dot
    } else if fits(&with(&[w.chip_dot, w.fades, w.more, w.warp])) {
        Layout::FoldProcessing
    } else if fits(&with(&[w.chip_dot, w.fades, w.more])) {
        Layout::FoldWarp
    } else {
        Layout::FoldFades
    }
}

/// How wide Gain and the File chip are drawn in `layout`. Nothing but these
/// two gives way once every group that can fold has: the File chip shortens
/// first, to its icon, then Gain loses its label and shortens to
/// [`theme::size::STRIP_GAIN_LEAST`], in the room the other parts leave (D33).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ends {
    /// The Gain group: its label (when `label`) and its slider.
    pub gain: f32,
    pub label: bool,
    pub file: f32,
}

pub(crate) fn ends(layout: Layout, w: Widths) -> Ends {
    let whole = Ends {
        gain: w.gain,
        label: true,
        file: w.file,
    };
    if layout != Layout::FoldFades {
        return whole;
    }
    // Every other part, and the gaps between the five parts.
    let others = w.chip_dot + w.toggles + w.more + 4.0 * GROUP_GAP;
    let left = w.room - others;
    if left >= w.gain + w.file {
        return whole;
    }
    let least = theme::size::STRIP_GAIN_LEAST;
    let file = (left - w.gain).clamp(FILE_LEAST, w.file);
    let gain = left - file;
    if gain >= least + w.gain_label {
        Ends {
            gain: gain.min(w.gain),
            label: true,
            file,
        }
    } else {
        Ends {
            gain: gain.max(least),
            label: false,
            file,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(room: f32) -> Widths {
        Widths {
            room,
            chip: 120.0,
            chip_dot: 24.0,
            gain: 150.0,
            gain_label: 34.0,
            toggles: 52.0,
            fades: 260.0,
            processing_words: 220.0,
            processing_icons: 84.0,
            more: 24.0,
            file: 90.0,
            warp: 70.0,
        }
    }

    #[test]
    fn the_widest_layout_that_fits_is_taken() {
        let full = 120.0 + 150.0 + 52.0 + 260.0 + 220.0 + 90.0 + 70.0 + 6.0 * GROUP_GAP;
        assert_eq!(choose(widths(full)), Layout::Full);
        assert_eq!(choose(widths(full - 1.0)), Layout::Icons);
        let icons = full - 220.0 + 84.0;
        assert_eq!(choose(widths(icons)), Layout::Icons);
        assert_eq!(choose(widths(icons - 1.0)), Layout::Dot);
        let dot = icons - 120.0 + 24.0;
        assert_eq!(choose(widths(dot)), Layout::Dot);
        assert_eq!(choose(widths(dot - 1.0)), Layout::FoldProcessing);
        let fold = dot - 84.0 + 24.0;
        assert_eq!(choose(widths(fold)), Layout::FoldProcessing);
        assert_eq!(choose(widths(fold - 1.0)), Layout::FoldWarp);
        let warp = fold - 70.0 - GROUP_GAP;
        assert_eq!(choose(widths(warp)), Layout::FoldWarp);
        assert_eq!(choose(widths(warp - 1.0)), Layout::FoldFades);
        assert_eq!(choose(widths(0.0)), Layout::FoldFades);
    }

    #[test]
    fn gain_and_the_file_chip_shrink_only_when_everything_that_can_fold_has() {
        // Wide: both are whole.
        let wide = ends(Layout::FoldFades, widths(1000.0));
        assert_eq!((wide.gain, wide.label, wide.file), (150.0, true, 90.0));
        let full = ends(Layout::Full, widths(100.0));
        assert_eq!(
            (full.gain, full.file),
            (150.0, 90.0),
            "other layouts: whole"
        );
        // Tight: the dot, Gain, Mute and Lock, the menu and the file, 4 gaps.
        let others = 24.0 + 52.0 + 24.0 + 4.0 * GROUP_GAP;
        // The file gives way first, to its least; Gain keeps its label.
        let tight = ends(Layout::FoldFades, widths(others + 150.0 + 40.0));
        assert_eq!((tight.gain, tight.label, tight.file), (150.0, true, 40.0));
        // Then Gain shortens, with its label while a slider of the least
        // width fits beside it.
        let room = others + 56.0 + 34.0 + FILE_LEAST;
        let narrow = ends(Layout::FoldFades, widths(room));
        assert_eq!(narrow.file, FILE_LEAST);
        assert!(narrow.label && narrow.gain == 90.0);
        // Narrower still: the label goes, then the slider stops at its least.
        let bare = ends(Layout::FoldFades, widths(room - 10.0));
        assert!(!bare.label);
        assert_eq!(bare.gain, 80.0);
        let nothing = ends(Layout::FoldFades, widths(0.0));
        assert_eq!((nothing.gain, nothing.file), (56.0, FILE_LEAST));
    }

    #[test]
    fn processing_folds_before_the_fades_and_gain_never_folds() {
        let mut seen = Vec::new();
        for room in (0..1200).rev() {
            let layout = choose(widths(room as f32));
            if seen.last() != Some(&layout) {
                seen.push(layout);
            }
        }
        // Narrowing the room only ever steps through the layouts in order.
        assert_eq!(
            seen,
            [
                Layout::Full,
                Layout::Icons,
                Layout::Dot,
                Layout::FoldProcessing,
                Layout::FoldWarp,
                Layout::FoldFades
            ]
        );
        for layout in seen {
            // Whatever the layout, the menu holds exactly what the row lacks.
            assert_eq!(layout.has_menu(), !layout.shows_processing());
            // The Warp chip is in the row wherever Processing is, and in one
            // narrower layout too.
            assert!(layout.shows_warp() || !layout.shows_processing());
        }
    }
}
