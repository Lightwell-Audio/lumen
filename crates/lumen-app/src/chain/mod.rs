mod actions;
mod art;
mod card;
pub(crate) mod editor;
mod face;
pub(crate) use face::cadence;
mod geometry;
pub(crate) mod outputs;
mod params;
pub(crate) mod pictures;
pub(crate) mod place;
pub(crate) mod presets;
mod sidechain;
mod stepped;
mod strip;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

use lumen_project::model::{Device, Id};

pub(crate) use actions::placeable;
pub(crate) use geometry::panel_least;
pub(crate) use params::unit_name;
pub(crate) use pictures::Pictures;
pub(crate) use view::paint_drag_ghost;

impl Chain {
    pub(crate) fn least_panel(&self) -> f32 {
        panel_least(self.least.max(geometry::COMPACT_H))
    }
}

pub(crate) struct Dragged(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardDrag {
    pub track: Id,
    pub device: Id,
}

#[derive(Default)]
pub(crate) struct Chain {
    pub selected: Option<Id>,
    pub live: bool,
    pub hovered: bool,
    pub grow: bool,
    pub height: Option<f32>,
    pub compact: bool,
    pub steps: HashMap<Id, geometry::Step>,
    pub bands: HashMap<Id, usize>,
    pub analyzers: BTreeMap<Id, Vec<f32>>,
    pub pretend_spectrum: Option<(Id, Vec<f32>)>,
    pub pretend_level: Option<f32>,
    pub least: f32,
    pub folded: HashSet<Id>,
    pub clipboard: Option<Device>,
    pub pictures: Option<Pictures>,
    pub shown: BTreeMap<Id, Vec<u32>>,
    pub text_asked: RefCell<HashMap<(u64, u32), (f64, std::time::Instant)>>,
    pub text_pending: Cell<bool>,
    pub texts_seen: Cell<u64>,
    pub panel: Option<params::Panel>,
    pub asking: outputs::Asking,
    pub sidechain: sidechain::Readings,
    pub learning: Option<Id>,
    pub presets: presets::Presets,
    pub open_when_ready: Option<Id>,
    pub deferred: BTreeMap<(Id, u32), f64>,
}
