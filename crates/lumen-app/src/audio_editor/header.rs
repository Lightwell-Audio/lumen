//! Which layout the audio editor's header takes (M9.5 D24): the first that
//! fits, from what its parts measure this frame. The drawing is
//! `app::audio_editor::header`.

use crate::theme;

/// The header's layouts, widest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Layout {
    /// Every control with its word.
    #[default]
    Words,
    /// The view buttons lose their words.
    Icons,
    /// The title gives up its caption and shortens; the grid chip loses its
    /// word.
    Compact,
    /// Everything right of the grid chip but the pills moves into a menu.
    More,
    /// The colour bar, the grid's icon and the menu.
    Least,
}

/// What the parts measure, in points, gaps within a part included.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Widths {
    /// The room for the row: the strip less its padding.
    pub room: f32,
    /// The title with its caption, and shortened; its colour bar alone.
    pub title: f32,
    pub title_short: f32,
    pub bar: f32,
    /// The grid chip, without its word, and as its icon.
    pub grid: f32,
    pub grid_value: f32,
    pub grid_icon: f32,
    /// The view buttons with their words.
    pub view_words: f32,
    /// The view buttons as icons.
    pub view_icons: f32,
    /// The ⋯ button.
    pub more: f32,
    /// The ⋯ button is there in every layout: the properties column is
    /// folded, and Clip properties lives in it.
    pub always_more: bool,
}

/// How much room the left parts and the right parts keep between them, and
/// between parts of a group.
const LEFT_GAP: f32 = theme::space::M;
const RIGHT_GAP: f32 = theme::space::S;

/// The first layout that fits. [`Layout::Least`] is taken when none does:
/// nothing is left out of it, as the menu holds the rest.
pub(crate) fn choose(w: Widths) -> Layout {
    let more = if w.always_more {
        w.more + RIGHT_GAP
    } else {
        0.0
    };
    let fits = |left: f32, right: f32| left + LEFT_GAP + right <= w.room;
    if fits(w.title + LEFT_GAP + w.grid, w.view_words + more) {
        Layout::Words
    } else if fits(w.title + LEFT_GAP + w.grid, w.view_icons + more) {
        Layout::Icons
    } else if fits(w.title_short + LEFT_GAP + w.grid_value, w.view_icons + more) {
        Layout::Compact
    } else if fits(w.title_short + LEFT_GAP + w.grid_value, w.more) {
        Layout::More
    } else {
        Layout::Least
    }
}

/// Whether the properties column folds: it would leave the wave too little.
pub(crate) fn folds(area: f32, column: f32) -> bool {
    area - column < theme::size::WAVE_LEAST
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(room: f32) -> Widths {
        Widths {
            room,
            title: 200.0,
            title_short: 90.0,
            bar: 8.0,
            grid: 130.0,
            grid_value: 90.0,
            grid_icon: 24.0,
            view_words: 180.0,
            view_icons: 56.0,
            more: 24.0,
            always_more: false,
        }
    }

    #[test]
    fn each_layout_is_taken_where_it_first_fits() {
        // title + gap + grid = 342 (short: 192); then a gap and the right
        // group.
        assert_eq!(choose(widths(534.0)), Layout::Words);
        assert_eq!(choose(widths(533.0)), Layout::Icons);
        assert_eq!(choose(widths(410.0)), Layout::Icons);
        assert_eq!(choose(widths(409.0)), Layout::Compact);
        assert_eq!(choose(widths(260.0)), Layout::Compact);
        assert_eq!(choose(widths(259.0)), Layout::More);
        assert_eq!(choose(widths(228.0)), Layout::More);
        assert_eq!(choose(widths(227.0)), Layout::Least);
        assert_eq!(choose(widths(10.0)), Layout::Least);
    }

    #[test]
    fn a_folded_column_keeps_the_menu_in_every_layout() {
        let plain = widths(534.0);
        let folded = Widths {
            always_more: true,
            ..plain
        };
        assert_eq!(choose(plain), Layout::Words);
        assert_eq!(choose(folded), Layout::Icons, "the menu takes room");
    }

    #[test]
    fn the_column_folds_when_it_would_leave_too_little_wave() {
        assert!(!folds(1_000.0, 296.0));
        assert!(folds(444.0, 296.0));
        assert!(folds(260.0, 296.0));
    }
}
