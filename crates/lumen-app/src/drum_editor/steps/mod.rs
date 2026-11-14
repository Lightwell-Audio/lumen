//! The step sequencer (M8 slice D3, the owner's SQ A and DV A): the drum
//! clip's notes as a grid of steps, one row per sound in the map's order, the
//! third view of the one clip editor beside Lanes and Keys.
//!
//! - this file: what a grid is (its count, its resolution, its page, its
//!   accent) and the state the editor keeps for it;
//! - [`geometry`]: where the cells are, and the cell under a pointer;
//! - [`paint`]: how a cell reads (the specimen's filled well, the accent's
//!   rim, the off-grid mark, the playing step's light);
//! - [`input`]: what a press, a drag and a key mean.
//!
//! It is a view, not a second editor. The notes, the selection, the history,
//! the snapping and the commands are the note editor's: a step is a window of
//! time, a hit belongs to the step it starts in, and an edit goes through
//! `Document::edit_steps` as one undo step. Nothing here moves a note by
//! showing it (`lumen_project::edit::steps`). None of this state is saved: it
//! is a view, like Lanes' hidden sounds, and it lasts while the app runs.

pub(crate) mod geometry;
pub(crate) mod input;
pub(crate) mod paint;

use std::collections::BTreeMap;
use std::sync::Arc;

use lumen_core::grid::{Feel, Size, size_ticks};
use lumen_core::meter::Meter;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::edit::steps::{self as model, ACCENT_VELOCITY, Cell, Target};
use lumen_project::model::{Clip, Id, Note};

pub(crate) use input::Gesture;

/// The step counts the strip offers; the model takes any 8 to 64.
pub(crate) const COUNTS: [u32; 4] = [8, 16, 32, 64];
/// How many steps a page has until the user says otherwise (SQ A).
pub(crate) const DEFAULT_COUNT: u32 = 16;
/// The fewest and the most steps a page holds (SQ A).
pub(crate) const MIN_COUNT: u32 = 8;
pub(crate) const MAX_COUNT: u32 = 64;
/// The accents the strip offers: above the click's own velocity, so an accent
/// is a different step. The first is the default.
pub(crate) const ACCENTS: [u8; 4] = [ACCENT_VELOCITY, 120, 112, 104];

/// How fine a step is: a size of the grid list and its feel (SQ A: any
/// resolution of the list, triplets included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resolution {
    pub size: Size,
    pub feel: Feel,
}

const fn res(n: u32, feel: Feel) -> Resolution {
    Resolution {
        size: Size::Note(n),
        feel,
    }
}

/// The resolutions the strip offers, coarsest first.
pub(crate) const RESOLUTIONS: [Resolution; 7] = [
    res(4, Feel::Straight),
    res(8, Feel::Straight),
    res(8, Feel::Triplet),
    res(16, Feel::Straight),
    res(16, Feel::Triplet),
    res(32, Feel::Straight),
    res(32, Feel::Triplet),
];
/// 1/16, the resolution a grid starts at.
pub(crate) const DEFAULT_RESOLUTION: usize = 3;

impl Resolution {
    /// "1/16", "1/16T".
    pub fn label(self) -> String {
        let name = self.size.name();
        match self.feel {
            Feel::Straight => name,
            Feel::Triplet => format!("{name}T"),
            Feel::Dotted => format!("{name}."),
        }
    }

    /// The step's length in `meter`.
    pub fn ticks(self, meter: Meter) -> Option<Ticks> {
        size_ticks(self.size, self.feel, meter).filter(|t| *t > Ticks::ZERO)
    }
}

/// One clip's grid as the user set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Setup {
    pub count: u32,
    /// An index into [`RESOLUTIONS`].
    pub resolution: usize,
    /// The page shown, from 0.
    pub page: u32,
    /// The velocity an accent is (the clip remembers it while the app runs).
    pub accent: u8,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            count: DEFAULT_COUNT,
            resolution: DEFAULT_RESOLUTION,
            page: 0,
            accent: ACCENT_VELOCITY,
        }
    }
}

/// What a grid is made of at one moment: the step, the page's place, how many
/// steps the clip holds. Everything the drawing and the pointer need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    /// A step's length in clip ticks.
    pub step: Ticks,
    /// The steps a page shows.
    pub count: usize,
    /// The index of the page's first step.
    pub first: i64,
    /// How many steps the clip's content holds (the last one may be partial):
    /// a step from this index on is past the end.
    pub in_clip: i64,
    /// The pages the clip has at this count.
    pub pages: u32,
    /// Steps in a beat's group: the wells alternate by it.
    pub group: usize,
}

impl Setup {
    /// The grid's layout for a clip of `length` content ticks in `meter`.
    pub fn layout(&self, length: Ticks, meter: Meter) -> Option<Layout> {
        let resolution = RESOLUTIONS.get(self.resolution).copied()?;
        let step = resolution.ticks(meter)?;
        let count = self.count.clamp(MIN_COUNT, MAX_COUNT) as usize;
        let in_clip = model::steps_in(length, step);
        let pages = ((in_clip + count as i64 - 1) / count as i64).max(1) as u32;
        let page = self.page.min(pages - 1);
        let quarter = (PPQ as f64 / step.0 as f64).round().max(1.0) as usize;
        Some(Layout {
            step,
            count,
            first: i64::from(page) * count as i64,
            in_clip,
            pages,
            group: quarter,
        })
    }

    /// The setup with its page kept inside the clip.
    pub fn with_page(mut self, page: i64, pages: u32) -> Self {
        self.page = page.clamp(0, i64::from(pages.saturating_sub(1))) as u32;
        self
    }
}

