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

pub(crate) const COUNTS: [u32; 4] = [8, 16, 32, 64];

pub(crate) const DEFAULT_COUNT: u32 = 16;

pub(crate) const MIN_COUNT: u32 = 8;
pub(crate) const MAX_COUNT: u32 = 64;

pub(crate) const ACCENTS: [u8; 4] = [ACCENT_VELOCITY, 120, 112, 104];

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

pub(crate) const RESOLUTIONS: [Resolution; 7] = [
    res(4, Feel::Straight),
    res(8, Feel::Straight),
    res(8, Feel::Triplet),
    res(16, Feel::Straight),
    res(16, Feel::Triplet),
    res(32, Feel::Straight),
    res(32, Feel::Triplet),
];

pub(crate) const DEFAULT_RESOLUTION: usize = 3;

impl Resolution {
    pub fn label(self) -> String {
        let name = self.size.name();
        match self.feel {
            Feel::Straight => name,
            Feel::Triplet => format!("{name}T"),
            Feel::Dotted => format!("{name}."),
        }
    }

    pub fn ticks(self, meter: Meter) -> Option<Ticks> {
        size_ticks(self.size, self.feel, meter).filter(|t| *t > Ticks::ZERO)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Setup {
    pub count: u32,

    pub resolution: usize,

    pub page: u32,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    pub step: Ticks,

    pub count: usize,

    pub first: i64,

    pub in_clip: i64,

    pub pages: u32,

    pub group: usize,
}

impl Setup {
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

    pub fn with_page(mut self, page: i64, pages: u32) -> Self {
        self.page = page.clamp(0, i64::from(pages.saturating_sub(1))) as u32;
        self
    }
}

pub(crate) fn fits(length: Ticks, meter: Meter) -> bool {
    let Some(step) = RESOLUTIONS[DEFAULT_RESOLUTION].ticks(meter) else {
        return false;
    };
    length > Ticks::ZERO && length.0 % step.0 == 0 && length.0 / step.0 <= i64::from(MAX_COUNT)
}

#[derive(Debug)]
struct PageCache {
    clip: Arc<Clip>,
    pitches: Vec<u8>,
    step: Ticks,
    first: i64,
    count: usize,
    cells: Arc<Vec<Cell>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawn {
    pub layout: Layout,
    pub body: eframe::egui::Rect,
    pub row_height: f32,
    pub scroll: f32,
    pub rows: Vec<u8>,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    setups: BTreeMap<Id, Setup>,

    pub cursor: Option<Target>,
    pub gesture: Option<Gesture>,

    pub focused: bool,
    pub drawn: Option<Drawn>,

    pub playing: Option<i64>,
    page: Option<PageCache>,
}

impl State {
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

        let one = setup
            .layout(Ticks::quarters(4), meter)
            .expect("the default resolution is whole ticks");
        assert_eq!((one.count, one.in_clip, one.pages), (16, 16, 1));
        assert_eq!(one.step, Ticks(PPQ / 4));
        assert_eq!(one.group, 4, "a beat is four sixteenths");

        let long = Setup { page: 9, ..setup }
            .layout(Ticks::quarters(12), meter)
            .unwrap();
        assert_eq!((long.pages, long.first), (3, 32));

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
