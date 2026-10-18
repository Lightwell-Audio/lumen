//! The Devices tab (M6 slice 2d): a track's device chain as cards in a sunk
//! tray, joined by cables (the owner's C4, design system v3). `place` decides
//! where a chosen device goes, `art` makes a card's picture from its
//! identity, `geometry` lays the chain out, `card` paints one device and
//! `view` is the tab.

mod actions;
mod art;
mod card;
pub(crate) mod editor;
mod geometry;
pub(crate) mod outputs;
mod params;
mod pictures;
pub(crate) mod place;
pub(crate) mod presets;
mod sidechain;
mod stepped;
mod strip;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

use lumen_project::model::{Device, Id};

pub(crate) use geometry::{panel_height, panel_min_height, panel_min_height_keyed};
pub(crate) use pictures::Pictures;
pub(crate) use view::paint_drag_ghost;

/// What is being dragged from the browser: a device's catalog id.
pub(crate) struct Dragged(pub String);

/// A card being dragged: the device and the track it is on. Dropped in the
/// tray it moves within the chain; on a track's header it moves to that track
/// (Alt: copies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardDrag {
    pub track: Id,
    pub device: Id,
}

/// The chain's own state: which card is chosen (the commands act on it) and
/// whether the tab holds the keyboard.
#[derive(Default)]
pub(crate) struct Chain {
    /// The chosen device; it belongs to the shown track.
    pub selected: Option<Id>,
    /// The Devices tab has the keyboard: Delete removes the chosen device.
    pub live: bool,
    /// The pointer was over the tray last frame.
    pub hovered: bool,
    /// The detail panel should be tall enough for a whole card at the next
    /// frame's layout (it only ever grows; the user can drag it back).
    pub grow: bool,
    /// Folded cards (their power button and name only). A view setting, not
    /// an edit; kept for the session.
    pub folded: HashSet<Id>,
    /// The device last copied, with its settings as they were then.
    pub clipboard: Option<Device>,
    /// The plugins' own pictures, read in the background (made on first use).
    pub pictures: Option<Pictures>,
    /// The parameters on each card's strip, where a person chose them (saved
    /// with the project's view).
    pub shown: BTreeMap<Id, Vec<u32>>,
    /// When the plugin's own text for a value was last asked for, and for
    /// which value, by device and parameter: asked when the value changes,
    /// not faster than every 50 ms.
    pub text_asked: RefCell<HashMap<(u64, u32), (f64, std::time::Instant)>>,
    /// A knob's text is waiting on a later frame to be asked for.
    pub text_pending: Cell<bool>,
    /// The panel with every parameter of one plugin, when open.
    pub panel: Option<params::Panel>,
    /// Outputs of an instrument being turned off whose tracks hold work,
    /// waiting for the person's answer in the menu.
    pub asking: outputs::Asking,
    /// What the shown track's sidechain carried, as the cards' lights show it.
    pub sidechain: sidechain::Readings,
    /// The device whose Learn is on: the next control touched in its window
    /// takes a slot of its strip.
    pub learning: Option<Id>,
    /// The cards' preset menus: the sound each card has, the files found, and
    /// a save under way.
    pub presets: presets::Presets,
}