/// Whether a clip fits the step grid, so a double-click opens it in Steps: its
/// content is a whole number of 1/16 steps, at most four bars of them (64
/// steps), in the clip's meter. Any other clip opens in Lanes, and Steps is a
/// click away.
pub(crate) fn fits(length: Ticks, meter: Meter) -> bool {
    let Some(step) = RESOLUTIONS[DEFAULT_RESOLUTION].ticks(meter) else {
        return false;
    };
    length > Ticks::ZERO && length.0 % step.0 == 0 && length.0 / step.0 <= i64::from(MAX_COUNT)
}

/// The cells of the page: gathered again only when the clip, the rows, the
/// page or the step changed.
#[derive(Debug)]
struct PageCache {
    clip: Arc<Clip>,
    pitches: Vec<u8>,
    step: Ticks,
    first: i64,
    count: usize,
    cells: Arc<Vec<Cell>>,
}

/// The drawn cells of the last frame, for the tests: where each row's first
/// cell was and how wide a cell is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawn {
    pub layout: Layout,
    pub body: eframe::egui::Rect,
    pub row_height: f32,
    pub scroll: f32,
    pub rows: Vec<u8>,
}

/// The step sequencer's state, kept with the drum editor's.
#[derive(Debug, Default)]
pub(crate) struct State {
    setups: BTreeMap<Id, Setup>,
    /// The step the keyboard is on: the row's note and the step's index. It
    /// is a place, not a selection; the editor's selection follows it.
    pub cursor: Option<Target>,
    pub gesture: Option<Gesture>,
    /// The Tab stop of the grid had the keyboard on the last frame.
    pub focused: bool,
    pub drawn: Option<Drawn>,
    /// The step (its index in the clip) the playhead is in, while playing.
    pub playing: Option<i64>,
    page: Option<PageCache>,
}

impl State {
    /// The clip's grid setup (the defaults until it is set).
    pub fn setup(&self, clip: Id) -> Setup {
        self.setups.get(&clip).copied().unwrap_or_default()
    }

    pub fn set_setup(&mut self, clip: Id, setup: Setup) {
        if setup == Setup::default() {
            self.setups.remove(&clip);
        } else {
            self.setups.insert(clip, setup);
        }
    }

    /// The cells of `layout`'s page for the rows `pitches`, in `clip`'s notes.
    pub fn cells_for(
        &mut self,
        clip: &Arc<Clip>,
        notes: &[Note],
        pitches: &[u8],
        layout: &Layout,
    ) -> Arc<Vec<Cell>> {
        let fresh = self.page.as_ref().is_some_and(|cache| {
            Arc::ptr_eq(&cache.clip, clip)
                && cache.step == layout.step
                && cache.first == layout.first
                && cache.count == layout.count
                && cache.pitches == pitches
        });
        if !fresh {
            let mut cells = Vec::new();
            model::page_into(
                notes,
                layout.step,
                layout.first,
                layout.count,
                pitches,
                &mut cells,
            );
            self.page = Some(PageCache {
                clip: clip.clone(),
                pitches: pitches.to_vec(),
                step: layout.step,
                first: layout.first,
                count: layout.count,
                cells: Arc::new(cells),
            });
        }
        self.page.as_ref().expect("just built").cells.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_four() -> Meter {
        Meter {
            numerator: 4,
            denominator: 4,
        }
    }

    #[test]
    fn a_grid_starts_at_sixteen_sixteenths_and_pages_a_longer_clip() {
        let meter = four_four();
        let setup = Setup::default();
        // One bar: one page of 16 steps.
        let one = setup
            .layout(Ticks::quarters(4), meter)
            .expect("the default resolution is whole ticks");
        assert_eq!((one.count, one.in_clip, one.pages), (16, 16, 1));
        assert_eq!(one.step, Ticks(PPQ / 4));
        assert_eq!(one.group, 4, "a beat is four sixteenths");
        // Three bars: three pages, the page kept inside them.
        let long = Setup { page: 9, ..setup }
            .layout(Ticks::quarters(12), meter)
            .unwrap();
        assert_eq!((long.pages, long.first), (3, 32));
        // A triplet grid has its own step and a beat of three... six.
        let triplet = Setup {
            resolution: 4,
            ..setup
        }
        .layout(Ticks::quarters(4), meter)
        .unwrap();
        assert_eq!(triplet.step, Ticks(PPQ / 6));
        assert_eq!(triplet.group, 6);
    }

    #[test]
    fn a_clip_fits_when_it_is_whole_sixteenths_and_at_most_four_bars() {
        let meter = four_four();
        assert!(fits(Ticks::quarters(4), meter));
        assert!(fits(Ticks::quarters(16), meter), "four bars are 64 steps");
        assert!(!fits(Ticks::quarters(17), meter), "past 64 steps");
        assert!(!fits(Ticks(PPQ / 4 + 1), meter), "not whole steps");
        assert!(!fits(Ticks::ZERO, meter), "an empty clip has no grid");
    }

    #[test]
    fn the_resolutions_are_the_grid_lists_and_read_as_one_writes_them() {
        let labels: Vec<String> = RESOLUTIONS.iter().map(|r| r.label()).collect();
        assert_eq!(
            labels,
            ["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32", "1/32T"]
        );
        for r in RESOLUTIONS {
            assert!(r.ticks(four_four()).is_some(), "{}", r.label());
        }
        assert_eq!(RESOLUTIONS[DEFAULT_RESOLUTION].label(), "1/16");
    }

    #[test]
    fn a_setup_equal_to_the_default_is_not_kept() {
        let mut state = State::default();
        let clip = Id(7);
        state.set_setup(
            clip,
            Setup {
                count: 32,
                ..Setup::default()
            },
        );
        assert_eq!(state.setup(clip).count, 32);
        state.set_setup(clip, Setup::default());
        assert!(state.setups.is_empty());
    }
}
