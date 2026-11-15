use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Layout {
    #[default]
    Full,
    Icons,
    Dot,
    FoldProcessing,
    FoldWarp,
    FoldPitch,
    FoldFades,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Processing,
    Warp,
    Pitch,
    Fades,
}

impl Layout {
    pub(crate) fn shows(self, group: Group) -> bool {
        match group {
            Group::Processing => self.shows_processing(),
            Group::Warp => self.shows_warp(),
            Group::Pitch => self.shows_pitch(),
            Group::Fades => self.shows_fades(),
        }
    }

    pub(crate) fn names_chip(self) -> bool {
        matches!(self, Self::Full | Self::Icons)
    }

    pub(crate) fn shows_processing(self) -> bool {
        matches!(self, Self::Full | Self::Icons | Self::Dot)
    }

    pub(crate) fn shows_warp(self) -> bool {
        !matches!(self, Self::FoldWarp | Self::FoldPitch | Self::FoldFades)
    }

    pub(crate) fn shows_pitch(self) -> bool {
        !matches!(self, Self::FoldPitch | Self::FoldFades)
    }

    pub(crate) fn words(self) -> bool {
        self == Self::Full
    }

    pub(crate) fn shows_fades(self) -> bool {
        self != Self::FoldFades
    }

    pub(crate) fn has_menu(self) -> bool {
        matches!(
            self,
            Self::FoldProcessing | Self::FoldWarp | Self::FoldPitch | Self::FoldFades
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Widths {
    pub room: f32,
    pub chip: f32,
    pub chip_dot: f32,
    pub gain: f32,
    pub gain_label: f32,
    pub toggles: f32,
    pub fades: f32,
    pub processing_words: f32,
    pub processing_icons: f32,
    pub more: f32,
    pub file: f32,
    pub warp: f32,
    pub pitch: f32,
}

pub(crate) const GROUP_GAP: f32 = theme::space::M;

pub(crate) const FILE_LEAST: f32 = theme::size::CONTROL;

pub(crate) fn choose(w: Widths) -> Layout {
    let fits =
        |parts: &[f32]| parts.iter().sum::<f32>() + GROUP_GAP * (parts.len() - 1) as f32 <= w.room;
    let always = [w.gain, w.toggles, w.file];
    let with = |extra: &[f32]| {
        let mut parts = always.to_vec();
        parts.extend_from_slice(extra);
        parts
    };
    if fits(&with(&[
        w.chip,
        w.fades,
        w.processing_words,
        w.warp,
        w.pitch,
    ])) {
        Layout::Full
    } else if fits(&with(&[
        w.chip,
        w.fades,
        w.processing_icons,
        w.warp,
        w.pitch,
    ])) {
        Layout::Icons
    } else if fits(&with(&[
        w.chip_dot,
        w.fades,
        w.processing_icons,
        w.warp,
        w.pitch,
    ])) {
        Layout::Dot
    } else if fits(&with(&[w.chip_dot, w.fades, w.more, w.warp, w.pitch])) {
        Layout::FoldProcessing
    } else if fits(&with(&[w.chip_dot, w.fades, w.more, w.pitch])) {
        Layout::FoldWarp
    } else if fits(&with(&[w.chip_dot, w.fades, w.more])) {
        Layout::FoldPitch
    } else {
        Layout::FoldFades
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ends {
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
            pitch: 64.0,
        }
    }

    #[test]
    fn the_widest_layout_that_fits_is_taken() {
        let full = 120.0 + 150.0 + 52.0 + 260.0 + 220.0 + 90.0 + 70.0 + 64.0 + 7.0 * GROUP_GAP;
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
        assert_eq!(choose(widths(warp - 1.0)), Layout::FoldPitch);
        let pitch = warp - 64.0 - GROUP_GAP;
        assert_eq!(choose(widths(pitch)), Layout::FoldPitch);
        assert_eq!(choose(widths(pitch - 1.0)), Layout::FoldFades);
        assert_eq!(choose(widths(0.0)), Layout::FoldFades);
    }

    #[test]
    fn gain_and_the_file_chip_shrink_only_when_everything_that_can_fold_has() {
        let wide = ends(Layout::FoldFades, widths(1000.0));
        assert_eq!((wide.gain, wide.label, wide.file), (150.0, true, 90.0));
        let full = ends(Layout::Full, widths(100.0));
        assert_eq!(
            (full.gain, full.file),
            (150.0, 90.0),
            "other layouts: whole"
        );
        let others = 24.0 + 52.0 + 24.0 + 4.0 * GROUP_GAP;
        let tight = ends(Layout::FoldFades, widths(others + 150.0 + 40.0));
        assert_eq!((tight.gain, tight.label, tight.file), (150.0, true, 40.0));
        let room = others + 56.0 + 34.0 + FILE_LEAST;
        let narrow = ends(Layout::FoldFades, widths(room));
        assert_eq!(narrow.file, FILE_LEAST);
        assert!(narrow.label && narrow.gain == 90.0);
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
        assert_eq!(
            seen,
            [
                Layout::Full,
                Layout::Icons,
                Layout::Dot,
                Layout::FoldProcessing,
                Layout::FoldWarp,
                Layout::FoldPitch,
                Layout::FoldFades
            ]
        );
        for layout in seen {
            assert_eq!(layout.has_menu(), !layout.shows_processing());
            assert!(layout.shows_warp() || !layout.shows_processing());
            assert!(layout.shows_pitch() || !layout.shows_warp());
        }
    }
}
