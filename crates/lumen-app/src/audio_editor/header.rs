use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Layout {
    #[default]
    Words,
    Icons,
    Compact,
    More,
    Least,
}

impl Layout {
    pub(crate) fn shows_views(self) -> bool {
        matches!(self, Self::Words | Self::Icons | Self::Compact)
    }

    pub(crate) fn shows_pills(self) -> bool {
        self != Self::Least
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Widths {
    pub room: f32,
    pub title: f32,
    pub title_short: f32,
    pub bar: f32,
    pub tools: f32,
    pub tools_icons: f32,
    pub grid: f32,
    pub grid_value: f32,
    pub grid_icon: f32,
    pub toggles_words: f32,
    pub toggles_icons: f32,
    pub pills: f32,
    pub view_words: f32,
    pub view_icons: f32,
    pub more: f32,
}

const LEFT_GAP: f32 = theme::space::M;
const RIGHT_GAP: f32 = theme::space::S;

pub(crate) fn choose(w: Widths) -> Layout {
    let left = |parts: &[f32]| parts.iter().sum::<f32>() + LEFT_GAP * (parts.len() - 1) as f32;
    let pills = w.pills + RIGHT_GAP;
    let fits = |left: f32, right: f32| left + LEFT_GAP + right <= w.room;
    if fits(
        left(&[w.title, w.tools, w.grid, w.toggles_words]),
        pills + w.view_words,
    ) {
        Layout::Words
    } else if fits(
        left(&[w.title, w.tools, w.grid, w.toggles_icons]),
        pills + w.view_icons,
    ) {
        Layout::Icons
    } else if fits(
        left(&[w.title_short, w.tools_icons, w.grid_value, w.toggles_icons]),
        pills + w.view_icons,
    ) {
        Layout::Compact
    } else if fits(
        left(&[w.title_short, w.tools_icons, w.grid_value]),
        pills + w.more,
    ) {
        Layout::More
    } else {
        Layout::Least
    }
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
            tools: 150.0,
            tools_icons: 70.0,
            grid: 130.0,
            grid_value: 90.0,
            grid_icon: 24.0,
            toggles_words: 120.0,
            toggles_icons: 24.0,
            pills: 100.0,
            view_words: 180.0,
            view_icons: 56.0,
            more: 24.0,
        }
    }

    #[test]
    fn each_layout_is_taken_where_it_first_fits() {
        assert_eq!(choose(widths(936.0)), Layout::Words);
        assert_eq!(choose(widths(935.0)), Layout::Icons);
        assert_eq!(choose(widths(716.0)), Layout::Icons);
        assert_eq!(choose(widths(715.0)), Layout::Compact);
        assert_eq!(choose(widths(486.0)), Layout::Compact);
        assert_eq!(choose(widths(485.0)), Layout::More);
        assert_eq!(choose(widths(418.0)), Layout::More);
        assert_eq!(choose(widths(417.0)), Layout::Least);
        assert_eq!(choose(widths(10.0)), Layout::Least);
    }

    #[test]
    fn only_the_least_layout_moves_the_pills_into_the_menu() {
        assert!(Layout::More.shows_pills() && !Layout::More.shows_views());
        assert!(!Layout::Least.shows_pills());
        assert!(Layout::Compact.shows_views());
    }
}
