mod paint;
pub(crate) mod scale;

use eframe::egui::{Pos2, Rect};
use lumen_core::time::Ticks;

use crate::theme;
pub(crate) use paint::{Look, draw};
pub(crate) use scale::{Plane, Scale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    Tempo,
    Meter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub which: Which,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gesture {
    Tempo { index: usize, scale: Scale },
    Meter { index: usize, grab: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Typing {
    pub which: Which,
    pub index: Option<usize>,
    pub at: Ticks,
    pub text: String,
    pub anchor: Pos2,
}

#[derive(Debug, Default)]
pub(crate) struct SongLanes {
    pub shown: bool,
    pub picked: Option<Picked>,
    pub focus: bool,
    pub gesture: Option<Gesture>,
    pub typing: Option<Typing>,
    pub hover: Option<Picked>,
    pub menu: Option<MenuSpot>,
    pub ask: Option<(Which, Option<usize>, Ticks)>,
    pub drawn: Option<(Band, Scale)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuSpot {
    pub which: Which,
    pub index: Option<usize>,
    pub at: Ticks,
}

impl SongLanes {
    pub fn full_height() -> f32 {
        theme::size::SONG_TEMPO + theme::size::SONG_METER
    }

    pub fn picked_on(&self, tempo: usize, meter: usize) -> Option<Picked> {
        self.picked.filter(|p| {
            p.index
                < match p.which {
                    Which::Tempo => tempo,
                    Which::Meter => meter,
                }
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Band {
    pub corner: Rect,
    pub tempo: Plane,
    pub meter: Plane,
}
