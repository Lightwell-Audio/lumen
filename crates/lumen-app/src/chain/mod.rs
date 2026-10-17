//! The Devices tab (M6 slice 2d): a track's device chain as cards in a sunk
//! tray, joined by cables (the owner's C4, design system v3). `place` decides
//! where a chosen device goes, `art` makes a card's picture from its
//! identity, `geometry` lays the chain out, `card` paints one device and
//! `view` is the tab.

mod actions;
mod art;
mod card;
mod geometry;
pub(crate) mod place;
mod view;

use std::collections::HashSet;

use lumen_project::model::{Device, Id};

pub(crate) use geometry::panel_height;
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
}
